//! `POST /v1/webhooks/github` : route publique (hors `/api/`, donc hors CSRF).
//!
//! Ordre des controles :
//! 0. limite par IP en memoire (`public_limit::WEBHOOK_PER_IP` livraisons
//!    par minute, 429 avec `Retry-After`), avant la base et le secret ;
//! 1. corps limite a 5 Mio (413 au-dela) ;
//! 2. `X-Hub-Signature-256` verifiee par HMAC SHA-256 sur le corps BRUT,
//!    avant toute lecture du JSON, comparaison a temps constant (401) ;
//! 3. fraicheur : la date du push, signee avec le corps (`repository.pushed_at`,
//!    sinon `head_commit.timestamp`), doit dater de moins de 24 h et d'au plus
//!    5 min dans le futur ; sinon (ou sans date) 200 ignore, rien en file. La
//!    fenetre egale la retention de la table anti-rejeu : une livraison
//!    capturee ne peut pas etre rejouee apres l'oubli de son identifiant ;
//! 4. anti-rejeu par `X-GitHub-Delivery` (table, entrees de plus de 24 h
//!    purgees) : une livraison deja vue repond 200 sans rien mettre en file ;
//! 5. depot inconnu : 404 ; autre branche (ou projet sans environnement
//!    `production`, journalise) : 200 ignore ; puis les reglages Git de
//!    chaque projet (`domain::projects::git::skip_reason`) : deploiement
//!    automatique coupe, `[skip ci]`, compte bot, ou aucun fichier modifie
//!    (ajoute, modifie ou supprime, tous commits confondus) dans les chemins
//!    surveilles : projet ignore, raison journalisee (200 ignore avec la
//!    raison si aucun projet ne deploie) ;
//! 6. sinon une tache `Deploy` par projet concerne (environnement
//!    `production`), charge utile = identifiants + commit, cle de
//!    deduplication `<delivery>:<projet>`, dans la transaction qui enregistre
//!    la livraison.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::json;
use sha2::Sha256;
use shared::error::ApiError;
use uuid::Uuid;

use super::load;
use crate::auth::internal;
use crate::domain::projects::git::{PushFacts, PushSettings, skip_reason};
use crate::http::client_ip::ClientIp;
use crate::jobs::{self, JobKind};
use crate::secrets::labels;
use crate::state::AppState;

/// Taille maximale d'une livraison (GitHub plafonne a 25 Mio ; un `push`
/// reste tres en dessous).
pub const MAX_BODY: usize = 5 * 1024 * 1024;
/// Duree de conservation des identifiants de livraison (24 h), et age
/// maximal d'un push accepte (jamais plus long que la conservation).
pub const REPLAY_WINDOW_SECS: f64 = 24.0 * 3600.0;
/// Avance d'horloge toleree pour la date d'un push (5 min).
pub const MAX_CLOCK_SKEW_SECS: f64 = 300.0;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/webhooks/github", post(receive))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn bad_signature() -> ApiError {
    ApiError::new(401, "invalid_signature", "Signature absente ou invalide.")
}

fn bad_request(message: &str) -> ApiError {
    ApiError::new(400, "invalid_payload", message)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// `sha256=<64 hex>` -> 32 octets.
fn parse_signature(value: &str) -> Option<[u8; 32]> {
    let hex = value.strip_prefix("sha256=")?.as_bytes();
    if hex.len() != 64 {
        return None;
    }
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 32];
    for (i, [hi, lo]) in hex.as_chunks::<2>().0.iter().enumerate() {
        out[i] = nibble(*hi)? << 4 | nibble(*lo)?;
    }
    Some(out)
}

/// HMAC SHA-256 de `body` par `secret`, compare a temps constant.
pub fn signature_valid(secret: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(given) = header.and_then(parse_signature) else {
        return false;
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC accepte toute cle");
    mac.update(body);
    mac.verify_slice(&given).is_ok()
}

/// Identifiant de livraison : GUID de GitHub (`[A-Za-z0-9-]`, 100 au plus).
fn delivery_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// SHA-1 (40) ou SHA-256 (64) en hexadecimal.
fn commit_ok(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Deserialize)]
struct PushEvent {
    #[serde(rename = "ref")]
    git_ref: String,
    after: String,
    #[serde(default)]
    deleted: bool,
    repository: Repository,
    #[serde(default)]
    head_commit: Option<HeadCommit>,
    #[serde(default)]
    sender: Option<Sender>,
    #[serde(default)]
    commits: Vec<PushCommit>,
}

#[derive(Deserialize)]
struct Sender {
    login: String,
}

