//! Configuration de l'agent (variables d'environnement) et identite mTLS
//! lue dans son dossier : `agent.key` (cle privee, 0600, jamais transmise),
//! `agent.crt` (certificat emis par la CA de l'instance) et `ca.crt` (CA de
//! l'instance : une, ou deux pendant une rotation de la CA).

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use shared::ids::ServerId;

pub const KEY_FILE: &str = "agent.key";
pub const CERT_FILE: &str = "agent.crt";
pub const CA_FILE: &str = "ca.crt";
/// Nombre maximal de CA dans un jeu `TrustedCas` : la courante et, pendant
/// une rotation, la suivante. Aucune racine de plus ne peut s'y glisser a
/// cote de deux CA valides.
pub const MAX_TRUSTED_CAS: usize = 2;

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
    /// CA de l'instance (au moins une), dans l'ordre de `ca.crt`.
    pub(crate) ca: Vec<CertificateDer<'static>>,
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
    /// `agent.key` accessible au groupe ou aux autres (mode lu).
    KeyMode(u32),
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(file, e) => write!(f, "lecture de {file} : {e}"),
            Self::Pem(file) => write!(f, "{file} illisible (PEM attendu)"),
            Self::NoAgentId => write!(f, "{CERT_FILE} ne porte pas d'identifiant d'agent"),
            Self::KeyMode(mode) => write!(
                f,
                "{KEY_FILE} en mode {mode:o} : accessible au groupe ou aux autres (attendu 0600)"
            ),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Mode admis pour `agent.key` : aucun bit pour le groupe ni les autres.
pub fn key_mode_ok(mode: u32) -> bool {
    mode & 0o077 == 0
}

/// Lit `agent.key` ; sous Unix, refusee si son mode donne un acces au
/// groupe ou aux autres (mode lu sur le fichier ouvert).
fn read_key(path: &Path) -> Result<Vec<u8>, IdentityError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| IdentityError::Read(KEY_FILE, e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = file
            .metadata()
            .map_err(|e| IdentityError::Read(KEY_FILE, e))?
            .permissions()
            .mode()
            & 0o7777;
        if !key_mode_ok(mode) {
            return Err(IdentityError::KeyMode(mode));
        }
    }
    let mut key = Vec::new();
    file.read_to_end(&mut key)
        .map_err(|e| IdentityError::Read(KEY_FILE, e))?;
    Ok(key)
}

