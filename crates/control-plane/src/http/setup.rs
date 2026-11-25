//! Assistant d'installation : `/api/setup/*`.
//!
//! Le jeton de setup (`<config_dir>/setup_token`, ecrit par `ciphera init` ou
//! `ciphera admin setup-token`, voir `crate::setup_token`) est a usage unique
//! et expire 24 heures apres son emission (401 `setup_token_expired`). Il
//! cesse d'etre valable des qu'un compte existe, meme si le fichier traine
//! encore, et le fichier est supprime apres la creation du proprietaire. La creation est une transaction SERIALIZABLE gardee par
//! `INSERT ... WHERE NOT EXISTS (SELECT 1 FROM users)` : en cas de course,
//! un seul appel gagne, les autres recoivent 410.
//!
//! Ces routes sont exemptees de CSRF : `X-Setup-Token` est la preuve.

use std::path::Path;

use api_types::setup::{
    SetupCheck, SetupChecks, SetupComplete, SetupCompleted, SetupStatus, SetupUser,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use sha2::{Digest, Sha256};
use shared::error::ApiError;
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use zeroize::Zeroizing;

use crate::auth::password;
use crate::auth::session::{self, SessionToken};
use crate::setup_token::{self, FILE as TOKEN_FILE};
use crate::state::AppState;

const TOKEN_HEADER: &str = "x-setup-token";
/// Corps JSON maximal : bien au-dessus d'un formulaire legitime (mot de passe de 1024 caracteres).
const MAX_BODY: usize = 16 * 1024;
const MIN_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const DOCKER_SOCKET: &str = "/var/run/docker.sock";
/// Tentatives en cas d'echec de serialisation (les perdants finissent en 410).
const MAX_ATTEMPTS: u64 = 10;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/setup/status", get(status))
        .route("/api/setup/checks", post(checks))
        .route("/api/setup/complete", post(complete))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn gone() -> ApiError {
    ApiError::new(410, "setup_closed", "L'installation est déjà terminée.")
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn internal(context: &str, error: impl std::fmt::Display) -> ApiError {
    // Le detail va dans le journal du serveur, jamais dans la reponse.
    tracing::error!(%error, "{context}");
    ApiError::new(500, "internal_error", "Erreur interne du serveur.")
}

async fn has_users(db: &PgPool) -> Result<bool, ApiError> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(db)
        .await
        .map_err(|e| internal("lecture des comptes", e))
}

/// `GET /api/setup/status`
async fn status(State(state): State<AppState>) -> Result<Json<SetupStatus>, ApiError> {
    Ok(Json(SetupStatus {
        initialized: has_users(&state.db).await?,
    }))
}

/// Verifie l'etat de l'assistant et le jeton `X-Setup-Token`.
async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    // Verrou : tout compte existant ferme l'assistant, jeton valide ou non.
    if has_users(&state.db).await? {
        return Err(gone());
    }
    let path = state.cfg.config_dir.join(TOKEN_FILE);
    let read = async {
        let text = Zeroizing::new(tokio::fs::read_to_string(&path).await?);
        let modified = tokio::fs::metadata(&path).await?.modified().ok();
        Ok::<_, std::io::Error>((text, modified))
    };
    let (expected, expires_at) = match read.await {
        Ok((text, modified)) => match setup_token::parse(&text) {
            Some(stored) => {
                let expires_at = setup_token::effective_expiry(&stored, modified);
                (stored.token, expires_at)
            }
            None => return Err(missing_token()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Le fichier disparait juste apres la creation du proprietaire :
            // un perdant de la course doit recevoir 410, pas 503.
            if has_users(&state.db).await? {
                return Err(gone());
            }
            return Err(missing_token());
        }
        Err(e) => return Err(internal("lecture du jeton de setup", e)),
    };
    let given = headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    // Comparaison a temps constant des empreintes (longueurs egales, rien ne fuit).
    let same = Sha256::digest(given.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes()));
    if given.is_empty() || !bool::from(same) {
        return Err(ApiError::new(
            401,
            "invalid_setup_token",
            "Jeton d'installation invalide.",
        ));
    }
    // Expiration verifiee apres le jeton : seul son detenteur l'apprend.
    if setup_token::unix_now() >= expires_at {
        return Err(ApiError::new(
            401,
            "setup_token_expired",
            "Lien d'installation expiré (24 h) : lancez `sudo ciphera admin setup-token` sur le serveur pour en obtenir un nouveau.",
        ));
    }
    Ok(())
}