#[derive(Deserialize)]
struct PushCommit {
    #[serde(default)]
    added: Vec<String>,
    #[serde(default)]
    modified: Vec<String>,
    #[serde(default)]
    removed: Vec<String>,
}

/// GitHub ne decrit pas plus de 2048 commits par push : au-dela, la liste
/// des fichiers est incomplete.
const MAX_DESCRIBED_COMMITS: usize = 2048;

/// Faits du push utiles aux reglages Git ; fichiers inconnus (`None`) sans
/// commit decrit ou si la liste est tronquee.
fn push_facts(event: &PushEvent) -> PushFacts<'_> {
    let files =
        (!event.commits.is_empty() && event.commits.len() < MAX_DESCRIBED_COMMITS).then(|| {
            event
                .commits
                .iter()
                .flat_map(|c| c.added.iter().chain(&c.modified).chain(&c.removed))
                .map(String::as_str)
                .collect()
        });
    PushFacts {
        head_message: event
            .head_commit
            .as_ref()
            .and_then(|c| c.message.as_deref()),
        sender: event.sender.as_ref().map(|s| s.login.as_str()),
        files,
    }
}

#[derive(Deserialize)]
struct Repository {
    full_name: String,
    /// Secondes Unix dans les evenements `push`, parfois RFC 3339.
    #[serde(default)]
    pushed_at: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct HeadCommit {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Date du push signee avec le corps, en secondes Unix.
fn pushed_at(event: &PushEvent) -> Option<f64> {
    let from_text = |t: &str| {
        t.parse::<jiff::Timestamp>()
            .ok()
            .map(|t| t.as_second() as f64)
    };
    let repository = match &event.repository.pushed_at {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(t)) => from_text(t),
        _ => None,
    };
    repository.or_else(|| {
        event
            .head_commit
            .as_ref()
            .and_then(|c| c.timestamp.as_deref())
            .and_then(from_text)
    })
}

/// `true` si le push date de la fenetre anti-rejeu (ni trop vieux, ni futur).
fn fresh(pushed_at: Option<f64>, now: f64) -> bool {
    pushed_at.is_some_and(|at| at >= now - REPLAY_WINDOW_SECS && at <= now + MAX_CLOCK_SKEW_SECS)
}

fn ignored(reason: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "status": "ignored", "reason": reason })),
    )
        .into_response()
}

