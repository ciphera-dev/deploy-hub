//! Demande de mise a jour de Ciphera, deposee par le plan de controle
//! (`POST /api/system/update`) et lue par l'updater (`ciphera-cli updater`,
//! root) : `<data_dir>/update-request.json`, signee par HMAC-SHA256 avec la
//! cle propre aux demandes, `/etc/ciphera/update-hmac.key` ([`KEY_FILE`]) :
//! 32 octets aleatoires, `root:ciphera 0640`, crees par `ciphera init` ou, si
//! elle manque, par l'updater ([`ensure_key`]). Le plan de controle la lit
//! pour signer, l'updater pour verifier ; l'updater ne lit jamais l'identite
//! age de l'instance (`age.key`).
//!
//! Le HMAC prouve que la demande vient d'un detenteur de cette cle, pas
//! qu'elle est sans danger : un processus `ciphera` compromis la detient.
//! L'updater refuse donc toute version inferieure ou egale a la version
//! installee (seul `--allow-downgrade`, lance par root, l'autorise) et ne
//! fait confiance qu'aux releases signees minisign.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::str::FromStr;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

/// Dossier des demandes, surveille par `ciphera-updater.path` : le dossier
/// de donnees par defaut du plan de controle (`CIPHERA_DATA_DIR`). Avec un
/// autre dossier de donnees, les mises a jour par le panneau sont refusees.
pub const REQUEST_DIR: &str = "/var/lib/ciphera";
/// Nom du fichier de demande dans [`REQUEST_DIR`].
pub const REQUEST_FILE: &str = "update-request.json";
/// Taille maximale du fichier de demande.
pub const MAX_REQUEST_BYTES: u64 = 4096;
/// Age maximal d'une demande (secondes) : une demande plus ancienne est
/// ignoree (rejeu d'un fichier oublie).
pub const MAX_AGE_SECS: u64 = 15 * 60;
/// Avance d'horloge toleree sur `requested_at` (secondes).
pub const MAX_SKEW_SECS: u64 = 5 * 60;
/// Cle HMAC des demandes, dans le dossier de configuration de l'instance
/// (`/etc/ciphera`).
pub const KEY_FILE: &str = "update-hmac.key";
/// Taille de la cle : octets aleatoires bruts, rien d'autre dans le fichier.
pub const KEY_BYTES: usize = 32;
/// Droits de la cle : lue par root (updater) et par le groupe du service
/// (plan de controle), modifiable par root seul.
pub const KEY_MODE: u32 = 0o640;

/// Version `vX.Y.Z` d'une release (meme forme que `install.sh --version`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Version `CARGO_PKG_VERSION` (`X.Y.Z`) d'un binaire.
    pub fn from_pkg(pkg: &str) -> Option<Version> {
        format!("v{pkg}").parse().ok()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidVersion;

impl fmt::Display for InvalidVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("version invalide (attendu vX.Y.Z)")
    }
}

impl std::error::Error for InvalidVersion {}

impl FromStr for Version {
    type Err = InvalidVersion;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rest = s.strip_prefix('v').ok_or(InvalidVersion)?;
        let mut parts = rest.split('.');
        let mut next = || -> Result<u32, InvalidVersion> {
            let p = parts.next().ok_or(InvalidVersion)?;
            if p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InvalidVersion);
            }
            p.parse().map_err(|_| InvalidVersion)
        };
        let v = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return Err(InvalidVersion);
        }
        Ok(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Contenu du fichier de demande.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    /// Version demandee (`vX.Y.Z`).
    pub version: String,
    /// Secondes depuis l'epoque Unix.
    pub requested_at: u64,
    /// HMAC-SHA256 hexadecimal (minuscules) de [`UpdateRequest::message`].
    pub hmac: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RequestError {
    /// JSON illisible ou champ inconnu.
    Malformed,
    /// HMAC faux : demande ignoree.
    BadHmac,
    /// Demande trop ancienne, ou datee du futur.
    Stale,
    /// Version mal formee.
    Version,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "demande illisible",
            Self::BadHmac => "HMAC de la demande invalide",
            Self::Stale => "demande perimee ou datee du futur",
            Self::Version => "version demandee invalide",
        })
    }
}

