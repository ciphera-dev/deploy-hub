//! Conteneurs d'infrastructure : `ciphera-postgres` (base du plan de
//! controle) et `ciphera-buildkitd` (builds), images epinglees par digest
//! (`deploy/containers/*.digest`, compilees dans le binaire), sans aucun
//! port publie. L'agent les demarre au lancement, independamment du lien
//! avec le plan de controle (qui a besoin de la base pour demarrer).
//!
//! PostgreSQL, sur le reseau interne `ciphera-internal` (`internal: true`) :
//! - tourne sous le compte systeme dedie `ciphera-postgres` (uid:gid de
//!   l'hote), sans aucune capacite, avec `no-new-privileges` ;
//! - n'ecoute pas en TCP (`listen_addresses=`) : seulement sur son socket
//!   Unix, dans `/run/ciphera/postgres` (dossier de l'hote cree par
//!   systemd-tmpfiles, `ciphera-postgres:ciphera` en 0750 : le plan de
//!   controle, compte `ciphera`, s'y connecte) ;
//! - lit son mot de passe dans un fichier (`POSTGRES_PASSWORD_FILE`) du
//!   dossier `/etc/ciphera/postgres`, monte en lecture seule ; copie ecrite
//!   par `ciphera init` (0600, proprietaire `ciphera-postgres`) : jamais dans
//!   une variable d'environnement visible par `docker inspect`. Le dossier
//!   (et non le fichier) est monte : s'il manquait, Docker creerait un
//!   dossier vide a sa place, et PostgreSQL s'arreterait sur un fichier
//!   introuvable au lieu de lire un dossier pris pour le secret ;
//! - exige le mot de passe (scram-sha-256) aussi sur le socket local ;
//! - garde ses donnees dans le volume nomme `ciphera-postgres-data`.
//!
//! buildkitd, image rootless (`moby/buildkit:v0.35.1-rootless`), seul sur le
//! reseau `ciphera-build` (non interne : images de base et depots git en
//! Task 24 ; jamais relie a PostgreSQL ni aux applications) :
//! - non privilegie, sous le compte systeme dedie `ciphera-build` (uid:gid de
//!   l'hote, jamais l'uid 1000 fixe par l'image ni un compte humain) et sa
//!   propre plage de sous-uids/sous-gids (`/etc/subuid`, `/etc/subgid`,
//!   allouee par install.sh sans recouvrir une autre plage ni un uid/gid
//!   enumerable de l'hote ; les comptes d'annuaire non enumerables, sssd
//!   sans `enumerate`, ne peuvent pas etre verifies : sur ces hotes,
//!   l'administrateur reserve la plage hors de celle de l'annuaire).
//!   Compte ou plage absents : l'agent demarre quand meme PostgreSQL, ne
//!   lance pas buildkitd et journalise que les builds sont indisponibles.
//!   rootlesskit et newuidmap
//!   resolvent ce compte et sa plage dans des fichiers `passwd`, `group`,
//!   `subuid` et `subgid` minimaux generes par l'agent
//!   (`DEFAULT_BUILDKIT_IDENTITY_DIR`, 0644) et montes en lecture seule sur
//!   ceux de l'image (aucune image derivee : le digest reste epingle) ;
//! - etat (cache, couches) dans `/var/lib/ciphera-buildkit`, dossier de
//!   l'hote 0700 au compte `ciphera-build` (systemd-tmpfiles), monte comme
//!   HOME du compte ; un volume nomme appartiendrait a l'uid 1000 de l'image ;
//! - toutes les capacites retirees sauf `SETUID` et `SETGID`, dont
//!   newuidmap/newgidmap ont besoin pour ecrire la correspondance des uids
//!   (sans elles, rootlesskit echoue : verifie avec Docker) ;
//! - la configuration Docker recommandee par BuildKit (docs/rootless.md) :
//!   `seccomp=unconfined` (unshare, mount), AppArmor non confine
//!   (`apparmor=unconfined`, ou le profil `ciphera-buildkitd` qui n'autorise
//!   que les espaces de noms utilisateur, installe par install.sh la ou le
//!   noyau les restreint, Ubuntu >= 23.10) et `systempaths=unconfined`
//!   (`MaskedPaths`/`ReadonlyPaths` vides) : chaque etape RUN recoit son
//!   propre espace de noms PID et son /proc. Le bac a sable des processus de
//!   BuildKit reste actif : jamais `--oci-worker-no-process-sandbox`, qui
//!   laisserait une etape d'un build tuer ou tracer les processus des autres
//!   builds (acces ou empoisonnement du cache et des secrets d'un autre
//!   projet). Les masques de /proc ne protegent plus le conteneur buildkitd
//!   lui-meme : sur avis de BuildKit, c'est sans danger tant qu'il ne tourne
//!   pas en root ;
//! - exception documentee a `no-new-privileges` : rootlesskit passe par
//!   newuidmap/newgidmap (setuid) pour creer son espace de noms ; avec
//!   `no-new-privileges` ils echouent et buildkitd ne demarre pas ;
//! - n'ecoute que sur un socket Unix (`--addr unix://…`), jamais en TCP,
//!   dans `/run/ciphera-agent/buildkit` (dossier de l'hote au compte
//!   `ciphera-build`, setgid au groupe `ciphera-agent` : le socket 0660
//!   herite de ce groupe, et `--group=-1` empeche buildkitd de le changer ;
//!   aucune ACL ; parent 0750 ferme aux autres comptes, voir
//!   ciphera.tmpfiles.conf). buildkitd et rootlesskit tournent sous l'uid de
//!   `ciphera-build` dans l'espace de noms initial : seul ce compte systeme
//!   (sans shell ni mot de passe, uid unique refuse s'il vaut 1000 ou
//!   appartient a la plage des comptes humains, voir install.sh) et root
//!   peuvent atteindre le socket par `/proc/<pid>/root` ou entrer dans
//!   l'espace de noms de rootlesskit.
//!   Les entitlements `security.insecure` et `network.host` restent refuses
//!   (aucun `--allow-insecure-entitlement`) ;
//! - configuration `buildkitd.toml` ecrite par l'agent dans le dossier
//!   d'identite, montee en lecture seule et passee par `--config` (le
//!   fichier par defaut de HOME n'est pas lu) : frontends `gateway.v0`
//!   limites au frontend Railpack epingle et a `docker/dockerfile`. Un
//!   Dockerfile ne peut donc pas charger un frontend arbitraire, qui
//!   choisirait ses identifiants de cache hors de l'espace de noms du
//!   projet (`build::dockerfile`). Son empreinte est une etiquette du
//!   conteneur : la changer recree buildkitd. Les caches restent dans un
//!   seul etat BuildKit pour tous les projets, cloisonnes par espace de noms
//!   (UUID du projet et de l'environnement), pas par des etats separes.
//!
//! - ressources bornees (`BuildkitResources`) : au plus 4096 processus,
//!   50 % de la memoire du noeud (1 Gio au moins) et tous les CPU sauf un
//!   (un au moins), pour qu'un `RUN` hostile n'affame ni l'agent, ni le plan
//!   de controle, ni les applications ; le ramasse-miettes garde au plus 10 %
//!   du disque de l'etat (entre 5 et 50 Gio, `reservedSpace`). Un seul build
//!   a la fois par noeud (`build::BUILD_SLOTS`).
//!
//! Les montages de l'hote sont etiquetes pour SELinux (`z`) : sous Fedora,
//! le conteneur lit le secret et cree ses sockets.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use crate::docker::{BUILD_NETWORK, INFRA_NETWORK};
use crate::docker::{ContainerId, ContainerSpec, DockerError, DockerOps, MountSpec};

