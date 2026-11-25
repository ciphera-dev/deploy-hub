//! `ciphera init` : genere les secrets de l'instance dans `<config_dir>` et
//! imprime l'URL de setup avec l'empreinte du certificat du panneau.
//!
//! Fichiers ecrits (tous en 0600, dossiers en 0700) :
//! `db_password`, `age.key`, `ca/ca.{crt,key}`, `tls/panel.{crt,key}`,
//! `tls/link.{crt,key}` (lien QUIC des agents), `agents/local.crt` (copie
//! publique du certificat de l'agent local), `setup_token`, et
//! `postgres/db_password` : copie du mot de passe de la base lue par le
//! conteneur PostgreSQL (`POSTGRES_PASSWORD_FILE`, monte en lecture seule
//! par l'agent), dossier et fichier au compte `--postgres-owner`
//! (`ciphera-postgres`, sous lequel tourne le conteneur).
//!
//! Tous appartiennent au compte du service (`--owner`), sauf le dossier de
//! la CA : `ca/ca.key` reste a root (qui lance l'init), lue par le seul
//! signataire root (`ciphera admin sign-requests`, `agents::signer`) ;
//! `ca/` (0750) et `ca/ca.crt` (0640) restent a root, groupe du service :
//! le service lit le certificat mais ne peut ni remplacer ni supprimer la
//! cle ou le certificat.
//!
//! Enrolement de l'agent local, dans `<agent_dir>` : la paire de cles est
//! generee cote agent (`agent.key`), la CA ne recoit que la cle publique et
//! signe `agent.crt` ; `ca.crt` sert a verifier le plan de controle. La ligne
//! `servers` du serveur local est creee au demarrage du serveur, d'apres
//! `agents/local.crt` (l'init ne touche pas a la base).
//!
//! Rien n'est jamais ecrase : si un seul fichier existe deja, l'init
//! s'arrete avant d'ecrire quoi que ce soit.

use std::fmt;
use std::fs;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;
use base64::Engine;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use rand::TryRng;
use rand::rngs::SysRng;
use zeroize::Zeroizing;

/// Fichiers geres par l'init, relatifs a `config_dir`.
const DB_PASSWORD: &str = "db_password";
const AGE_KEY: &str = "age.key";
const CA_CRT: &str = "ca/ca.crt";
const CA_KEY: &str = "ca/ca.key";
const PANEL_CRT: &str = "tls/panel.crt";
const PANEL_KEY: &str = "tls/panel.key";
const SETUP_TOKEN: &str = "setup_token";
const LINK_CRT: &str = crate::agents::transport::LINK_CRT;
const LINK_KEY: &str = crate::agents::transport::LINK_KEY;
const LOCAL_AGENT_CRT: &str = crate::agents::servers::LOCAL_AGENT_CERT;
/// Copie du mot de passe pour le conteneur PostgreSQL (chemin connu de
/// l'agent : `ciphera_agent::infra::DEFAULT_POSTGRES_PASSWORD_FILE`).
const PG_DIR: &str = "postgres";
const PG_PASSWORD: &str = "postgres/db_password";
const ALL_FILES: [&str; 10] = [
    DB_PASSWORD,
    AGE_KEY,
    CA_CRT,
    CA_KEY,
    PANEL_CRT,
    PANEL_KEY,
    LINK_CRT,
    LINK_KEY,
    LOCAL_AGENT_CRT,
    SETUP_TOKEN,
];
const CA_DIR: &str = "ca";
const SUBDIRS: [&str; 3] = [CA_DIR, "tls", "agents"];

/// Fichiers de l'agent local, relatifs a `agent_dir` (lus par `ciphera-agent`).
const AGENT_KEY: &str = "agent.key";
const AGENT_CRT: &str = "agent.crt";
const AGENT_CA_CRT: &str = "ca.crt";
const AGENT_FILES: [&str; 3] = [AGENT_KEY, AGENT_CRT, AGENT_CA_CRT];

