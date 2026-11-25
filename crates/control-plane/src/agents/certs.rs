//! Renouvellement des certificats emis par la CA de l'instance (`ciphera
//! init`). Le plan de controle n'a pas la cle de la CA (`ca/ca.key`, root
//! 0600) : il demande chaque certificat au signataire root
//! (`agents::signer`, `ciphera admin sign-requests`) et n'accepte que la
//! reponse a sa propre demande ([`Renewals`]) :
//!
//! - lien des agents (`tls/link.{crt,key}`) et acces direct du panneau
//!   (`tls/panel.{crt,key}`), 730 jours : demandes aux deux tiers de leur
//!   validite, au demarrage puis toutes les [`CHECK_EVERY`] ; le certificat
//!   re-signe (meme cle) est installe puis recharge a chaud ;
//! - certificat client de chaque agent, 365 jours : a la connexion d'un
//!   agent dont le certificat a passe les deux tiers, demande au signataire,
//!   qui note la nouvelle empreinte en `servers.next_cert_fingerprint` ; le
//!   certificat (meme cle, meme identifiant) est envoye a l'agent sur le lien
//!   (`ControlToAgent::RenewedCertificate`) et remplace l'ancien a la
//!   premiere connexion qui le presente ;
//! - certificat deja expire (machine eteinte longtemps) : `ciphera admin
//!   renew-certs` ([`renew_all`], root), memes fonctions de signature que le
//!   signataire, voir `docs/operations/certificats.md` ; seul le certificat
//!   du serveur local enregistre, emis par la CA de l'instance, est re-signe.
//!
//! Rotation de la CA (`ciphera admin rotate-ca`, `agents::rotation`) : tant
//! que `ca/ca-next.crt` existe, un certificat d'agent qui n'est pas emis par
//! la CA suivante est redemande a la connexion (le signataire le signe avec
//! elle) ; le lien et le panneau restent a la CA courante jusqu'a la fin de
//! la rotation, puis tout certificat emis par une CA qui n'est plus de
//! confiance est redemande.
//!
//! Un certificat est remplace sous le verrou des paires ([`lock_pairs`]) ;
//! une paire certificat + cle interrompue ([`replace_pair`] : fichiers
//! prepares, manifeste, renommages) est reprise avant toute lecture
//! ([`recover_pairs`]).

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shared::ids::ServerId;
use sqlx::PgPool;

use super::servers;
use super::signer::{self, CertRequest, Usage};
use super::transport::{LINK_CRT, LINK_KEY, TrustedCas};

/// Cle privee de la CA (relative au dossier de configuration) : root 0600,
/// lue par le seul signataire root.
pub const CA_KEY: &str = "ca/ca.key";
/// Certificat et cle de l'acces direct du panneau.
pub const PANEL_CRT: &str = "tls/panel.crt";
pub const PANEL_KEY: &str = "tls/panel.key";
/// Certificat de l'agent dans son dossier (`/etc/ciphera-agent`).
pub const AGENT_CRT: &str = "agent.crt";
/// Intervalle de verification des certificats du plan de controle.
pub const CHECK_EVERY: Duration = Duration::from_secs(12 * 3600);
/// Intervalle de lecture des reponses du signataire.
pub const RESPONSE_POLL: Duration = Duration::from_secs(2);
/// Demande sans reponse apres ce delai : reecrite (nouvel identifiant).
pub const REQUEST_RETRY: Duration = Duration::from_secs(600);
/// Delai maximal entre deux demandes sans reponse pour un meme certificat.
pub const REQUEST_RETRY_MAX: Duration = Duration::from_secs(12 * 3600);

/// Certificats serveur du plan de controle : (certificat, cle).
pub const OWN_CERTS: [(&str, &str); 2] = [(LINK_CRT, LINK_KEY), (PANEL_CRT, PANEL_KEY)];

/// Secondes Unix.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// CA de l'instance (`ca/ca.crt`, `ca/ca.key`), pour root seulement : voir
/// [`signer::load_ca`] (cle du proprietaire du processus, 0600, jamais a
/// travers un lien).
pub fn load_ca(config_dir: &Path) -> io::Result<pki::Ca> {
    signer::load_ca(config_dir)
}