pub const POSTGRES_CONTAINER: &str = "ciphera-postgres";
pub const BUILDKIT_CONTAINER: &str = "ciphera-buildkitd";
pub const POSTGRES_VOLUME: &str = "ciphera-postgres-data";
/// Plages de sous-uids/sous-gids de buildkitd (comprises dans l'empreinte).
pub const SUBIDS_LABEL: &str = "dev.ciphera.buildkit.subids";
/// SHA-256 de `buildkitd_config()` (comprise dans l'empreinte).
pub const CONFIG_LABEL: &str = "dev.ciphera.buildkit.config";
/// Configuration de buildkitd dans le dossier d'identite.
pub const BUILDKIT_CONFIG_FILE: &str = "buildkitd.toml";

/// Images epinglees (`depot:tag@sha256:…`), fichiers de `deploy/containers/`.
pub const POSTGRES_IMAGE_FILE: &str = include_str!("../../../deploy/containers/postgres.digest");
pub const BUILDKIT_IMAGE_FILE: &str = include_str!("../../../deploy/containers/buildkit.digest");

/// Chemins de l'hote, identiques a ceux de `ciphera init`, de l'unite
/// systemd-tmpfiles et de `ciphera.service`.
pub const DEFAULT_POSTGRES_SECRET_DIR: &str = "/etc/ciphera/postgres";
/// Copie du mot de passe lue par PostgreSQL (dans `DEFAULT_POSTGRES_SECRET_DIR`).
pub const DEFAULT_POSTGRES_PASSWORD_FILE: &str = "/etc/ciphera/postgres/db_password";
pub const DEFAULT_POSTGRES_SOCKET_DIR: &str = "/run/ciphera/postgres";
pub const DEFAULT_BUILDKIT_SOCKET_DIR: &str = "/run/ciphera-agent/buildkit";
/// Etat de buildkitd (dossier de l'hote au compte `ciphera-build`, tmpfiles).
pub const DEFAULT_BUILDKIT_STATE_DIR: &str = "/var/lib/ciphera-buildkit";
/// Fichiers d'identite de buildkitd, ecrits par l'agent (son StateDirectory).
pub const DEFAULT_BUILDKIT_IDENTITY_DIR: &str = "/var/lib/ciphera-agent/buildkit-identity";
/// Compte systeme du conteneur PostgreSQL, cree par `install.sh`.
pub const POSTGRES_ACCOUNT: &str = "ciphera-postgres";
/// Compte systeme de buildkitd, cree par `install.sh` avec sa plage de
/// sous-uids/sous-gids.
pub const BUILD_ACCOUNT: &str = "ciphera-build";
/// Profil AppArmor de buildkitd (Ubuntu >= 23.10), installe par install.sh.
pub const BUILDKIT_APPARMOR_PROFILE: &str = "ciphera-buildkitd";
pub const BUILDKIT_APPARMOR_FILE: &str = "/etc/apparmor.d/ciphera-buildkitd";