#[derive(Clone, Debug)]
pub struct InitOptions {
    pub config_dir: PathBuf,
    pub public_ip: IpAddr,
    pub panel_port: u16,
    /// Compte systeme proprietaire des fichiers (ex. `ciphera`), si possible.
    pub owner: Option<String>,
    /// Dossier de l'identite de l'agent local (ex. `/etc/ciphera-agent`).
    pub agent_dir: PathBuf,
    /// Compte systeme proprietaire de `agent_dir` (ex. `ciphera-agent`).
    pub agent_owner: Option<String>,
    /// Compte systeme du conteneur PostgreSQL (ex. `ciphera-postgres`),
    /// proprietaire de `postgres/db_password`.
    pub postgres_owner: Option<String>,
}

pub struct InitReport {
    /// Contient le jeton de setup : a n'afficher qu'une fois.
    pub setup_url: String,
    pub fingerprint: String,
    /// Avertissements non bloquants (ex. proprietaire non modifie).
    pub warnings: Vec<String>,
}

impl fmt::Debug for InitReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InitReport")
            .field("fingerprint", &self.fingerprint)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

impl InitReport {
    /// Texte imprime sur la sortie standard : l'URL, puis l'empreinte.
    pub fn render(&self) -> String {
        format!(
            "Ciphera est initialise.\n\
             Ouvrez cette adresse pour creer le compte administrateur (jeton a usage unique) :\n\
             {}\n\
             Empreinte SHA-256 : {}\n\
             Comparez cette empreinte a celle du certificat affiche par votre navigateur \
             avant de continuer.\n",
            self.setup_url, self.fingerprint
        )
    }
}

#[derive(Debug)]
pub enum InitError {
    /// Un fichier cible existe deja ; rien n'a ete ecrit.
    Exists(PathBuf),
    Io(&'static str, std::io::Error),
    Crypto(String),
    /// Dossier existant qui casse l'invariant 0700 (lien, ecriture par autrui).
    UnsafeDir(PathBuf, &'static str),
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exists(p) => write!(
                f,
                "{} existe deja : l'init n'ecrase jamais un secret (aucun fichier modifie)",
                p.display()
            ),
            Self::Io(what, e) => write!(f, "{what} : {e}"),
            Self::Crypto(m) => write!(f, "{m}"),
            Self::UnsafeDir(p, why) => write!(f, "{} refuse : {why}", p.display()),
        }
    }
}

impl std::error::Error for InitError {}