impl std::error::Error for RequestError {}

/// Cle HMAC des demandes, effacee de la memoire a sa destruction. Son
/// `Debug` n'affiche jamais les octets (`dbg!`, `{:?}` d'un `Result`).
pub struct RequestKey(Zeroizing<[u8; KEY_BYTES]>);

impl RequestKey {
    fn zeroed() -> RequestKey {
        RequestKey(Zeroizing::new([0u8; KEY_BYTES]))
    }
}

impl std::ops::Deref for RequestKey {
    type Target = [u8; KEY_BYTES];

    fn deref(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

impl std::ops::DerefMut for RequestKey {
    fn deref_mut(&mut self) -> &mut [u8; KEY_BYTES] {
        &mut self.0
    }
}

impl fmt::Debug for RequestKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RequestKey(..)")
    }
}

#[derive(Debug)]
pub enum KeyError {
    /// Fichier absent.
    Missing,
    /// Fichier present mais refuse (lien, droits, proprietaire, taille).
    Refused(&'static str),
    /// Lecture, creation ou generateur aleatoire en echec.
    Io(io::Error),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("cle des demandes de mise a jour absente"),
            Self::Refused(why) => write!(f, "cle des demandes de mise a jour refusee : {why}"),
            Self::Io(e) => write!(f, "cle des demandes de mise a jour illisible : {e}"),
        }
    }
}

impl std::error::Error for KeyError {}

/// Ouvre sans suivre de lien symbolique ni bloquer sur un tube nomme.
fn open_key(path: &Path) -> io::Result<File> {
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    opts.open(path)
}