/// Chemins dans les conteneurs.
const PG_SECRET_DIR: &str = "/run/secrets/ciphera";
const PG_SECRET: &str = "/run/secrets/ciphera/db_password";
const PG_SOCKET_DIR: &str = "/var/run/postgresql";
const PG_VOLUME_TARGET: &str = "/var/lib/postgresql/data";
/// Sous-dossier du volume : cree par l'image avec le compte non root (le
/// volume lui-meme appartient a root).
const PGDATA: &str = "/var/lib/postgresql/data/pgdata";
const BUILDKIT_SOCKET_DIR: &str = "/run/buildkit";
/// HOME de `ciphera-build` dans le conteneur : etat de buildkitd rootless
/// (`~/.local/share/buildkit`), dossier de l'hote `buildkit_state_dir`.
const BUILDKIT_HOME: &str = "/home/ciphera-build";
/// Fichiers d'identite montes sur ceux de l'image.
const BUILDKIT_IDENTITY_FILES: [&str; 4] = ["passwd", "group", "subuid", "subgid"];
/// Configuration de buildkitd dans le conteneur (`--config`).
const BUILDKIT_CONFIG: &str = "/etc/buildkit/buildkitd.toml";

/// Nombre maximal de processus de buildkitd (et de ses etapes `RUN`).
pub const BUILDKIT_PIDS_LIMIT: i64 = 4096;
/// Memoire de buildkitd au moins (octets), quelle que soit la RAM du noeud.
pub const BUILDKIT_MIN_MEMORY: u64 = 1 << 30;
/// Bornes du stockage garde par le ramasse-miettes (Gio).
pub const GC_KEEP_MIN_GIB: u64 = 5;
pub const GC_KEEP_MAX_GIB: u64 = 50;

const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfraConfig {
    /// Dossier contenant `db_password`, copie du mot de passe de la base
    /// lisible par `postgres_user`.
    pub postgres_secret_dir: PathBuf,
    /// Dossier de l'hote ou PostgreSQL cree son socket.
    pub postgres_socket_dir: PathBuf,
    /// `(uid, gid)` du compte `ciphera-postgres`.
    pub postgres_user: (u32, u32),
    /// Dossier de l'hote ou buildkitd cree son socket.
    pub buildkit_socket_dir: PathBuf,
    /// Profil AppArmor de buildkitd ; `None` : `apparmor=unconfined`.
    pub buildkit_apparmor_profile: Option<String>,
    /// Compte `ciphera-build` et ses plages ; `None` (compte ou plage
    /// absents) : buildkitd n'est pas lance, les builds sont indisponibles,
    /// PostgreSQL tourne quand meme.
    pub buildkit_account: Option<BuildkitAccount>,
    /// Dossier de l'hote de l'etat de buildkitd (au compte `ciphera-build`).
    pub buildkit_state_dir: PathBuf,
    /// Dossier ou l'agent ecrit les fichiers d'identite de buildkitd.
    pub buildkit_identity_dir: PathBuf,
    /// Plafonds de ressources de buildkitd.
    pub buildkit_resources: BuildkitResources,
}

/// Plafonds de ressources de buildkitd, calcules sur le noeud.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildkitResources {
    /// Memoire du conteneur, en octets : la moitie de la RAM, 1 Gio au moins.
    pub memory: u64,
    /// CPU du conteneur, en milliardiemes de CPU : tous sauf un, un au moins.
    pub nano_cpus: u64,
    /// Stockage garde par le ramasse-miettes, en Gio : 10 % du disque de
    /// l'etat, entre `GC_KEEP_MIN_GIB` et `GC_KEEP_MAX_GIB`.
    pub gc_keep_gib: u64,
}