impl Identity {
    /// Lit l'identite dans `dir` (fichiers ecrits par `ciphera init`) ;
    /// `agent.key` doit etre en 0600 (ou plus strict).
    pub fn load(dir: &Path) -> Result<Self, IdentityError> {
        let read = |file: &'static str| {
            std::fs::read(dir.join(file)).map_err(|e| IdentityError::Read(file, e))
        };
        let key = read_key(&dir.join(KEY_FILE))?;
        Self::from_pem(&read(CERT_FILE)?, &key, &read(CA_FILE)?)
    }

    pub fn from_pem(cert_pem: &[u8], key_pem: &[u8], ca_pem: &[u8]) -> Result<Self, IdentityError> {
        let cert =
            CertificateDer::from_pem_slice(cert_pem).map_err(|_| IdentityError::Pem(CERT_FILE))?;
        let key =
            PrivateKeyDer::from_pem_slice(key_pem).map_err(|_| IdentityError::Pem(KEY_FILE))?;
        let ca = CertificateDer::pem_slice_iter(ca_pem)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| IdentityError::Pem(CA_FILE))?;
        if ca.is_empty() {
            return Err(IdentityError::Pem(CA_FILE));
        }
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
    /// accepte seulement s'il est signe par une CA de l'instance (`ca.crt`),
    /// porte le meme identifiant et la meme cle publique que le certificat
    /// actuel et n'est pas lui-meme a renouveler a `now` (secondes Unix).
    /// Renvoie son DER ; rien n'est ecrit ici.
    pub fn check_renewed(
        &self,
        cert_pem: &str,
        now: i64,
    ) -> Result<CertificateDer<'static>, &'static str> {
        let cert = CertificateDer::from_pem_slice(cert_pem.as_bytes())
            .map_err(|_| "certificat illisible")?;
        if !self.ca.iter().any(|ca| pki::issued_by(ca, &cert)) {
            return Err("certificat non emis par la CA de l'instance");
        }
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

    /// CA envoyees par le plan de controle (`ControlToAgent::TrustedCas`),
    /// sur une connexion dont le certificat du lien est `link_leaf` (DER).
    /// Adoptees seulement si chaque certificat est une CA racine (au plus
    /// [`MAX_TRUSTED_CAS`]), si `link_leaf` est emis par l'une d'elles
    /// (l'agent pourra se reconnecter) et si le certificat actuel de l'agent
    /// l'est aussi : une fois l'agent passe a une nouvelle CA, aucun jeu qui
    /// l'exclut (retour a l'ancienne seule, racine etrangere) n'est adopte.
    /// `Ok(None)` : jeu identique au sien. Rien n'est ecrit ici.
    ///
    /// La seconde CA d'un jeu de deux peut ne couvrir ni le lien ni l'agent :
    /// c'est le debut d'une rotation (`rotate-ca`), ou la CA suivante arrive
    /// avant le certificat qu'elle emet ([`Self::check_renewed`] exige une CA
    /// deja adoptee) et le lien reste sur la courante jusqu'a `--finish`.
    /// Elle est donc acceptee sur la seule foi du plan de controle, joint par
    /// un lien que la CA courante authentifie.
    pub fn check_trusted(
        &self,
        ca_pem: &str,
        link_leaf: &[u8],
    ) -> Result<Option<Vec<CertificateDer<'static>>>, &'static str> {
        let cas = CertificateDer::pem_slice_iter(ca_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "PEM illisible")?;
        if cas.is_empty() {
            return Err("aucune CA");
        }
        if cas.len() > MAX_TRUSTED_CAS {
            return Err("trop de CA");
        }
        if !cas.iter().all(|ca| pki::is_root_ca(ca)) {
            return Err("certificat qui n'est pas une CA racine");
        }
        if !cas.iter().any(|ca| pki::issued_by(ca, link_leaf)) {
            return Err("certificat du lien emis par aucune de ces CA");
        }
        if !cas.iter().any(|ca| pki::issued_by(ca, &self.cert)) {
            return Err("certificat de l'agent emis par aucune de ces CA");
        }
        if cas == self.ca {
            return Ok(None);
        }
        Ok(Some(cas))
    }

    /// Remplace les CA de confiance ; renvoie les precedentes.
    pub(crate) fn set_cas(
        &mut self,
        cas: Vec<CertificateDer<'static>>,
    ) -> Vec<CertificateDer<'static>> {
        std::mem::replace(&mut self.ca, cas)
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

/// Remplace `<dir>/agent.crt` : voir [`replace_file`].
pub fn replace_cert_file(dir: &Path, pem: &[u8]) -> std::io::Result<()> {
    replace_file(dir, CERT_FILE, pem)
}

/// Remplace `<dir>/<name>` (`agent.crt`, `ca.crt`) : fichier temporaire
/// 0600 du meme dossier, donnees sur disque, renommage, dossier sur disque.
pub fn replace_file(dir: &Path, name: &str, pem: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = dir.join(format!(".{name}.renew"));
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
        .and_then(|()| std::fs::rename(&tmp, dir.join(name)));
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

    /// Certificat renouvele emis par une autre CA (meme identifiant, meme
    /// cle publique) : refuse avant toute ecriture.
    #[test]
    fn agent_rejects_renewed_cert_from_other_ca() {
        let ca = pki::Ca::generate("instance").unwrap();
        let other = pki::Ca::generate("instance").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let id = ServerId::new();
        let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
        let identity = Identity::from_pem(
            cert.cert_pem.as_bytes(),
            key.key_pem().as_bytes(),
            ca.cert_pem().as_bytes(),
        )
        .unwrap();
        let now = pki::validity_unix(&identity.cert).unwrap().0 + 86_400;
        let foreign = other.issue_agent_cert(id, key.public_key_der()).unwrap();
        assert_eq!(
            identity.check_renewed(&foreign.cert_pem, now).unwrap_err(),
            "certificat non emis par la CA de l'instance"
        );
        let renewed = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
        assert!(identity.check_renewed(&renewed.cert_pem, now).is_ok());
    }

    /// `ca.crt` a deux CA (rotation) : un certificat renouvele emis par
    /// l'une ou l'autre est accepte.
    #[test]
    fn identity_trusts_every_ca_of_its_bundle() {
        let ca = pki::Ca::generate("Ciphera CA").unwrap();
        let next = pki::Ca::generate("Ciphera CA 2").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let id = ServerId::new();
        let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
        let bundle = format!("{}{}", ca.cert_pem(), next.cert_pem());
        let identity = Identity::from_pem(
            cert.cert_pem.as_bytes(),
            key.key_pem().as_bytes(),
            bundle.as_bytes(),
        )
        .unwrap();
        let now = pki::validity_unix(&identity.cert).unwrap().0 + 86_400;
        let renewed = next.issue_agent_cert(id, key.public_key_der()).unwrap();
        assert!(identity.check_renewed(&renewed.cert_pem, now).is_ok());
        let other = pki::Ca::generate("Ciphera CA 3").unwrap();
        let foreign = other.issue_agent_cert(id, key.public_key_der()).unwrap();
        assert!(identity.check_renewed(&foreign.cert_pem, now).is_err());
    }

    /// CA envoyees par le plan de controle (`TrustedCas`) : adoptees
    /// seulement si ce sont des CA racines et si le certificat du lien de
    /// la connexion en cours est emis par l'une d'elles (l'agent pourra se
    /// reconnecter) ; un jeu identique ne change rien.
    #[test]
    fn trusted_cas_keep_the_link_reachable() {
        let old = pki::Ca::generate("Ciphera CA").unwrap();
        let next = pki::Ca::generate("Ciphera CA 2").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let id = ServerId::new();
        let cert = old.issue_agent_cert(id, key.public_key_der()).unwrap();
        let identity = Identity::from_pem(
            cert.cert_pem.as_bytes(),
            key.key_pem().as_bytes(),
            old.cert_pem().as_bytes(),
        )
        .unwrap();
        let der = |pem: &str| pki::der_from_pem(pem).unwrap();
        let mut link = old.issue_server_cert(&["localhost".into()]).unwrap();
        let _ = link.key_pem.take();
        let link_old = der(&link.cert_pem);
        let mut link = next.issue_server_cert(&["localhost".into()]).unwrap();
        let _ = link.key_pem.take();
        let link_next = der(&link.cert_pem);

        assert_eq!(
            identity.check_trusted(old.cert_pem(), &link_old),
            Ok(None),
            "inchange"
        );
        let both = format!("{}{}", old.cert_pem(), next.cert_pem());
        let adopted = identity.check_trusted(&both, &link_old).unwrap().unwrap();
        assert_eq!(adopted.len(), 2);
        assert_eq!(
            identity
                .check_trusted(next.cert_pem(), &link_old)
                .unwrap_err(),
            "certificat du lien emis par aucune de ces CA"
        );
        assert_eq!(
            identity.check_trusted("", &link_old).unwrap_err(),
            "aucune CA"
        );
        let leaf = format!("{}{}", old.cert_pem(), cert.cert_pem);
        assert_eq!(
            identity.check_trusted(&leaf, &link_old).unwrap_err(),
            "certificat qui n'est pas une CA racine"
        );
        let many = format!("{both}{both}{both}");
        assert_eq!(
            identity.check_trusted(&many, &link_old).unwrap_err(),
            "trop de CA"
        );
        // Certificat de l'agent encore de l'ancienne CA : la nouvelle seule
        // est refusee, meme sur un lien emis par elle.
        assert_eq!(
            identity
                .check_trusted(next.cert_pem(), &link_next)
                .unwrap_err(),
            "certificat de l'agent emis par aucune de ces CA"
        );
        // L'agent passe a la nouvelle CA (meme cle).
        let mut identity = identity;
        identity.set_cas(adopted);
        let renewed = next.issue_agent_cert(id, key.public_key_der()).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let renewed = identity.check_renewed(&renewed.cert_pem, now).unwrap();
        identity.set_cert(renewed);
        // Retour en arriere refuse : un lien de l'ancienne CA ne peut plus
        // faire adopter l'ancienne seule, ni l'ancienne et une autre racine.
        assert_eq!(
            identity
                .check_trusted(old.cert_pem(), &link_old)
                .unwrap_err(),
            "certificat de l'agent emis par aucune de ces CA"
        );
        let rogue = pki::Ca::generate("Ciphera CA").unwrap();
        let old_and_rogue = format!("{}{}", old.cert_pem(), rogue.cert_pem());
        assert_eq!(
            identity
                .check_trusted(&old_and_rogue, &link_old)
                .unwrap_err(),
            "certificat de l'agent emis par aucune de ces CA"
        );
        // Les deux CA (rotation en cours) restent acceptees.
        assert_eq!(identity.check_trusted(&both, &link_old), Ok(None));
        // Fin de rotation : la nouvelle seule, le lien deja emis par elle.
        assert!(
            identity
                .check_trusted(next.cert_pem(), &link_next)
                .unwrap()
                .is_some()
        );
    }

    /// Au plus deux CA (courante et suivante pendant une rotation) : une
    /// troisieme racine, meme a cote de deux CA valides, fait refuser le jeu.
    #[test]
    fn trusted_cas_are_at_most_two() {
        let old = pki::Ca::generate("Ciphera CA").unwrap();
        let next = pki::Ca::generate("Ciphera CA 2").unwrap();
        let rogue = pki::Ca::generate("Ciphera CA 3").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let cert = old
            .issue_agent_cert(ServerId::new(), key.public_key_der())
            .unwrap();
        let identity = Identity::from_pem(
            cert.cert_pem.as_bytes(),
            key.key_pem().as_bytes(),
            old.cert_pem().as_bytes(),
        )
        .unwrap();
        let mut link = old.issue_server_cert(&["localhost".into()]).unwrap();
        let _ = link.key_pem.take();
        let link = pki::der_from_pem(&link.cert_pem).unwrap();

        let three = format!("{}{}{}", old.cert_pem(), next.cert_pem(), rogue.cert_pem());
        assert_eq!(
            identity.check_trusted(&three, &link).unwrap_err(),
            "trop de CA"
        );
        // Debut de rotation : la suivante ne couvre encore ni le lien ni
        // l'agent (son certificat arrive apres elle) ; acceptee.
        let two = format!("{}{}", old.cert_pem(), next.cert_pem());
        assert_eq!(
            identity.check_trusted(&two, &link).unwrap().unwrap().len(),
            2
        );
    }

    /// `agent.key` lisible par le groupe ou les autres : identite refusee.
    #[test]
    fn agent_key_mode_checked() {
        assert!(key_mode_ok(0o600));
        assert!(key_mode_ok(0o400));
        for bad in [0o640, 0o604, 0o644, 0o660, 0o700 | 0o001] {
            assert!(!key_mode_ok(bad), "{bad:o}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ca = pki::Ca::generate("t").unwrap();
            let key = pki::AgentKey::generate().unwrap();
            let cert = ca
                .issue_agent_cert(ServerId::new(), key.public_key_der())
                .unwrap();
            let dir = std::env::temp_dir().join(format!("ciphera-key-mode-{}", ServerId::new()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(CERT_FILE), &cert.cert_pem).unwrap();
            std::fs::write(dir.join(CA_FILE), ca.cert_pem()).unwrap();
            std::fs::write(dir.join(KEY_FILE), key.key_pem()).unwrap();
            let set = |mode| {
                std::fs::set_permissions(dir.join(KEY_FILE), std::fs::Permissions::from_mode(mode))
                    .unwrap()
            };
            set(0o640);
            let err = Identity::load(&dir).unwrap_err();
            assert!(matches!(err, IdentityError::KeyMode(0o640)), "{err}");
            set(0o600);
            assert!(Identity::load(&dir).is_ok());
            std::fs::remove_dir_all(&dir).unwrap();
        }
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