fn missing_token() -> ApiError {
    ApiError::new(
        503,
        "setup_token_missing",
        "Jeton d'installation introuvable : lancez `sudo ciphera admin setup-token` sur le serveur.",
    )
}

/// `POST /api/setup/checks`
async fn checks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SetupChecks>, ApiError> {
    authorize(&state, &headers).await?;

    let database =
        match sqlx::query_scalar::<_, bool>("SELECT to_regclass('public.users') IS NOT NULL")
            .fetch_one(&state.db)
            .await
        {
            Ok(true) => check(
                "database",
                true,
                false,
                "Base de données joignable, migrations appliquées.",
            ),
            Ok(false) => check(
                "database",
                false,
                false,
                "Base de données joignable, mais migrations non appliquées.",
            ),
            Err(e) => {
                tracing::warn!(error = %e, "verification de la base");
                check("database", false, false, "Base de données injoignable.")
            }
        };

    let tls_path = state.cfg.config_dir.join("tls").join("panel.crt");
    let tls = if tls_path.is_file() {
        check("tls", true, false, "Certificat HTTPS du panneau présent.")
    } else {
        check(
            "tls",
            false,
            false,
            "Certificat HTTPS du panneau introuvable : relancez `ciphera init`.",
        )
    };

    let data_dir = state.cfg.data_dir.clone();
    let data = tokio::task::spawn_blocking(move || check_data_dir(&data_dir))
        .await
        .map_err(|e| internal("verification du dossier de données", e))?;

    let docker = if Path::new(DOCKER_SOCKET).exists() {
        check("docker", true, false, "Docker détecté.")
    } else {
        check(
            "docker",
            true,
            true,
            "Docker introuvable (/var/run/docker.sock) : il sera nécessaire pour déployer des projets.",
        )
    };

    Ok(Json(SetupChecks {
        checks: vec![database, tls, data, docker],
    }))
}

fn check(id: &str, ok: bool, warning: bool, detail: &str) -> SetupCheck {
    SetupCheck {
        id: id.to_string(),
        ok,
        warning,
        detail: detail.to_string(),
    }
}

fn check_data_dir(dir: &Path) -> SetupCheck {
    if !dir.is_dir() {
        return check("data_dir", false, false, "Dossier de données introuvable.");
    }
    let probe = dir.join(format!(".ciphera-write-test-{}", Uuid::new_v4().simple()));
    let writable = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    let _ = std::fs::remove_file(&probe);
    if !writable {
        return check(
            "data_dir",
            false,
            false,
            "Dossier de données non accessible en écriture.",
        );
    }
    match free_bytes(dir) {
        Some(free) if free < MIN_FREE_BYTES => check(
            "data_dir",
            true,
            true,
            &format!(
                "Espace disque faible : {} Go libres (5 Go recommandés).",
                free / (1024 * 1024 * 1024)
            ),
        ),
        Some(free) => check(
            "data_dir",
            true,
            false,
            &format!(
                "Dossier de données accessible en écriture, {} Go libres.",
                free / (1024 * 1024 * 1024)
            ),
        ),
        None => check(
            "data_dir",
            true,
            false,
            "Dossier de données accessible en écriture.",
        ),
    }
}

#[cfg(unix)]
fn free_bytes(dir: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(dir).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

#[cfg(not(unix))]
fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// Champs valides, prets a etre ecrits.
struct Validated {
    name: String,
    email: String,
    password_hash: String,
    org_name: String,
    slug: String,
    timezone: String,
    language: String,
}

pub(crate) fn validate_email(email: &str) -> bool {
    if email.len() > 254 || email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && !domain.contains('@')
        && domain.contains('.')
        && domain.split('.').all(|label| !label.is_empty())
}

pub(crate) fn validate_slug(slug: &str) -> bool {
    let bytes = slug.as_bytes();
    (1..=50).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

fn validate_timezone(tz: &str) -> bool {
    (1..=64).contains(&tz.len())
        && tz.split('/').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-'))
        })
}

