//! GitHub App par instance (flux « manifest ») et webhooks entrants signes.
//!
//! - `manifest` : creation de l'App depuis le panneau (`state` a usage unique
//!   lie a la session de l'admin), etat et oubli de l'App ;
//! - `app` : client de l'API GitHub, JWT RS256 de l'App, jetons
//!   d'installation (memoire seulement) et liste des depots ;
//! - `webhook` : `POST /v1/webhooks/github` (HMAC SHA-256, anti-rejeu, mise
//!   en file des deploiements).
//!
//! La cle privee et le secret de webhook sont chiffres par `SecretBox` et ne
//! sont jamais renvoyes par l'API ni journalises. L'URL de l'API GitHub vient
//! de la configuration (`CIPHERA_GITHUB_API_URL`), jamais d'une requete.

pub mod app;
pub mod manifest;
pub mod webhook;

use axum::Router;
use shared::error::ApiError;
use sqlx::PgPool;

use shared::config::Config;

use crate::secrets::SecretBox;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(manifest::routes())
        .merge(app::routes())
        .merge(webhook::routes())
}

pub(crate) fn not_connected() -> ApiError {
    ApiError::new(404, "github_not_connected", "GitHub n'est pas connecté.")
}

pub(crate) fn already_connected() -> ApiError {
    ApiError::new(
        409,
        "github_already_connected",
        "Une GitHub App est déjà connectée à cette instance.",
    )
}

pub(crate) fn unavailable() -> ApiError {
    ApiError::new(
        502,
        "github_unavailable",
        "GitHub ne répond pas comme prévu. Réessayez.",
    )
}

/// Ligne `github_app` : les secrets restent chiffres.
#[derive(sqlx::FromRow)]
pub(crate) struct StoredApp {
    pub app_id: i64,
    pub slug: String,
    pub name: String,
    pub owner_login: String,
    pub html_url: String,
    pub private_key_sealed: Vec<u8>,
    pub webhook_secret_sealed: Vec<u8>,
}

pub(crate) async fn load(db: &PgPool) -> sqlx::Result<Option<StoredApp>> {
    sqlx::query_as(
        "SELECT app_id, slug, name, owner_login, html_url, private_key_sealed, \
         webhook_secret_sealed FROM github_app WHERE id",
    )
    .fetch_optional(db)
    .await
}

/// Echec du deplacement du webhook de l'App.
#[derive(Debug)]
pub enum WebhookMoveError {
    Db(sqlx::Error),
    /// Cle privee illisible (coffre).
    Key,
    Github(app::GithubError),
}

/// Deplace le webhook de l'App enregistree vers `{origin}/v1/webhooks/github`
/// (`PATCH /app/hook/config`, JWT de l'App). Renvoie l'URL posee, ou `None`
/// sans App.
pub async fn move_webhook(
    db: &PgPool,
    cfg: &Config,
    secrets: &SecretBox,
    client: &app::GithubClient,
    now: i64,
    origin: &str,
) -> Result<Option<String>, WebhookMoveError> {
    let Some(stored) = load(db).await.map_err(WebhookMoveError::Db)? else {
        return Ok(None);
    };
    let pem = secrets
        .open(&stored.private_key_sealed)
        .map_err(|_| WebhookMoveError::Key)?;
    let url = format!("{origin}/v1/webhooks/github");
    client
        .set_webhook_url(&cfg.github_api_url, stored.app_id, &pem, now, &url)
        .await
        .map_err(WebhookMoveError::Github)?;
    Ok(Some(url))
}

/// Origine de l'acces direct au panneau (8443) : `CIPHERA_PUBLIC_URL`, sinon
/// `https://<IP publique>:<port du panneau>`.
pub fn direct_origin(cfg: &Config) -> Option<String> {
    if let Some(url) = &cfg.public_url {
        return Some(url.trim_end_matches('/').to_string());
    }
    let (v4, v6) = cfg.node_ips();
    match (v4, v6) {
        (Some(ip), _) => Some(format!("https://{ip}:{}", cfg.panel_port)),
        (None, Some(ip)) => Some(format!("https://[{ip}]:{}", cfg.panel_port)),
        (None, None) => None,
    }
}

/// Page d'installation de l'App (le slug est valide a l'enregistrement).
pub(crate) fn install_url(slug: &str) -> String {
    format!("https://github.com/apps/{slug}/installations/new")
}

impl StoredApp {
    pub(crate) fn status(&self) -> api_types::github::GithubStatus {
        api_types::github::GithubStatus {
            connected: true,
            app: Some(api_types::github::GithubAppInfo {
                app_id: self.app_id,
                slug: self.slug.clone(),
                name: self.name.clone(),
                owner: self.owner_login.clone(),
                html_url: self.html_url.clone(),
                install_url: install_url(&self.slug),
            }),
        }
    }
}