/// Genere les secrets et les ecrit. Echoue sans rien ecrire si un fichier
/// cible existe deja.
pub fn run(opts: &InitOptions) -> Result<InitReport, InitError> {
    let dir = opts.config_dir.as_path();
    let agent_dir = opts.agent_dir.as_path();

    // 1. Tout verifier avant d'ecrire quoi que ce soit.
    let config_paths = ALL_FILES
        .iter()
        .chain(&[PG_PASSWORD])
        .map(|rel| dir.join(rel));
    let agent_paths = AGENT_FILES.iter().map(|rel| agent_dir.join(rel));
    for path in config_paths.chain(agent_paths) {
        // symlink_metadata : un lien (meme casse) compte comme existant.
        if fs::symlink_metadata(&path).is_ok() {
            return Err(InitError::Exists(path));
        }
    }

    // Dossiers deja presents : vrais dossiers, non modifiables par autrui.
    check_dir(dir)?;
    for sub in SUBDIRS.iter().chain(&[PG_DIR]) {
        check_dir(&dir.join(sub))?;
    }
    check_dir(agent_dir)?;

    // 2. Tout generer en memoire.
    let db_password = Zeroizing::new(URL_SAFE.encode(&random::<32>()?[..]));
    let token = Zeroizing::new(URL_SAFE_NO_PAD.encode(&random::<32>()?[..]));
    let identity = age::x25519::Identity::generate();
    let age_key = Zeroizing::new(format!("{}\n", identity.to_string().expose_secret()));

    let ca = pki::Ca::generate("Ciphera CA").map_err(|e| InitError::Crypto(e.to_string()))?;
    let sans = [opts.public_ip.to_string(), "localhost".to_string()];
    let mut panel = ca
        .issue_server_cert(&sans)
        .map_err(|e| InitError::Crypto(e.to_string()))?;
    let panel_key = panel.key_pem.take().unwrap_or_default();
    let mut link = ca
        .issue_server_cert(&sans)
        .map_err(|e| InitError::Crypto(e.to_string()))?;
    let link_key = link.key_pem.take().unwrap_or_default();
    let ca_key = ca.key_pem();

    // Agent local : cle generee de son cote, seule la cle publique va a la CA.
    let agent_key = pki::AgentKey::generate().map_err(|e| InitError::Crypto(e.to_string()))?;
    let agent_cert = ca
        .issue_agent_cert(shared::ids::ServerId::new(), agent_key.public_key_der())
        .map_err(|e| InitError::Crypto(e.to_string()))?;

    // 3. Ecrire (dossiers 0700, fichiers 0600 des la creation).
    let config_dir_created = ensure_dir(dir)?;
    for sub in SUBDIRS.iter().chain(&[PG_DIR]) {
        ensure_dir(&dir.join(sub))?;
    }
    ensure_dir(agent_dir)?;
    let files: [(&str, &[u8]); 10] = [
        (DB_PASSWORD, db_password.as_bytes()),
        (AGE_KEY, age_key.as_bytes()),
        (CA_CRT, ca.cert_pem().as_bytes()),
        (CA_KEY, ca_key.as_bytes()),
        (PANEL_CRT, panel.cert_pem.as_bytes()),
        (PANEL_KEY, panel_key.as_bytes()),
        (LINK_CRT, link.cert_pem.as_bytes()),
        (LINK_KEY, link_key.as_bytes()),
        (LOCAL_AGENT_CRT, agent_cert.cert_pem.as_bytes()),
        (SETUP_TOKEN, token.as_bytes()),
    ];
    let agent_files: [(&str, &[u8]); 3] = [
        (AGENT_KEY, agent_key.key_pem().as_bytes()),
        (AGENT_CRT, agent_cert.cert_pem.as_bytes()),
        (AGENT_CA_CRT, ca.cert_pem().as_bytes()),
    ];
    let mut written: Vec<PathBuf> = Vec::new();
    let mut agent_written: Vec<PathBuf> = Vec::new();
    let mut pg_written: Vec<PathBuf> = Vec::new();
    let mut root_written: Vec<PathBuf> = Vec::new();
    let mut shared_written: Vec<PathBuf> = Vec::new();
    let all = files
        .iter()
        .map(|(rel, bytes)| (dir.join(rel), *bytes, config_owner(rel)))
        .chain(
            agent_files
                .iter()
                .map(|(rel, bytes)| (agent_dir.join(rel), *bytes, Owner::Agent)),
        )
        .chain([(
            dir.join(PG_PASSWORD),
            db_password.as_bytes(),
            Owner::Postgres,
        )]);
    for (path, bytes, owner) in all {
        if let Err(e) = write_new(&path, bytes) {
            // Meilleur effort : ne pas laisser une init a moitie faite.
            for p in written
                .iter()
                .chain(&agent_written)
                .chain(&pg_written)
                .chain(&root_written)
                .chain(&shared_written)
            {
                let _ = fs::remove_file(p);
            }
            return Err(e);
        }
        match owner {
            Owner::Root => root_written.push(path),
            Owner::RootShared => shared_written.push(path),
            Owner::Instance => written.push(path),
            Owner::Agent => agent_written.push(path),
            Owner::Postgres => pg_written.push(path),
        }
    }

    // 4. Proprietaires.
    let mut warnings = Vec::new();
    if let Some(owner) = &opts.owner {
        let targets = chown_targets(dir, config_dir_created, written);
        if let Some(w) = chown_all(&targets, owner) {
            warnings.push(w);
        } else if let Some(w) = share_with_group(&shared_targets(dir), owner) {
            warnings.push(w);
        }
    }
    if let Some(owner) = &opts.agent_owner {
        let targets = agent_chown_targets(agent_dir, agent_written);
        if let Some(w) = chown_all(&targets, owner) {
            warnings.push(w);
        }
    }
    if let Some(owner) = &opts.postgres_owner {
        let mut targets = pg_written;
        targets.push(dir.join(PG_DIR));
        if let Some(w) = chown_all(&targets, owner) {
            warnings.push(w);
        }
    }

    let addr = SocketAddr::new(opts.public_ip, opts.panel_port);
    Ok(InitReport {
        setup_url: format!("https://{addr}/setup?token={}", token.as_str()),
        fingerprint: panel.fingerprint_sha256,
        warnings,
    })
}

