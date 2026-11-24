//! Configuration de l'agent (variables d'environnement) et identite mTLS
//! lue dans son dossier : `agent.key` (cle privee, 0600, jamais transmise),
//! `agent.crt` (certificat emis par la CA de l'instance) et `ca.crt`.

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shared::ids::ServerId;

pub const KEY_FILE: &str = "agent.key";
pub const CERT_FILE: &str = "agent.crt";
pub const CA_FILE: &str = "ca.crt";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentConfig {
    /// Dossier de l'identite de l'agent.
    pub dir: PathBuf,
    /// Adresse UDP du lien QUIC du plan de controle.
    pub control_addr: SocketAddr,
    /// Nom attendu dans le certificat du plan de controle.
    pub server_name: String,
    /// Base SQLite de l'etat local.
    pub state_db: PathBuf,
    /// Socket de pilotage du proxy du noeud.
    pub proxy_socket: PathBuf,
    /// Ecoute locale du panneau (HTTP en boucle locale) : seul amont admis
    /// pour la route `panel` envoyee par le plan de controle.
    pub panel_upstream: SocketAddr,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError(pub &'static str);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} invalide", self.0)
    }
}

impl std::error::Error for ConfigError {}

impl AgentConfig {
    pub const DEFAULT_DIR: &'static str = "/etc/ciphera-agent";
    pub const DEFAULT_CONTROL_ADDR: &'static str = "127.0.0.1:7443";
    pub const DEFAULT_SERVER_NAME: &'static str = "localhost";
    /// Dans le `StateDirectory` de `ciphera-agent.service` (0700).
    pub const DEFAULT_STATE_DB: &'static str = "/var/lib/ciphera-agent/state.db";
    /// Cree par `ciphera-proxy.socket` (0660, groupe ciphera-agent).
    pub const DEFAULT_PROXY_SOCKET: &'static str = "/run/ciphera/proxy.sock";
    /// `CIPHERA_PANEL_LOCAL_PORT` du plan de controle (8444 par defaut).
    pub const DEFAULT_PANEL_UPSTREAM: &'static str = "127.0.0.1:8444";

    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let text = |name: &str, default: &str| {
            get(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        const ADDR: &str = "CIPHERA_AGENT_CONTROL_ADDR";
        const PANEL: &str = "CIPHERA_AGENT_PANEL_UPSTREAM";
        Ok(Self {
            dir: PathBuf::from(text("CIPHERA_AGENT_DIR", Self::DEFAULT_DIR)),
            control_addr: text(ADDR, Self::DEFAULT_CONTROL_ADDR)
                .parse()
                .map_err(|_| ConfigError(ADDR))?,
            server_name: text("CIPHERA_AGENT_SERVER_NAME", Self::DEFAULT_SERVER_NAME),
            state_db: PathBuf::from(text("CIPHERA_AGENT_STATE_DB", Self::DEFAULT_STATE_DB)),
            proxy_socket: PathBuf::from(text(
                "CIPHERA_AGENT_PROXY_SOCKET",
                Self::DEFAULT_PROXY_SOCKET,
            )),
            // Boucle locale seulement : jamais un amont joignable du reseau.
            panel_upstream: text(PANEL, Self::DEFAULT_PANEL_UPSTREAM)
                .parse::<SocketAddr>()
                .ok()
                .filter(|a| a.ip().is_loopback() && a.port() != 0)
                .ok_or(ConfigError(PANEL))?,
        })
    }
}

/// Identite mTLS de l'agent.
pub struct Identity {
    pub agent_id: ServerId,
    pub(crate) cert: CertificateDer<'static>,
    pub(crate) key: PrivateKeyDer<'static>,
    pub(crate) ca: CertificateDer<'static>,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("agent_id", &self.agent_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum IdentityError {
    Read(&'static str, std::io::Error),
    /// PEM illisible (nom du fichier).
    Pem(&'static str),
    /// Certificat sans SAN `urn:ciphera:agent:<id>`.
    NoAgentId,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(file, e) => write!(f, "lecture de {file} : {e}"),
            Self::Pem(file) => write!(f, "{file} illisible (PEM attendu)"),
            Self::NoAgentId => write!(f, "{CERT_FILE} ne porte pas d'identifiant d'agent"),
        }
    }
}

impl std::error::Error for IdentityError {}

impl Identity {
    /// Lit l'identite dans `dir` (fichiers ecrits par `ciphera init`).
    pub fn load(dir: &Path) -> Result<Self, IdentityError> {
        let read = |file: &'static str| {
            std::fs::read(dir.join(file)).map_err(|e| IdentityError::Read(file, e))
        };
        let key = read(KEY_FILE)?;
        Self::from_pem(&read(CERT_FILE)?, &key, &read(CA_FILE)?)
    }

