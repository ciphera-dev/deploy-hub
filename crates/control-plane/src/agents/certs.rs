//! Renouvellement des certificats emis par la CA de l'instance (`ciphera
//! init`) :
//!
//! - lien des agents (`tls/link.{crt,key}`) et acces direct du panneau
//!   (`tls/panel.{crt,key}`), 730 jours : renouveles par le plan de controle
//!   lui-meme aux deux tiers de leur validite, au demarrage puis toutes les
//!   [`CHECK_EVERY`], avec rechargement a chaud ([`renew_own`]) ;
//! - certificat client de chaque agent, 365 jours : a la connexion d'un
//!   agent dont le certificat a passe les deux tiers, un nouveau certificat
//!   (meme cle, meme identifiant) lui est envoye sur le lien
//!   (`ControlToAgent::RenewedCertificate`, [`renew_agent`]) et son empreinte
//!   notee en `servers.next_cert_fingerprint` ; elle remplace l'ancienne a
//!   la premiere connexion qui la presente ;
//! - certificat deja expire (machine eteinte longtemps) : `ciphera admin
//!   renew-certs` ([`renew_all`]), voir `docs/operations/certificats.md`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use shared::ids::ServerId;
use sqlx::PgPool;

use super::servers;
use super::transport::{CA_CRT, LINK_CRT, LINK_KEY};

/// Cle privee de la CA (relative au dossier de configuration).
pub const CA_KEY: &str = "ca/ca.key";
/// Certificat et cle de l'acces direct du panneau.
pub const PANEL_CRT: &str = "tls/panel.crt";
pub const PANEL_KEY: &str = "tls/panel.key";
/// Certificat de l'agent dans son dossier (`/etc/ciphera-agent`).
pub const AGENT_CRT: &str = "agent.crt";
/// Intervalle de verification des certificats du plan de controle.
pub const CHECK_EVERY: Duration = Duration::from_secs(12 * 3600);

/// Certificats serveur du plan de controle : (certificat, cle).
pub const OWN_CERTS: [(&str, &str); 2] = [(LINK_CRT, LINK_KEY), (PANEL_CRT, PANEL_KEY)];

/// Secondes Unix.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// CA de l'instance (`ca/ca.crt`, `ca/ca.key`).
pub fn load_ca(config_dir: &Path) -> io::Result<pki::Ca> {
    let cert = fs::read_to_string(config_dir.join(CA_CRT))?;
    let key = zeroize::Zeroizing::new(fs::read_to_string(config_dir.join(CA_KEY))?);
    pki::Ca::load(&cert, &key).map_err(io::Error::other)
}

/// Remplace `path` par `bytes` : fichier temporaire du meme dossier, au
/// proprietaire et aux droits du fichier remplace (0600 s'il n'existe pas),
/// donnees sur disque, renommage, dossier sur disque. Jamais a travers un
/// lien.
pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.renew"));
    let previous = match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() || !m.is_file() => {
            return Err(io::Error::other(format!(
                "{} n'est pas un fichier ordinaire",
                path.display()
            )));
        }
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let _ = fs::remove_file(&tmp);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        opts.mode(previous.as_ref().map_or(0o600, |m| m.mode() & 0o777));
    }
    let mut file = opts.open(&tmp)?;
    #[cfg(unix)]
    if let Some(m) = &previous {
        use std::os::unix::fs::MetadataExt;
        std::os::unix::fs::fchown(&file, Some(m.uid()), Some(m.gid()))?;
    }
    #[cfg(not(unix))]
    let _ = previous;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written?;
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

/// Renouvelle le certificat serveur `crt` (et sa cle `key`) s'il a passe
/// les deux tiers de sa validite, ou toujours avec `force`. Vrai si
/// renouvele. La cle est ecrite avant le certificat.
pub fn renew_server(
    config_dir: &Path,
    ca: &pki::Ca,
    (crt, key): (&str, &str),
    now: i64,
    force: bool,
) -> io::Result<bool> {
    let pem = fs::read_to_string(config_dir.join(crt))?;
    let due = pki::der_from_pem(&pem)
        .and_then(|der| pki::renewal_due(&der, now))
        .unwrap_or(true);
    if !due && !force {
        return Ok(false);
    }
    let mut bundle = ca.renew_server_cert(&pem).map_err(io::Error::other)?;
    let new_key = bundle.key_pem.take().unwrap_or_default();
    replace_file(&config_dir.join(key), new_key.as_bytes())?;
    replace_file(&config_dir.join(crt), bundle.cert_pem.as_bytes())?;
    Ok(true)
}

