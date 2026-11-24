use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use shared::config::Config;
use sqlx::PgPool;

use crate::agents::registry::AgentRegistry;
use crate::connectors::github::app::GithubClient;
use crate::domain::domains::DnsResolver;
use crate::domain::domains::panel::PanelSwitch;
use crate::domain::logs::hub::LogHub;
use crate::http::ip_allowlist::IpAllowlist;
use crate::http::update::LatestRelease;
use crate::http::ws_logs::AccessChanges;
use crate::mail::Delivery;
use crate::secrets::SecretBox;

/// Etat partage par tous les handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cfg: Arc<Config>,
    /// Chiffrement des secrets stockes en base (`<config_dir>/age.key`).
    pub secrets: Arc<SecretBox>,
    /// Horloge des echeances calculees par le serveur (invitations) ;
    /// remplacable dans les tests.
    pub clock: Clock,
    /// Envoi des e-mails (SMTP ; remplacable dans les tests).
    pub mail: Delivery,
    /// Agents connectes par le lien QUIC (vide si le lien n'a pas demarre).
    pub agents: AgentRegistry,
    /// Client de l'API GitHub et cache memoire des jetons d'installation.
    pub github: Arc<GithubClient>,
    /// Logs en direct : diffusion aux clients et suivis demandes aux agents.
    pub logs: LogHub,
    /// Resolution DNS de la verification des domaines (remplacable dans les
    /// tests).
    pub dns: Arc<dyn DnsResolver>,
    /// Domaine du panneau (etat en base, charge au demarrage) et ouverture
    /// de l'acces 8443.
    pub panel: PanelSwitch,
    /// Liste d'IP autorisees (memoire relue regulierement en base).
    pub ip_allowlist: IpAllowlist,
    /// Droits d'acces modifies : les flux de logs ouverts se reverifient.
    pub access: AccessChanges,
    /// Derniere version publiee (verification opt-in, en memoire).
    pub releases: LatestRelease,
    /// Dossier surveille par `ciphera-updater.path`
    /// ([`shared::update::REQUEST_DIR`]) : les demandes de mise a jour ne
    /// sont acceptees que si le dossier de donnees est celui-ci.
    pub update_request_dir: std::path::PathBuf,
    /// Limitation de debit en memoire des routes publiques (hooks, webhook).
    pub public_limits: crate::http::public_limit::PublicLimits,
}

impl AppState {
    /// Origine publique du panneau : son domaine confirme, sinon
    /// `CIPHERA_PUBLIC_URL`. Jamais deduite de l'en-tete `Host`.
    pub fn public_url(&self) -> Option<String> {
        self.panel.public_url(&self.cfg)
    }

    /// Lien vers `path` (commence par `/`) : absolu avec une origine
    /// publique, relatif sinon (le panneau le complete).
    pub fn public_link(&self, path: &str) -> String {
        link(self.public_url().as_deref(), path)
    }
}

/// Lien vers `path` depuis l'origine `origin` (relatif sans origine).
pub fn link(origin: Option<&str>, path: &str) -> String {
    match origin {
        Some(origin) => format!("{origin}{path}"),
        None => path.to_string(),
    }
}

/// Horloge injectable : l'heure du systeme, ou une fonction fournie par un test.
#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> SystemTime + Send + Sync>);

impl Clock {
    /// Heure du systeme.
    pub fn system() -> Self {
        Self(Arc::new(SystemTime::now))
    }

    pub fn from_fn(now: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        Self(Arc::new(now))
    }

    pub fn now(&self) -> SystemTime {
        (self.0)()
    }

    /// Secondes depuis l'epoque Unix (avec fraction), pour `to_timestamp()`.
    pub fn unix_secs(&self) -> f64 {
        self.now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64())
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::system()
    }
}

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Clock")
    }
}