    pub fn from_pem(cert_pem: &[u8], key_pem: &[u8], ca_pem: &[u8]) -> Result<Self, IdentityError> {
        let cert =
            CertificateDer::from_pem_slice(cert_pem).map_err(|_| IdentityError::Pem(CERT_FILE))?;
        let key =
            PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| IdentityError::Pem(KEY_FILE))?;
        let ca = CertificateDer::from_pem_slice(ca_pem).map_err(|_| IdentityError::Pem(CA_FILE))?;
        let agent_id = pki::agent_id_from_cert(&cert).ok_or(IdentityError::NoAgentId)?;
        Ok(Self {
            agent_id,
            cert,
            key,
            ca,
        })
    }

    /// Copie (la cle privee comprise).
    pub(crate) fn duplicate(&self) -> Self {
        Self {
            agent_id: self.agent_id,
            cert: self.cert.clone(),
            key: self.key.clone_key(),
            ca: self.ca.clone(),
        }
    }

    /// Certificat renouvele `cert_pem` (envoye par le plan de controle) :
    /// accepte seulement s'il porte le meme identifiant et la meme cle
    /// publique que le certificat actuel et n'est pas lui-meme a
    /// renouveler a `now` (secondes Unix). Renvoie son DER.
    pub fn check_renewed(
        &self,
        cert_pem: &str,
        now: i64,
    ) -> Result<CertificateDer<'static>, &'static str> {
        let cert = CertificateDer::from_pem_slice(cert_pem.as_bytes())
            .map_err(|_| "certificat illisible")?;
        if pki::agent_id_from_cert(&cert) != Some(self.agent_id) {
            return Err("identifiant d'agent different");
        }
        let key = pki::public_key_der(&cert);
        if key.is_none() || key != pki::public_key_der(&self.cert) {
            return Err("cle publique differente");
        }
        if pki::renewal_due(&cert, now) != Some(false) {
            return Err("certificat deja a renouveler");
        }
        Ok(cert)
    }

    /// Le certificat actuel a-t-il passe les deux tiers de sa validite ?
    pub fn renewal_due(&self, now: i64) -> bool {
        pki::renewal_due(&self.cert, now) != Some(false)
    }

    /// Jours restants avant l'expiration du certificat actuel (negatif :
    /// expire). `None` : illisible.
    pub fn days_left(&self, now: i64) -> Option<i64> {
        pki::validity_unix(&self.cert).map(|(_, to)| (to - now).div_euclid(86_400))
    }

    /// Remplace le certificat (apres son ecriture sur disque).
    pub(crate) fn set_cert(&mut self, cert: CertificateDer<'static>) {
        self.cert = cert;
    }
}