/// Fichier temporaire d'ecriture de `path` : `.<nom>.renew`, meme dossier.
fn staged(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(".{name}.renew"))
}

/// Manifeste d'une paire certificat + cle en cours de remplacement :
/// `.<certificat>.pair`, a cote du certificat `crt` (relatif au dossier de
/// configuration). Sa presence signifie que les deux fichiers prepares
/// sont complets sur disque et doivent remplacer l'ancienne paire.
pub fn pair_manifest(config_dir: &Path, crt: &str) -> PathBuf {
    let path = config_dir.join(crt);
    let dir = path.parent().unwrap_or(config_dir).to_path_buf();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(".{name}.pair"))
}

/// Changement de proprietaire a faire pour passer de `current` a `wanted`
/// (uid, gid) : `Ok(false)` s'il n'y a rien a changer, `Ok(true)` pour
/// root, erreur sinon. Le service (compte ciphera) tourne sous
/// `SystemCallFilter=~@privileged` : un appel a `fchown` le tuerait
/// (SIGSYS) ; il ne doit donc jamais en faire, meme pour rien.
pub fn owner_change(current: (u32, u32), wanted: (u32, u32), root: bool) -> io::Result<bool> {
    if current == wanted {
        Ok(false)
    } else if root {
        Ok(true)
    } else {
        Err(io::Error::other(format!(
            "proprietaire {}:{} attendu ({}:{} en place) : seul root peut le changer \
             (ciphera admin renew-certs)",
            wanted.0, wanted.1, current.0, current.1
        )))
    }
}

/// `file` a `wanted` (uid, gid) : aucun appel systeme si c'est deja le cas
/// ([`owner_change`]).
#[cfg(unix)]
fn match_owner(file: &fs::File, wanted: (u32, u32)) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    let root = rustix::process::geteuid().is_root();
    if owner_change((meta.uid(), meta.gid()), wanted, root)? {
        std::os::unix::fs::fchown(file, Some(wanted.0), Some(wanted.1))?;
    }
    Ok(())
}

/// Ecrit `bytes` dans `tmp`, fichier neuf au proprietaire et aux droits de
/// `path` (0600 si `path` n'existe pas), donnees sur disque. `path` doit
/// etre un fichier ordinaire s'il existe (jamais a travers un lien). Le
/// proprietaire n'est change que par root ([`owner_change`]).
fn stage(path: &Path, tmp: &Path, bytes: &[u8]) -> io::Result<()> {
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
    let _ = fs::remove_file(tmp);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        opts.mode(previous.as_ref().map_or(0o600, |m| m.mode() & 0o777));
    }
    let mut file = opts.open(tmp)?;
    #[cfg(unix)]
    if let Some(m) = &previous {
        use std::os::unix::fs::MetadataExt;
        if let Err(e) = match_owner(&file, (m.uid(), m.gid())) {
            let _ = fs::remove_file(tmp);
            return Err(e);
        }
    }
    #[cfg(not(unix))]
    let _ = previous;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        let _ = fs::remove_file(tmp);
    }
    written
}

/// Dossier `dir` sur disque (renommages durables) ; sans effet hors Unix.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Remplace `path` par `bytes` : fichier temporaire du meme dossier, au
/// proprietaire et aux droits du fichier remplace (0600 s'il n'existe pas),
/// donnees sur disque, renommage, dossier sur disque. Jamais a travers un
/// lien.
pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = staged(path);
    stage(path, &tmp, bytes)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(dir)
}

/// Fichier de verrou des paires d'un dossier (`tls/.pair.lock`).
pub const PAIR_LOCK: &str = ".pair.lock";

/// Verrou exclusif sur les paires du dossier de `crt` : relache a sa
/// destruction.
#[derive(Debug)]
pub struct PairLock {
    _file: fs::File,
}

