//! `ciphera admin reset-password --email <e>` : dernier recours quand aucun
//! administrateur ne peut se connecter. Cree un lien de reinitialisation
//! (30 minutes, usage unique, liens precedents remplaces) et l'imprime ; rien
//! n'est journalise (ni jeton ni e-mail). Meme base et meme configuration
//! que le serveur.
//!
//! `ciphera admin reset-panel-domain` : retire le domaine du panneau quand il
//! ne repond plus (DNS change, certificat perdu). L'acces 8443 se rouvre au
//! redemarrage du service. Domaine confirme avec une GitHub App : son webhook
//! revient sur l'acces direct (8443) ; un echec est signale (a refaire a la
//! main sur GitHub) sans empecher le retrait.
//!
//! `ciphera admin clear-ip-allowlist` : vide la liste d'IP autorisees quand
//! l'adresse de l'administrateur a change. Le service la relit en quelques
//! secondes, sans redemarrage.

use std::fmt;

use serde_json::json;
use shared::config::Config;
use sqlx::PgPool;

use crate::connectors::github::{self, app::GithubClient};
use crate::domain::domains::panel::{self, PanelSwitch};
use crate::http::password_reset::{self, audit};
use crate::secrets::SecretBox;
use crate::state::Clock;

#[derive(Debug)]
pub struct AdminError(String);

impl fmt::Display for AdminError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AdminError {}

fn database(error: sqlx::Error) -> AdminError {
    AdminError(format!("base de donnees : {error}"))
}

/// Lien de reinitialisation pour le compte de cet e-mail (casse ignoree) ;
/// absolu avec le domaine confirme du panneau ou `CIPHERA_PUBLIC_URL`.
pub async fn reset_password(db: &PgPool, cfg: &Config, email: &str) -> Result<String, AdminError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(AdminError("e-mail manquant".into()));
    }
    let mut tx = db.begin().await.map_err(database)?;
    let issued = password_reset::issue(&mut tx, email, Clock::system().unix_secs(), 0.0)
        .await
        .map_err(database)?
        .ok_or_else(|| AdminError("aucun compte pour cet e-mail".into()))?;
    audit(
        &mut tx,
        None,
        "auth.password.reset_link.cli",
        json!({ "user_id": issued.user_id }),
    )
    .await
    .map_err(database)?;
    tx.commit().await.map_err(database)?;
    let public_url = PanelSwitch::load(db)
        .await
        .map_err(database)?
        .public_url(cfg);
    Ok(password_reset::link(
        public_url.as_deref(),
        issued.token.expose(),
    ))
}

/// Webhook de la GitHub App au retrait du domaine du panneau.
#[derive(Debug, PartialEq, Eq)]
pub enum WebhookMove {
    /// Rien a deplacer (domaine non confirme ou pas d'App).
    Unchanged,
    /// Webhook ramene sur cette URL.
    Moved(String),
    /// Echec : a corriger a la main sur GitHub, vers cette URL (si connue).
    Failed(Option<String>),
}

/// Issue de `reset-panel-domain`.
#[derive(Debug)]
pub struct PanelReset {
    /// Hote retire, ou `None` s'il n'y en avait pas.
    pub removed: Option<String>,
    pub github: WebhookMove,
}

/// Retire le domaine du panneau (audite). Domaine confirme : le webhook de
/// la GitHub App revient d'abord sur l'acces direct. Commande de secours :
/// sans cle (`age.key`) ni client GitHub (`None`), ou si GitHub refuse, le
/// domaine est retire quand meme et `github` dit quoi corriger a la main.
pub async fn reset_panel_domain(
    db: &PgPool,
    cfg: &Config,
    secrets: Option<&SecretBox>,
    client: Option<&GithubClient>,
) -> Result<PanelReset, AdminError> {
    let confirmed: Option<bool> =
        sqlx::query_scalar("SELECT panel_confirmed_at IS NOT NULL FROM domains WHERE panel")
            .fetch_optional(db)
            .await
            .map_err(database)?;
    let origin = github::direct_origin(cfg);
    let github = match (confirmed, &origin) {
        (Some(true), Some(origin)) => match (secrets, client) {
            (Some(secrets), Some(client)) => match github::move_webhook(
                db,
                cfg,
                secrets,
                client,
                Clock::system().unix_secs() as i64,
                origin,
            )
            .await
            {
                Ok(Some(url)) => WebhookMove::Moved(url),
                Ok(None) => WebhookMove::Unchanged,
                Err(_) => WebhookMove::Failed(Some(format!("{origin}/v1/webhooks/github"))),
            },
            // Cle ou client indisponible : App presente, webhook a deplacer
            // a la main.
            _ => match github::load(db).await.map_err(database)? {
                Some(_) => WebhookMove::Failed(Some(format!("{origin}/v1/webhooks/github"))),
                None => WebhookMove::Unchanged,
            },
        },
        (Some(true), None) => match github::load(db).await.map_err(database)? {
            Some(_) => WebhookMove::Failed(None),
            None => WebhookMove::Unchanged,
        },
        _ => WebhookMove::Unchanged,
    };
    let moved = matches!(github, WebhookMove::Moved(_));
    let remove = async {
        let mut tx = db.begin().await?;
        let removed = panel::remove(&mut tx).await?;
        if let Some(host) = &removed {
            panel::audit(
                &mut tx,
                None,
                "panel_domain.reset.cli",
                json!({ "domain": host, "github_webhook_moved": moved }),
            )
            .await?;
        }
        tx.commit().await?;
        Ok::<_, sqlx::Error>(removed)
    };
    match remove.await {
        Ok(removed) => Ok(PanelReset { removed, github }),
        // Webhook deja ramene sur 8443 mais domaine toujours actif : le dire.
        Err(e) if moved => Err(AdminError(format!(
            "base de donnees : {e} ; attention : le webhook de la GitHub App pointe deja sur l'acces direct alors que le domaine du panneau est toujours actif : relancez la commande"
        ))),
        Err(e) => Err(database(e)),
    }
}

/// Vide la liste d'IP autorisees (auditee). Renvoie le nombre d'entrees
/// retirees.
pub async fn clear_ip_allowlist(db: &PgPool) -> Result<u64, AdminError> {
    let mut tx = db.begin().await.map_err(database)?;
    let entries: Option<i32> =
        sqlx::query_scalar("SELECT cardinality(ip_allowlist) FROM instance LIMIT 1 FOR UPDATE")
            .fetch_optional(&mut *tx)
            .await
            .map_err(database)?;
    let entries = u64::try_from(entries.unwrap_or(0)).unwrap_or(0);
    if entries > 0 {
        sqlx::query("UPDATE instance SET ip_allowlist = '{}'")
            .execute(&mut *tx)
            .await
            .map_err(database)?;
        sqlx::query(
            "INSERT INTO audit_events (id, actor_id, action, target, detail) \
             VALUES ($1, NULL, 'settings.security.ip_allowlist.clear.cli', 'settings', $2)",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(json!({ "entries": entries }))
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    }
    tx.commit().await.map_err(database)?;
    Ok(entries)
}
