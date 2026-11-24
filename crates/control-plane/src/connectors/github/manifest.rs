//! Flux « manifest » de la GitHub App (owner et admin) :
//!
//! 1. `POST /api/connectors/github/manifest` renvoie le manifest et l'URL du
//!    formulaire GitHub, avec un `state` aleatoire a usage unique (1 h), lie a
//!    la session de l'admin ; seul son SHA-256 est stocke.
//! 2. GitHub renvoie le navigateur sur `/connectors?code=…&state=…` ; le
//!    panneau appelle `GET /api/connectors/github/callback`, qui consomme le
//!    `state` (meme session, non expire) AVANT de convertir le code.
//! 3. La cle privee et le secret de webhook sont chiffres par `SecretBox`.
//!
//! Aussi : `GET /api/connectors/github` (etat, sans secret) et
//! `DELETE /api/connectors/github` (oubli de l'App).

use api_types::github::{GithubManifest, GithubManifestRequest, GithubStatus};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use shared::error::ApiError;
use uuid::Uuid;

use super::app::valid_private_key;
use super::{already_connected, load, not_connected, unavailable};
use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::state::AppState;

/// Duree de vie du `state` du flux manifest (1 h).
pub const STATE_TTL_SECS: f64 = 3600.0;
const MAX_BODY: usize = 4 * 1024;
/// Formulaire GitHub de creation d'App depuis un manifest.
const NEW_APP_URL: &str = "https://github.com/settings/apps/new";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/connectors/github", get(status).delete(disconnect))
        .route("/api/connectors/github/manifest", post(manifest))
        .route("/api/connectors/github/callback", get(callback))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid_state() -> ApiError {
    ApiError::new(
        400,
        "invalid_state",
        "Lien de retour GitHub invalide ou expiré : recommencez la connexion.",
    )
}

fn state_hash(state: &str) -> Vec<u8> {
    Sha256::digest(state.as_bytes()).to_vec()
}

/// Jeton de `[A-Za-z0-9_-]`, 1 a 128 caracteres (code et `state` de GitHub).
fn token_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Slug d'App GitHub : minuscules, chiffres et tirets.
fn slug_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `GET /api/connectors/github`
async fn status(
    State(state): State<AppState>,
    _auth: Require<res::Settings, lvl::Admin>,
) -> Result<Response, ApiError> {
    let app = load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?;
    let status = match app {
        Some(app) => app.status(),
        None => GithubStatus {
            connected: false,
            app: None,
        },
    };
    Ok(Json(status).into_response())
}