/// Remplace `<dir>/agent.crt` : fichier temporaire 0600 du meme dossier,
/// donnees sur disque, renommage, dossier sur disque.
pub fn replace_cert_file(dir: &Path, pem: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = dir.join(format!(".{CERT_FILE}.renew"));
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    let written = file
        .write_all(pem)
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&tmp, dir.join(CERT_FILE)));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
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
        move |k| map.get(k).cloned()
    }

    #[test]
    fn defaults_point_to_the_local_control_plane() {
        let c = AgentConfig::from_lookup(lookup(&[])).unwrap();
        assert_eq!(c.control_addr, "127.0.0.1:7443".parse().unwrap());
        assert_eq!(c.server_name, "localhost");
        assert_eq!(c.dir, PathBuf::from("/etc/ciphera-agent"));
        assert_eq!(c.state_db, PathBuf::from("/var/lib/ciphera-agent/state.db"));
        assert_eq!(c.proxy_socket, PathBuf::from("/run/ciphera/proxy.sock"));
    }

    #[test]
    fn overrides_apply_and_bad_address_is_rejected() {
        let c = AgentConfig::from_lookup(lookup(&[
            ("CIPHERA_AGENT_CONTROL_ADDR", "10.0.0.2:9443"),
            ("CIPHERA_AGENT_SERVER_NAME", "cp.example.com"),
            ("CIPHERA_AGENT_DIR", "/tmp/agent"),
            ("CIPHERA_AGENT_STATE_DB", "/tmp/agent/state.db"),
            ("CIPHERA_AGENT_PROXY_SOCKET", "/tmp/agent/proxy.sock"),
        ]))
        .unwrap();
        assert_eq!(c.proxy_socket, PathBuf::from("/tmp/agent/proxy.sock"));
        assert_eq!(c.state_db, PathBuf::from("/tmp/agent/state.db"));
        assert_eq!(c.control_addr, "10.0.0.2:9443".parse().unwrap());
        assert_eq!(c.server_name, "cp.example.com");
        assert_eq!(
            AgentConfig::from_lookup(lookup(&[("CIPHERA_AGENT_CONTROL_ADDR", "nope")])),
            Err(ConfigError("CIPHERA_AGENT_CONTROL_ADDR"))
        );
    }

    #[test]
    fn panel_upstream_is_a_loopback_address() {
        let c = AgentConfig::from_lookup(lookup(&[])).unwrap();
        assert_eq!(c.panel_upstream, "127.0.0.1:8444".parse().unwrap());
        let c = AgentConfig::from_lookup(lookup(&[("CIPHERA_AGENT_PANEL_UPSTREAM", "[::1]:9000")]))
            .unwrap();
        assert_eq!(c.panel_upstream, "[::1]:9000".parse().unwrap());
        for bad in ["10.0.0.2:8444", "127.0.0.1:0", "nope"] {
            assert_eq!(
                AgentConfig::from_lookup(lookup(&[("CIPHERA_AGENT_PANEL_UPSTREAM", bad)])),
                Err(ConfigError("CIPHERA_AGENT_PANEL_UPSTREAM")),
                "{bad}"
            );
        }
    }

    #[test]
    fn identity_debug_hides_the_key() {
        let ca = pki::Ca::generate("t").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let id = ServerId::new();
        let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
        let identity = Identity::from_pem(
            cert.cert_pem.as_bytes(),
            key.key_pem().as_bytes(),
            ca.cert_pem().as_bytes(),
        )
        .unwrap();
        assert_eq!(identity.agent_id, id);
        assert!(!format!("{identity:?}").contains("key"));
    }

    #[test]
    fn days_left_counts_down_to_expiry() {
        let ca = pki::Ca::generate("t").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let cert = ca
            .issue_agent_cert(ServerId::new(), key.public_key_der())
            .unwrap();
        let identity = Identity::from_pem(
            cert.cert_pem.as_bytes(),
            key.key_pem().as_bytes(),
            ca.cert_pem().as_bytes(),
        )
        .unwrap();
        let (_, to) = pki::validity_unix(&identity.cert).unwrap();
        assert_eq!(identity.days_left(to - 10 * 86_400), Some(10));
        assert_eq!(identity.days_left(to), Some(0));
        assert_eq!(identity.days_left(to + 1), Some(-1));
    }
}