/// Controles sur le descripteur ouvert (`fstat`) : fichier ordinaire a un
/// seul lien, exactement [`KEY_BYTES`] octets, proprietaire accepte par
/// [`key_owner_ok`], ni ecriture par le groupe ni acces pour les autres.
fn check_key_file(file: &File) -> Result<(), KeyError> {
    let meta = file.metadata().map_err(KeyError::Io)?;
    if !meta.is_file() {
        return Err(KeyError::Refused("fichier ordinaire attendu"));
    }
    if meta.len() != KEY_BYTES as u64 {
        return Err(KeyError::Refused("32 octets attendus"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err(KeyError::Refused("lien dur refuse"));
        }
        if !key_owner_ok(meta.uid(), rustix::process::geteuid().as_raw()) {
            return Err(KeyError::Refused("proprietaire inattendu (root attendu)"));
        }
        if meta.mode() & 0o7037 != 0 {
            return Err(KeyError::Refused(
                "droits trop larges (root:ciphera 0640 attendu)",
            ));
        }
    }
    Ok(())
}

/// Proprietaire accepte : root, ou l'utilisateur courant (tests, instance
/// lancee sans root). Pour root (updater), root seulement.
#[cfg_attr(not(unix), allow(dead_code))]
fn key_owner_ok(owner_uid: u32, euid: u32) -> bool {
    owner_uid == 0 || owner_uid == euid
}

/// Lit la cle des demandes (`<config_dir>/update-hmac.key`) sans suivre de
/// lien, apres les controles de [`check_key_file`].
pub fn load_key(path: &Path) -> Result<RequestKey, KeyError> {
    let mut file = match open_key(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(KeyError::Missing),
        Err(e) => return Err(KeyError::Io(e)),
    };
    check_key_file(&file)?;
    let mut key = RequestKey::zeroed();
    file.read_exact(&mut key[..]).map_err(KeyError::Io)?;
    Ok(key)
}

/// Cle des demandes, creee si elle manque, de facon atomique : jamais de
/// cle partielle sous le nom definitif, meme apres un arret brutal ou une
/// coupure de courant.
/// 1. [`KEY_BYTES`] octets aleatoires ecrits dans un fichier temporaire du
///    meme dossier (`.update-hmac.key.<pid>.<alea>.tmp`, cree en 0600,
///    `O_EXCL`, `O_NOFOLLOW`) ;
/// 2. avec `group`, ce groupe et [`KEY_MODE`] par le descripteur (`fchown`,
///    `fchmod`) ; sans `group`, 0600 ; donnees sur disque (`fsync`) ;
/// 3. publication sans jamais remplacer une cle presente
///    (`renameat2(RENAME_NOREPLACE)` sous Linux, sinon `link` puis retrait
///    du temporaire), puis le dossier sur disque.
///
/// Renvoie la cle et `true` si elle vient d'etre creee. Un fichier deja
/// present est lu par [`load_key`], jamais remplace. Un temporaire laisse
/// par un arret brutal (root 0600) n'est jamais lu.
pub fn ensure_key(path: &Path, group: Option<u32>) -> Result<(RequestKey, bool), KeyError> {
    match load_key(path) {
        Err(KeyError::Missing) => {}
        other => return other.map(|k| (k, false)),
    }
    let mut key = RequestKey::zeroed();
    getrandom::fill(&mut key[..])
        .map_err(|_| KeyError::Io(io::Error::other("generateur aleatoire indisponible")))?;
    let mut tag = [0u8; 8];
    getrandom::fill(&mut tag)
        .map_err(|_| KeyError::Io(io::Error::other("generateur aleatoire indisponible")))?;
    let tmp = path.with_file_name(format!(
        ".{KEY_FILE}.{}.{:016x}.tmp",
        std::process::id(),
        u64::from_le_bytes(tag)
    ));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let mut file = opts.open(&tmp).map_err(KeyError::Io)?;
    let written = file
        .write_all(&key[..])
        .and_then(|()| share_key(&file, group))
        .and_then(|()| file.sync_all());
    drop(file);
    let published = written.and_then(|()| publish_no_replace(&tmp, path));
    let _ = fs::remove_file(&tmp);
    match published {
        Ok(()) => {
            sync_parent(path);
            Ok((key, true))
        }
        // Creee entre-temps par un autre processus : c'est celle-la.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => load_key(path).map(|k| (k, false)),
        Err(e) => Err(KeyError::Io(e)),
    }
}

/// Donne a `tmp` le nom `dest` sans jamais remplacer un `dest` present
/// (`AlreadyExists`). Linux : `renameat2(RENAME_NOREPLACE)`, et `link` si le
/// systeme de fichiers ne le prend pas en charge ; ailleurs : `link`
/// (l'appelant retire `tmp`).
fn publish_no_replace(tmp: &Path, dest: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use rustix::fs::{CWD, RenameFlags, renameat_with};
        match renameat_with(CWD, tmp, CWD, dest, RenameFlags::NOREPLACE) {
            Ok(()) => return Ok(()),
            Err(e) if e == rustix::io::Errno::INVAL || e == rustix::io::Errno::NOSYS => {}
            Err(e) => return Err(e.into()),
        }
    }
    fs::hard_link(tmp, dest)
}

/// Dossier de `path` sur disque (entree creee) ; au mieux.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent()
        && let Ok(d) = File::open(dir)
    {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Groupe `group` et droits [`KEY_MODE`] (0600 sans groupe), par le
/// descripteur ouvert.
#[cfg(unix)]
fn share_key(file: &File, group: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let Some(gid) = group else {
        return file.set_permissions(fs::Permissions::from_mode(0o600));
    };
    std::os::unix::fs::fchown(file, None, Some(gid))?;
    file.set_permissions(fs::Permissions::from_mode(KEY_MODE))
}

#[cfg(not(unix))]
fn share_key(_file: &File, _group: Option<u32>) -> io::Result<()> {
    Ok(())
}

fn mac(key: &[u8], version: &str, requested_at: u64) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepte toute cle");
    mac.update(UpdateRequest::message(version, requested_at).as_bytes());
    mac
}

impl UpdateRequest {
    /// Message signe : contexte, version et date, une valeur par ligne.
    pub fn message(version: &str, requested_at: u64) -> String {
        format!("ciphera-update-request\n{version}\n{requested_at}")
    }