/// Certificats du lien et du panneau a renouveler : renouveles. Renvoie les
/// certificats renouveles ; une erreur est journalisee et n'arrete rien.
pub fn renew_own(config_dir: &Path, ca: &pki::Ca, now: i64) -> Vec<&'static str> {
    let mut renewed = Vec::new();
    for pair in OWN_CERTS {
        match renew_server(config_dir, ca, pair, now, false) {
            Ok(true) => {
                tracing::info!("certificat {} renouvele", pair.0);
                renewed.push(pair.0);
            }
            Ok(false) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => tracing::error!("renouvellement de {} impossible : {e}", pair.0),
        }
    }
    renewed
}

/// Agent connecte avec le certificat `leaf` (empreinte `fingerprint`,
/// l'actuelle) : s'il a passe les deux tiers de sa validite, nouveau
/// certificat pour la meme cle, note comme suivant en base ; son PEM est a
/// envoyer a l'agent. `None` : pas encore du (ou deja note par une autre
/// connexion).
pub async fn renew_agent(
    db: &PgPool,
    ca: &pki::Ca,
    id: ServerId,
    fingerprint: &str,
    leaf: &[u8],
    now: i64,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    if pki::renewal_due(leaf, now) != Some(true) {
        return Ok(None);
    }
    let bundle = ca.renew_agent_cert(leaf)?;
    if !servers::set_next_cert(db, id, fingerprint, &bundle.fingerprint_sha256).await? {
        return Ok(None);
    }
    Ok(Some(bundle.cert_pem))
}

/// Resultat de `ciphera admin renew-certs`.
#[derive(Debug)]
pub struct RenewAll {
    pub server_certs: Vec<&'static str>,
    pub agent: Option<(ServerId, PathBuf)>,
}

/// Reprise manuelle (root) : certificats du lien et du panneau renouveles
/// quelle que soit leur date, et certificat de l'agent local (meme cle) ;
/// la nouvelle empreinte de l'agent est notee en base avant l'ecriture du
/// fichier, puis remplace l'ancienne a sa premiere connexion. Services a
/// redemarrer ensuite.
pub async fn renew_all(
    db: &PgPool,
    config_dir: &Path,
    agent_dir: &Path,
) -> Result<RenewAll, Box<dyn std::error::Error>> {
    let ca = load_ca(config_dir).map_err(|e| format!("CA de l'instance illisible : {e}"))?;
    let now = now_unix();
    let mut server_certs = Vec::new();
    for pair in OWN_CERTS {
        renew_server(config_dir, &ca, pair, now, true).map_err(|e| format!("{} : {e}", pair.0))?;
        server_certs.push(pair.0);
    }
    let agent_crt = agent_dir.join(AGENT_CRT);
    let agent = match fs::read_to_string(&agent_crt) {
        Ok(pem) => {
            let der = pki::der_from_pem(&pem).ok_or("certificat de l'agent illisible")?;
            let id =
                pki::agent_id_from_cert(&der).ok_or("certificat de l'agent sans identifiant")?;
            let bundle = ca.renew_agent_cert(&der)?;
            let noted = sqlx::query(
                "UPDATE servers SET next_cert_fingerprint = $2 \
                 WHERE id = $1 AND revoked_at IS NULL",
            )
            .bind(id)
            .bind(&bundle.fingerprint_sha256)
            .execute(db)
            .await?
            .rows_affected();
            if noted == 0 {
                return Err(
                    format!("serveur {id} inconnu ou revoque : agent non renouvele").into(),
                );
            }
            replace_file(&agent_crt, bundle.cert_pem.as_bytes())
                .map_err(|e| format!("{} : {e}", agent_crt.display()))?;
            // Copie publique lue au demarrage du plan de controle.
            let local = config_dir.join(servers::LOCAL_AGENT_CERT);
            if local.exists() {
                replace_file(&local, bundle.cert_pem.as_bytes())
                    .map_err(|e| format!("{} : {e}", local.display()))?;
            }
            Some((id, agent_crt))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{} : {e}", agent_crt.display()).into()),
    };
    Ok(RenewAll {
        server_certs,
        agent,
    })
}
