//! `ciphera admin reset-password --email <e>` : dernier recours quand aucun
//! administrateur ne peut se connecter. Cree un lien de reinitialisation
//! (30 minutes, usage unique, liens precedents remplaces) et l'imprime ; rien
//! n'est journalise (ni jeton ni e-mail). Meme base et meme configuration
//! que le serveur.
//!
//! `ciphera admin reset-mfa <email>` : supprime le second facteur du compte
//! (TOTP, passkeys, codes de secours) et revoque ses sessions ; audite sans
//! e-mail ni secret.
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
//!
//! `ciphera admin setup-token` : nouveau lien de l'assistant d'installation
//! (24 heures) tant qu'aucun compte n'existe (premier lien expire ou perdu).
//!
//! `ciphera admin revoke-agent <id>` : revoque un serveur (`revoked_at`,
//! definitif). Le service ferme sa connexion en quelques secondes
//! (`agents::transport::watch_revocations`, qui appelle
//! `AgentRegistry::disconnect`) et refuse ses poignees de main suivantes.

use std::fmt;
use std::path::Path;

use serde_json::json;
use shared::config::Config;
use shared::ids::ServerId;
use sqlx::PgPool;
use zeroize::Zeroizing;

use crate::connectors::github::{self, app::GithubClient};
use crate::domain::domains::panel::{self, PanelSwitch};
use crate::http::password_reset::{self, Origin, audit};
use crate::secrets::SecretBox;
use crate::setup_token;
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
    let issued = password_reset::issue(
        &mut tx,
        email,
        Clock::system().unix_secs(),
        0.0,
        Origin::Cli,
    )
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

/// Second facteur supprime par `reset-mfa`.
#[derive(Debug, PartialEq, Eq)]
pub struct MfaReset {
    pub totp: u64,
    pub passkeys: u64,
    pub recovery_codes: u64,
    pub sessions_revoked: u64,
}

/// Remet a zero le second facteur du compte de cet e-mail (casse ignoree) et
/// revoque ses sessions ; commande de secours (root, sur le serveur).
pub async fn reset_mfa(db: &PgPool, email: &str) -> Result<MfaReset, AdminError> {
    let email = email.trim();
    if email.is_empty() {
        return Err(AdminError("e-mail manquant".into()));
    }
    let mut tx = db.begin().await.map_err(database)?;
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE lower(email) = lower($1)")
        .bind(email)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database)?
        .ok_or_else(|| AdminError("aucun compte pour cet e-mail".into()))?;
    let mut removed = [0_u64; 4];
    for (slot, sql) in [
        "DELETE FROM user_totp WHERE user_id = $1",
        "DELETE FROM webauthn_credentials WHERE user_id = $1",
        "DELETE FROM recovery_codes WHERE user_id = $1",
        "DELETE FROM sessions WHERE user_id = $1",
    ]
    .into_iter()
    .enumerate()
    {
        removed[slot] = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(database)?
            .rows_affected();
    }
    crate::auth::rate_limit::clear_mfa(&mut tx, id)
        .await
        .map_err(database)?;
    let report = MfaReset {
        totp: removed[0],
        passkeys: removed[1],
        recovery_codes: removed[2],
        sessions_revoked: removed[3],
    };
    audit(
        &mut tx,
        None,
        "auth.mfa.reset.cli",
        json!({
            "user_id": id,
            "totp": report.totp,
            "passkeys": report.passkeys,
            "recovery_codes": report.recovery_codes,
            "sessions_revoked": report.sessions_revoked,
        }),
    )
    .await
    .map_err(database)?;
    tx.commit().await.map_err(database)?;
    Ok(report)
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

/// Nouveau jeton de l'assistant d'installation (24 heures), tant qu'aucun
/// compte n'existe ; le precedent cesse aussitot d'etre valable. Le fichier
/// est donne a `owner` (uid, gid du compte du service) s'il est fourni.
/// Audite, sans le jeton.
///
/// Si un compte existe (avant, ou cree pendant l'ecriture), le fichier
/// `setup_token` restant est supprime : il ne vaut plus rien (l'assistant
/// verifie les comptes d'abord), et le service, qui ne peut pas toujours
/// ecrire dans son dossier de configuration, a pu le laisser.
pub async fn setup_token(
    db: &PgPool,
    config_dir: &Path,
    owner: Option<(u32, u32)>,
) -> Result<Zeroizing<String>, AdminError> {
    let closed = || {
        AdminError(
            "un compte existe deja : l'assistant d'installation est ferme (connexion ou reset-password)"
                .into(),
        )
    };
    if has_accounts(db).await? {
        remove_stale_setup_token(config_dir)?;
        return Err(closed());
    }
    let token = setup_token::replace(config_dir, owner, setup_token::unix_now())
        .map_err(|e| AdminError(format!("ecriture du jeton : {e}")))?;
    // Assistant termine pendant l'ecriture : ce jeton neuf est deja mort.
    if has_accounts(db).await? {
        remove_stale_setup_token(config_dir)?;
        return Err(closed());
    }
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail)          VALUES ($1, NULL, 'setup.token.cli', 'instance', '{}'::jsonb)",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(db)
    .await
    .map_err(database)?;
    Ok(token)
}

async fn has_accounts(db: &PgPool) -> Result<bool, AdminError> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users)")
        .fetch_one(db)
        .await
        .map_err(database)
}

/// Supprime `<config_dir>/setup_token` s'il existe.
fn remove_stale_setup_token(config_dir: &Path) -> Result<(), AdminError> {
    match std::fs::remove_file(config_dir.join(setup_token::FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(AdminError(format!(
            "suppression de l'ancien jeton d'installation : {e}"
        ))),
        _ => Ok(()),
    }
}

/// Issue de `revoke-agent`.
#[derive(Debug, PartialEq, Eq)]
pub enum AgentRevoke {
    /// `revoked_at` pose maintenant.
    Revoked,
    /// Deja revoque : date d'origine conservee.
    AlreadyRevoked,
}

/// Revoque le serveur `id` (auditee) : `revoked_at` pose s'il ne l'etait
/// pas. Le service en cours le deconnecte en quelques secondes et refuse
/// ses reconnexions (`agents::transport`).
pub async fn revoke_agent(db: &PgPool, id: ServerId) -> Result<AgentRevoke, AdminError> {
    let mut tx = db.begin().await.map_err(database)?;
    let revoked: Option<bool> =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM servers WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(database)?;
    match revoked {
        None => return Err(AdminError(format!("aucun serveur {id}"))),
        Some(true) => return Ok(AgentRevoke::AlreadyRevoked),
        Some(false) => {}
    }
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(database)?;
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, NULL, 'server.revoke.cli', 'server', $2)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(json!({ "server_id": id }))
    .execute(&mut *tx)
    .await
    .map_err(database)?;
    tx.commit().await.map_err(database)?;
    Ok(AgentRevoke::Revoked)
}