    pub fn sign(version: Version, requested_at: u64, key: &[u8]) -> UpdateRequest {
        let version = version.to_string();
        let tag = mac(key, &version, requested_at).finalize().into_bytes();
        UpdateRequest {
            hmac: tag.iter().map(|b| format!("{b:02x}")).collect(),
            version,
            requested_at,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("serialisation d'une structure simple")
    }

    pub fn from_json(bytes: &[u8]) -> Result<UpdateRequest, RequestError> {
        serde_json::from_slice(bytes).map_err(|_| RequestError::Malformed)
    }

    /// Verifie le HMAC (comparaison en temps constant), puis la fraicheur
    /// par rapport a `now`, et renvoie la version demandee.
    pub fn verify(&self, key: &[u8], now: u64) -> Result<Version, RequestError> {
        let tag = decode_hex(&self.hmac).ok_or(RequestError::BadHmac)?;
        mac(key, &self.version, self.requested_at)
            .verify_slice(&tag)
            .map_err(|_| RequestError::BadHmac)?;
        if self.requested_at > now.saturating_add(MAX_SKEW_SECS)
            || now.saturating_sub(self.requested_at) > MAX_AGE_SECS
        {
            return Err(RequestError::Stale);
        }
        self.version.parse().map_err(|_| RequestError::Version)
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() != 64 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn versions_parse_and_compare_numerically() {
        assert_eq!(v("v0.1.0").to_string(), "v0.1.0");
        assert!(v("v0.10.0") > v("v0.9.9"));
        assert!(v("v1.0.0") > v("v0.99.99"));
        assert_eq!(Version::from_pkg("0.1.0"), Some(v("v0.1.0")));
        for bad in [
            "",
            "0.1.0",
            "v0.1",
            "v0.1.0.1",
            "v0.1.x",
            "v-1.0.0",
            "v0..1",
            "v0.1.0 ",
            "v+1.0.0",
            "v1234567890.0.0",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn signed_request_verifies_and_rejects_tampering() {
        let key = [7u8; 32];
        let req = UpdateRequest::sign(v("v0.2.0"), 1_000, &key);
        let back = UpdateRequest::from_json(req.to_json().as_bytes()).unwrap();
        assert_eq!(back.verify(&key, 1_010), Ok(v("v0.2.0")));
        assert_eq!(back.verify(&[8u8; 32], 1_010), Err(RequestError::BadHmac));

        let mut other = back.clone();
        other.version = "v0.3.0".into();
        assert_eq!(other.verify(&key, 1_010), Err(RequestError::BadHmac));
        let mut other = back.clone();
        other.requested_at += 1;
        assert_eq!(other.verify(&key, 1_010), Err(RequestError::BadHmac));
        let mut other = back.clone();
        other.hmac = "zz".into();
        assert_eq!(other.verify(&key, 1_010), Err(RequestError::BadHmac));

        assert_eq!(
            back.verify(&key, 1_000 + MAX_AGE_SECS + 1),
            Err(RequestError::Stale)
        );
        assert_eq!(
            UpdateRequest::sign(v("v0.2.0"), 1_000 + MAX_SKEW_SECS + 1, &key).verify(&key, 1_000),
            Err(RequestError::Stale)
        );
        assert_eq!(
            UpdateRequest::from_json(br#"{"version":"v0.2.0","requested_at":1,"hmac":"","x":1}"#),
            Err(RequestError::Malformed)
        );
    }

    #[test]
    fn request_dir_is_the_default_data_dir() {
        assert_eq!(REQUEST_DIR, crate::config::Config::DEFAULT_DATA_DIR);
    }

    /// Dossier temporaire supprime a la fin du test.
    struct Tmp(std::path::PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Tmp {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ciphera-update-key-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_key_is_created_once_then_reused() {
        let tmp = Tmp::new("create");
        let path = tmp.0.join(KEY_FILE);
        assert!(matches!(load_key(&path), Err(KeyError::Missing)));
        let (key, created) = ensure_key(&path, None).unwrap();
        assert!(created);
        assert_eq!(fs::read(&path).unwrap(), key[..]);
        assert_ne!(*key, [0u8; KEY_BYTES], "cle aleatoire");
        // Deja presente : lue, jamais remplacee.
        let (again, created) = ensure_key(&path, None).unwrap();
        assert!(!created);
        assert_eq!(*again, *key);
        assert_eq!(*load_key(&path).unwrap(), *key);
        // Une autre instance a une autre cle.
        let other = tmp.0.join("autre.key");
        assert_ne!(*ensure_key(&other, None).unwrap().0, *key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
            assert_eq!(mode, 0o600, "sans groupe : 0600");
        }
    }

    #[test]
    fn key_is_published_without_leftover_nor_overwrite() {
        let tmp = Tmp::new("atomic");
        let path = tmp.0.join(KEY_FILE);
        ensure_key(&path, None).unwrap();
        let names: Vec<_> = fs::read_dir(&tmp.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            [std::ffi::OsString::from(KEY_FILE)],
            "aucun temporaire"
        );
        // Publication : une cle presente n'est jamais remplacee.
        let other = tmp.0.join("nouvelle.tmp");
        fs::write(&other, [9u8; KEY_BYTES]).unwrap();
        let before = fs::read(&path).unwrap();
        let err = publish_no_replace(&other, &path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn key_debug_never_shows_the_bytes() {
        let mut key = RequestKey::zeroed();
        key[..].copy_from_slice(&[0xab; KEY_BYTES]);
        let shown = format!("{key:?} {:?}", Ok::<_, ()>(&key));
        assert!(!shown.contains("171") && !shown.to_lowercase().contains("ab,"));
        assert!(shown.contains("RequestKey(..)"));
    }

    #[test]
    fn key_of_the_wrong_size_is_refused_and_kept() {
        let tmp = Tmp::new("size");
        let path = tmp.0.join(KEY_FILE);
        for bad in [&b""[..], &[1u8; 31][..], &[1u8; 33][..]] {
            fs::write(&path, bad).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            assert!(matches!(load_key(&path), Err(KeyError::Refused(_))));
            assert!(matches!(ensure_key(&path, None), Err(KeyError::Refused(_))));
            assert_eq!(fs::read(&path).unwrap(), bad, "jamais remplacee");
        }
    }

    #[cfg(unix)]
    #[test]
    fn key_rights_are_checked_and_group_is_set_by_descriptor() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let tmp = Tmp::new("rights");
        let path = tmp.0.join(KEY_FILE);
        // Groupe donne (le sien : possible sans root) : 0640.
        let gid = rustix::process::getegid().as_raw();
        let (key, created) = ensure_key(&path, Some(gid)).unwrap();
        assert!(created);
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, KEY_MODE);
        assert_eq!(meta.gid(), gid);
        assert_eq!(*load_key(&path).unwrap(), *key);
        // Ecriture par le groupe, acces pour les autres, bits speciaux :
        // refuses.
        for mode in [0o660, 0o644, 0o604, 0o650, 0o4640] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            assert!(
                matches!(load_key(&path), Err(KeyError::Refused(_))),
                "{mode:o}"
            );
        }
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        // Lien symbolique ou lien dur : refuses.
        let link = tmp.0.join("lien.key");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(load_key(&link).is_err());
        assert!(ensure_key(&link, None).is_err());
        let hard = tmp.0.join("dur.key");
        fs::hard_link(&path, &hard).unwrap();
        assert!(matches!(load_key(&path), Err(KeyError::Refused(_))));
        fs::remove_file(&hard).unwrap();
        assert!(load_key(&path).is_ok());
        // Lien casse a la place de la cle : jamais suivi pour creer.
        let dangling = tmp.0.join("casse.key");
        std::os::unix::fs::symlink(tmp.0.join("cible"), &dangling).unwrap();
        assert!(ensure_key(&dangling, None).is_err());
        assert!(!tmp.0.join("cible").exists());
    }

    #[test]
    fn key_owner_rules() {
        assert!(key_owner_ok(0, 0));
        assert!(key_owner_ok(0, 1000));
        assert!(key_owner_ok(1000, 1000));
        // L'updater (root) refuse une cle d'un autre compte.
        assert!(!key_owner_ok(1000, 0));
        assert!(!key_owner_ok(1001, 1000));
    }
}
