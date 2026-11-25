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
//! 4. La reponse `<data_dir>/cert-responses/<nom>.crt` (dossier root, groupe
//!    du service, 0750 ; fichier 0640) porte l'identifiant de la demande en
//!    premiere ligne, puis le certificat PEM ([`read_response`]) ; la demande
//!    est supprimee, signee ou non (jamais de boucle du `.path`).
//! 5. Le plan de controle n'accepte que la reponse a sa propre demande
//!    (identifiant), la verifie (CA, meme cle) puis l'envoie a l'agent
//!    (`RenewedCertificate`) ou installe le certificat du lien ou du panneau
//!    (voir `agents::certs::Renewals`).
//!
//! `ciphera admin renew-certs` passe par les memes fonctions de signature
//! ([`sign_agent`], [`sign_server`]).

use std::fmt;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use shared::ids::ServerId;
use sqlx::PgPool;
use uuid::Uuid;

use super::certs::CA_KEY;
use super::transport::{CA_CRT, LINK_CRT};

/// Dossier des demandes, dans le dossier de donnees du service.
pub const REQUESTS_DIR: &str = "cert-requests";
/// Dossier des reponses, dans le dossier de donnees du service.
pub const RESPONSES_DIR: &str = "cert-responses";
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
    let ca_der = pki::der_from_pem(ca.cert_pem()).ok_or("certificat de la CA illisible")?;
    if !pki::issued_by(&ca_der, der) {
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
    let ca_der = pki::der_from_pem(ca.cert_pem()).ok_or("certificat de la CA illisible")?;
    let der = pki::der_from_pem(pem).ok_or("certificat serveur illisible")?;
    if !pki::issued_by(&ca_der, &der) {
        return Err("certificat serveur non emis par la CA de l'instance".into());
    }
    ca.reissue_server_cert(pem).map_err(|e| e.to_string())
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

/// Passage d'une installation anterieure (cle de la CA au compte du
/// service) : `ca/ca.key` remise au proprietaire du processus (root), 0600.
/// Jamais a travers un lien ; refusee si elle a plusieurs liens durs. Vrai
/// si quelque chose a change. Sans dossier ou sans cle : rien a faire.
pub fn harden_ca_key(config_dir: &Path) -> io::Result<bool> {
    let ca = match fsio::Dir::open(&config_dir.join("ca")) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    ca.harden(file_name(CA_KEY))
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

/// Une demande attend-elle (`*.json` du dossier des demandes) ?
pub fn has_requests(data_dir: &Path) -> io::Result<bool> {
    let Some(data) = open_data(data_dir)? else {
        return Ok(false);
    };
    match data.open_sub(REQUESTS_DIR)? {
        Some(dir) => Ok(!request_names(&dir)?.is_empty()),
        None => Ok(false),
    }
}

/// Noms (sans `.json`) des demandes : `*.json`, fichiers caches (fichiers
/// temporaires du plan de controle) exclus.
fn request_names(dir: &fsio::Dir) -> io::Result<Vec<String>> {
    let mut names: Vec<String> = dir
        .names()?
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
        .collect();
    names.sort();
    names.truncate(MAX_REQUESTS_PER_RUN);
    Ok(names)
}

/// Resultat d'un passage du signataire.
#[derive(Debug, Default)]
pub struct SignReport {
    /// Demandes signees (noms).
    pub signed: Vec<String>,
    /// Demandes refusees : nom et raison.
    pub refused: Vec<(String, String)>,
}

#[derive(Debug)]
pub struct SignError(String);

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SignError {}

/// Supprime toutes les demandes sans les traiter (CA ou base
/// indisponible) : le `.path` ne boucle pas, le plan de controle redemande
/// plus tard.
pub fn discard_requests(data_dir: &Path) -> io::Result<usize> {
    let Some(data) = open_data(data_dir)? else {
        return Ok(0);
    };
    let Some(dir) = data.open_sub(REQUESTS_DIR)? else {
        return Ok(0);
    };
    let names = request_names(&dir)?;
    for name in &names {
        dir.remove(&format!("{name}.json"))?;
    }
    Ok(names.len())
}

/// `ciphera admin sign-requests` : traite les demandes en attente (voir le
/// module). Chaque demande est supprimee apres son traitement, signee ou
/// refusee. Erreur (demandes supprimees aussi) si la CA est illisible ou si
/// un dossier est un lien symbolique ou n'appartient pas au signataire.
pub async fn sign_requests(
    db: &PgPool,
    config_dir: &Path,
    data_dir: &Path,
) -> Result<SignReport, SignError> {
    let Some(data) = open_data(data_dir).map_err(|e| fatal("dossier de donnees", e))? else {
        return Ok(SignReport::default());
    };
    let Some(requests) = data
        .open_sub(REQUESTS_DIR)
        .map_err(|e| fatal("dossier des demandes", e))?
    else {
        return Ok(SignReport::default());
    };
    let names = request_names(&requests).map_err(|e| fatal("dossier des demandes", e))?;
    if names.is_empty() {
        return Ok(SignReport::default());
    }
    let discard = |requests: &fsio::Dir| {
        for name in &names {
            let _ = requests.remove(&format!("{name}.json"));
        }
    };
    let ca = match load_ca(config_dir) {
        Ok(ca) => ca,
        Err(e) => {
            discard(&requests);
            return Err(fatal("CA de l'instance illisible", e));
        }
    };
    let responses = match data.ensure_sub(RESPONSES_DIR) {
        Ok(d) => d,
        Err(e) => {
            discard(&requests);
            return Err(fatal("dossier des reponses", e));
        }
    };
    let mut report = SignReport::default();
    for name in names {
        let file = format!("{name}.json");
        let outcome = sign_one(db, &ca, config_dir, &requests, &responses, &name, &file).await;
        if let Err(e) = requests.remove(&file) {
            tracing::warn!("demande {file} non supprimee : {e}");
        }
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

async fn sign_one(
    db: &PgPool,
    ca: &pki::Ca,
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
            sign_agent_request(db, ca, id, &request, pem).await?
        }
        (Usage::Link | Usage::Panel, None, None) => {
            let rel = request.usage.own_cert().unwrap_or(LINK_CRT);
            sign_own_request(ca, config_dir, rel, &request)?
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
    ca: &pki::Ca,
    id: ServerId,
    request: &CertRequest,
    pem: &str,
) -> Result<String, String> {
    let der = pki::der_from_pem(pem).ok_or("certificat de l'agent illisible")?;
    if public_key_sha256(&der).as_deref() != Some(request.public_key_sha256.as_str()) {
        return Err("cle publique differente du certificat joint".into());
    }
    let bundle = sign_agent(ca, &der, id)?;
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
    ca: &pki::Ca,
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
    Ok(sign_server(ca, &pem)?.cert_pem)
}

/// Acces aux fichiers du signataire : sous Unix, descripteurs de dossier
/// ouverts sans suivre de lien et fichiers ouverts relativement a eux
/// (`openat`, `O_NOFOLLOW`) ; ailleurs (developpement), chemins avec refus
/// des liens.
#[cfg(unix)]
mod fsio {
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
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
        /// (0750, groupe de ce dossier : celui du service).
        pub fn ensure_sub(&self, name: &str) -> io::Result<Dir> {
            let dir = match self.open_sub(name)? {
                Some(d) => d,
                None => {
                    match rustix::fs::mkdirat(&self.fd, name, Mode::from_raw_mode(0o750)) {
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
            rustix::fs::fchmod(&dir.fd, Mode::from_raw_mode(0o750))?;
            Ok(dir)
        }

        pub fn names(&self) -> io::Result<Vec<String>> {
            let mut out = Vec::new();
            for entry in rustix::fs::Dir::read_from(&self.fd)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name != "." && name != ".." {
                    out.push(name);
                }
            }
            Ok(out)
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

        pub fn harden(&self, name: &str) -> io::Result<bool> {
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
            let (uid, gid) = (euid(), rustix::process::getegid().as_raw());
            if meta.uid() != uid || meta.gid() != gid {
                std::os::unix::fs::fchown(&file, Some(uid), Some(gid))?;
                changed = true;
            }
            if meta.mode() & 0o777 != 0o600 {
                rustix::fs::fchmod(file.as_fd(), Mode::from_raw_mode(0o600))?;
                changed = true;
            }
            Ok(changed)
        }

        pub fn remove(&self, name: &str) -> io::Result<()> {
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
            let tmp = format!(".{name}.tmp");
            self.remove(&tmp)?;
            let flags =
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let fd = rustix::fs::openat(&self.fd, tmp.as_str(), flags, Mode::from_raw_mode(0o640))?;
            let gid = rustix::fs::fstat(&self.fd)?.st_gid;
            let mut file = File::from(fd);
            let written = std::os::unix::fs::fchown(&file, None, Some(gid))
                .and_then(|()| {
                    rustix::fs::fchmod(file.as_fd(), Mode::from_raw_mode(0o640)).map_err(Into::into)
                })
                .and_then(|()| file.write_all(bytes))
                .and_then(|()| file.sync_all())
                .and_then(|()| {
                    rustix::fs::renameat(&self.fd, tmp.as_str(), &self.fd, name).map_err(Into::into)
                });
            if written.is_err() {
                let _ = self.remove(&tmp);
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

        pub fn ensure_sub(&self, name: &str) -> io::Result<Dir> {
            let path = self.path.join(name);
            if std::fs::symlink_metadata(&path).is_err() {
                std::fs::create_dir(&path)?;
            }
            Dir::open(&path)
        }

        pub fn names(&self) -> io::Result<Vec<String>> {
            let mut out = Vec::new();
            for entry in std::fs::read_dir(&self.path)? {
                out.push(entry?.file_name().to_string_lossy().into_owned());
            }
            Ok(out)
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

        pub fn harden(&self, name: &str) -> io::Result<bool> {
            match refuse_link(&self.path.join(name)) {
                Ok(_) => Ok(false),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        }

        pub fn remove(&self, name: &str) -> io::Result<()> {
            let path = self.path.join(name);
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