/// `POST /api/connectors/github/manifest`
async fn manifest(
    State(state): State<AppState>,
    auth: Require<res::Settings, lvl::Admin>,
    body: Result<Json<GithubManifestRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) =
        body.map_err(|_| ApiError::new(422, "validation_failed", "Requête invalide."))?;
    let origin = match (state.public_url(), input.panel_url.as_deref()) {
        (Some(public), _) => public,
        (None, Some(given)) => shared::config::parse_origin(given).ok_or_else(|| {
            ApiError::new(
                422,
                "validation_failed",
                "Adresse du panneau invalide (https://hôte[:port]).",
            )
        })?,
        (None, None) => {
            return Err(ApiError::new(
                422,
                "validation_failed",
                "Adresse publique du panneau inconnue : définissez CIPHERA_PUBLIC_URL.",
            ));
        }
    };
    if load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?
        .is_some()
    {
        return Err(already_connected());
    }

    let mut raw = [0u8; 32];
    SysRng
        .try_fill_bytes(&mut raw)
        .map_err(|e| internal("generation du state GitHub", e))?;
    let token = URL_SAFE_NO_PAD.encode(raw);
    let mut suffix = [0u8; 3];
    SysRng
        .try_fill_bytes(&mut suffix)
        .map_err(|e| internal("generation du nom de l'App", e))?;
    let now = state.clock.unix_secs();
    sqlx::query("DELETE FROM github_manifest_states WHERE expires_at <= to_timestamp($1::float8)")
        .bind(now)
        .execute(&state.db)
        .await
        .map_err(|e| internal("purge des state GitHub", e))?;
    sqlx::query(
        "INSERT INTO github_manifest_states (state_hash, session_id, expires_at) \
         VALUES ($1, $2, to_timestamp($3::float8))",
    )
    .bind(state_hash(&token))
    .bind(auth.user.session.id)
    .bind(now + STATE_TTL_SECS)
    .execute(&state.db)
    .await
    .map_err(|e| internal("enregistrement du state GitHub", e))?;

    let name: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let manifest = json!({
        "name": format!("Ciphera {name}"),
        "url": origin,
        "hook_attributes": { "url": format!("{origin}/v1/webhooks/github"), "active": true },
        "redirect_url": format!("{origin}/connectors"),
        "setup_url": format!("{origin}/connectors"),
        "public": false,
        "default_permissions": {
            "contents": "read",
            "metadata": "read",
            "pull_requests": "read"
        },
        "default_events": ["push"]
    });
    Ok(Json(GithubManifest {
        manifest,
        post_url: format!("{NEW_APP_URL}?state={token}"),
    })
    .into_response())
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// `GET /api/connectors/github/callback?code=&state=`
async fn callback(
    State(state): State<AppState>,
    auth: Require<res::Settings, lvl::Admin>,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(query) = query.map_err(|_| invalid_state())?;
    let given = query
        .state
        .filter(|s| token_ok(s))
        .ok_or_else(invalid_state)?;
    // Usage unique : consomme meme s'il a expire.
    let expires_at: Option<f64> = sqlx::query_scalar(
        "DELETE FROM github_manifest_states WHERE state_hash = $1 AND session_id = $2 \
         RETURNING extract(epoch FROM expires_at)::float8",
    )
    .bind(state_hash(&given))
    .bind(auth.user.session.id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du state GitHub", e))?;
    if !expires_at.is_some_and(|at| at > state.clock.unix_secs()) {
        return Err(invalid_state());
    }
    let code = query
        .code
        .filter(|c| token_ok(c))
        .ok_or_else(|| ApiError::new(400, "invalid_code", "Code de retour GitHub invalide."))?;
    if load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?
        .is_some()
    {
        return Err(already_connected());
    }

    let app = state
        .github
        .convert_manifest(&state.cfg.github_api_url, &code)
        .await
        .map_err(|e| e.into_api("conversion du manifest GitHub"))?;
    let secret = app
        .webhook_secret
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            tracing::warn!("conversion du manifest GitHub : secret de webhook absent");
            unavailable()
        })?;
    if app.id <= 0
        || !slug_ok(&app.slug)
        || !app.html_url.starts_with("https://")
        || app.name.is_empty()
        || app.owner.login.is_empty()
        || !valid_private_key(app.pem.as_bytes())
    {
        tracing::warn!("conversion du manifest GitHub : reponse inattendue");
        return Err(unavailable());
    }
    let key_sealed = state
        .secrets
        .seal(app.pem.as_bytes())
        .map_err(|e| internal("chiffrement de la cle de la GitHub App", e))?;
    let secret_sealed = state
        .secrets
        .seal(secret.as_bytes())
        .map_err(|e| internal("chiffrement du secret de webhook", e))?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de la GitHub App", e))?;
    let inserted = sqlx::query(
        "INSERT INTO github_app (id, app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed, created_by) \
         VALUES (true, $1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (id) DO NOTHING",
    )
    .bind(app.id)
    .bind(&app.slug)
    .bind(&app.name)
    .bind(&app.owner.login)
    .bind(&app.html_url)
    .bind(&key_sealed)
    .bind(&secret_sealed)
    .bind(auth.user.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("enregistrement de la GitHub App", e))?;
    if inserted.rows_affected() == 0 {
        return Err(already_connected());
    }
    audit(&mut tx, auth.user.id, "github.connect", app.id, &app.slug).await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de la GitHub App", e))?;

    let stored = load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?
        .ok_or_else(not_connected)?;
    Ok(Json(stored.status()).into_response())
}

/// `DELETE /api/connectors/github`
async fn disconnect(
    State(state): State<AppState>,
    auth: Require<res::Settings, lvl::Admin>,
) -> Result<Response, ApiError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de la GitHub App", e))?;
    let removed: Option<(i64, String)> =
        sqlx::query_as("DELETE FROM github_app WHERE id RETURNING app_id, slug")
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| internal("suppression de la GitHub App", e))?;
    let (app_id, slug) = removed.ok_or_else(not_connected)?;
    audit(&mut tx, auth.user.id, "github.disconnect", app_id, &slug).await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de la GitHub App", e))?;
    state.github.forget_tokens();
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn audit(
    conn: &mut sqlx::PgConnection,
    actor: Uuid,
    action: &str,
    app_id: i64,
    slug: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, 'github_app', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(json!({ "app_id": app_id, "slug": slug }))
    .execute(conn)
    .await
    .map_err(|e| internal("audit de la GitHub App", e))?;
    Ok(())
}