impl BuildkitResources {
    /// Plafonds pour un noeud de `total_memory` octets de RAM, `cpus` CPU et
    /// un disque d'etat de `disk_bytes` octets.
    pub fn compute(total_memory: u64, cpus: usize, disk_bytes: u64) -> Self {
        let cpus = u64::try_from(cpus.saturating_sub(1).max(1)).unwrap_or(1);
        Self {
            memory: (total_memory / 2).max(BUILDKIT_MIN_MEMORY),
            nano_cpus: cpus.saturating_mul(1_000_000_000),
            gc_keep_gib: ((disk_bytes / 10) >> 30).clamp(GC_KEEP_MIN_GIB, GC_KEEP_MAX_GIB),
        }
    }

    /// Plafonds du noeud : RAM, CPU et disque portant `state_dir` (le plus
    /// specifique des points de montage qui le contiennent ; introuvable :
    /// seuil minimal).
    pub fn detect(state_dir: &Path) -> Self {
        // Lecture impossible : valeur nulle, donc les seuils minimaux.
        let memory = sysinfo::System::new().map_or(0, |mut s| {
            s.refresh_memory();
            s.total_memory()
        });
        let cpus = std::thread::available_parallelism().map_or(1, usize::from);
        let disk = sysinfo::Disks::new_with_refreshed_list().map_or(0, |disks| {
            disks
                .iter()
                .filter(|d| state_dir.starts_with(d.mount_point()))
                .max_by_key(|d| d.mount_point().as_os_str().len())
                .map_or(0, sysinfo::Disk::total_space)
        });
        Self::compute(memory, cpus, disk)
    }
}

/// Compte systeme de buildkitd et ses plages de sous-identifiants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildkitAccount {
    /// `(uid, gid)` du compte `ciphera-build`.
    pub user: (u32, u32),
    /// Plage de sous-uids (`/etc/subuid`).
    pub subuids: SubIds,
    /// Plage de sous-gids (`/etc/subgid`).
    pub subgids: SubIds,
}

/// Plage de sous-identifiants (`nom:debut:nombre` de `/etc/subuid`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubIds {
    pub start: u32,
    pub count: u32,
}

