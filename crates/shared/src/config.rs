//! Configuration du control-plane, lue depuis les variables `CIPHERA_*`.
//!
//! | Variable                     | Defaut           |
//! |------------------------------|------------------|
//! | `CIPHERA_PANEL_PORT`         | `8443` (HTTPS)   |
//! | `CIPHERA_HTTP_PORT`          | facultatif : port HTTP qui redirige vers HTTPS (aucun par defaut : le port 80 appartient a `ciphera-proxy`, qui redirige deja) |
//! | `CIPHERA_PANEL_LOCAL_PORT`   | `8444` (HTTP sur la boucle locale : le proxy y sert le domaine du panneau) |
//! | `CIPHERA_AGENT_LINK_PORT`    | `7443` (UDP, lien QUIC + mTLS avec les agents) |
//! | `CIPHERA_AGENT_LINK_BIND`    | `127.0.0.1` (adresse IP d'ecoute du lien QUIC) |
//! | `CIPHERA_CONFIG_DIR`         | `/etc/ciphera`   |
//! | `CIPHERA_DATA_DIR`           | `/var/lib/ciphera` |
//! | `CIPHERA_DATABASE_URL`       | facultatif : URL complete, prioritaire |
//! | `CIPHERA_DATABASE_URL_FILE`  | chemin d'un fichier contenant l'URL (evite l'URL en clair dans l'environnement) |
//! | `CIPHERA_DB_SOCKET_DIR`      | facultatif : dossier du socket Unix de PostgreSQL (sans URL ; prioritaire sur l'hote) |
//! | `CIPHERA_DB_HOST`            | `127.0.0.1` (sans URL) |
//! | `CIPHERA_DB_PORT`            | `5432` (sans URL) |
//! | `CIPHERA_DB_USER`            | `ciphera` (sans URL) |
//! | `CIPHERA_DB_NAME`            | `ciphera` (sans URL) |
//! | `CIPHERA_DB_PASSWORD_FILE`   | `<CIPHERA_CONFIG_DIR>/db_password` (sans URL) |
//! | `CIPHERA_WEBAUTHN_RP_ID`     | facultatif : nom de domaine de l'origine publique (domaine du panneau confirme, sinon `CIPHERA_PUBLIC_URL`), a defaut hote du panneau (en-tete `Host`) |
//! | `CIPHERA_WEBAUTHN_RP_ORIGIN` | facultatif : `https://<RP ID>[:port du panneau]` |
//! | `CIPHERA_PUBLIC_URL`         | facultatif : origine publique du panneau (`https://hote[:port]`, `http` pour localhost seulement) ; liens envoyes par e-mail |
//! | `CIPHERA_GITHUB_API_URL`     | `https://api.github.com` (origine de l'API GitHub ; `http` pour localhost seulement, tests) |
//! | `CIPHERA_PUBLIC_IP`          | facultatif : IP publique du noeud (verification DNS des domaines, domaine genere `<projet>.<ip>.sslip.io`) ; sans valeur, l'hote de `CIPHERA_PUBLIC_URL` s'il est une IP, sinon l'IP du certificat du panneau (`ciphera init --public-ip`) |
//! | `CIPHERA_PUBLIC_IP6`         | facultatif : IPv6 publique du noeud (enregistrements AAAA des domaines) quand `CIPHERA_PUBLIC_IP` est une IPv4 |
//! | `CIPHERA_PANEL_ALIASES`      | facultatif : autres noms DNS du panneau, separes par des virgules, reserves comme son domaine (`CIPHERA_PUBLIC_URL`, `CIPHERA_WEBAUTHN_RP_ID`) ; l'acces direct (8443) ne sert que les IP, `localhost` et ces noms reserves (421 sinon) |
//! | `CIPHERA_UPDATE_CHECK`       | `0` : verification de la derniere version desactivee (`1` pour l'activer ; aucune telemetrie) |
//! | `CIPHERA_RELEASE_BASE_URL`   | `https://get.ciphera.ch` (origine des releases lue par la verification ; `http` pour localhost seulement, tests) |
//!
//! Sans URL, la connexion est construite champ par champ : le mot de passe
//! (base64, avec `+`, `/`, `=`) n'est jamais place dans une URL ni journalise.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

/// URL de connexion a la base. Son `Debug` masque le mot de passe.
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// URL complete, mot de passe compris : a ne passer qu'au pilote de base.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// URL dont le mot de passe est remplace par `***`.
    pub fn redacted(&self) -> String {
        let Some((scheme, rest)) = self.0.split_once("://") else {
            return "***".to_string();
        };
        let Some((userinfo, host)) = rest.rsplit_once('@') else {
            return self.0.clone();
        };
        match userinfo.split_once(':') {
            Some((user, _)) => format!("{scheme}://{user}:***@{host}"),
            None => self.0.clone(),
        }
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DatabaseUrl({})", self.redacted())
    }
}