/// Compte proprietaire d'un fichier ecrit par l'init.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    /// Laisse a root (qui lance l'init) : cle de la CA.
    Root,
    /// A root, lisible par le groupe du service : certificat de la CA.
    RootShared,
    Instance,
    Agent,
    Postgres,
}

/// Proprietaire d'un fichier du dossier de configuration : le compte du
/// service, sauf le dossier de la CA (root, signataire separe).
fn config_owner(rel: &str) -> Owner {
    match rel {
        CA_KEY => Owner::Root,
        CA_CRT => Owner::RootShared,
        _ => Owner::Instance,
    }
}

/// Dossier de la CA et son certificat : a root, groupe du service, avec ces
/// droits (le service lit le certificat, rien d'autre).
pub fn shared_targets(dir: &Path) -> [(PathBuf, u32); 2] {
    [(dir.join(CA_DIR), 0o750), (dir.join(CA_CRT), 0o640)]
}

fn random<const N: usize>() -> Result<Zeroizing<[u8; N]>, InitError> {
    let mut buf = Zeroizing::new([0u8; N]);
    SysRng
        .try_fill_bytes(buf.as_mut())
        .map_err(|_| InitError::Crypto("generateur aleatoire du systeme indisponible".into()))?;
    Ok(buf)
}

/// Cibles du changement de proprietaire : fichiers ecrits, sous-dossiers
/// sauf `ca/` (root, voir [`shared_targets`]), et `config_dir` seulement si
/// l'init l'a cree (sinon il appartient a l'installeur ; le compte de
/// service doit pouvoir le traverser).
pub fn chown_targets(dir: &Path, config_dir_created: bool, written: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut targets = written;
    targets.extend(
        SUBDIRS
            .iter()
            .filter(|s| **s != CA_DIR)
            .map(|s| dir.join(s)),
    );
    if config_dir_created {
        targets.push(dir.to_path_buf());
    }
    targets
}

/// Cibles du changement de proprietaire cote agent : ses fichiers et son
/// dossier, meme s'il existait deja (ex. cree vide par root) : il est
/// propre a l'agent, qui doit pouvoir le traverser.
pub fn agent_chown_targets(agent_dir: &Path, written: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut targets = written;
    targets.push(agent_dir.to_path_buf());
    targets
}

/// Un dossier present doit etre un vrai dossier (pas un lien) que ni le
/// groupe ni les autres ne peuvent modifier : sinon une cle pourrait etre
/// redirigee ou remplacee.
fn check_dir(path: &Path) -> Result<(), InitError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(InitError::Io("lecture d'un dossier de configuration", e)),
    };
    if meta.file_type().is_symlink() {
        return Err(InitError::UnsafeDir(
            path.to_path_buf(),
            "c'est un lien symbolique",
        ));
    }
    if !meta.is_dir() {
        return Err(InitError::UnsafeDir(
            path.to_path_buf(),
            "ce n'est pas un dossier",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o022 != 0 {
            return Err(InitError::UnsafeDir(
                path.to_path_buf(),
                "modifiable par le groupe ou les autres (chmod go-w)",
            ));
        }
    }
    Ok(())
}

/// Cree le dossier (0700) s'il manque, puis le verifie. `true` si cree.
fn ensure_dir(path: &Path) -> Result<bool, InitError> {
    let existed = fs::symlink_metadata(path).is_ok();
    if !existed {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|e| InitError::Io("creation d'un dossier de configuration", e))?;
    }
    check_dir(path)?;
    Ok(!existed)
}
/// Ecrit `bytes` dans un nouveau fichier : temporaire cree en 0600 des le
/// depart (jamais de chmod apres coup), puis lien dur vers la destination,
/// qui echoue si elle existe (pas d'ecrasement meme en cas de course).
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), InitError> {
    let io = |e| InitError::Io("ecriture d'un secret", e);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let suffix: String = random::<8>()?.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = path.with_file_name(format!(
        ".{}.tmp-{suffix}",
        name.as_deref().unwrap_or("secret")
    ));

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(io(e));
    }
    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(InitError::Exists(path.to_path_buf()))
        }
        Err(e) => Err(io(e)),
    }
}