/// Prend le verrou exclusif (attente) des paires certificat + cle du
/// dossier de `crt` (relatif au dossier de configuration) : le service et
/// `ciphera admin renew-certs` ne preparent, ne terminent ni n'annulent
/// jamais une paire en meme temps. Fichier `.pair.lock` cree au besoin
/// (0644, proprietaire du dossier, pour que le service puisse l'ouvrir
/// apres une creation par root) ; jamais a travers un lien.
pub fn lock_pairs(config_dir: &Path, crt: &str) -> io::Result<PairLock> {
    let path = config_dir.join(crt);
    let dir = path.parent().unwrap_or(config_dir);
    let lock = dir.join(PAIR_LOCK);
    let mut read = fs::OpenOptions::new();
    read.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        read.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = match read.open(&lock) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut create = fs::OpenOptions::new();
            create.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                create
                    .mode(0o644)
                    .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
            }
            let file = create.open(&lock)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let owner = fs::metadata(dir)?;
                // Service (non root) : jamais de `fchown` (filtre seccomp) ;
                // le fichier est alors simplement le sien.
                if rustix::process::geteuid().is_root() {
                    match_owner(&file, (owner.uid(), owner.gid()))?;
                }
            }
            file
        }
        Err(e) => return Err(e),
    };
    file.lock()?;
    Ok(PairLock { _file: file })
}

/// Remplace la paire `crt` + `key` (relatifs au dossier de configuration,
/// meme dossier) sans jamais laisser un certificat et une cle qui ne vont
/// pas ensemble : les deux fichiers sont prepares sur disque, puis le
/// manifeste ([`pair_manifest`]) est ecrit (point de non-retour), puis le
/// certificat et la cle sont renommes et le manifeste retire. Une coupure
/// est reprise par [`recover_pair`] : avant le manifeste, l'ancienne paire
/// reste ; apres, la nouvelle est terminee. Sous le verrou des paires
/// ([`lock_pairs`]).
pub fn replace_pair(
    config_dir: &Path,
    pair: (&str, &str),
    cert_pem: &[u8],
    key_pem: &[u8],
) -> io::Result<()> {
    let _lock = lock_pairs(config_dir, pair.0)?;
    replace_pair_locked(config_dir, pair, cert_pem, key_pem)
}

fn replace_pair_locked(
    config_dir: &Path,
    (crt, key): (&str, &str),
    cert_pem: &[u8],
    key_pem: &[u8],
) -> io::Result<()> {
    let (crt_path, key_path) = (config_dir.join(crt), config_dir.join(key));
    let dir = crt_path.parent().unwrap_or(config_dir);
    let manifest = pair_manifest(config_dir, crt);
    let (crt_tmp, key_tmp) = (staged(&crt_path), staged(&key_path));
    let prepared = stage(&key_path, &key_tmp, key_pem)
        .and_then(|()| stage(&crt_path, &crt_tmp, cert_pem))
        .and_then(|()| sync_dir(dir))
        .and_then(|()| stage(&crt_path, &manifest, format!("{crt}\n{key}\n").as_bytes()))
        .and_then(|()| sync_dir(dir));
    if let Err(e) = prepared {
        for tmp in [&manifest, &crt_tmp, &key_tmp] {
            let _ = fs::remove_file(tmp);
        }
        return Err(e);
    }
    finish_pair(
        dir,
        &manifest,
        [(&crt_tmp, &crt_path), (&key_tmp, &key_path)],
    )
}