fn validate(input: SetupComplete) -> Result<(Zeroizing<String>, Validated), ApiError> {
    let name = input.admin.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(invalid("Nom : 1 à 200 caractères."));
    }
    let email = input.admin.email.trim().to_string();
    if !validate_email(&email) {
        return Err(invalid("E-mail invalide."));
    }
    if let Err(message) = password::check_policy(&input.admin.password) {
        return Err(invalid(message));
    }
    let org_name = input.organization.name.trim().to_string();
    if org_name.is_empty() || org_name.chars().count() > 100 {
        return Err(invalid("Nom de l'organisation : 1 à 100 caractères."));
    }
    let slug = input.organization.slug;
    if !validate_slug(&slug) {
        return Err(invalid(
            "Identifiant de l'organisation invalide (minuscules, chiffres et tirets).",
        ));
    }
    let timezone = input.organization.timezone;
    if !validate_timezone(&timezone) {
        return Err(invalid("Fuseau horaire invalide."));
    }
    let language = input.organization.language;
    if !matches!(language.as_str(), "fr" | "en") {
        return Err(invalid("Langue non prise en charge."));
    }
    Ok((
        input.admin.password,
        Validated {
            name,
            email,
            password_hash: String::new(),
            org_name,
            slug,
            timezone,
            language,
        },
    ))
}

fn retryable(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01" | "23505"))
}

/// Une tentative. `Ok(None)` : un compte existe deja.
async fn try_create(
    db: &PgPool,
    v: &Validated,
    token: &SessionToken,
) -> Result<Option<Uuid>, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await?;
    let user_id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) \
         SELECT $1, $2, $3, 'owner', $4, now() WHERE NOT EXISTS (SELECT 1 FROM users)",
    )
    .bind(user_id)
    .bind(&v.email)
    .bind(&v.password_hash)
    .bind(&v.name)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        tx.rollback().await?;
        return Ok(None);
    }
    sqlx::query(
        "INSERT INTO instance (id, name, slug, timezone, language) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT ((true)) DO UPDATE \
         SET name = EXCLUDED.name, slug = EXCLUDED.slug, \
             timezone = EXCLUDED.timezone, language = EXCLUDED.language",
    )
    .bind(Uuid::new_v4())
    .bind(&v.org_name)
    .bind(&v.slug)
    .bind(&v.timezone)
    .bind(&v.language)
    .execute(&mut *tx)
    .await?;
    session::create(&mut tx, user_id, token, session::NewSession::default()).await?;
    // Ni mot de passe ni e-mail dans le journal d'audit.
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, $2, 'setup.completed', 'instance', $3)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(json!({ "organization": v.slug }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(user_id))
}

/// `POST /api/setup/complete`
async fn complete(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<SetupComplete>, JsonRejection>,
) -> Result<Response, ApiError> {
    authorize(&state, &headers).await?;
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let (plain_password, mut valid) = validate(input)?;

    // Argon2id coute ~50 ms et 19 Mio : hors du thread asynchrone.
    valid.password_hash = password::offload(move || password::hash(&plain_password))
        .await
        .map_err(|e| e.into_api("hachage du mot de passe"))?
        .map_err(|e| internal("hachage du mot de passe", e))?;

    let token = SessionToken::generate().map_err(|e| internal("jeton de session", e))?;

    let mut created = None;
    for attempt in 0..MAX_ATTEMPTS {
        // Laisse au gagnant le temps de valider avant de reessayer.
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(25 * attempt)).await;
        }
        match try_create(&state.db, &valid, &token).await {
            Ok(result) => {
                created = Some(result);
                break;
            }
            Err(e) if retryable(&e) => continue,
            Err(e) => return Err(internal("creation du proprietaire", e)),
        }
    }
    let user_id = match created {
        Some(Some(id)) => id,
        Some(None) => return Err(gone()),
        // Course persistante : si un compte existe, c'est un perdant.
        None if has_users(&state.db).await? => return Err(gone()),
        None => {
            return Err(ApiError::new(
                503,
                "setup_busy",
                "Installation momentanément indisponible, réessayez.",
            ));
        }
    };

    // Le compte existe deja : meme si la suppression echoue, le jeton est mort.
    if let Err(e) = tokio::fs::remove_file(state.cfg.config_dir.join(TOKEN_FILE)).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %e, "suppression du jeton de setup");
    }

    let body = SetupCompleted {
        user: SetupUser {
            id: user_id.to_string(),
            name: valid.name,
            email: valid.email,
            role: "owner".to_string(),
        },
    };
    Ok((
        StatusCode::CREATED,
        [
            (header::SET_COOKIE, token.set_cookie(false)),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        Json(body),
    )
        .into_response())
}
