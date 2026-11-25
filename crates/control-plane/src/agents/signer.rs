//! Signataire de certificats root, sur le modele de l'updater : la cle de la
//! CA de l'instance (`ca/ca.key`, root 0600) ne quitte jamais root, le plan
//! de controle (compte `ciphera`) n'en garde que le certificat.
//!
//! 1. Aux deux tiers de la validite d'un certificat, le plan de controle
//!    ecrit une demande `<data_dir>/cert-requests/<nom>.json` ([`write_request`],
//!    renommage atomique, 0640) : `<nom>` est l'identifiant du serveur pour
//!    un certificat d'agent, `link` ou `panel` pour ceux du plan de controle.
//! 2. `ciphera-certsign.path` (`PathExistsGlob`) lance `ciphera-certsign.service`
//!    (root, oneshot) : `ciphera admin sign-requests` ([`sign_requests`]).
//! 3. Le signataire ne signe que :
//!    - pour un agent : un certificat emis par la CA, de cet identifiant,
//!      dont l'empreinte est celle enregistree en base pour un serveur non
//!      revoque, et pour la **meme cle publique** (empreinte annoncee
//!      comprise) ; l'empreinte du nouveau certificat est notee en base
//!      (`servers.next_cert_fingerprint`) dans la meme instruction ;
//!    - pour le lien ou le panneau : la meme cle publique que le certificat
//!      en place (`tls/link.crt`, `tls/panel.crt`), emis par la CA, memes SAN.
//!
//!    Duree et usages sont ceux de `pki` (agent : 365 jours, client ;
//!    serveur : 730 jours, serveur). Toute autre forme est refusee, un lien
//!    symbolique n'est jamais suivi (`O_NOFOLLOW` sur des descripteurs de
//!    dossier ouverts sans suivre de lien).
//!
//!    Face a un service compromis, la barriere est « meme cle publique,
//!    certificat emis par la CA, meme identifiant » : les controles en base
//!    (serveur enregistre, non revoque) ne sont qu'une coherence, la base
//!    appartenant au service.
//! 4. La reponse `<data_dir>/cert-responses/<nom>.crt` (dossier root, groupe
//!    du service, 0750 ; fichier 0640) porte l'identifiant de la demande en
//!    premiere ligne, puis le certificat PEM ([`read_response`]) ; la demande
//!    est supprimee, signee ou non (jamais de boucle du `.path`) ; ce qui ne
//!    peut pas l'etre (nom non UTF-8, dossier non vide) est deplace dans
//!    `<data_dir>/cert-rejected/` (root 0700).
//! 5. Le plan de controle n'accepte que la reponse a sa propre demande
//!    (identifiant), la verifie (CA, meme cle) puis l'envoie a l'agent
//!    (`RenewedCertificate`) ou installe le certificat du lien ou du panneau
//!    (voir `agents::certs::Renewals`).
//!
//! `ciphera admin renew-certs` passe par les memes fonctions de signature
//! ([`sign_agent`], [`sign_server`]).
//!
//! Rotation de la CA (`ciphera admin rotate-ca`, `agents::rotation`) : tant
//! que `ca/ca-next.{crt,key}` existent ([`load_cas`]), un certificat emis par
//! la CA courante ou par la suivante est accepte ; les certificats d'agent
//! sont signes par la suivante, ceux du lien et du panneau par la courante
//! (un agent qui n'a pas encore recu la suivante garde un lien joignable).

use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shared::ids::ServerId;
use sqlx::PgPool;
use uuid::Uuid;

use super::certs::CA_KEY;
use super::transport::{CA_CRT, CA_NEXT_CRT, LINK_CRT};

/// Cle privee de la CA suivante, pendant une rotation (root 0600).
pub const CA_NEXT_KEY: &str = "ca/ca-next.key";

/// Dossier des demandes, dans le dossier de donnees du service.
pub const REQUESTS_DIR: &str = "cert-requests";
/// Dossier des reponses, dans le dossier de donnees du service.
pub const RESPONSES_DIR: &str = "cert-responses";
/// Entrees des demandes impossibles a supprimer, mises de cote (root 0700),
/// dans le dossier de donnees du service.
pub const REJECTED_DIR: &str = "cert-rejected";
/// Taille maximale d'une demande ou d'une reponse.
pub const MAX_FILE_BYTES: u64 = 16 * 1024;
/// Demandes traitees au plus par passage (le `.path` relance s'il en reste).
pub const MAX_REQUESTS_PER_RUN: usize = 64;
/// Prefixe de la premiere ligne d'une reponse.
const RESPONSE_HEADER: &str = "Ciphera-Request-Id: ";
/// Taille maximale de la cle et du certificat de la CA.
const MAX_CA_BYTES: u64 = 64 * 1024;

/// Usage du certificat demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Usage {
    /// Certificat client d'un agent.
    Agent,
    /// Certificat serveur du lien QUIC (`tls/link.crt`).
    Link,
    /// Certificat de l'acces direct du panneau (`tls/panel.crt`).
    Panel,
}

impl Usage {
    /// Certificat du plan de controle (relatif au dossier de configuration).
    pub fn own_cert(self) -> Option<&'static str> {
        match self {
            Self::Agent => None,
            Self::Link => Some(LINK_CRT),
            Self::Panel => Some(super::certs::PANEL_CRT),
        }
    }
}

/// Demande de certificat (`cert-requests/<nom>.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CertRequest {
    /// Identifiant de la demande, repris dans la reponse.
    pub request_id: Uuid,
    pub usage: Usage,
    /// Serveur de l'agent (usage `agent` seulement).
    pub server_id: Option<ServerId>,
    /// Empreinte SHA-256 de la cle publique actuelle (SubjectPublicKeyInfo).
    pub public_key_sha256: String,
    /// Certificat actuel de l'agent (usage `agent` seulement) : public, son
    /// empreinte doit etre celle enregistree en base.
    pub cert_pem: Option<String>,
}