/// Apres le manifeste : fichiers prepares encore presents renommes a leur
/// place (certificat puis cle), dossier sur disque, manifeste retire.
fn finish_pair(dir: &Path, manifest: &Path, moves: [(&Path, &Path); 2]) -> io::Result<()> {
    for (tmp, path) in moves {
        match fs::symlink_metadata(tmp) {
            Ok(m) if m.is_file() => fs::rename(tmp, path)?,
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{} n'est pas un fichier ordinaire",
                    tmp.display()
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    sync_dir(dir)?;
    fs::remove_file(manifest)?;
    sync_dir(dir)
}

/// Reprise d'un remplacement de paire interrompu (voir [`replace_pair`]) :
/// manifeste present, la nouvelle paire est terminee ; absent, les fichiers
/// prepares sont supprimes et l'ancienne paire reste. Vrai si une paire a
/// ete terminee. Sous le verrou des paires ([`lock_pairs`]) ; dossier
/// absent (instance sans ce certificat) : rien a faire.
pub fn recover_pair(config_dir: &Path, pair: (&str, &str)) -> io::Result<bool> {
    let crt_path = config_dir.join(pair.0);
    if !crt_path.parent().unwrap_or(config_dir).is_dir() {
        return Ok(false);
    }
    let _lock = lock_pairs(config_dir, pair.0)?;
    recover_pair_locked(config_dir, pair)
}

fn recover_pair_locked(config_dir: &Path, (crt, key): (&str, &str)) -> io::Result<bool> {
    let (crt_path, key_path) = (config_dir.join(crt), config_dir.join(key));
    let dir = crt_path.parent().unwrap_or(config_dir);
    let manifest = pair_manifest(config_dir, crt);
    let (crt_tmp, key_tmp) = (staged(&crt_path), staged(&key_path));
    match fs::symlink_metadata(&manifest) {
        Ok(_) => {
            finish_pair(
                dir,
                &manifest,
                [(&crt_tmp, &crt_path), (&key_tmp, &key_path)],
            )?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            for tmp in [&crt_tmp, &key_tmp] {
                match fs::remove_file(tmp) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Reprise des remplacements interrompus des certificats du lien et du
/// panneau ([`OWN_CERTS`]), a faire avant de les lire. Un dossier sans
/// manifeste ni fichier prepare (installation d'avant ce mecanisme) est
/// laisse tel quel.
pub fn recover_pairs(config_dir: &Path) -> io::Result<()> {
    for pair in OWN_CERTS {
        if recover_pair(config_dir, pair)? {
            tracing::warn!(
                "certificat {} : remplacement interrompu termine a la reprise",
                pair.0
            );
        }
    }
    Ok(())
}

/// Re-signe le certificat serveur `crt` (meme cle `key`, memes SAN) s'il a
/// passe les deux tiers de sa validite, ou toujours avec `force`. Vrai si
/// renouvele. Root seulement (`ciphera admin renew-certs`) : meme signature
/// que le signataire ([`signer::sign_server`]). Lecture et ecriture sous le
/// verrou des paires.
pub fn renew_server(
    config_dir: &Path,
    ca: &pki::Ca,
    pair: (&str, &str),
    now: i64,
    force: bool,
) -> io::Result<bool> {
    renew_server_with(config_dir, ca, &[ca], pair, now, force)
}

/// Comme [`renew_server`] : certificat en place emis par l'une des CA
/// `trusted`, re-signe par `ca` (rotation de la CA : fin de rotation).
pub fn renew_server_with(
    config_dir: &Path,
    ca: &pki::Ca,
    trusted: &[&pki::Ca],
    (crt, key): (&str, &str),
    now: i64,
    force: bool,
) -> io::Result<bool> {
    if !config_dir.join(crt).exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{crt} absent"),
        ));
    }
    let _lock = lock_pairs(config_dir, crt)?;
    recover_pair_locked(config_dir, (crt, key))?;
    let pem = fs::read_to_string(config_dir.join(crt))?;
    let due = pki::der_from_pem(&pem)
        .and_then(|der| pki::renewal_due(&der, now))
        .unwrap_or(true);
    if !due && !force {
        return Ok(false);
    }
    let bundle = signer::sign_server_with(ca, trusted, &pem).map_err(io::Error::other)?;
    replace_file(&config_dir.join(crt), bundle.cert_pem.as_bytes())?;
    Ok(true)
}

/// Demande en attente du signataire.
struct Pending {
    request: CertRequest,
    sent: Instant,
    /// Delai avant de reecrire la demande sans reponse : [`REQUEST_RETRY`],
    /// double a chaque fois jusqu'a [`REQUEST_RETRY_MAX`] (une demande
    /// toujours refusee ne sature ni le journal ni le signataire).
    retry: Duration,
    /// Certificat actuel de l'agent (demandes d'agent seulement).
    leaf: Option<Vec<u8>>,
    /// Derniere reponse ignoree (autre demande) deja signalee au journal.
    ignored: Option<uuid::Uuid>,
}

/// Demandes de certificats du plan de controle au signataire root, et
/// reponses acceptees : seulement celle qui porte l'identifiant de la
/// demande en cours, emise par la CA, pour la meme cle (et le meme agent).
/// Une demande par certificat a la fois ; sans reponse apres
/// [`REQUEST_RETRY`], elle est reecrite avec un nouvel identifiant.
pub struct Renewals {
    config_dir: PathBuf,
    data_dir: PathBuf,
    pending: Mutex<HashMap<String, Pending>>,
}

impl std::fmt::Debug for Renewals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renewals").finish_non_exhaustive()
    }
}

impl Renewals {
    /// `config_dir` : `ca/ca.crt` et `tls/` ; `data_dir` : dossiers des
    /// demandes et des reponses (`agents::signer`).
    pub fn new(config_dir: &Path, data_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashMap<String, Pending>> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Ecrit la demande si aucune n'est en attente pour ce certificat.
    fn submit(&self, request: CertRequest, leaf: Option<Vec<u8>>) -> io::Result<bool> {
        let name = request.name();
        let mut pending = self.pending();
        if pending.contains_key(&name) {
            return Ok(false);
        }
        signer::write_request(&self.data_dir, &request)?;
        pending.insert(
            name,
            Pending {
                request,
                sent: Instant::now(),
                retry: REQUEST_RETRY,
                leaf,
                ignored: None,
            },
        );
        Ok(true)
    }

    /// CA de confiance (`ca/ca.crt` et, pendant une rotation,
    /// `ca/ca-next.crt`).
    pub fn trusted_cas(&self) -> io::Result<TrustedCas> {
        TrustedCas::load(&self.config_dir)
    }

    /// Agent `id` connecte avec le certificat `leaf` : demande de
    /// renouvellement s'il a passe les deux tiers de sa validite, ou s'il
    /// n'est pas emis par la CA qui signe les agents (la suivante pendant
    /// une rotation). Vrai si une demande a ete ecrite.
    pub fn request_agent(&self, id: ServerId, leaf: &[u8], now: i64) -> io::Result<bool> {
        let due = pki::renewal_due(leaf, now) == Some(true);
        let stale = self
            .trusted_cas()
            .is_ok_and(|cas| !pki::issued_by(cas.active(), leaf));
        if !due && !stale {
            return Ok(false);
        }
        let request = CertRequest::agent(id, leaf)
            .ok_or_else(|| io::Error::other("certificat de l'agent illisible"))?;
        self.submit(request, Some(leaf.to_vec()))
    }

    /// Certificats du lien et du panneau a renouveler : demandes ecrites.
    /// Renvoie les certificats demandes ; une erreur est journalisee et
    /// n'arrete rien.
    pub fn request_own(&self, now: i64) -> Vec<&'static str> {
        let mut requested = Vec::new();
        for usage in [Usage::Link, Usage::Panel] {
            let Some(crt) = usage.own_cert() else {
                continue;
            };
            match self.request_one(usage, crt, now) {
                Ok(true) => {
                    tracing::info!("certificat {crt} : renouvellement demande au signataire");
                    requested.push(crt);
                }
                Ok(false) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => tracing::error!("renouvellement de {crt} non demande : {e}"),
            }
        }
        requested
    }

    fn request_one(&self, usage: Usage, crt: &str, now: i64) -> io::Result<bool> {
        let pem = fs::read_to_string(self.config_dir.join(crt))?;
        let der = pki::der_from_pem(&pem);
        let due = der
            .as_deref()
            .and_then(|der| pki::renewal_due(der, now))
            .unwrap_or(true);
        // Fin de rotation de la CA : certificat d'une CA qui n'est plus de
        // confiance.
        let stale = match (self.ca_ders(), der.as_deref()) {
            (Some(cas), Some(der)) => !pki::issued_by_any(&cas, der),
            _ => false,
        };
        if !due && !stale {
            return Ok(false);
        }
        let request = CertRequest::own(usage, &pem)
            .ok_or_else(|| io::Error::other(format!("{crt} illisible")))?;
        self.submit(request, None)
    }

    /// Reponse a la demande en attente `name`, si elle porte son
    /// identifiant ; demande reecrite sans reponse apres [`REQUEST_RETRY`],
    /// puis apres un delai double a chaque fois (au plus [`REQUEST_RETRY_MAX`]).
    fn answer(&self, name: &str, pending: &mut Pending) -> Option<String> {
        match signer::read_response(&self.data_dir, name) {
            Ok(Some((id, pem))) if id == pending.request.request_id => return Some(pem),
            Ok(Some((id, _))) => {
                if pending.ignored != Some(id) {
                    pending.ignored = Some(id);
                    tracing::warn!(
                        "certificat {name} : reponse du signataire ignoree (demande {id}, \
                         une autre est en cours)"
                    );
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("reponse du signataire {name} illisible : {e}"),
        }
        if pending.sent.elapsed() >= pending.retry {
            pending.request.request_id = uuid::Uuid::new_v4();
            pending.sent = Instant::now();
            pending.retry = (pending.retry * 2).min(REQUEST_RETRY_MAX);
            tracing::warn!(
                "certificat {name} : pas de reponse du signataire (journalctl -u ciphera-certsign), demande reecrite"
            );
            if let Err(e) = signer::write_request(&self.data_dir, &pending.request) {
                tracing::error!("certificat {name} : demande non reecrite : {e}");
            }
        }
        None
    }

    /// CA de confiance (DER) ; `None` si illisibles.
    fn ca_ders(&self) -> Option<Vec<Vec<u8>>> {
        self.trusted_cas().ok().map(|cas| cas.ders())
    }

    /// Certificats d'agents signes : `(serveur, PEM)` a envoyer, verifies
    /// (CA, identifiant, meme cle, pas encore a renouveler). Les demandes
    /// d'agents absents de `connected` sont abandonnees (redemandees a leur
    /// prochaine connexion).
    pub fn agent_responses(&self, now: i64, connected: &[ServerId]) -> Vec<(ServerId, String)> {
        let ca_ders = self.ca_ders();
        let mut out = Vec::new();
        let mut pending = self.pending();
        pending.retain(|name, p| {
            let Some(id) = p.request.server_id else {
                return true;
            };
            if !connected.contains(&id) {
                tracing::warn!(
                    "agent {id} : deconnecte, demande de certificat abandonnee \
                     (redemandee a sa prochaine connexion)"
                );
                return false;
            }
            let Some(pem) = self.answer(name, p) else {
                return true;
            };
            let leaf = p.leaf.as_deref().unwrap_or_default();
            let ok = pki::der_from_pem(&pem).is_some_and(|der| {
                ca_ders
                    .as_deref()
                    .is_some_and(|cas| pki::issued_by_any(cas, &der))
                    && pki::agent_id_from_cert(&der) == Some(id)
                    && pki::public_key_der(&der).is_some()
                    && pki::public_key_der(&der) == pki::public_key_der(leaf)
                    && pki::renewal_due(&der, now) == Some(false)
            });
            if ok {
                out.push((id, pem));
            } else {
                tracing::error!("agent {id} : certificat du signataire refuse (CA ou cle)");
            }
            false
        });
        out
    }

    /// Certificats du lien et du panneau signes : installes (meme cle,
    /// verifiee sous le verrou des paires). Renvoie les certificats
    /// installes, a recharger.
    pub fn install_own_responses(&self, now: i64) -> Vec<&'static str> {
        let ca_ders = self.ca_ders();
        let mut installed = Vec::new();
        let mut pending = self.pending();
        for (usage, (crt, key)) in [Usage::Link, Usage::Panel].into_iter().zip(OWN_CERTS) {
            let name = match usage {
                Usage::Link => "link",
                _ => "panel",
            };
            let Some(p) = pending.get_mut(name) else {
                continue;
            };
            let Some(pem) = self.answer(name, p) else {
                continue;
            };
            pending.remove(name);
            match self.install_own(ca_ders.as_deref(), (crt, key), &pem, now) {
                Ok(()) => {
                    tracing::info!("certificat {crt} renouvele");
                    installed.push(crt);
                }
                Err(e) => tracing::error!("certificat {crt} du signataire refuse : {e}"),
            }
        }
        installed
    }

    fn install_own(
        &self,
        ca_ders: Option<&[Vec<u8>]>,
        (crt, key): (&str, &str),
        pem: &str,
        now: i64,
    ) -> io::Result<()> {
        let der = pki::der_from_pem(pem).ok_or_else(|| io::Error::other("illisible"))?;
        if !ca_ders.is_some_and(|cas| pki::issued_by_any(cas, &der)) {
            return Err(io::Error::other("non emis par la CA de l'instance"));
        }
        if pki::renewal_due(&der, now) != Some(false) {
            return Err(io::Error::other("deja a renouveler"));
        }
        let _lock = lock_pairs(&self.config_dir, crt)?;
        recover_pair_locked(&self.config_dir, (crt, key))?;
        let path = self.config_dir.join(crt);
        let current = fs::read_to_string(&path)?;
        let same_key = pki::der_from_pem(&current)
            .and_then(|d| pki::public_key_der(&d))
            .is_some_and(|k| Some(k) == pki::public_key_der(&der));
        if !same_key {
            return Err(io::Error::other("cle differente du certificat en place"));
        }
        replace_file(&path, pem.as_bytes())
    }
}

/// Resultat de `ciphera admin renew-certs`.
#[derive(Debug)]
pub struct RenewAll {
    pub server_certs: Vec<&'static str>,
    pub agent: Option<(ServerId, PathBuf)>,
}

/// Reprise manuelle (root) : certificats du lien et du panneau re-signes
/// (meme cle) quelle que soit leur date, et certificat de l'agent local
/// (meme cle), avec les fonctions de signature du signataire ;
/// la nouvelle empreinte de l'agent est notee en base avant l'ecriture du
/// fichier, puis remplace l'ancienne a sa premiere connexion. Services a
/// redemarrer ensuite.
///
/// Le fichier de l'agent est modifiable par le compte de l'agent : son
/// certificat n'est signe que s'il est emis par la CA de l'instance et que
/// son empreinte est celle (actuelle ou suivante) du serveur local
/// enregistre et non revoque. Ce controle passe avant toute ecriture.
pub async fn renew_all(
    db: &PgPool,
    config_dir: &Path,
    agent_dir: &Path,
) -> Result<RenewAll, Box<dyn std::error::Error>> {
    // Installation anterieure : dossiers de configuration et de la CA, et
    // cle de la CA, remis a root d'abord.
    if signer::harden_config_dir(config_dir)
        .map_err(|e| format!("{} : {e}", config_dir.display()))?
    {
        eprintln!(
            "{} remis a root (groupe conserve, 0750)",
            config_dir.display()
        );
    }
    if signer::harden_ca(config_dir).map_err(|e| format!("ca/ : {e}"))? {
        eprintln!("ca/ remis a root (cle 0600, certificat 0640, dossier 0750)");
    }
    let cas =
        signer::load_cas(config_dir).map_err(|e| format!("CA de l'instance illisible : {e}"))?;
    recover_pairs(config_dir)
        .map_err(|e| format!("reprise d'un renouvellement interrompu : {e}"))?;
    let agent_crt = agent_dir.join(AGENT_CRT);
    let local_agent = match fs::read_to_string(&agent_crt) {
        Ok(pem) => Some(local_agent_cert(db, &cas, config_dir, &pem).await?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{} : {e}", agent_crt.display()).into()),
    };
    let now = now_unix();
    let mut server_certs = Vec::new();
    for pair in OWN_CERTS {
        renew_server_with(config_dir, &cas.current, &cas.trusted(), pair, now, true)
            .map_err(|e| format!("{} : {e}", pair.0))?;
        server_certs.push(pair.0);
    }
    let agent = match local_agent {
        Some((id, known, der)) => {
            let bundle = cas.sign_agent(&der, id)?;
            // Empreinte connue : la meme qu'au controle (atomique). Base
            // restauree (`known` vide) : identifiant et ligne locale.
            let noted = sqlx::query(
                "UPDATE servers SET next_cert_fingerprint = $3 \
                 WHERE id = $1 AND kind = 'local' AND revoked_at IS NULL \
                 AND ($2::text IS NULL \
                      OR cert_fingerprint = $2 OR next_cert_fingerprint = $2)",
            )
            .bind(id)
            .bind(known.as_deref())
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
        None => None,
    };
    Ok(RenewAll {
        server_certs,
        agent,
    })
}

/// Certificat PEM de l'agent local accepte pour `renew-certs` : emis par
/// une CA de l'instance (courante, ou suivante pendant une rotation), avec
/// l'identifiant du serveur local non revoque,
/// et :
/// - soit d'empreinte enregistree (actuelle ou suivante) pour ce serveur ;
/// - soit, empreinte inconnue de la base (base restauree depuis une
///   sauvegarde anterieure a un renouvellement), de meme cle publique que
///   la copie root `agents/local.crt` (la cle de l'agent ne change jamais).
///
/// Renvoie l'identifiant, l'empreinte enregistree (`None` dans le second
/// cas) et le DER.
async fn local_agent_cert(
    db: &PgPool,
    cas: &signer::Cas,
    config_dir: &Path,
    pem: &str,
) -> Result<(ServerId, Option<String>, Vec<u8>), Box<dyn std::error::Error>> {
    let der = pki::der_from_pem(pem).ok_or("certificat de l'agent illisible")?;
    if !cas.issued(&der) {
        return Err(
            "certificat de l'agent local non emis par la CA de l'instance : agent non renouvele"
                .into(),
        );
    }
    let id = pki::agent_id_from_cert(&der).ok_or("certificat de l'agent sans identifiant")?;
    let fingerprint = pki::fingerprint_sha256(&der);
    let local: Option<(bool, bool)> = sqlx::query_as(
        "SELECT revoked_at IS NOT NULL, \
                (cert_fingerprint = $2 OR next_cert_fingerprint IS NOT DISTINCT FROM $2) \
         FROM servers WHERE id = $1 AND kind = 'local'",
    )
    .bind(id)
    .bind(&fingerprint)
    .fetch_optional(db)
    .await?;
    match local {
        None => Err(format!(
            "certificat de l'agent : {id} n'est pas le serveur local enregistre : agent non renouvele"
        )
        .into()),
        Some((true, _)) => Err(format!("serveur {id} revoque : agent non renouvele").into()),
        Some((false, true)) => Ok((id, Some(fingerprint), der)),
        Some((false, false)) => {
            // Base restauree : meme cle que la copie root de l'agent local.
            let copy = config_dir.join(servers::LOCAL_AGENT_CERT);
            let same_key = fs::read_to_string(&copy)
                .ok()
                .and_then(|pem| pki::der_from_pem(&pem))
                .and_then(|der| pki::public_key_der(&der))
                .is_some_and(|key| Some(key) == pki::public_key_der(&der));
            if same_key {
                tracing::warn!(
                    "agent local {id} : empreinte {fingerprint} inconnue de la base (base \
                     restauree ?), meme cle que {} : renouvele",
                    copy.display()
                );
                Ok((id, None, der))
            } else {
                Err(format!(
                    "certificat de l'agent ({fingerprint}) different de celui enregistre pour le \
                     serveur local, et cle differente de {} : agent non renouvele",
                    copy.display()
                )
                .into())
            }
        }
    }
}