/// `POST /v1/webhooks/github`
async fn receive(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    // Limite par IP en memoire, avant la base et le dechiffrement du secret.
    if let Err(retry_after) = state.public_limits.webhook_ip(ip) {
        tracing::warn!(%ip, "webhook GitHub refuse : trop de livraisons");
        let mut res = ApiError::new(
            429,
            "rate_limited",
            "Trop de livraisons depuis cette adresse. Réessayez plus tard.",
        )
        .into_response();
        res.headers_mut()
            .insert(axum::http::header::RETRY_AFTER, retry_after.into());
        return Ok(res);
    }
    // Sans App, aucun secret : la signature est inverifiable (401, sans
    // reveler si une App existe).
    let app = load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?
        .ok_or_else(bad_signature)?;
    let secret = state
        .secrets
        .open_bound(labels::GITHUB_WEBHOOK_SECRET, &app.webhook_secret_sealed)
        .map_err(|e| internal("ouverture du secret de webhook", e))?;
    if !signature_valid(&secret, &body, header(&headers, "x-hub-signature-256")) {
        return Err(bad_signature());
    }
    drop(secret);

    let delivery = header(&headers, "x-github-delivery")
        .filter(|d| delivery_ok(d))
        .ok_or_else(|| bad_request("En-tête X-GitHub-Delivery absent ou invalide."))?
        .to_string();
    if header(&headers, "x-github-event") != Some("push") {
        return Ok(ignored("event"));
    }
    let event: PushEvent =
        serde_json::from_slice(&body).map_err(|_| bad_request("Événement push illisible."))?;

    let now = state.clock.unix_secs();
    if !fresh(pushed_at(&event), now) {
        return Ok(ignored("stale"));
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction du webhook GitHub", e))?;
    sqlx::query("DELETE FROM github_deliveries WHERE received_at < to_timestamp($1::float8)")
        .bind(now - REPLAY_WINDOW_SECS)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("purge des livraisons GitHub", e))?;
    let fresh = sqlx::query(
        "INSERT INTO github_deliveries (delivery_id, received_at) \
         VALUES ($1, to_timestamp($2::float8)) ON CONFLICT (delivery_id) DO NOTHING",
    )
    .bind(&delivery)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("enregistrement de la livraison GitHub", e))?
    .rows_affected()
        == 1;
    if !fresh {
        tx.commit()
            .await
            .map_err(|e| internal("transaction du webhook GitHub", e))?;
        return Ok((StatusCode::OK, Json(json!({ "status": "duplicate" }))).into_response());
    }

    #[derive(sqlx::FromRow)]
    struct Target {
        project_id: Uuid,
        branch: String,
        environment_id: Option<Uuid>,
        #[sqlx(flatten)]
        settings: PushSettings,
    }
    let rows: Vec<Target> = sqlx::query_as(
        "SELECT g.project_id, g.branch, e.id AS environment_id, g.auto_deploy, g.skip_ci, \
                g.ignore_bots, g.watch_paths \
         FROM git_sources g \
         LEFT JOIN environments e ON e.project_id = g.project_id AND e.name = 'production' \
         WHERE lower(g.repository) = lower($1) ORDER BY g.project_id",
    )
    .bind(event.repository.full_name.trim())
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| internal("recherche des projets du depot", e))?;
    if rows.is_empty() {
        // Rien n'est enregistre : la livraison n'est pas la notre.
        drop(tx);
        return Err(ApiError::new(
            404,
            "unknown_repository",
            "Aucun projet ne suit ce dépôt.",
        ));
    }

    let branch = event.git_ref.strip_prefix("refs/heads/");
    let matching: Vec<Target> = match branch {
        Some(branch) if !event.deleted => rows.into_iter().filter(|t| t.branch == branch).collect(),
        _ => Vec::new(),
    };
    let facts = push_facts(&event);
    let mut targets: Vec<(Uuid, Uuid)> = Vec::new();
    let mut reason = if matching.is_empty() {
        "branch"
    } else {
        "environment"
    };
    for t in &matching {
        let Some(env) = t.environment_id else {
            tracing::warn!(
                project_id = %t.project_id,
                "push GitHub ignore : projet sans environnement production"
            );
            continue;
        };
        if let Some(skip) = skip_reason(&t.settings, &facts) {
            tracing::info!(project_id = %t.project_id, reason = skip, "push GitHub ignore par les reglages Git");
            reason = skip;
            continue;
        }
        targets.push((t.project_id, env));
    }
    if targets.is_empty() {
        tx.commit()
            .await
            .map_err(|e| internal("transaction du webhook GitHub", e))?;
        return Ok(ignored(reason));
    }
    if !commit_ok(&event.after) {
        return Err(bad_request("Commit invalide."));
    }
    for (project, environment) in &targets {
        jobs::enqueue_in(
            &mut tx,
            JobKind::Deploy,
            json!({
                "project_id": project,
                "environment_id": environment,
                "commit_sha": event.after,
            }),
            Some(format!("{delivery}:{project}")),
        )
        .await
        .map_err(|e| internal("mise en file du deploiement", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| internal("transaction du webhook GitHub", e))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "status": "queued", "jobs": targets.len() })),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_hmac_sha256_of_the_raw_body() {
        let secret = b"s3cret";
        let body = br#"{"zen":"ok"}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(body);
        let hex: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(signature_valid(
            secret,
            body,
            Some(&format!("sha256={hex}"))
        ));
        assert!(signature_valid(
            secret,
            body,
            Some(&format!("sha256={}", hex.to_uppercase()))
        ));
        assert!(!signature_valid(
            b"other",
            body,
            Some(&format!("sha256={hex}"))
        ));
        assert!(!signature_valid(
            secret,
            b"{}",
            Some(&format!("sha256={hex}"))
        ));
        assert!(!signature_valid(secret, body, Some(&hex)));
        assert!(!signature_valid(secret, body, Some("sha256=zz")));
        assert!(!signature_valid(secret, body, None));
    }

    #[test]
    fn delivery_and_commit_formats() {
        assert!(delivery_ok("72d3162e-cc78-11e3-81ab-4c9367dc0958"));
        assert!(!delivery_ok(""));
        assert!(!delivery_ok("a'b"));
        assert!(commit_ok(&"a".repeat(40)));
        assert!(!commit_ok(&"g".repeat(40)));
        assert!(!commit_ok("abc"));
    }
}

#[cfg(test)]
mod freshness_tests {
    use super::*;

    #[test]
    fn window_matches_replay_retention() {
        let now = 1_000_000.0;
        assert!(fresh(Some(now), now));
        assert!(fresh(Some(now - REPLAY_WINDOW_SECS), now));
        assert!(!fresh(Some(now - REPLAY_WINDOW_SECS - 1.0), now));
        assert!(fresh(Some(now + MAX_CLOCK_SKEW_SECS), now));
        assert!(!fresh(Some(now + MAX_CLOCK_SKEW_SECS + 1.0), now));
        assert!(!fresh(None, now));
    }
}