#[derive(Debug)]
pub enum InfraError {
    /// Fichier de digest sans `@sha256:<64 hex>`.
    Unpinned(&'static str),
    /// Compte systeme absent de `/etc/passwd`.
    MissingAccount(&'static str),
    /// Compte `ciphera-build` sans plage dans `/etc/subuid` ou `/etc/subgid`.
    MissingSubIds(&'static str),
    /// Compte systeme d'uid ou de gid 0 : refuse.
    RootAccount(&'static str),
    /// Lecture de `/etc/passwd`, `/etc/subuid` ou `/etc/subgid`.
    Read(&'static str, std::io::Error),
    /// Ecriture des fichiers d'identite de buildkitd.
    Identity(std::io::Error),
    Docker(DockerError),
}

impl fmt::Display for InfraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unpinned(what) => write!(f, "image {what} non epinglee par digest"),
            Self::MissingAccount(name) => {
                write!(f, "compte systeme {name} introuvable (relancer install.sh)")
            }
            Self::MissingSubIds(file) => write!(
                f,
                "plage de {file} du compte {BUILD_ACCOUNT} introuvable dans /etc/{file} (relancer install.sh)"
            ),
            Self::RootAccount(name) => {
                write!(f, "compte systeme {name} d'uid ou de gid 0 : refuse")
            }
            Self::Read(file, e) => write!(f, "lecture de /etc/{file} : {e}"),
            Self::Identity(e) => write!(f, "fichiers d'identite de buildkitd : {e}"),
            Self::Docker(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InfraError {}

impl From<DockerError> for InfraError {
    fn from(e: DockerError) -> Self {
        Self::Docker(e)
    }
}

/// `(uid, gid)` d'un compte dans le texte de `/etc/passwd`.
pub fn lookup_account(passwd: &str, name: &str) -> Option<(u32, u32)> {
    passwd.lines().find_map(|line| {
        let mut f = line.split(':');
        (f.next()? == name).then_some(())?;
        let uid = f.nth(1)?.parse().ok()?;
        let gid = f.next()?.parse().ok()?;
        Some((uid, gid))
    })
}

/// Premiere plage de `name` dans le texte de `/etc/subuid` ou
/// `/etc/subgid` ; nombre non nul, sans debordement. Recherche par nom
/// seulement, comme install.sh (une ligne `<uid>:debut:nombre` est ignoree
/// ici et comptee comme plage d'un autre proprietaire par install.sh).
pub fn lookup_subids(text: &str, name: &str) -> Option<SubIds> {
    text.lines().find_map(|line| {
        let mut f = line.split(':');
        (f.next()? == name).then_some(())?;
        let start: u32 = f.next()?.parse().ok()?;
        let count: u32 = f.next()?.parse().ok()?;
        (count > 0 && start.checked_add(count).is_some()).then_some(SubIds { start, count })
    })
}

/// Profil AppArmor de buildkitd s'il est installe (`BUILDKIT_APPARMOR_FILE`).
pub fn buildkit_apparmor_profile() -> Option<String> {
    Path::new(BUILDKIT_APPARMOR_FILE)
        .is_file()
        .then(|| BUILDKIT_APPARMOR_PROFILE.to_string())
}

fn read_etc(etc: &Path, file: &'static str) -> Result<String, InfraError> {
    std::fs::read_to_string(etc.join(file)).map_err(|e| InfraError::Read(file, e))
}

/// Compte `ciphera-build` (`etc/passwd`, uid et gid 0 refuses) et ses
/// plages (`etc/subuid`, `etc/subgid`, par nom). install.sh les cree et
/// verifie qu'elles ne recouvrent ni une autre plage ni un uid/gid
/// enumerable de l'hote (les comptes d'annuaire non enumerables, sssd sans
/// `enumerate`, echappent a ce controle : voir install.sh).
pub fn resolve_buildkit_account(etc: &Path) -> Result<BuildkitAccount, InfraError> {
    let passwd = read_etc(etc, "passwd")?;
    let user =
        lookup_account(&passwd, BUILD_ACCOUNT).ok_or(InfraError::MissingAccount(BUILD_ACCOUNT))?;
    if user.0 == 0 || user.1 == 0 {
        return Err(InfraError::RootAccount(BUILD_ACCOUNT));
    }
    let mut ranges = [SubIds { start: 0, count: 0 }; 2];
    for (range, file) in ranges.iter_mut().zip(["subuid", "subgid"]) {
        *range = lookup_subids(&read_etc(etc, file)?, BUILD_ACCOUNT)
            .ok_or(InfraError::MissingSubIds(file))?;
    }
    let [subuids, subgids] = ranges;
    Ok(BuildkitAccount {
        user,
        subuids,
        subgids,
    })
}

impl InfraConfig {
    /// Chemins par defaut ; compte de PostgreSQL lu dans `etc/passwd` (`etc` :
    /// `/etc`), obligatoire ; compte et plages de buildkitd
    /// (`resolve_buildkit_account`) facultatifs : s'ils manquent, l'erreur est
    /// journalisee et seuls les builds sont indisponibles (le plan de controle
    /// a besoin de PostgreSQL pour demarrer) ; profil AppArmor de buildkitd
    /// s'il est installe.
    pub fn resolve(etc: &Path) -> Result<Self, InfraError> {
        let passwd = read_etc(etc, "passwd")?;
        let postgres_user = lookup_account(&passwd, POSTGRES_ACCOUNT)
            .ok_or(InfraError::MissingAccount(POSTGRES_ACCOUNT))?;
        let buildkit_account = match resolve_buildkit_account(etc) {
            Ok(account) => Some(account),
            Err(e) => {
                tracing::error!("builds indisponibles, buildkitd non lance : {e}");
                None
            }
        };
        Ok(Self {
            postgres_secret_dir: PathBuf::from(DEFAULT_POSTGRES_SECRET_DIR),
            postgres_socket_dir: PathBuf::from(DEFAULT_POSTGRES_SOCKET_DIR),
            postgres_user,
            buildkit_socket_dir: PathBuf::from(DEFAULT_BUILDKIT_SOCKET_DIR),
            buildkit_apparmor_profile: buildkit_apparmor_profile(),
            buildkit_account,
            buildkit_state_dir: PathBuf::from(DEFAULT_BUILDKIT_STATE_DIR),
            buildkit_identity_dir: PathBuf::from(DEFAULT_BUILDKIT_IDENTITY_DIR),
            buildkit_resources: BuildkitResources::detect(Path::new(DEFAULT_BUILDKIT_STATE_DIR)),
        })
    }

    /// buildkitd peut tourner (compte `ciphera-build` et plages presents).
    pub fn builds_available(&self) -> bool {
        self.buildkit_account.is_some()
    }
}

/// Fichiers d'identite montes dans buildkitd, `(nom, contenu)` : `root`
/// (uid 0 de l'espace de noms de rootlesskit, que git ou ssh resolvent) et
/// `ciphera-build` (compte de l'hote qui lance rootlesskit), avec sa seule
/// plage de sous-uids/sous-gids.
pub fn buildkit_identity(account: &BuildkitAccount) -> [(&'static str, String); 4] {
    let (uid, gid) = account.user;
    let range = |r: SubIds| format!("{BUILD_ACCOUNT}:{}:{}\n", r.start, r.count);
    let [passwd, group, subuid, subgid] = BUILDKIT_IDENTITY_FILES;
    [
        (
            passwd,
            format!(
                "root:x:0:0:root:/root:/sbin/nologin\n\
                 {BUILD_ACCOUNT}:x:{uid}:{gid}::{BUILDKIT_HOME}:/sbin/nologin\n"
            ),
        ),
        (group, format!("root:x:0:\n{BUILD_ACCOUNT}:x:{gid}:\n")),
        (subuid, range(account.subuids)),
        (subgid, range(account.subgids)),
    ]
}

/// Configuration de buildkitd (`buildkitd.toml`) :
/// - `gateway.v0` n'accepte que le frontend Railpack epingle et
///   `docker/dockerfile` (une directive `syntax` vers un autre depot fait
///   echouer le build) ;
/// - `dockerfile.v0` (frontend integre) actif ;
/// - ramasse-miettes du worker OCI actif, avec `reservedSpace` (le cache
///   garde au plus ce volume, en Gio : `BuildkitResources::gc_keep_gib`).
pub fn buildkitd_config(resources: &BuildkitResources) -> String {
    let railpack = crate::build::railpack::FRONTEND_REPOSITORY;
    let keep = resources.gc_keep_gib;
    format!(
        "# Ecrit par ciphera-agent a chaque demarrage : ne pas modifier.\n\
         [worker.oci]\n  gc = true\n  reservedSpace = \"{keep}GB\"\n\n\
         [frontend.\"dockerfile.v0\"]\n  enabled = true\n\n\
         [frontend.\"gateway.v0\"]\n  enabled = true\n  \
         allowedRepositories = [\"{railpack}\", \"docker/dockerfile\"]\n"
    )
}

fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Ecrit les fichiers d'identite et la configuration de buildkitd (0644 :
/// lus par `ciphera-build` dans le conteneur) s'ils manquent ou different ;
/// remplacement atomique (un conteneur en marche garde l'ancien fichier
/// monte ; l'empreinte de la configuration le fait recreer).
pub fn write_buildkit_identity(
    dir: &Path,
    account: &BuildkitAccount,
    resources: &BuildkitResources,
) -> Result<(), InfraError> {
    std::fs::create_dir_all(dir).map_err(InfraError::Identity)?;
    let config = (BUILDKIT_CONFIG_FILE, buildkitd_config(resources));
    for (name, content) in buildkit_identity(account).into_iter().chain([config]) {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|c| c == content) {
            continue;
        }
        let tmp = dir.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, &content).map_err(InfraError::Identity)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
                .map_err(InfraError::Identity)?;
        }
        std::fs::rename(&tmp, &path).map_err(InfraError::Identity)?;
    }
    Ok(())
}

/// Reference epinglee lue dans un fichier de digest.
pub fn pinned_image(file: &str, what: &'static str) -> Result<String, InfraError> {
    let image = file.trim();
    let digest = image
        .split_once("@sha256:")
        .map(|(_, d)| d)
        .ok_or(InfraError::Unpinned(what))?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(InfraError::Unpinned(what));
    }
    Ok(image.to_string())
}

fn kv(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

/// Specs de `ciphera-postgres` puis, si son compte existe,
/// `ciphera-buildkitd`.
pub fn infra_specs(cfg: &InfraConfig) -> Result<Vec<ContainerSpec>, InfraError> {
    let (uid, gid) = cfg.postgres_user;
    let postgres = ContainerSpec {
        name: POSTGRES_CONTAINER.to_string(),
        image: pinned_image(POSTGRES_IMAGE_FILE, "postgres")?,
        network: INFRA_NETWORK.to_string(),
        cmd: vec!["postgres".into(), "-c".into(), "listen_addresses=".into()],
        env: vec![
            kv("POSTGRES_USER", "ciphera"),
            kv("POSTGRES_DB", "ciphera"),
            kv("POSTGRES_PASSWORD_FILE", PG_SECRET),
            kv(
                "POSTGRES_INITDB_ARGS",
                "--auth-local=scram-sha-256 --auth-host=scram-sha-256",
            ),
            kv("PGDATA", PGDATA),
        ],
        env_is_public: true,
        user: Some(format!("{uid}:{gid}")),
        mounts: vec![
            MountSpec::Volume {
                name: POSTGRES_VOLUME.to_string(),
                target: PG_VOLUME_TARGET.to_string(),
            },
            MountSpec::Bind {
                source: cfg.postgres_socket_dir.clone(),
                target: PG_SOCKET_DIR.to_string(),
                read_only: false,
            },
            MountSpec::Bind {
                source: cfg.postgres_secret_dir.clone(),
                target: PG_SECRET_DIR.to_string(),
                read_only: true,
            },
        ],
        cap_drop: vec!["ALL".to_string()],
        cap_add: Vec::new(),
        no_new_privileges: true,
        security_opt: Vec::new(),
        unmask_system_paths: false,
        pids_limit: None,
        memory: None,
        nano_cpus: None,
        labels: Vec::new(),
        aliases: Vec::new(),
    };
    let Some(account) = cfg.buildkit_account else {
        return Ok(vec![postgres]);
    };
    let apparmor = cfg
        .buildkit_apparmor_profile
        .as_deref()
        .unwrap_or("unconfined");
    let (bk_uid, bk_gid) = account.user;
    let mut mounts = vec![
        MountSpec::Bind {
            source: cfg.buildkit_state_dir.clone(),
            target: BUILDKIT_HOME.to_string(),
            read_only: false,
        },
        MountSpec::Bind {
            source: cfg.buildkit_socket_dir.clone(),
            target: BUILDKIT_SOCKET_DIR.to_string(),
            read_only: false,
        },
    ];
    mounts.extend(BUILDKIT_IDENTITY_FILES.map(|file| MountSpec::Bind {
        source: cfg.buildkit_identity_dir.join(file),
        target: format!("/etc/{file}"),
        read_only: true,
    }));
    mounts.push(MountSpec::Bind {
        source: cfg.buildkit_identity_dir.join(BUILDKIT_CONFIG_FILE),
        target: BUILDKIT_CONFIG.to_string(),
        read_only: true,
    });
    let buildkit = ContainerSpec {
        name: BUILDKIT_CONTAINER.to_string(),
        image: pinned_image(BUILDKIT_IMAGE_FILE, "buildkit")?,
        network: BUILD_NETWORK.to_string(),
        // Point d'entree de l'image : `rootlesskit buildkitd`.
        cmd: vec![
            "--addr".into(),
            format!("unix://{BUILDKIT_SOCKET_DIR}/buildkitd.sock"),
            // chown(uid, -1) : le groupe herite du dossier setgid
            // (ciphera-agent) est conserve.
            "--group=-1".into(),
            // Fichier absent ou invalide : buildkitd ne demarre pas (aucun
            // repli sur une configuration sans liste de frontends).
            "--config".into(),
            BUILDKIT_CONFIG.into(),
        ],
        // L'image fixe HOME, USER, XDG_RUNTIME_DIR et TMPDIR pour son uid
        // 1000 : remplaces par des chemins accessibles a ciphera-build (/tmp
        // du conteneur, 1777).
        env: vec![
            kv("HOME", BUILDKIT_HOME),
            kv("USER", BUILD_ACCOUNT),
            kv("TMPDIR", "/tmp"),
            kv("XDG_RUNTIME_DIR", "/tmp"),
        ],
        env_is_public: true,
        user: Some(format!("{bk_uid}:{bk_gid}")),
        mounts,
        // newuidmap/newgidmap (setuid) ecrivent la correspondance des uids :
        // rien d'autre n'est garde.
        cap_drop: vec!["ALL".to_string()],
        cap_add: vec!["SETUID".to_string(), "SETGID".to_string()],
        // Exception documentee en tete du module (newuidmap/newgidmap).
        no_new_privileges: false,
        security_opt: vec![
            "seccomp=unconfined".to_string(),
            format!("apparmor={apparmor}"),
        ],
        unmask_system_paths: true,
        pids_limit: Some(BUILDKIT_PIDS_LIMIT),
        memory: i64::try_from(cfg.buildkit_resources.memory).ok(),
        nano_cpus: i64::try_from(cfg.buildkit_resources.nano_cpus).ok(),
        labels: vec![
            (
                SUBIDS_LABEL.to_string(),
                format!(
                    "subuid={}:{} subgid={}:{}",
                    account.subuids.start,
                    account.subuids.count,
                    account.subgids.start,
                    account.subgids.count
                ),
            ),
            (
                CONFIG_LABEL.to_string(),
                sha256_hex(&buildkitd_config(&cfg.buildkit_resources)),
            ),
        ],
        aliases: Vec::new(),
    };
    Ok(vec![postgres, buildkit])
}

/// Cree `ciphera-internal` et `ciphera-build`, ecrit les fichiers d'identite
/// de buildkitd, puis fait tourner PostgreSQL et buildkitd. Sans compte
/// `ciphera-build` : PostgreSQL seul, buildkitd arrete s'il existe, erreur
/// journalisee (builds indisponibles).
pub async fn ensure_infra<D: DockerOps>(docker: &D, cfg: &InfraConfig) -> Result<(), InfraError> {
    let specs = infra_specs(cfg)?;
    if let Some(account) = &cfg.buildkit_account {
        write_buildkit_identity(&cfg.buildkit_identity_dir, account, &cfg.buildkit_resources)?;
    }
    docker.ensure_network(INFRA_NETWORK).await?;
    if cfg.builds_available() {
        docker.ensure_network(BUILD_NETWORK).await?;
    }
    for spec in &specs {
        docker.run(spec).await?;
        tracing::info!("{} en marche ({})", spec.name(), spec.image());
    }
    if !cfg.builds_available() {
        // Un buildkitd d'une configuration precedente (par exemple sous
        // l'uid 1000 de l'image) ne doit pas continuer a tourner.
        docker
            .stop(&ContainerId(BUILDKIT_CONTAINER.to_string()))
            .await?;
        tracing::error!(
            "builds indisponibles : compte {BUILD_ACCOUNT} ou ses plages absents (relancer install.sh)"
        );
    }
    Ok(())
}

/// `ensure_infra` jusqu'a reussite (Docker pas encore pret, image en cours
/// de telechargement…), delai double a chaque echec jusqu'a 60 s.
pub async fn ensure_infra_with_retry<D: DockerOps>(docker: &D, cfg: &InfraConfig) {
    let mut delay = RETRY_FIRST;
    loop {
        match ensure_infra(docker, cfg).await {
            Ok(()) => return,
            Err(e) => tracing::warn!(
                "conteneurs d'infrastructure : {e} ; nouvel essai dans {}s",
                delay.as_secs()
            ),
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RETRY_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_files_are_pinned() {
        assert!(pinned_image(POSTGRES_IMAGE_FILE, "postgres").is_ok());
        assert!(pinned_image(BUILDKIT_IMAGE_FILE, "buildkit").is_ok());
        for bad in [
            "postgres:17",
            "postgres@sha256:abc",
            "x@sha256:",
            &format!("x@sha256:{}", "G".repeat(64)),
        ] {
            assert!(pinned_image(bad, "t").is_err(), "{bad}");
        }
    }

    const GIB: u64 = 1 << 30;

    fn cfg(resources: BuildkitResources) -> InfraConfig {
        InfraConfig {
            postgres_secret_dir: PathBuf::from("/etc/ciphera/postgres"),
            postgres_socket_dir: PathBuf::from("/run/ciphera/postgres"),
            postgres_user: (991, 991),
            buildkit_socket_dir: PathBuf::from("/run/ciphera-agent/buildkit"),
            buildkit_apparmor_profile: None,
            buildkit_account: Some(BuildkitAccount {
                user: (995, 994),
                subuids: SubIds {
                    start: 165536,
                    count: 65536,
                },
                subgids: SubIds {
                    start: 231072,
                    count: 65536,
                },
            }),
            buildkit_state_dir: PathBuf::from("/var/lib/ciphera-buildkit"),
            buildkit_identity_dir: PathBuf::from("/var/lib/ciphera-agent/buildkit-identity"),
            buildkit_resources: resources,
        }
    }

    #[test]
    fn resources_follow_the_node() {
        // 16 Gio, 8 CPU, disque de 200 Gio.
        let r = BuildkitResources::compute(16 * GIB, 8, 200 * GIB);
        assert_eq!(r.memory, 8 * GIB);
        assert_eq!(r.nano_cpus, 7_000_000_000);
        assert_eq!(r.gc_keep_gib, 20);
        // Petit noeud : 1 Gio au moins, un CPU au moins, 5 Gio au moins.
        let r = BuildkitResources::compute(GIB, 1, 20 * GIB);
        assert_eq!(
            (r.memory, r.nano_cpus, r.gc_keep_gib),
            (GIB, 1_000_000_000, 5)
        );
        assert_eq!(BuildkitResources::compute(0, 0, 0).gc_keep_gib, 5);
        // Gros disque : 50 Gio au plus.
        assert_eq!(
            BuildkitResources::compute(GIB, 2, 4000 * GIB).gc_keep_gib,
            50
        );
    }

    #[test]
    fn buildkit_spec_has_limits() {
        let r = BuildkitResources::compute(16 * GIB, 8, 200 * GIB);
        let specs = infra_specs(&cfg(r)).unwrap();
        let bk = specs
            .iter()
            .find(|s| s.name() == BUILDKIT_CONTAINER)
            .unwrap();
        assert_eq!(bk.pids_limit, Some(4096));
        assert_eq!(bk.memory, Some((8 * GIB) as i64));
        assert_eq!(bk.nano_cpus, Some(7_000_000_000));
        // PostgreSQL : aucun plafond ajoute.
        let pg = specs
            .iter()
            .find(|s| s.name() == POSTGRES_CONTAINER)
            .unwrap();
        assert_eq!((pg.memory, pg.nano_cpus), (None, None));
        // Un autre plafond recree le conteneur (empreinte differente).
        let other = BuildkitResources::compute(32 * GIB, 8, 200 * GIB);
        let bk2 = &infra_specs(&cfg(other)).unwrap()[1];
        assert_ne!(bk.fingerprint(), bk2.fingerprint());
    }

    #[test]
    fn gc_config_rendered() {
        let r = BuildkitResources::compute(16 * GIB, 8, 200 * GIB);
        let toml = buildkitd_config(&r);
        assert!(
            toml.contains("[worker.oci]\n  gc = true\n  reservedSpace = \"20GB\"\n"),
            "{toml}"
        );
        // Le seuil change l'empreinte, donc recree buildkitd.
        let other = BuildkitResources::compute(16 * GIB, 8, 400 * GIB);
        assert_ne!(sha256_hex(&toml), sha256_hex(&buildkitd_config(&other)));
    }
}