impl CertRequest {
    /// Demande pour le certificat actuel `der` de l'agent `id`.
    pub fn agent(id: ServerId, der: &[u8]) -> Option<Self> {
        Some(Self {
            request_id: Uuid::new_v4(),
            usage: Usage::Agent,
            server_id: Some(id),
            public_key_sha256: public_key_sha256(der)?,
            cert_pem: Some(pem_of(der)),
        })
    }

    /// Demande pour un certificat du plan de controle (`usage` lien ou
    /// panneau), de certificat actuel `cert_pem`.
    pub fn own(usage: Usage, cert_pem: &str) -> Option<Self> {
        usage.own_cert()?;
        let der = pki::der_from_pem(cert_pem)?;
        Some(Self {
            request_id: Uuid::new_v4(),
            usage,
            server_id: None,
            public_key_sha256: public_key_sha256(&der)?,
            cert_pem: None,
        })
    }

    /// Nom du fichier de la demande et de la reponse (sans extension).
    pub fn name(&self) -> String {
        match (self.usage, self.server_id) {
            (Usage::Agent, Some(id)) => id.to_string(),
            (Usage::Agent, None) => "agent".into(),
            (Usage::Link, _) => "link".into(),
            (Usage::Panel, _) => "panel".into(),
        }
    }
}

/// Empreinte SHA-256 (`AB:CD:...`) de la cle publique d'un certificat DER.
pub fn public_key_sha256(der: &[u8]) -> Option<String> {
    pki::public_key_der(der).map(|spki| pki::fingerprint_sha256(&spki))
}

fn pem_of(der: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

/// Nom de fichier de demande valide : identifiant de serveur (UUID en
/// minuscules), `link` ou `panel`.
fn valid_name(name: &str) -> bool {
    matches!(name, "link" | "panel") || name.parse::<Uuid>().is_ok_and(|u| u.to_string() == name)
}

/// Ecrit la demande (plan de controle) : fichier temporaire du dossier des
/// demandes, 0640, donnees sur disque, renommage en `<nom>.json`.
pub fn write_request(data_dir: &Path, request: &CertRequest) -> io::Result<()> {
    let dir = data_dir.join(REQUESTS_DIR);
    if fs_symlink(&dir)? {
        return Err(io::Error::other(format!(
            "{} est un lien symbolique",
            dir.display()
        )));
    }
    std::fs::create_dir_all(&dir)?;
    set_mode(&dir, 0o750)?;
    let name = request.name();
    let tmp = dir.join(format!(".{name}.json.tmp"));
    let path = dir.join(format!("{name}.json"));
    let bytes = serde_json::to_vec(request).map_err(io::Error::other)?;
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o640);
    }
    let written = (|| {
        use std::io::Write;
        let mut file = opts.open(&tmp)?;
        // Masque de creation du service (0077) : droits fixes ensuite.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o640))?;
        }
        file.write_all(&bytes)?;
        file.sync_all()
    })();
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, &path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn fs_symlink(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => Ok(m.file_type().is_symlink()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

/// Reponse a la demande `<nom>` (plan de controle) : identifiant de la
/// demande et certificat PEM. `None` : pas de reponse (ou illisible).
pub fn read_response(data_dir: &Path, name: &str) -> io::Result<Option<(Uuid, String)>> {
    use std::io::Read;
    let path = data_dir.join(RESPONSES_DIR).join(format!("{name}.crt"));
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    #[cfg(not(unix))]
    if fs_symlink(&path)? {
        return Ok(None);
    }
    let file = match opts.open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Ok(None);
    }
    let mut text = String::new();
    (&file).take(MAX_FILE_BYTES).read_to_string(&mut text)?;
    Ok(parse_response(&text))
}

fn response_text(request_id: Uuid, cert_pem: &str) -> String {
    format!("{RESPONSE_HEADER}{request_id}\n{cert_pem}")
}

fn parse_response(text: &str) -> Option<(Uuid, String)> {
    let (first, pem) = text.split_once('\n')?;
    let id = first.strip_prefix(RESPONSE_HEADER)?.trim().parse().ok()?;
    pki::der_from_pem(pem)?;
    Some((id, pem.to_string()))
}

/// Nouveau certificat d'agent : `der` emis par la CA, identifiant `id`,
/// meme cle publique. Partage avec `ciphera admin renew-certs`.
pub fn sign_agent(ca: &pki::Ca, der: &[u8], id: ServerId) -> Result<pki::CertBundle, String> {
    sign_agent_with(ca, &[ca], der, id)
}

/// Comme [`sign_agent`] : `der` emis par l'une des CA `trusted`, nouveau
/// certificat signe par `ca`.
pub fn sign_agent_with(
    ca: &pki::Ca,
    trusted: &[&pki::Ca],
    der: &[u8],
    id: ServerId,
) -> Result<pki::CertBundle, String> {
    if !issued_by_one_of(trusted, der) {
        return Err("certificat non emis par la CA de l'instance".into());
    }
    if pki::agent_id_from_cert(der) != Some(id) {
        return Err(format!("certificat sans l'identifiant du serveur {id}"));
    }
    ca.renew_agent_cert(der).map_err(|e| e.to_string())
}

/// Nouveau certificat serveur (lien, panneau) : `pem` emis par la CA, meme
/// cle publique, memes SAN. Partage avec `ciphera admin renew-certs`.
pub fn sign_server(ca: &pki::Ca, pem: &str) -> Result<pki::CertBundle, String> {
    sign_server_with(ca, &[ca], pem)
}

/// Comme [`sign_server`] : `pem` emis par l'une des CA `trusted`, nouveau
/// certificat signe par `ca`.
pub fn sign_server_with(
    ca: &pki::Ca,
    trusted: &[&pki::Ca],
    pem: &str,
) -> Result<pki::CertBundle, String> {
    let der = pki::der_from_pem(pem).ok_or("certificat serveur illisible")?;
    if !issued_by_one_of(trusted, &der) {
        return Err("certificat serveur non emis par la CA de l'instance".into());
    }
    ca.reissue_server_cert(pem).map_err(|e| e.to_string())
}

fn issued_by_one_of(cas: &[&pki::Ca], der: &[u8]) -> bool {
    cas.iter().any(|ca| {
        pki::der_from_pem(ca.cert_pem()).is_some_and(|ca_der| pki::issued_by(&ca_der, der))
    })
}

/// CA de l'instance pour root : courante et, pendant une rotation,
/// suivante.
pub struct Cas {
    pub current: pki::Ca,
    pub next: Option<pki::Ca>,
}

impl fmt::Debug for Cas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cas")
            .field("rotation", &self.next.is_some())
            .finish_non_exhaustive()
    }
}