/// Mot de passe de la base. Son `Debug` ne l'affiche jamais.
#[derive(Clone, PartialEq, Eq)]
pub struct DbPassword(String);

impl DbPassword {
    /// Mot de passe en clair : a ne passer qu'au pilote de base.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DbPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Connexion construite champ par champ (pas d'URL, donc pas d'encodage).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatabaseParams {
    /// Dossier du socket Unix (`<dossier>/.s.PGSQL.<port>`) ; s'il est
    /// defini, `host` n'est pas utilise.
    pub socket_dir: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub name: String,
    password: DbPassword,
}

impl DatabaseParams {
    pub fn password(&self) -> &DbPassword {
        &self.password
    }
}

/// Source de la connexion a la base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Database {
    /// `CIPHERA_DATABASE_URL` ou `CIPHERA_DATABASE_URL_FILE`.
    Url(DatabaseUrl),
    /// Variables `CIPHERA_DB_*` et fichier de mot de passe.
    Params(DatabaseParams),
}

#[cfg(feature = "sqlx")]
impl Database {
    /// Options de connexion PostgreSQL. L'erreur ne reprend jamais l'URL.
    /// Attention : le `Debug` de `PgConnectOptions` affiche le mot de passe,
    /// ne jamais le journaliser.
    pub fn connect_options(&self) -> Result<sqlx::postgres::PgConnectOptions, ConfigError> {
        use sqlx::postgres::PgConnectOptions;
        match self {
            Self::Url(url) => url
                .expose()
                .parse::<PgConnectOptions>()
                .map_err(|_| ConfigError::Invalid("CIPHERA_DATABASE_URL")),
            // Sans .pgpass ni variables PG* : seule la configuration Ciphera compte.
            Self::Params(p) => {
                let opts = PgConnectOptions::new_without_pgpass()
                    .host(&p.host)
                    .port(p.port)
                    .username(&p.user)
                    .database(&p.name)
                    .password(p.password.expose());
                Ok(match &p.socket_dir {
                    Some(dir) => opts.socket(dir),
                    None => opts,
                })
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// URL de base vide.
    MissingDatabaseUrl,
    /// Valeur invalide pour la variable nommee (la valeur n'est pas reprise).
    Invalid(&'static str),
    /// Fichier illisible pour la variable nommee.
    UnreadableFile(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDatabaseUrl => {
                write!(
                    f,
                    "CIPHERA_DATABASE_URL ou CIPHERA_DATABASE_URL_FILE est vide"
                )
            }
            Self::Invalid(name) => write!(f, "valeur invalide pour {name}"),
            Self::UnreadableFile(name) => write!(f, "fichier illisible pour {name}"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Port HTTPS du panneau.
    pub panel_port: u16,
    /// Port HTTP qui redirige vers HTTPS ; aucun par defaut (le proxy du
    /// noeud tient le port 80 et redirige deja vers le panneau).
    pub http_port: Option<u16>,
    /// Port HTTP du panneau sur la boucle locale (127.0.0.1), joint par le
    /// proxy quand un domaine du panneau est configure.
    pub panel_local_port: u16,
    /// Port UDP du lien QUIC avec les agents.
    pub agent_link_port: u16,
    /// Adresse d'ecoute du lien QUIC (boucle locale par defaut en v0.1 :
    /// seul l'agent local s'y connecte).
    pub agent_link_bind: IpAddr,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub database: Database,
    /// RP ID WebAuthn impose ; sans valeur, l'hote par lequel le panneau est joint.
    pub webauthn_rp_id: Option<String>,
    /// Origine WebAuthn imposee (ex. `https://panel.example.com`).
    pub webauthn_rp_origin: Option<String>,
    /// Origine publique du panneau, sans `/` final (ex. `https://panel.example.com`) :
    /// prefixe des liens d'invitation et de reinitialisation. Jamais deduite
    /// de l'en-tete `Host`.
    pub public_url: Option<String>,
    /// Origine de l'API GitHub, sans `/` final. Fixee par l'operateur (tests :
    /// serveur simule), jamais deduite d'une requete.
    pub github_api_url: String,
    /// IP publique du noeud : cible des enregistrements A/AAAA des domaines.
    pub public_ip: Option<IpAddr>,
    /// IPv6 publique du noeud (facultative) : cible des enregistrements AAAA
    /// quand `public_ip` est une IPv4.
    pub public_ip6: Option<IpAddr>,
    /// Autres noms du panneau (minuscules), reserves comme son domaine.
    pub panel_aliases: Vec<String>,
    /// Verification de la derniere version publiee (opt-in, desactivee par
    /// defaut) : une requete GET vers `release_base_url`, sans aucune donnee
    /// de l'instance.
    pub update_check: bool,
    /// Origine des releases, sans `/` final.
    pub release_base_url: String,
}

impl Config {
    pub const DEFAULT_PANEL_PORT: u16 = 8443;
    pub const DEFAULT_PANEL_LOCAL_PORT: u16 = 8444;
    pub const DEFAULT_AGENT_LINK_PORT: u16 = 7443;
    pub const DEFAULT_AGENT_LINK_BIND: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    pub const DEFAULT_CONFIG_DIR: &'static str = "/etc/ciphera";
    pub const DEFAULT_DATA_DIR: &'static str = "/var/lib/ciphera";
    pub const DEFAULT_DB_HOST: &'static str = "127.0.0.1";
    pub const DEFAULT_DB_PORT: u16 = 5432;
    pub const DEFAULT_DB_USER: &'static str = "ciphera";
    pub const DEFAULT_DB_NAME: &'static str = "ciphera";
    /// Nom du fichier de mot de passe dans le dossier de configuration.
    pub const DB_PASSWORD_FILE_NAME: &'static str = "db_password";
    pub const DEFAULT_GITHUB_API_URL: &'static str = "https://api.github.com";
    pub const DEFAULT_RELEASE_BASE_URL: &'static str = "https://get.ciphera.ch";

    /// Lit la configuration depuis l'environnement du processus.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Lit la configuration via une fonction de recherche (testable sans
    /// toucher a l'environnement du processus).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        // Port TCP 1..=65535 (0 : port choisi au hasard par le noyau, refuse).
        let parse_port = |name: &'static str, v: &str| -> Result<u16, ConfigError> {
            match v.trim().parse::<u16>() {
                Ok(p) if p != 0 => Ok(p),
                _ => Err(ConfigError::Invalid(name)),
            }
        };
        let port = |name: &'static str, default: u16| -> Result<u16, ConfigError> {
            match get(name) {
                None => Ok(default),
                Some(v) => parse_port(name, &v),
            }
        };
        let text = |name: &str, default: &str| get(name).unwrap_or_else(|| default.to_string());
        // Valeur vide = non definie.
        let optional = |name: &str| {
            get(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let config_dir = PathBuf::from(text("CIPHERA_CONFIG_DIR", Self::DEFAULT_CONFIG_DIR));

        let url = match (
            get("CIPHERA_DATABASE_URL"),
            get("CIPHERA_DATABASE_URL_FILE"),
        ) {
            (Some(url), _) => Some(url),
            (None, Some(path)) => Some(
                std::fs::read_to_string(path)
                    .map_err(|_| ConfigError::UnreadableFile("CIPHERA_DATABASE_URL_FILE"))?
                    .trim()
                    .to_string(),
            ),
            (None, None) => None,
        };
        let database = match url {
            Some(url) if url.is_empty() => return Err(ConfigError::MissingDatabaseUrl),
            Some(url) => Database::Url(DatabaseUrl::new(url)),
            None => {
                const PW: &str = "CIPHERA_DB_PASSWORD_FILE";
                let pw_path = get(PW)
                    .map(PathBuf::from)
                    .unwrap_or_else(|| config_dir.join(Self::DB_PASSWORD_FILE_NAME));
                let password = std::fs::read_to_string(pw_path)
                    .map_err(|_| ConfigError::UnreadableFile(PW))?
                    .trim()
                    .to_string();
                if password.is_empty() {
                    return Err(ConfigError::Invalid(PW));
                }
                Database::Params(DatabaseParams {
                    socket_dir: optional("CIPHERA_DB_SOCKET_DIR").map(PathBuf::from),
                    host: text("CIPHERA_DB_HOST", Self::DEFAULT_DB_HOST),
                    port: port("CIPHERA_DB_PORT", Self::DEFAULT_DB_PORT)?,
                    user: text("CIPHERA_DB_USER", Self::DEFAULT_DB_USER),
                    name: text("CIPHERA_DB_NAME", Self::DEFAULT_DB_NAME),
                    password: DbPassword(password),
                })
            }
        };

        Ok(Self::with_public_ip_from_url(Self {
            panel_port: port("CIPHERA_PANEL_PORT", Self::DEFAULT_PANEL_PORT)?,
            http_port: match optional("CIPHERA_HTTP_PORT") {
                None => None,
                Some(v) => Some(parse_port("CIPHERA_HTTP_PORT", &v)?),
            },
            panel_local_port: port("CIPHERA_PANEL_LOCAL_PORT", Self::DEFAULT_PANEL_LOCAL_PORT)?,
            agent_link_port: port("CIPHERA_AGENT_LINK_PORT", Self::DEFAULT_AGENT_LINK_PORT)?,
            agent_link_bind: match optional("CIPHERA_AGENT_LINK_BIND") {
                None => Self::DEFAULT_AGENT_LINK_BIND,
                Some(v) => v
                    .parse()
                    .map_err(|_| ConfigError::Invalid("CIPHERA_AGENT_LINK_BIND"))?,
            },
            config_dir,
            data_dir: PathBuf::from(text("CIPHERA_DATA_DIR", Self::DEFAULT_DATA_DIR)),
            database,
            webauthn_rp_id: optional("CIPHERA_WEBAUTHN_RP_ID"),
            webauthn_rp_origin: optional("CIPHERA_WEBAUTHN_RP_ORIGIN"),
            public_url: match optional("CIPHERA_PUBLIC_URL") {
                None => None,
                Some(v) => {
                    Some(parse_public_url(&v).ok_or(ConfigError::Invalid("CIPHERA_PUBLIC_URL"))?)
                }
            },
            github_api_url: match optional("CIPHERA_GITHUB_API_URL") {
                None => Self::DEFAULT_GITHUB_API_URL.to_string(),
                Some(v) => {
                    parse_public_url(&v).ok_or(ConfigError::Invalid("CIPHERA_GITHUB_API_URL"))?
                }
            },
            public_ip: match optional("CIPHERA_PUBLIC_IP") {
                None => None,
                Some(v) => Some(
                    v.parse::<IpAddr>()
                        .ok()
                        .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
                        .ok_or(ConfigError::Invalid("CIPHERA_PUBLIC_IP"))?,
                ),
            },
            public_ip6: match optional("CIPHERA_PUBLIC_IP6") {
                None => None,
                Some(v) => Some(
                    v.parse::<IpAddr>()
                        .ok()
                        .filter(|ip| ip.is_ipv6() && !ip.is_unspecified() && !ip.is_multicast())
                        .ok_or(ConfigError::Invalid("CIPHERA_PUBLIC_IP6"))?,
                ),
            },
            panel_aliases: match optional("CIPHERA_PANEL_ALIASES") {
                None => Vec::new(),
                Some(v) => v
                    .split(',')
                    .map(|h| h.trim().trim_end_matches('.').to_ascii_lowercase())
                    .filter(|h| !h.is_empty())
                    .map(|h| {
                        if h.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
                        {
                            Ok(h)
                        } else {
                            Err(ConfigError::Invalid("CIPHERA_PANEL_ALIASES"))
                        }
                    })
                    .collect::<Result<_, _>>()?,
            },
            update_check: match optional("CIPHERA_UPDATE_CHECK").as_deref() {
                None | Some("0" | "false" | "no") => false,
                Some("1" | "true" | "yes") => true,
                Some(_) => return Err(ConfigError::Invalid("CIPHERA_UPDATE_CHECK")),
            },
            release_base_url: match optional("CIPHERA_RELEASE_BASE_URL") {
                None => Self::DEFAULT_RELEASE_BASE_URL.to_string(),
                Some(v) => {
                    parse_public_url(&v).ok_or(ConfigError::Invalid("CIPHERA_RELEASE_BASE_URL"))?
                }
            },
        }))
    }

    /// Sans `CIPHERA_PUBLIC_IP`, l'hote de `CIPHERA_PUBLIC_URL` s'il est une
    /// adresse IP.
    fn with_public_ip_from_url(mut self) -> Self {
        if self.public_ip.is_none() {
            self.public_ip = self
                .public_url
                .as_deref()
                .and_then(|u| u.split_once("://"))
                .map(|(_, authority)| authority)
                .and_then(|a| {
                    a.strip_prefix('[')
                        .and_then(|v6| v6.split_once(']'))
                        .map(|(v6, _)| v6)
                        .or_else(|| Some(a.rsplit_once(':').map_or(a, |(h, _)| h)))
                })
                .and_then(|h| h.parse::<IpAddr>().ok())
                .filter(|ip| !ip.is_loopback());
        }
        self
    }

    /// IP du noeud par famille : (IPv4, IPv6). `CIPHERA_PUBLIC_IP` donne sa
    /// famille ; `CIPHERA_PUBLIC_IP6` complete l'IPv6.
    pub fn node_ips(&self) -> (Option<IpAddr>, Option<IpAddr>) {
        let v4 = self.public_ip.filter(IpAddr::is_ipv4);
        let v6 = self.public_ip.filter(IpAddr::is_ipv6).or(self.public_ip6);
        (v4, v6)
    }

    /// Noms reserves au panneau (minuscules, sans port) : hote de
    /// `CIPHERA_PUBLIC_URL`, RP ID WebAuthn et alias. Aucun domaine de projet
    /// ne peut les prendre.
    pub fn reserved_hosts(&self) -> Vec<String> {
        let url_host = self.public_url.as_deref().and_then(|u| {
            let authority = u.split_once("://")?.1;
            if authority.starts_with('[') {
                return None;
            }
            Some(
                authority
                    .rsplit_once(':')
                    .map_or(authority, |(h, _)| h)
                    .to_string(),
            )
        });
        let mut hosts: Vec<String> = url_host
            .into_iter()
            .chain(self.webauthn_rp_id.clone())
            .chain(self.panel_aliases.iter().cloned())
            .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }

    /// Lien vers `path` (commence par `/`) : absolu si `CIPHERA_PUBLIC_URL`
    /// est defini, relatif sinon (le panneau le complete).
    pub fn public_link(&self, path: &str) -> String {
        match &self.public_url {
            Some(origin) => format!("{origin}{path}"),
            None => path.to_string(),
        }
    }
}

/// Origine `https://hote[:port]` validee comme `CIPHERA_PUBLIC_URL` (forme
/// normalisee, ou `None` si invalide).
pub fn parse_origin(value: &str) -> Option<String> {
    parse_public_url(value.trim())
}

/// Origine `https://hote[:port]` (un `/` final est tolere) ; `http` seulement
/// pour `localhost`, `127.0.0.1` et `[::1]`. Ni chemin, ni requete, ni
/// identifiants. Renvoie la forme normalisee (schema et hote en minuscules).
fn parse_public_url(value: &str) -> Option<String> {
    let value = value.strip_suffix('/').unwrap_or(value);
    let (scheme, authority) = value.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let authority = authority.to_ascii_lowercase();
    if authority.is_empty()
        || authority
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']')))
    {
        return None;
    }
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')?;
        let (host, after) = authority.split_at(end + 1);
        let v6 = &host[1..host.len() - 1];
        if v6.is_empty() || !v6.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
            return None;
        }
        let port = if after.is_empty() {
            None
        } else {
            Some(after.strip_prefix(':')?)
        };
        (host, port)
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority.as_str(), None),
        }
    };
    if let Some(port) = port {
        port.parse::<u16>().ok().filter(|p| *p != 0)?;
    }
    if !host.starts_with('[')
        && (host.is_empty()
            || host.contains(['[', ']'])
            || host
                .split('.')
                .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-')))
    {
        return None;
    }
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    match scheme.as_str() {
        "https" => {}
        "http" if local => {}
        _ => return None,
    }
    Some(format!("{scheme}://{authority}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn defaults_apply() {
        let c = Config::from_lookup(lookup(&[("CIPHERA_DATABASE_URL", "postgres://u:p@db/c")]))
            .unwrap();
        assert_eq!(c.panel_port, 8443);
        // Le port 80 appartient a ciphera-proxy : aucune redirection par defaut.
        assert_eq!(c.http_port, None);
        assert_eq!(c.panel_local_port, 8444);
        assert_eq!(c.config_dir, PathBuf::from("/etc/ciphera"));
        assert_eq!(c.data_dir, PathBuf::from("/var/lib/ciphera"));
        assert_eq!(c.public_url, None);
        assert_eq!(c.public_link("/reset/abc"), "/reset/abc");
        assert_eq!((c.webauthn_rp_id, c.webauthn_rp_origin), (None, None));
        assert_eq!(c.github_api_url, "https://api.github.com");
    }

    #[test]
    fn public_ip_and_reserved_panel_hosts() {
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PUBLIC_URL", "https://Panel.Example.com:8443"),
            (
                "CIPHERA_PANEL_ALIASES",
                "admin.example.com, OPS.example.com.",
            ),
            ("CIPHERA_PUBLIC_IP", "203.0.113.7"),
        ]))
        .unwrap();
        assert_eq!(c.public_ip, Some("203.0.113.7".parse().unwrap()));
        assert_eq!(
            c.reserved_hosts(),
            vec!["admin.example.com", "ops.example.com", "panel.example.com"]
        );
        // Sans CIPHERA_PUBLIC_IP : l'IP de l'URL du panneau.
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PUBLIC_URL", "https://198.51.100.4:8443"),
        ]))
        .unwrap();
        assert_eq!(c.public_ip, Some("198.51.100.4".parse().unwrap()));
        assert_eq!(c.node_ips(), (Some("198.51.100.4".parse().unwrap()), None));
        // IPv6 du noeud, facultative.
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PUBLIC_IP", "203.0.113.7"),
            ("CIPHERA_PUBLIC_IP6", "2001:db8::7"),
        ]))
        .unwrap();
        assert_eq!(
            c.node_ips(),
            (
                Some("203.0.113.7".parse().unwrap()),
                Some("2001:db8::7".parse().unwrap())
            )
        );
        for bad in [
            ("CIPHERA_PUBLIC_IP6", "203.0.113.7"),
            ("CIPHERA_PUBLIC_IP6", "::"),
            ("CIPHERA_PUBLIC_IP", "0.0.0.0"),
            ("CIPHERA_PUBLIC_IP", "x"),
            ("CIPHERA_PANEL_ALIASES", "a b.com"),
        ] {
            let r = Config::from_lookup(lookup(&[
                ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
                bad,
            ]));
            assert!(r.is_err(), "{bad:?}");
        }
    }

    #[test]
    fn github_api_url_is_an_origin() {
        let api = |v: &str| {
            Config::from_lookup(lookup(&[
                ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
                ("CIPHERA_GITHUB_API_URL", v),
            ]))
            .map(|c| c.github_api_url)
        };
        assert_eq!(
            api("http://127.0.0.1:9000/").unwrap(),
            "http://127.0.0.1:9000"
        );
        assert_eq!(
            api("https://GHE.example.com").unwrap(),
            "https://ghe.example.com"
        );
        for bad in ["http://api.github.com", "https://a.b/path", "ftp://x"] {
            assert_eq!(
                api(bad),
                Err(ConfigError::Invalid("CIPHERA_GITHUB_API_URL")),
                "{bad}"
            );
        }
    }

    fn public_url(value: &str) -> Result<Option<String>, ConfigError> {
        Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PUBLIC_URL", value),
        ]))
        .map(|c| c.public_url)
    }

    #[test]
    fn public_url_is_an_https_origin() {
        for (given, expected) in [
            ("https://panel.example.com", "https://panel.example.com"),
            ("https://Panel.Example.com/", "https://panel.example.com"),
            (
                "HTTPS://panel.example.com:8443",
                "https://panel.example.com:8443",
            ),
            ("https://203.0.113.7:8443", "https://203.0.113.7:8443"),
            ("https://[2001:db8::1]:8443", "https://[2001:db8::1]:8443"),
            ("http://localhost:5173", "http://localhost:5173"),
            ("http://127.0.0.1:8443", "http://127.0.0.1:8443"),
            ("http://[::1]", "http://[::1]"),
        ] {
            assert_eq!(public_url(given), Ok(Some(expected.to_string())), "{given}");
        }
        assert_eq!(public_url(" "), Ok(None), "vide = non defini");
        for bad in [
            "http://panel.example.com",
            "panel.example.com",
            "ftp://panel.example.com",
            "https://",
            "https://panel.example.com/chemin",
            "https://panel.example.com?x=1",
            "https://panel.example.com#x",
            "https://user:pw@panel.example.com",
            "https://panel.example.com:0",
            "https://panel.example.com:99999",
            "https://panel.example.com:",
            "https://panel..example.com",
            "https://-panel.example.com",
            "https://pan el.example.com",
            "https://[zz::1]",
            "https://[::1]x",
            "javascript://x",
        ] {
            assert_eq!(
                public_url(bad),
                Err(ConfigError::Invalid("CIPHERA_PUBLIC_URL")),
                "{bad}"
            );
        }
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PUBLIC_URL", "https://panel.example.com/"),
        ]))
        .unwrap();
        assert_eq!(
            c.public_link("/reset/abc"),
            "https://panel.example.com/reset/abc"
        );
    }

    #[test]
    fn webauthn_rp_can_be_set_and_empty_means_unset() {
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_WEBAUTHN_RP_ID", " panel.example.com "),
            ("CIPHERA_WEBAUTHN_RP_ORIGIN", ""),
        ]))
        .unwrap();
        assert_eq!(c.webauthn_rp_id.as_deref(), Some("panel.example.com"));
        assert_eq!(c.webauthn_rp_origin, None);
    }

    #[test]
    fn agent_link_defaults_to_loopback_7443_and_can_be_overridden() {
        let c = Config::from_lookup(lookup(&[("CIPHERA_DATABASE_URL", "postgres://u:p@db/c")]))
            .unwrap();
        assert_eq!(c.agent_link_port, 7443);
        assert_eq!(c.agent_link_bind, "127.0.0.1".parse::<IpAddr>().unwrap());
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_AGENT_LINK_PORT", "9000"),
            ("CIPHERA_AGENT_LINK_BIND", "::"),
        ]))
        .unwrap();
        assert_eq!(c.agent_link_port, 9000);
        assert_eq!(c.agent_link_bind, "::".parse::<IpAddr>().unwrap());
        for (key, bad) in [
            ("CIPHERA_AGENT_LINK_BIND", "localhost"),
            ("CIPHERA_AGENT_LINK_PORT", "70000"),
        ] {
            let err = Config::from_lookup(lookup(&[
                ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
                (key, bad),
            ]))
            .unwrap_err();
            assert_eq!(err, ConfigError::Invalid(key));
        }
    }

    #[test]
    fn zero_ports_are_refused() {
        for key in [
            "CIPHERA_HTTP_PORT",
            "CIPHERA_PANEL_PORT",
            "CIPHERA_PANEL_LOCAL_PORT",
            "CIPHERA_AGENT_LINK_PORT",
        ] {
            for bad in ["0", " 0 ", "70000", "-1"] {
                let err = Config::from_lookup(lookup(&[
                    ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
                    (key, bad),
                ]))
                .unwrap_err();
                assert_eq!(err, ConfigError::Invalid(key), "{key}={bad}");
            }
        }
    }

    #[test]
    fn overrides_apply() {
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PANEL_PORT", "9443"),
            ("CIPHERA_HTTP_PORT", "8080"),
            ("CIPHERA_CONFIG_DIR", "/tmp/cfg"),
            ("CIPHERA_DATA_DIR", "/tmp/data"),
        ]))
        .unwrap();
        assert_eq!((c.panel_port, c.http_port), (9443, Some(8080)));
        assert_eq!(c.data_dir, PathBuf::from("/tmp/data"));
    }

    /// Dossier temporaire contenant `db_password`, supprime a la fin.
    struct TempDir(PathBuf);
    impl TempDir {
        fn with_password(content: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("ciphera-cfg-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("db_password"), content).unwrap();
            Self(dir)
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // Mot de passe base64 (format de `ciphera init`) : `+`, `/` et `=`
    // casseraient une URL non encodee.
    const B64_PW: &str = "q+8/Zx3k9w==";

    #[test]
    fn without_url_params_come_from_defaults_and_config_dir_password() {
        let dir = TempDir::with_password(&format!("{B64_PW}\n"));
        let c = Config::from_lookup(lookup(&[("CIPHERA_CONFIG_DIR", dir.path())])).unwrap();
        let Database::Params(p) = &c.database else {
            panic!("parametres attendus : {:?}", c.database);
        };
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.socket_dir, None);
        assert_eq!(p.port, 5432);
        assert_eq!(p.user, "ciphera");
        assert_eq!(p.name, "ciphera");
        assert_eq!(p.password().expose(), B64_PW);
    }

    #[test]
    fn update_check_is_opt_in() {
        let base = [("CIPHERA_DATABASE_URL", "postgres://u:p@db/c")];
        let c = Config::from_lookup(lookup(&base)).unwrap();
        assert!(!c.update_check);
        assert_eq!(c.release_base_url, "https://get.ciphera.ch");
        let c = Config::from_lookup(lookup(&[
            base[0],
            ("CIPHERA_UPDATE_CHECK", "1"),
            ("CIPHERA_RELEASE_BASE_URL", "http://127.0.0.1:9000/"),
        ]))
        .unwrap();
        assert!(c.update_check);
        assert_eq!(c.release_base_url, "http://127.0.0.1:9000");
        for (k, v) in [
            ("CIPHERA_UPDATE_CHECK", "peut-etre"),
            ("CIPHERA_RELEASE_BASE_URL", "http://releases.example.com"),
            ("CIPHERA_RELEASE_BASE_URL", "https://x.example/releases"),
        ] {
            assert_eq!(
                Config::from_lookup(lookup(&[base[0], (k, v)])),
                Err(ConfigError::Invalid(k))
            );
        }
    }

    #[test]
    fn params_can_be_overridden() {
        let dir = TempDir::with_password(B64_PW);
        let pw_file = format!("{}/db_password", dir.path());
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DB_HOST", "db.internal"),
            ("CIPHERA_DB_PORT", "6543"),
            ("CIPHERA_DB_USER", "app"),
            ("CIPHERA_DB_NAME", "appdb"),
            ("CIPHERA_DB_PASSWORD_FILE", &pw_file),
        ]))
        .unwrap();
        let Database::Params(p) = &c.database else {
            panic!("parametres attendus");
        };
        assert_eq!(
            (p.host.as_str(), p.port, p.user.as_str(), p.name.as_str()),
            ("db.internal", 6543, "app", "appdb")
        );
        assert_eq!(p.password().expose(), B64_PW);
    }

    #[test]
    fn url_takes_precedence_over_params() {
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_DB_HOST", "ignored"),
        ]))
        .unwrap();
        assert_eq!(
            c.database,
            Database::Url(DatabaseUrl::new("postgres://u:p@db/c"))
        );
    }

    #[test]
    fn empty_url_is_rejected() {
        assert_eq!(
            Config::from_lookup(lookup(&[("CIPHERA_DATABASE_URL", "")])).unwrap_err(),
            ConfigError::MissingDatabaseUrl
        );
    }

    #[test]
    fn missing_password_file_is_reported_by_variable_name() {
        let dir = std::env::temp_dir().join(format!("ciphera-none-{}", uuid::Uuid::new_v4()));
        let err = Config::from_lookup(lookup(&[("CIPHERA_CONFIG_DIR", dir.to_str().unwrap())]))
            .unwrap_err();
        assert_eq!(err, ConfigError::UnreadableFile("CIPHERA_DB_PASSWORD_FILE"));
    }

    #[test]
    fn empty_password_file_is_rejected() {
        let dir = TempDir::with_password("\n");
        let err = Config::from_lookup(lookup(&[("CIPHERA_CONFIG_DIR", dir.path())])).unwrap_err();
        assert_eq!(err, ConfigError::Invalid("CIPHERA_DB_PASSWORD_FILE"));
    }

    #[test]
    fn invalid_db_port_is_rejected() {
        let dir = TempDir::with_password(B64_PW);
        let err = Config::from_lookup(lookup(&[
            ("CIPHERA_CONFIG_DIR", dir.path()),
            ("CIPHERA_DB_PORT", "nope"),
        ]))
        .unwrap_err();
        assert_eq!(err, ConfigError::Invalid("CIPHERA_DB_PORT"));
    }

    #[test]
    fn debug_of_params_never_shows_the_password() {
        let dir = TempDir::with_password(B64_PW);
        let c = Config::from_lookup(lookup(&[("CIPHERA_CONFIG_DIR", dir.path())])).unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains(B64_PW), "{shown}");
        assert!(shown.contains("***"), "{shown}");
    }

    #[cfg(feature = "sqlx")]
    #[test]
    fn params_become_connect_options_without_url_encoding() {
        let dir = TempDir::with_password(B64_PW);
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_CONFIG_DIR", dir.path()),
            ("CIPHERA_DB_HOST", "db.internal"),
        ]))
        .unwrap();
        let opts = c.database.connect_options().unwrap();
        assert_eq!(opts.get_host(), "db.internal");
        assert_eq!(opts.get_port(), 5432);
        assert_eq!(opts.get_username(), "ciphera");
        assert_eq!(opts.get_database(), Some("ciphera"));
    }

    #[test]
    fn socket_dir_can_be_set_and_empty_means_unset() {
        let dir = TempDir::with_password(B64_PW);
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_CONFIG_DIR", dir.path()),
            ("CIPHERA_DB_SOCKET_DIR", "/run/ciphera/postgres"),
        ]))
        .unwrap();
        let Database::Params(p) = &c.database else {
            panic!("parametres attendus");
        };
        assert_eq!(p.socket_dir, Some(PathBuf::from("/run/ciphera/postgres")));
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_CONFIG_DIR", dir.path()),
            ("CIPHERA_DB_SOCKET_DIR", " "),
        ]))
        .unwrap();
        let Database::Params(p) = &c.database else {
            panic!("parametres attendus");
        };
        assert_eq!(p.socket_dir, None);
    }

    #[cfg(feature = "sqlx")]
    #[test]
    fn socket_dir_becomes_a_unix_socket_connection() {
        let dir = TempDir::with_password(B64_PW);
        let c = Config::from_lookup(lookup(&[
            ("CIPHERA_CONFIG_DIR", dir.path()),
            ("CIPHERA_DB_SOCKET_DIR", "/run/ciphera/postgres"),
        ]))
        .unwrap();
        let opts = c.database.connect_options().unwrap();
        assert_eq!(
            opts.get_socket(),
            Some(&PathBuf::from("/run/ciphera/postgres"))
        );
        assert_eq!(opts.get_username(), "ciphera");
        // Sans dossier : TCP vers l'hote.
        let c = Config::from_lookup(lookup(&[("CIPHERA_CONFIG_DIR", dir.path())])).unwrap();
        assert_eq!(c.database.connect_options().unwrap().get_socket(), None);
    }

    #[cfg(feature = "sqlx")]
    #[test]
    fn url_becomes_connect_options() {
        let c = Config::from_lookup(lookup(&[(
            "CIPHERA_DATABASE_URL",
            "postgres://u:p@db:5433/c",
        )]))
        .unwrap();
        let opts = c.database.connect_options().unwrap();
        assert_eq!((opts.get_host(), opts.get_port()), ("db", 5433));
    }

    #[cfg(feature = "sqlx")]
    #[test]
    fn bad_url_error_does_not_echo_it() {
        let c = Config::from_lookup(lookup(&[(
            "CIPHERA_DATABASE_URL",
            "postgres://u:s3cr3t@[::1/c",
        )]))
        .unwrap();
        let err = c.database.connect_options().unwrap_err();
        assert!(!err.to_string().contains("s3cr3t"));
    }

    #[test]
    fn invalid_port_is_rejected_without_echoing_value() {
        let err = Config::from_lookup(lookup(&[
            ("CIPHERA_DATABASE_URL", "postgres://u:p@db/c"),
            ("CIPHERA_PANEL_PORT", "abc"),
        ]))
        .unwrap_err();
        assert_eq!(err, ConfigError::Invalid("CIPHERA_PANEL_PORT"));
        assert!(!err.to_string().contains("abc"));
    }

    #[test]
    fn database_url_can_come_from_a_file() {
        let path = std::env::temp_dir().join(format!("ciphera-dburl-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, "postgres://u:filepw@db/c\n").unwrap();
        let c = Config::from_lookup(lookup(&[(
            "CIPHERA_DATABASE_URL_FILE",
            path.to_str().unwrap(),
        )]));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            c.unwrap().database,
            Database::Url(DatabaseUrl::new("postgres://u:filepw@db/c"))
        );
    }

    #[test]
    fn debug_never_shows_the_password() {
        let c = Config::from_lookup(lookup(&[(
            "CIPHERA_DATABASE_URL",
            "postgres://ciphera:s3cr3t@db:5432/c",
        )]))
        .unwrap();
        let shown = format!("{c:?}");
        assert!(!shown.contains("s3cr3t"), "{shown}");
        assert!(shown.contains("postgres://ciphera:***@db:5432/c"));
    }
}