#[cfg(unix)]
fn chown_all(paths: &[PathBuf], owner: &str) -> Option<String> {
    if !running_as_root() {
        return Some(format!(
            "pas root : proprietaire des fichiers non change (attendu : {owner})"
        ));
    }
    let Some((uid, gid)) = lookup_user(owner) else {
        return Some(format!(
            "utilisateur {owner} introuvable : proprietaire des fichiers non change"
        ));
    };
    for p in paths {
        if std::os::unix::fs::chown(p, Some(uid), Some(gid)).is_err() {
            return Some(format!(
                "changement de proprietaire impossible pour {}",
                p.display()
            ));
        }
    }
    None
}

/// Groupe du compte `owner` et droits `mode` sur chaque chemin, proprietaire
/// inchange (root).
#[cfg(unix)]
fn share_with_group(paths: &[(PathBuf, u32)], owner: &str) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let (_, gid) = lookup_user(owner)?;
    for (p, mode) in paths {
        let done = std::os::unix::fs::chown(p, None, Some(gid))
            .and_then(|()| fs::set_permissions(p, fs::Permissions::from_mode(*mode)));
        if done.is_err() {
            return Some(format!("groupe ou droits non appliques a {}", p.display()));
        }
    }
    None
}

#[cfg(not(unix))]
fn share_with_group(_paths: &[(PathBuf, u32)], _owner: &str) -> Option<String> {
    None
}

#[cfg(not(unix))]
fn chown_all(_paths: &[PathBuf], owner: &str) -> Option<String> {
    Some(format!(
        "proprietaire {owner} non applique : plateforme sans comptes Unix"
    ))
}

/// UID effectif lu dans `/proc` (Linux) ; en cas de doute, pas root.
#[cfg(unix)]
fn running_as_root() -> bool {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|euid| euid == "0"))
        })
        .unwrap_or(false)
}

/// `(uid, gid)` d'un compte, lu dans `/etc/passwd`.
#[cfg(unix)]
fn lookup_user(name: &str) -> Option<(u32, u32)> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let mut f = line.split(':');
        (f.next()? == name).then_some(())?;
        let uid = f.nth(1)?.parse().ok()?;
        let gid = f.next()?.parse().ok()?;
        Some((uid, gid))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_dir_is_always_chowned_to_the_agent() {
        let dir = Path::new("/etc/ciphera-agent");
        let targets = agent_chown_targets(dir, vec![dir.join("agent.key")]);
        assert!(targets.contains(&dir.to_path_buf()));
        assert!(targets.contains(&dir.join("agent.key")));
    }

    #[test]
    fn ca_stays_with_root() {
        assert_eq!(config_owner(CA_KEY), Owner::Root);
        assert_eq!(config_owner(CA_CRT), Owner::RootShared);
        for rel in ALL_FILES.iter().filter(|r| **r != CA_KEY && **r != CA_CRT) {
            assert_eq!(config_owner(rel), Owner::Instance, "{rel}");
        }
        let dir = Path::new("/etc/ciphera");
        let targets = chown_targets(dir, true, vec![]);
        assert!(!targets.contains(&dir.join("ca")), "ca/ reste a root");
        assert_eq!(
            shared_targets(dir),
            [(dir.join("ca"), 0o750), (dir.join("ca/ca.crt"), 0o640)]
        );
    }

    #[test]
    fn chown_targets_include_config_dir_only_when_created() {
        let dir = Path::new("/etc/ciphera");
        let files = vec![dir.join("age.key")];
        let created = chown_targets(dir, true, files.clone());
        assert!(created.contains(&dir.to_path_buf()));
        assert!(created.contains(&dir.join("tls")) && created.contains(&dir.join("agents")));
        assert!(created.contains(&dir.join("age.key")));
        let existing = chown_targets(dir, false, files);
        assert!(!existing.contains(&dir.to_path_buf()));
    }
}