impl Cas {
    /// CA de confiance : courante, puis suivante.
    pub fn trusted(&self) -> Vec<&pki::Ca> {
        let mut out = vec![&self.current];
        out.extend(self.next.as_ref());
        out
    }

    /// `der` est-il emis par l'une des CA de confiance ?
    pub fn issued(&self, der: &[u8]) -> bool {
        issued_by_one_of(&self.trusted(), der)
    }

    /// Certificat d'agent : signe par la suivante pendant une rotation,
    /// sinon par la courante.
    pub fn sign_agent(&self, der: &[u8], id: ServerId) -> Result<pki::CertBundle, String> {
        let ca = self.next.as_ref().unwrap_or(&self.current);
        sign_agent_with(ca, &self.trusted(), der, id)
    }

    /// Certificat du lien ou du panneau : toujours signe par la courante
    /// (la suivante ne sert au lien qu'a la fin de la rotation).
    pub fn sign_server(&self, pem: &str) -> Result<pki::CertBundle, String> {
        sign_server_with(&self.current, &self.trusted(), pem)
    }
}

/// CA de l'instance, lue par root : `ca/ca.key` fichier ordinaire (jamais un
/// lien), du proprietaire du processus, sans droit pour le groupe ni les
/// autres ; `ca/ca.crt` de la meme cle.
pub fn load_ca(config_dir: &Path) -> io::Result<pki::Ca> {
    let ca = fsio::Dir::open(&config_dir.join("ca"))?;
    let key_name = file_name(CA_KEY);
    let key = zeroize::Zeroizing::new(ca.read_secret(key_name, MAX_CA_BYTES)?);
    let cert = ca.read_file(file_name(CA_CRT), MAX_CA_BYTES)?;
    let key = std::str::from_utf8(&key).map_err(io::Error::other)?;
    let cert = std::str::from_utf8(&cert).map_err(io::Error::other)?;
    pki::Ca::load(cert, key).map_err(io::Error::other)
}

fn file_name(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Releve des certificats du plan de controle au debut d'une rotation
/// (`ca/ca-next.own`, root 0600) : `<certificat> <identite>` par ligne
/// ([`pki::server_identity`]). `rotate-ca --finish` ne re-signe le lien et
/// le panneau que s'ils n'ont pas change depuis.
pub const CA_NEXT_OWN: &str = "ca/ca-next.own";

/// CA courante et, si `ca/ca-next.crt` existe (rotation en cours), CA
/// suivante : `ca/ca-next.key` aux memes conditions que la cle courante
/// ([`load_cas`] pour root seulement). Sous le verrou des paires de `ca/`,
/// apres la reprise d'une fin de rotation interrompue ([`settle_locked`]).
pub fn load_cas(config_dir: &Path) -> io::Result<Cas> {
    load_cas_settled(config_dir).map(|(cas, _)| cas)
}

/// Comme [`load_cas`] ; vrai en plus si une fin de rotation interrompue
/// vient d'etre terminee.
pub fn load_cas_settled(config_dir: &Path) -> io::Result<(Cas, bool)> {
    let _lock = super::certs::lock_pairs(config_dir, CA_CRT)?;
    let ca = fsio::Dir::open(&config_dir.join("ca"))?;
    let finished = settle_locked(config_dir, &ca)?;
    let current = load_ca(config_dir)?;
    let cert = match ca.read_file(file_name(CA_NEXT_CRT), MAX_CA_BYTES) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let cas = Cas {
                current,
                next: None,
            };
            return Ok((cas, finished));
        }
        Err(e) => return Err(e),
    };
    let key = zeroize::Zeroizing::new(ca.read_secret(file_name(CA_NEXT_KEY), MAX_CA_BYTES)?);
    let key = std::str::from_utf8(&key).map_err(io::Error::other)?;
    let cert = std::str::from_utf8(&cert).map_err(io::Error::other)?;
    let next = pki::Ca::load(cert, key).map_err(io::Error::other)?;
    let cas = Cas {
        current,
        next: Some(next),
    };
    Ok((cas, finished))
}

fn read_optional(ca: &fsio::Dir, rel: &str) -> io::Result<Option<Vec<u8>>> {
    match ca.read_file(file_name(rel), MAX_CA_BYTES) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reprise d'une fin de rotation interrompue, sous le verrou des paires de
/// `ca/` (root) :
/// - remplacement de `ca.crt` + `ca.key` interrompu : termine ou annule
///   ([`super::certs::recover_pair`]) ;
/// - `ca-next.crt` identique a `ca.crt` (la suivante est deja la courante) :
///   restes de la suivante retires ;
/// - `ca-next.key` (ou le releve) sans `ca-next.crt` : pas de rotation,
///   restes retires (debut interrompu, ou fin interrompue entre les deux
///   suppressions).
///
/// Vrai si une fin de rotation a ete terminee ici.
fn settle_locked(config_dir: &Path, ca: &fsio::Dir) -> io::Result<bool> {
    let mut finished = super::certs::recover_pair_locked(config_dir, (CA_CRT, CA_KEY))?;
    let next = read_optional(ca, CA_NEXT_CRT)?;
    let stale = match &next {
        Some(next) => {
            let current = read_optional(ca, CA_CRT)?;
            let der = |b: &[u8]| std::str::from_utf8(b).ok().and_then(pki::der_from_pem);
            let same = current
                .as_deref()
                .and_then(der)
                .is_some_and(|c| der(next) == Some(c));
            if same {
                finished = true;
            }
            same
        }
        None => {
            // Cle seule : celle de la CA courante si la fin l'avait deja
            // installee.
            let key = read_optional(ca, CA_NEXT_KEY)?;
            if let Some(key) = key {
                let current = read_optional(ca, CA_KEY)?;
                if current.is_some_and(|c| c == key) {
                    finished = true;
                }
            }
            true
        }
    };
    if stale {
        remove_next_locked(ca)?;
    }
    Ok(finished)
}

/// Fichiers de la suivante retires : certificat d'abord (sans lui, plus de
/// rotation), puis cle et releve, puis dossier sur disque.
fn remove_next_locked(ca: &fsio::Dir) -> io::Result<()> {
    ca.remove(file_name(CA_NEXT_CRT))?;
    ca.remove(file_name(CA_NEXT_KEY))?;
    ca.remove(file_name(CA_NEXT_OWN))?;
    ca.sync()
}

/// Ecrit la CA suivante (debut d'une rotation, root), sous le verrou des
/// paires de `ca/` : releve des certificats du plan de controle (`own`,
/// [`CA_NEXT_OWN`], 0600), `ca/ca-next.key` (0600, proprietaire et groupe
/// du processus) puis `ca/ca-next.crt` (0640, groupe du dossier : le
/// service le lit). Le certificat est ecrit en dernier : sa presence
/// signale la rotation au service. Jamais a travers un lien.
pub fn write_next_ca(config_dir: &Path, next: &pki::Ca, own: &str) -> io::Result<()> {
    let _lock = super::certs::lock_pairs(config_dir, CA_CRT)?;
    let ca = fsio::Dir::open(&config_dir.join("ca"))?;
    ca.write_atomic_mode(file_name(CA_NEXT_OWN), own.as_bytes(), 0o600, true)?;
    ca.write_atomic_mode(
        file_name(CA_NEXT_KEY),
        next.key_pem().as_bytes(),
        0o600,
        true,
    )?;
    ca.write_atomic_mode(
        file_name(CA_NEXT_CRT),
        next.cert_pem().as_bytes(),
        0o640,
        false,
    )?;
    ca.sync()
}

/// Releve des certificats du plan de controle ecrit au debut de la rotation
/// en cours ([`CA_NEXT_OWN`]) ; `None` s'il manque.
pub fn read_next_own(config_dir: &Path) -> io::Result<Option<String>> {
    let ca = fsio::Dir::open(&config_dir.join("ca"))?;
    match ca.read_secret(file_name(CA_NEXT_OWN), MAX_CA_BYTES) {
        Ok(b) => String::from_utf8(b).map(Some).map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Ecrit le releve seul (rotation commencee sans lui), sous le verrou.
pub fn write_next_own(config_dir: &Path, own: &str) -> io::Result<()> {
    let _lock = super::certs::lock_pairs(config_dir, CA_CRT)?;
    let ca = fsio::Dir::open(&config_dir.join("ca"))?;
    ca.write_atomic_mode(file_name(CA_NEXT_OWN), own.as_bytes(), 0o600, true)?;
    ca.sync()
}

/// Retire la CA suivante (fin d'une rotation, apres son installation comme
/// CA courante), sous le verrou des paires de `ca/` : certificat, puis cle
/// et releve, puis dossier sur disque.
pub fn remove_next_ca(config_dir: &Path) -> io::Result<()> {
    let _lock = super::certs::lock_pairs(config_dir, CA_CRT)?;
    remove_next_locked(&fsio::Dir::open(&config_dir.join("ca"))?)
}

/// Dossier de configuration (`/etc/ciphera`) a root, groupe de `ca/` (celui
/// du service, voir [`harden_ca`]), 0750 : le service le traverse et y lit
/// ses fichiers, mais ne peut ni renommer ni remplacer `ca/` ou un autre de
/// ses dossiers. Sans `ca/`, groupe conserve. Jamais a travers un lien.
/// Vrai si quelque chose a change.
pub fn harden_config_dir(config_dir: &Path) -> io::Result<bool> {
    let dir = fsio::Dir::open(config_dir)?;
    let group = match dir.open_sub("ca")? {
        Some(ca) => Some(ca.group()?),
        None => None,
    };
    dir.harden_self(0o750, group)
}

/// Passage d'une installation anterieure (dossier de la CA au compte du
/// service) : `ca/` au proprietaire du processus (root), groupe conserve
/// (celui du service), 0750 ; `ca/ca.crt` idem en 0640 (le service le lit) ;
/// `ca/ca.key` au proprietaire et au groupe du processus, 0600 (de meme pour
/// `ca-next.crt`, `ca-next.key` et le releve pendant une rotation). Le
/// service ne peut alors ni lire la cle ni remplacer ou supprimer la cle ou
/// le certificat. Jamais a travers un lien ; fichier a plusieurs liens durs
/// refuse. Vrai si quelque chose a change. Sans dossier : rien a faire. Une
/// fin de rotation interrompue est reprise par [`load_cas`], sous le verrou.
pub fn harden_ca(config_dir: &Path) -> io::Result<bool> {
    let ca = match fsio::Dir::open(&config_dir.join("ca")) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut changed = ca.harden(file_name(CA_KEY), 0o600, true)?;
    changed |= ca.harden(file_name(CA_CRT), 0o640, false)?;
    changed |= ca.harden(file_name(CA_NEXT_KEY), 0o600, true)?;
    changed |= ca.harden(file_name(CA_NEXT_CRT), 0o640, false)?;
    changed |= ca.harden(file_name(CA_NEXT_OWN), 0o600, true)?;
    changed |= ca.harden_self(0o750, None)?;
    Ok(changed)
}

/// Dossier de donnees ; `None` s'il n'existe pas encore (service jamais
/// demarre : aucune demande).
fn open_data(data_dir: &Path) -> io::Result<Option<fsio::Dir>> {
    match fsio::Dir::open(data_dir) {
        Ok(d) => Ok(Some(d)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Une demande attend-elle (`*.json` du dossier des demandes, nom
/// inutilisable compris : il sera mis de cote) ?
pub fn has_requests(data_dir: &Path) -> io::Result<bool> {
    let Some(data) = open_data(data_dir)? else {
        return Ok(false);
    };
    match data.open_sub(REQUESTS_DIR)? {
        Some(dir) => {
            let (names, unusable) = scan(&dir)?;
            Ok(!names.is_empty() || !unusable.is_empty())
        }
        None => Ok(false),
    }
}

/// Entrees qui correspondent au `PathExistsGlob` du `.path` (`*.json`,
/// fichiers caches exclus : temporaires du plan de controle) : noms UTF-8
/// (sans `.json`, au plus [`MAX_REQUESTS_PER_RUN`]) et noms non UTF-8, a
/// mettre de cote.
fn scan(dir: &fsio::Dir) -> io::Result<(Vec<String>, Vec<OsString>)> {
    let mut names = Vec::new();
    let mut unusable = Vec::new();
    for entry in dir.entries()? {
        let lossy = entry.to_string_lossy();
        if lossy.starts_with('.') || !lossy.ends_with(".json") {
            continue;
        }
        match entry.to_str().and_then(|n| n.strip_suffix(".json")) {
            Some(name) => names.push(name.to_string()),
            None => unusable.push(entry),
        }
    }
    names.sort();
    names.truncate(MAX_REQUESTS_PER_RUN);
    Ok((names, unusable))
}

/// Resultat d'un passage du signataire.
#[derive(Debug, Default)]
pub struct SignReport {
    /// Demandes signees (noms).
    pub signed: Vec<String>,
    /// Demandes refusees : nom et raison.
    pub refused: Vec<(String, String)>,
    /// Entrees impossibles a supprimer (nom non UTF-8, dossier non vide...)
    /// : nom lisible et ce qui en a ete fait.
    pub set_aside: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct SignError(String);

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SignError {}

/// Retire l'entree `name` du dossier des demandes : supprimee, sinon mise
/// de cote. Le `.path` ne doit jamais la retrouver (il relancerait sans
/// fin le signataire, jusqu'aux limites de demarrage de systemd).
fn clear(data: &fsio::Dir, requests: &fsio::Dir, name: &OsStr, report: &mut SignReport) {
    if requests.remove(name).is_ok() {
        return;
    }
    let shown = name.to_string_lossy().into_owned();
    let outcome = set_aside(data, requests, name);
    report.set_aside.push((shown, outcome));
}

/// Deplace `name` dans `<data_dir>/cert-rejected/` (root 0700, nom
/// aleatoire) ; a defaut, le renomme en fichier cache dans le dossier des
/// demandes (hors du motif du `.path`).
fn set_aside(data: &fsio::Dir, requests: &fsio::Dir, name: &OsStr) -> String {
    let new = Uuid::new_v4().to_string();
    let moved = data
        .ensure_sub(REJECTED_DIR, 0o700)
        .and_then(|rejected| requests.move_to(name, &rejected, &new));
    match moved {
        Ok(()) => format!("mise de cote dans {REJECTED_DIR}/{new}"),
        Err(e) => {
            let hidden = format!(".rejected-{new}");
            match requests.move_to(name, requests, &hidden) {
                Ok(()) => format!("renommee {hidden} ({REJECTED_DIR} : {e})"),
                Err(e2) => format!("impossible a retirer : {e} ; {e2}"),
            }
        }
    }
}

/// Supprime toutes les demandes sans les traiter (CA ou base
/// indisponible) : le `.path` ne boucle pas, le plan de controle redemande
/// plus tard. Renvoie le nombre de demandes et ce qui a du etre mis de cote.
pub fn discard_requests(data_dir: &Path) -> io::Result<(usize, SignReport)> {
    let mut report = SignReport::default();
    let Some(data) = open_data(data_dir)? else {
        return Ok((0, report));
    };
    let Some(dir) = data.open_sub(REQUESTS_DIR)? else {
        return Ok((0, report));
    };
    let (names, unusable) = scan(&dir)?;
    for name in &names {
        clear(
            &data,
            &dir,
            OsStr::new(&format!("{name}.json")),
            &mut report,
        );
    }
    for name in &unusable {
        clear(&data, &dir, name, &mut report);
    }
    Ok((names.len(), report))
}

/// `ciphera admin sign-requests` : traite les demandes en attente (voir le
/// module). Chaque demande est retiree apres son traitement, signee ou
/// refusee (supprimee, ou mise de cote si elle ne peut pas l'etre). Erreur
/// (demandes retirees aussi) si la CA est illisible ou si un dossier est un
/// lien symbolique ou n'appartient pas au signataire.
pub async fn sign_requests(
    db: &PgPool,
    config_dir: &Path,
    data_dir: &Path,
) -> Result<SignReport, SignError> {
    let mut report = SignReport::default();
    let Some(data) = open_data(data_dir).map_err(|e| fatal("dossier de donnees", e))? else {
        return Ok(report);
    };
    let Some(requests) = data
        .open_sub(REQUESTS_DIR)
        .map_err(|e| fatal("dossier des demandes", e))?
    else {
        return Ok(report);
    };
    let (names, unusable) = scan(&requests).map_err(|e| fatal("dossier des demandes", e))?;
    for name in &unusable {
        clear(&data, &requests, name, &mut report);
    }
    if names.is_empty() {
        return Ok(report);
    }
    let discard = |report: &mut SignReport| {
        for name in &names {
            clear(
                &data,
                &requests,
                OsStr::new(&format!("{name}.json")),
                report,
            );
        }
    };
    let cas = match load_cas(config_dir) {
        Ok(cas) => cas,
        Err(e) => {
            discard(&mut report);
            return Err(fatal_after(&report, "CA de l'instance illisible", e));
        }
    };
    let responses = match data.ensure_sub(RESPONSES_DIR, 0o750) {
        Ok(d) => d,
        Err(e) => {
            discard(&mut report);
            return Err(fatal_after(&report, "dossier des reponses", e));
        }
    };
    for name in names {
        let file = format!("{name}.json");
        let outcome = sign_one(db, &cas, config_dir, &requests, &responses, &name, &file).await;
        clear(&data, &requests, OsStr::new(&file), &mut report);
        match outcome {
            Ok(()) => report.signed.push(name),
            Err(why) => report.refused.push((name, why)),
        }
    }
    Ok(report)
}

fn fatal(what: &str, e: impl fmt::Display) -> SignError {
    SignError(format!("{what} : {e}"))
}

/// Erreur fatale, avec ce qui a deja du etre mis de cote (journalise une
/// seule fois, par l'appelant).
fn fatal_after(report: &SignReport, what: &str, e: impl fmt::Display) -> SignError {
    let mut message = format!("{what} : {e}");
    for (name, outcome) in &report.set_aside {
        let _ = write!(message, " ; entree {name} : {outcome}");
    }
    SignError(message)
}

async fn sign_one(
    db: &PgPool,
    cas: &Cas,
    config_dir: &Path,
    requests: &fsio::Dir,
    responses: &fsio::Dir,
    name: &str,
    file: &str,
) -> Result<(), String> {
    if !valid_name(name) {
        return Err("nom de demande invalide".into());
    }
    let bytes = requests
        .read_file(file, MAX_FILE_BYTES)
        .map_err(|e| format!("demande illisible : {e}"))?;
    let request: CertRequest =
        serde_json::from_slice(&bytes).map_err(|e| format!("demande mal formee : {e}"))?;
    if request.name() != name {
        return Err("nom de fichier different du contenu de la demande".into());
    }
    let cert_pem = match (request.usage, request.server_id, &request.cert_pem) {
        (Usage::Agent, Some(id), Some(pem)) => {
            sign_agent_request(db, cas, id, &request, pem).await?
        }
        (Usage::Link | Usage::Panel, None, None) => {
            let rel = request.usage.own_cert().unwrap_or(LINK_CRT);
            sign_own_request(cas, config_dir, rel, &request)?
        }
        _ => return Err("champs incoherents avec l'usage".into()),
    };
    responses
        .write_atomic(
            &format!("{name}.crt"),
            response_text(request.request_id, &cert_pem).as_bytes(),
        )
        .map_err(|e| format!("reponse non ecrite : {e}"))
}

async fn sign_agent_request(
    db: &PgPool,
    cas: &Cas,
    id: ServerId,
    request: &CertRequest,
    pem: &str,
) -> Result<String, String> {
    let der = pki::der_from_pem(pem).ok_or("certificat de l'agent illisible")?;
    if public_key_sha256(&der).as_deref() != Some(request.public_key_sha256.as_str()) {
        return Err("cle publique differente du certificat joint".into());
    }
    let bundle = cas.sign_agent(&der, id)?;
    let current = pki::fingerprint_sha256(&der);
    // Enregistre, non revoque, empreinte actuelle : controle et note en une
    // instruction.
    let noted = sqlx::query(
        "UPDATE servers SET next_cert_fingerprint = $3 \
         WHERE id = $1 AND cert_fingerprint = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(&current)
    .bind(&bundle.fingerprint_sha256)
    .execute(db)
    .await
    .map_err(|e| format!("base de donnees : {e}"))?
    .rows_affected();
    if noted == 1 {
        return Ok(bundle.cert_pem);
    }
    let revoked: Option<bool> =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM servers WHERE id = $1")
            .bind(id)
            .fetch_optional(db)
            .await
            .map_err(|e| format!("base de donnees : {e}"))?;
    Err(match revoked {
        None => format!("serveur {id} inconnu"),
        Some(true) => format!("serveur {id} revoque"),
        Some(false) => format!("certificat {current} non enregistre pour le serveur {id}"),
    })
}

fn sign_own_request(
    cas: &Cas,
    config_dir: &Path,
    rel: &str,
    request: &CertRequest,
) -> Result<String, String> {
    let path = config_dir.join(rel);
    let dir = fsio::Dir::open(path.parent().unwrap_or(config_dir))
        .map_err(|e| format!("dossier de {rel} : {e}"))?;
    let bytes = dir
        .read_file(file_name(rel), MAX_FILE_BYTES)
        .map_err(|e| format!("{rel} illisible : {e}"))?;
    let pem = String::from_utf8(bytes).map_err(|_| format!("{rel} illisible"))?;
    let der = pki::der_from_pem(&pem).ok_or_else(|| format!("{rel} illisible"))?;
    if public_key_sha256(&der).as_deref() != Some(request.public_key_sha256.as_str()) {
        return Err(format!("cle publique differente de celle de {rel}"));
    }
    Ok(cas.sign_server(&pem)?.cert_pem)
}

/// Acces aux fichiers du signataire : sous Unix, descripteurs de dossier
/// ouverts sans suivre de lien et fichiers ouverts relativement a eux
/// (`openat`, `O_NOFOLLOW`) ; ailleurs (developpement), chemins avec refus
/// des liens.
#[cfg(unix)]
mod fsio {
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use rustix::fs::{AtFlags, CWD, FileType, Mode, OFlags};

    pub struct Dir {
        fd: OwnedFd,
    }

    fn dir_flags() -> OFlags {
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
    }

    fn euid() -> u32 {
        rustix::process::geteuid().as_raw()
    }

    impl Dir {
        /// Dossier `path` (dernier composant jamais suivi s'il est un lien).
        pub fn open(path: &Path) -> io::Result<Dir> {
            let fd = rustix::fs::openat(CWD, path, dir_flags(), Mode::empty())?;
            Ok(Dir { fd })
        }

        /// Sous-dossier `name` ; `None` s'il n'existe pas.
        pub fn open_sub(&self, name: &str) -> io::Result<Option<Dir>> {
            match rustix::fs::openat(&self.fd, name, dir_flags(), Mode::empty()) {
                Ok(fd) => Ok(Some(Dir { fd })),
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(e) => Err(e.into()),
            }
        }

        /// Sous-dossier `name` du proprietaire du processus, cree au besoin
        /// (`mode`, groupe de ce dossier : celui du service).
        pub fn ensure_sub(&self, name: &str, mode: u32) -> io::Result<Dir> {
            let dir = match self.open_sub(name)? {
                Some(d) => d,
                None => {
                    match rustix::fs::mkdirat(&self.fd, name, Mode::from_raw_mode(mode)) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(e) => return Err(e.into()),
                    }
                    self.open_sub(name)?
                        .ok_or_else(|| io::Error::other(format!("{name} absent")))?
                }
            };
            let st = rustix::fs::fstat(&dir.fd)?;
            if st.st_uid != euid() {
                return Err(io::Error::other(format!(
                    "{name} n'appartient pas au signataire"
                )));
            }
            let gid = rustix::fs::fstat(&self.fd)?.st_gid;
            let file = File::from(dir.fd.try_clone()?);
            std::os::unix::fs::fchown(&file, None, Some(gid))?;
            rustix::fs::fchmod(&dir.fd, Mode::from_raw_mode(mode))?;
            Ok(dir)
        }

        /// Noms des entrees, octets compris (un nom non UTF-8 reste
        /// manipulable : jamais de conversion avec perte).
        pub fn entries(&self) -> io::Result<Vec<OsString>> {
            let mut out = Vec::new();
            for entry in rustix::fs::Dir::read_from(&self.fd)? {
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                if name != b"." && name != b".." {
                    out.push(OsStr::from_bytes(name).to_os_string());
                }
            }
            Ok(out)
        }

        /// Ce dossier au proprietaire du processus, `mode`, groupe `group`
        /// (`None` : conserve).
        pub fn harden_self(&self, mode: u32, group: Option<u32>) -> io::Result<bool> {
            let st = rustix::fs::fstat(&self.fd)?;
            let mut changed = false;
            // Droits d'abord, proprietaire ensuite : jamais d'instant ou le
            // dossier est a root avec des droits encore a resserrer (ni ou
            // le groupe du service perd sa lecture).
            if st.st_mode & 0o7777 != mode {
                rustix::fs::fchmod(&self.fd, Mode::from_raw_mode(mode))?;
                changed = true;
            }
            let group = group.filter(|g| *g != st.st_gid);
            if st.st_uid != euid() || group.is_some() {
                let file = File::from(self.fd.try_clone()?);
                std::os::unix::fs::fchown(&file, Some(euid()), group)?;
                changed = true;
            }
            Ok(changed)
        }

        /// Groupe de ce dossier.
        pub fn group(&self) -> io::Result<u32> {
            Ok(rustix::fs::fstat(&self.fd)?.st_gid)
        }

        /// Ce dossier sur disque (creations, renommages et suppressions
        /// durables).
        pub fn sync(&self) -> io::Result<()> {
            rustix::fs::fsync(&self.fd).map_err(Into::into)
        }

        /// Deplace l'entree `name` dans `to` sous le nom `new` (un lien est
        /// deplace lui-meme, jamais suivi).
        pub fn move_to(&self, name: &OsStr, to: &Dir, new: &str) -> io::Result<()> {
            rustix::fs::renameat(&self.fd, name, &to.fd, new).map_err(Into::into)
        }

        fn open_file(&self, name: &str, max: u64) -> io::Result<(File, rustix::fs::Stat)> {
            let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
            let fd = rustix::fs::openat(&self.fd, name, flags, Mode::empty()).map_err(|e| {
                if e == rustix::io::Errno::LOOP {
                    io::Error::other("lien symbolique refuse")
                } else {
                    e.into()
                }
            })?;
            let st = rustix::fs::fstat(&fd)?;
            if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
                return Err(io::Error::other("fichier ordinaire attendu"));
            }
            if u64::try_from(st.st_size).unwrap_or(u64::MAX) > max {
                return Err(io::Error::other("fichier trop long"));
            }
            Ok((File::from(fd), st))
        }

        pub fn read_file(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
            let (file, _) = self.open_file(name, max)?;
            read_bounded(file, max)
        }

        /// Comme [`Dir::read_file`], fichier du proprietaire du processus
        /// sans droit pour le groupe ni les autres.
        pub fn read_secret(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
            let (file, st) = self.open_file(name, max)?;
            if st.st_uid != euid() || st.st_mode & 0o077 != 0 {
                return Err(io::Error::other(format!(
                    "{name} : proprietaire root et droits 0600 attendus (ciphera admin sign-requests)"
                )));
            }
            read_bounded(file, max)
        }

        /// Fichier `name` au proprietaire du processus, `mode` ; groupe du
        /// processus si `own_group`, sinon conserve.
        pub fn harden(&self, name: &str, mode: u32, own_group: bool) -> io::Result<bool> {
            let (file, st) = match self.open_file(name, super::MAX_CA_BYTES) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e),
            };
            if st.st_nlink != 1 {
                return Err(io::Error::other(format!("{name} : plusieurs liens durs")));
            }
            let meta = file.metadata()?;
            let mut changed = false;
            // Droits d'abord (resserres, sans bit setid), proprietaire
            // ensuite : aucune fenetre de lecture elargie ni refusee.
            if meta.mode() & 0o7777 != mode {
                rustix::fs::fchmod(file.as_fd(), Mode::from_raw_mode(mode))?;
                changed = true;
            }
            let uid = euid();
            let gid = own_group.then(|| rustix::process::getegid().as_raw());
            if meta.uid() != uid || gid.is_some_and(|g| g != meta.gid()) {
                std::os::unix::fs::fchown(&file, Some(uid), gid)?;
                changed = true;
            }
            Ok(changed)
        }

        pub fn remove<N: AsRef<OsStr> + ?Sized>(&self, name: &N) -> io::Result<()> {
            let name = name.as_ref();
            match rustix::fs::unlinkat(&self.fd, name, AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
                Err(rustix::io::Errno::ISDIR) | Err(rustix::io::Errno::PERM) => {
                    rustix::fs::unlinkat(&self.fd, name, AtFlags::REMOVEDIR).map_err(Into::into)
                }
                Err(e) => Err(e.into()),
            }
        }

        /// Ecrit `name` : temporaire cree sans suivre de lien (`O_EXCL`),
        /// 0640, groupe de ce dossier, donnees sur disque, renommage.
        pub fn write_atomic(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
            self.write_atomic_mode(name, bytes, 0o640, false)
        }

        /// Comme [`Dir::write_atomic`], droits `mode`, groupe du processus si
        /// `own_group` (sinon celui de ce dossier).
        pub fn write_atomic_mode(
            &self,
            name: &str,
            bytes: &[u8],
            mode: u32,
            own_group: bool,
        ) -> io::Result<()> {
            let tmp = format!(".{name}.tmp");
            self.remove(tmp.as_str())?;
            let flags =
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let fd = rustix::fs::openat(&self.fd, tmp.as_str(), flags, Mode::from_raw_mode(mode))?;
            let gid = if own_group {
                rustix::process::getegid().as_raw()
            } else {
                rustix::fs::fstat(&self.fd)?.st_gid
            };
            let mut file = File::from(fd);
            let written = std::os::unix::fs::fchown(&file, None, Some(gid))
                .and_then(|()| {
                    rustix::fs::fchmod(file.as_fd(), Mode::from_raw_mode(mode)).map_err(Into::into)
                })
                .and_then(|()| file.write_all(bytes))
                .and_then(|()| file.sync_all())
                .and_then(|()| {
                    rustix::fs::renameat(&self.fd, tmp.as_str(), &self.fd, name).map_err(Into::into)
                });
            if written.is_err() {
                let _ = self.remove(tmp.as_str());
            }
            written
        }
    }

    fn read_bounded(file: File, max: u64) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        file.take(max + 1).read_to_end(&mut out)?;
        if out.len() as u64 > max {
            return Err(io::Error::other("fichier trop long"));
        }
        Ok(out)
    }
}

#[cfg(not(unix))]
mod fsio {
    use std::ffi::{OsStr, OsString};
    use std::io::{self, Read};
    use std::path::{Path, PathBuf};

    pub struct Dir {
        path: PathBuf,
    }

    fn refuse_link(path: &Path) -> io::Result<std::fs::Metadata> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::other("lien symbolique refuse"));
        }
        Ok(meta)
    }

    impl Dir {
        pub fn open(path: &Path) -> io::Result<Dir> {
            if !refuse_link(path)?.is_dir() {
                return Err(io::Error::other("dossier attendu"));
            }
            Ok(Dir {
                path: path.to_path_buf(),
            })
        }

        pub fn open_sub(&self, name: &str) -> io::Result<Option<Dir>> {
            match Dir::open(&self.path.join(name)) {
                Ok(d) => Ok(Some(d)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        pub fn ensure_sub(&self, name: &str, _mode: u32) -> io::Result<Dir> {
            let path = self.path.join(name);
            if std::fs::symlink_metadata(&path).is_err() {
                std::fs::create_dir(&path)?;
            }
            Dir::open(&path)
        }

        pub fn entries(&self) -> io::Result<Vec<OsString>> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&self.path)? {
                out.push(entry?.file_name());
            }
            Ok(out)
        }

        pub fn harden_self(&self, _mode: u32, _group: Option<u32>) -> io::Result<bool> {
            Ok(false)
        }

        pub fn group(&self) -> io::Result<u32> {
            Ok(0)
        }

        pub fn sync(&self) -> io::Result<()> {
            Ok(())
        }

        pub fn move_to(&self, name: &OsStr, to: &Dir, new: &str) -> io::Result<()> {
            std::fs::rename(self.path.join(name), to.path.join(new))
        }

        pub fn read_file(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
            let path = self.path.join(name);
            let meta = refuse_link(&path)?;
            if !meta.is_file() {
                return Err(io::Error::other("fichier ordinaire attendu"));
            }
            if meta.len() > max {
                return Err(io::Error::other("fichier trop long"));
            }
            let mut out = Vec::new();
            std::fs::File::open(&path)?
                .take(max + 1)
                .read_to_end(&mut out)?;
            if out.len() as u64 > max {
                return Err(io::Error::other("fichier trop long"));
            }
            Ok(out)
        }

        pub fn read_secret(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
            self.read_file(name, max)
        }

        pub fn harden(&self, name: &str, _mode: u32, _own_group: bool) -> io::Result<bool> {
            match refuse_link(&self.path.join(name)) {
                Ok(_) => Ok(false),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        }

        pub fn remove<N: AsRef<OsStr> + ?Sized>(&self, name: &N) -> io::Result<()> {
            let path = self.path.join(name.as_ref());
            let result = match std::fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() => std::fs::remove_dir(&path),
                Ok(_) => std::fs::remove_file(&path),
                Err(e) => Err(e),
            };
            match result {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }

        pub fn write_atomic_mode(
            &self,
            name: &str,
            bytes: &[u8],
            _mode: u32,
            _own_group: bool,
        ) -> io::Result<()> {
            self.write_atomic(name, bytes)
        }

        pub fn write_atomic(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
            let tmp = self.path.join(format!(".{name}.tmp"));
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, self.path.join(name)).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_server_ids_or_own_certs() {
        let id = ServerId::new();
        assert!(valid_name(&id.to_string()));
        assert!(valid_name("link") && valid_name("panel"));
        assert!(!valid_name(&id.to_string().to_uppercase()));
        assert!(!valid_name("../x") && !valid_name("agent") && !valid_name(""));
    }

    #[test]
    fn response_carries_the_request_id() {
        let ca = pki::Ca::generate("t").unwrap();
        let cert = ca.issue_server_cert(&["localhost".into()]).unwrap();
        let id = Uuid::new_v4();
        let text = response_text(id, &cert.cert_pem);
        assert_eq!(parse_response(&text), Some((id, cert.cert_pem.clone())));
        assert_eq!(parse_response(&cert.cert_pem), None);
        assert_eq!(parse_response("Ciphera-Request-Id: x\n"), None);
    }

    #[test]
    fn agent_request_pem_round_trips() {
        let ca = pki::Ca::generate("t").unwrap();
        let key = pki::AgentKey::generate().unwrap();
        let id = ServerId::new();
        let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
        let der = pki::der_from_pem(&cert.cert_pem).unwrap();
        let request = CertRequest::agent(id, &der).unwrap();
        assert_eq!(request.name(), id.to_string());
        let pem = request.cert_pem.as_deref().unwrap();
        assert_eq!(pki::der_from_pem(pem).unwrap(), der);
        assert_eq!(
            request.public_key_sha256,
            pki::fingerprint_sha256(key.public_key_der())
        );
    }
}
