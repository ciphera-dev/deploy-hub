//! Updater de Ciphera : `ciphera-cli updater`, lance en root par l'unite
//! oneshot `ciphera-updater.service`, elle-meme declenchee par
//! `ciphera-updater.path` quand le plan de controle depose une demande
//! (`/var/lib/ciphera/update-request.json`, `POST /api/system/update`).
//!
//! Ordre des operations (rien ne change sur la machine avant l'etape 5) :
//! 1. demande lue puis retiree (consommee une seule fois) ; HMAC (cle
//!    derivee de l'identite de l'instance) et fraicheur verifies ; toute
//!    version inferieure ou egale a la version installee est refusee. Un
//!    retour en arriere ne passe que par `--allow-downgrade`, en ligne de
//!    commande (root) : jamais par une demande ;
//! 2. verrou exclusif `flock` sur `/run/ciphera/update.lock` : deux mises a
//!    jour ne tournent jamais ensemble (la seconde attend, puis relit la
//!    version installee). La reprise (`--recover`) n'attend jamais : verrou
//!    tenu, elle s'arrete aussitot avec succes ;
//! 3. reprise : une mise a jour interrompue (journal present) est annulee,
//!    fichiers precedents remis en place, et base restauree si la nouvelle
//!    version a pu tourner. Au demarrage de la machine,
//!    `ciphera-updater-recover.service` (`ciphera-cli updater --recover`,
//!    avant les services de Ciphera) fait cette reprise seule ;
//! 4. telechargement et verification : `SHA256SUMS` signe minisign (cle
//!    integree, `install/ciphera.pub`), SHA-256 de l'archive, contenu de
//!    l'archive, puis outils tiers (railpack, buildctl, mise) d'apres le
//!    manifeste `tools.manifest` de la nouvelle release (empreintes
//!    epinglees par son `install.sh`), chacun verifie par SHA-256 ;
//! 5. `pg_dump` du conteneur `ciphera-postgres` vers
//!    `/var/backups/ciphera/<ts>.sql.zst` (les 5 dernieres sont gardees) ;
//! 6. remplacement : copies des fichiers actuels et journal ecrits d'abord,
//!    nouveaux fichiers prepares a cote des anciens (`.<nom>.ciphera-new`),
//!    puis renommes un par un (jamais de binaire a moitie ecrit) ;
//! 7. redemarrage des services et attente de `GET /api/health` (60 s) ;
//!    sans reponse : services arretes, fichiers precedents remis, base
//!    restauree depuis la sauvegarde de l'etape 5 (la nouvelle version a pu
//!    la migrer ; ce qui a ete ecrit depuis la sauvegarde, au plus la duree
//!    de la mise a jour, est perdu), puis nouveau redemarrage.
//!
//! Retour a une version anterieure : `--version vX.Y.Z --allow-downgrade
//! --restore-db <sauvegarde>`, en root ; la base est remise a la sauvegarde
//! donnee avant le demarrage de l'ancien binaire. La sauvegarde doit etre
//! dans `/var/backups/ciphera` avec sa fiche (`<ts>.meta.json` : version et
//! derniere migration appliquee), prise sous la version visee, avec les
//! migrations de cette release.
//!
//! Une sauvegarde n'est jamais restauree sans etre verifiee d'abord :
//! decompressee entierement dans un fichier root, fin de `pg_dump` presente
//! (`-- PostgreSQL database dump complete`), puis rejouee par `psql` dans
//! une seule transaction (`ON_ERROR_STOP`). En cas d'echec, aucun `DROP`
//! n'est valide.
//!
//! Configuration : `/etc/ciphera-updater/updater.env` (root, voir
//! [`config`]), jamais `/etc/ciphera/ciphera.env` (fichier du service, dans
//! un dossier du service). Programmes lances par chemin absolu, environnement vide.
//!
//! Les appels au systeme (telechargement, pg_dump, systemctl, healthcheck)
//! passent par [`System`] : [`host::Host`] en production, un systeme simule
//! dans les tests.

pub mod config;
pub mod host;
mod release;

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use shared::secretbox::SecretBox;
use shared::update::{MAX_REQUEST_BYTES, REQUEST_FILE, UpdateRequest, Version, request_key};

/// Binaires de la release (`bin/` de l'archive), installes dans `bin_dir`.
pub const BINARIES: [&str; 4] = ["ciphera", "ciphera-agent", "ciphera-proxy", "ciphera-cli"];
/// Unites systemd livrees par v0.1 (`deploy/systemd/`). L'updater installe
/// les unites de l'archive signee de la version visee ([`is_unit_name`]) et
/// les note dans le registre [`UNITS_FILE`] ; il n'active que les nouvelles
/// (section `[Install]`) et ne desactive puis retire que les unites du
/// registre absentes de l'archive. Sans registre (installation anterieure),
/// le registre vaut cette liste.
pub const UNITS: [&str; 11] = [
    "ciphera.service",
    "ciphera-proxy.socket",
    "ciphera-proxy.service",
    "ciphera-agent.service",
    "ciphera-updater.service",
    "ciphera-updater.path",
    "ciphera-updater-recover.service",
    "ciphera-egress-check.service",
    "ciphera-egress-check.timer",
    "ciphera-certsign.service",
    "ciphera-certsign.path",
];
/// Unites exigees dans toute archive : le plan de controle et la chaine de
/// mise a jour (sans elles, plus aucune mise a jour possible).
pub const REQUIRED_UNITS: [&str; 4] = [
    "ciphera.service",
    "ciphera-updater.service",
    "ciphera-updater.path",
    "ciphera-updater-recover.service",
];
/// Dossiers des sockets (`systemd/` de l'archive, installe dans `tmpfiles`).
pub const TMPFILES: &str = "ciphera.tmpfiles.conf";
/// Profil AppArmor de buildkitd (`apparmor/` de l'archive, variante
/// AppArmor 4), installe sous ce nom dans `/etc/apparmor.d` ; remplace s'il
/// est installe (`install.sh` ne le pose que la ou AppArmor est actif).
pub const APPARMOR_PROFILE: &str = "ciphera-buildkitd";
/// Variante AppArmor 3 du meme profil (`apparmor/` de l'archive), installee
/// sous le nom [`APPARMOR_PROFILE`] quand `apparmor_parser` est en version 3.
pub const APPARMOR_PROFILE_ABI3: &str = "ciphera-buildkitd.abi3";
/// Services de Ciphera surveilles apres le redemarrage.
pub const SERVICES: [&str; 3] = [
    "ciphera.service",
    "ciphera-proxy.service",
    "ciphera-agent.service",
];
/// Delai laisse au panneau pour repondre apres le redemarrage.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);
/// Fenetre pendant laquelle les services relances doivent rester actifs,
/// sans redemarrage automatique (service qui plante en boucle).
pub const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// Nom d'unite systemd de Ciphera accepte dans une archive :
/// `ciphera[-<mot>]*.{service,socket,path,timer}`, minuscules, chiffres et
/// tirets, 64 caracteres au plus.
pub fn is_unit_name(name: &str) -> bool {
    let Some((stem, kind)) = name.rsplit_once('.') else {
        return false;
    };
    name.len() <= 64
        && matches!(kind, "service" | "socket" | "path" | "timer")
        && (stem == "ciphera" || stem.starts_with("ciphera-"))
        && !stem.ends_with('-')
        && !stem.contains("--")
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// L'unite a-t-elle une section `[Install]` avec une cible (elle est alors
/// activee) ? `WantedBy=` : demarree aussi quand elle est nouvelle.
fn install_section(text: &str) -> (bool, bool) {
    let mut in_install = false;
    let (mut any, mut wanted) = (false, false);
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_install = line == "[Install]";
            continue;
        }
        if !in_install {
            continue;
        }
        let key = line.split_once('=').map(|(k, _)| k.trim());
        match key {
            Some("WantedBy") => {
                any = true;
                wanted = true;
            }
            Some("RequiredBy" | "UpheldBy" | "Also") => any = true,
            _ => {}
        }
    }
    (any, wanted)
}

/// Registre des unites installees par Ciphera (`<share_dir>/units.list`,
/// une par ligne), ecrit par `install.sh` et par l'updater.
pub const UNITS_FILE: &str = "units.list";

/// Unites installees par Ciphera et presentes dans `unit_dir` : celles du
/// registre, ou celles de [`UNITS`] sans registre. Une autre unite
/// `ciphera-*` (ecrite par l'administrateur) n'est jamais touchee.
fn installed_units(paths: &Paths) -> Vec<String> {
    let recorded: Vec<String> = match fs::read_to_string(paths.share_dir.join(UNITS_FILE)) {
        Ok(text) => text
            .lines()
            .map(str::trim)
            .filter(|u| is_unit_name(u))
            .map(str::to_string)
            .collect(),
        Err(_) => UNITS.iter().map(|u| u.to_string()).collect(),
    };
    let mut units: Vec<String> = recorded
        .into_iter()
        .filter(|u| {
            fs::symlink_metadata(paths.unit_dir.join(u)).is_ok_and(|m| m.file_type().is_file())
        })
        .collect();
    units.sort();
    units.dedup();
    units
}

/// Changement d'activation d'unites (`systemctl`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitChange {
    /// `systemctl disable` (`--now` si `stop`).
    pub disable: Vec<String>,
    pub stop: bool,
    /// `systemctl reenable` apres `daemon-reload` : unites nouvelles, ou
    /// unites retirees remises par un retour arriere (si elles etaient
    /// activees). Une unite conservee garde son etat.
    pub enable: Vec<String>,
    /// `systemctl start` ensuite (unites nouvelles `WantedBy=`).
    pub start: Vec<String>,
}

impl UnitChange {
    pub fn is_empty(&self) -> bool {
        self.disable.is_empty() && self.enable.is_empty() && self.start.is_empty()
    }
}

/// Unites changees par une mise a jour, notees au journal pour le retour
/// arriere.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct UnitJournal {
    /// Unites nouvelles activees : desactivees au retour arriere.
    added: Vec<String>,
    /// Unites retirees de la release : desactivees (fichiers gardes jusqu'au
    /// succes), reactivees au retour arriere.
    removed: Vec<String>,
    /// Unites retirees qui etaient activees : reactivees (et relancees) au
    /// retour arriere.
    previous_enabled: Vec<String>,
}
/// Sauvegardes de la base gardees dans `backup_dir`.
pub const KEEP_BACKUPS: usize = 5;
/// Extension des sauvegardes de la base.
const BACKUP_SUFFIX: &str = ".sql.zst";
/// Fiche d'une sauvegarde (`<ts>.meta.json`).
const META_SUFFIX: &str = ".meta.json";
/// Ligne de fin d'une sortie complete de `pg_dump`. Seuls la suivent des
/// lignes `--`, des lignes vides et, depuis pg_dump 17.6,
/// `\unrestrict <cle>` (voir [`dump_complete`]).
pub const DUMP_TRAILER: &str = "-- PostgreSQL database dump complete";
/// Fin de sortie examinee par [`dump_complete`].
const DUMP_TAIL_BYTES: usize = 1024;
/// Sauvegarde decompressee et verifiee, prete pour `psql` (state_dir).
const RESTORE_SQL: &str = "restore.sql";
/// Origine des releases (comme `install.sh`).
pub const DEFAULT_BASE_URL: &str = "https://get.ciphera.ch";
/// Origine des outils tiers (releases GitHub, comme `install.sh`).
pub const DEFAULT_TOOLS_BASE_URL: &str = "https://github.com";
/// Cles publiques minisign de confiance des releases (celles injectees dans
/// `install.sh` publie ; plusieurs pendant une rotation).
const PUBKEY_FILE: &str = include_str!("../../../install/ciphera.pub");

const JOURNAL: &str = "journal.json";
const PREVIOUS: &str = "previous";
const STAGING: &str = "staging";
const VERSION_FILE: &str = "VERSION";
const NEW_SUFFIX: &str = ".ciphera-new";
const RESTORE_SUFFIX: &str = ".ciphera-restore";

/// Chemins de la machine.
#[derive(Clone, Debug)]
pub struct Paths {
    /// Racine de la machine (`/`) : outils de l'hote exiges par les unites.
    pub root: PathBuf,
    /// Binaires de Ciphera (`/usr/local/bin`).
    pub bin_dir: PathBuf,
    /// Outils du pipeline de build (`/usr/local/lib/ciphera/bin`).
    pub tools_dir: PathBuf,
    /// Images epinglees et version installee (`/usr/local/share/ciphera`).
    pub share_dir: PathBuf,
    /// Unites systemd (`/etc/systemd/system`).
    pub unit_dir: PathBuf,
    /// Dossiers des sockets (`/etc/tmpfiles.d/ciphera.conf`).
    pub tmpfiles: PathBuf,
    /// Configuration de l'instance, dont `age.key` (`/etc/ciphera`).
    pub config_dir: PathBuf,
    /// Demande du plan de controle (`/var/lib/ciphera/update-request.json`).
    pub request: PathBuf,
    /// Etat de l'updater : journal, copies, telechargements
    /// (`/var/lib/ciphera-updater`, root, 0700).
    pub state_dir: PathBuf,
    /// Sauvegardes de la base (`/var/backups/ciphera`, root, 0700).
    pub backup_dir: PathBuf,
    /// Verrou (`/run/ciphera/update.lock`).
    pub lock: PathBuf,
    /// Configuration de l'updater (`/etc/ciphera-updater/updater.env`,
    /// root 0600).
    pub updater_config: PathBuf,
    /// Profils AppArmor (`/etc/apparmor.d`).
    pub apparmor_dir: PathBuf,
}

impl Paths {
    /// Chemins de production (ceux d'`install.sh`).
    pub fn system() -> Paths {
        Paths::with_root(Path::new("/"))
    }

    /// Meme arborescence sous `root` (tests).
    pub fn under(root: &Path) -> Paths {
        Paths::with_root(root)
    }

    fn with_root(root: &Path) -> Paths {
        Paths {
            root: root.to_path_buf(),
            bin_dir: root.join("usr/local/bin"),
            tools_dir: root.join("usr/local/lib/ciphera/bin"),
            share_dir: root.join("usr/local/share/ciphera"),
            unit_dir: root.join("etc/systemd/system"),
            tmpfiles: root.join("etc/tmpfiles.d/ciphera.conf"),
            config_dir: root.join("etc/ciphera"),
            request: root
                .join(shared::update::REQUEST_DIR.trim_start_matches('/'))
                .join(REQUEST_FILE),
            state_dir: root.join("var/lib/ciphera-updater"),
            backup_dir: root.join("var/backups/ciphera"),
            lock: root.join("run/ciphera/update.lock"),
            updater_config: root.join("etc/ciphera-updater/updater.env"),
            apparmor_dir: root.join("etc/apparmor.d"),
        }
    }
}

/// Origine des releases et cle de verification.
#[derive(Clone, Debug)]
pub struct Sources {
    pub release_base_url: String,
    pub tools_base_url: String,
    /// Cles publiques minisign de confiance (base64, `RW…`) : une signature
    /// de l'une d'elles suffit (rotation, docs/security/release-signing.md).
    pub pubkeys: Vec<String>,
    /// Cible des archives (`x86_64-unknown-linux-musl`…).
    pub target: String,
}

/// Cles integrees (`install/ciphera.pub` : chaque ligne qui n'est ni vide
/// ni un commentaire minisign).
pub fn embedded_pubkeys() -> Vec<String> {
    PUBKEY_FILE
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .map(str::to_string)
        .collect()
}

/// Cible des archives pour l'architecture de ce binaire.
pub fn host_target() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("x86_64-unknown-linux-musl")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64-unknown-linux-musl")
    } else {
        None
    }
}

impl Sources {
    /// Origines et cle par defaut ; surcharges de test (memes variables
    /// qu'`install.sh`) refusees sans `CIPHERA_INSTALL_TEST=1`.
    pub fn from_env(get: impl Fn(&str) -> Option<String>, target: &str) -> Result<Sources, String> {
        let test = get("CIPHERA_INSTALL_TEST").as_deref() == Some("1");
        let url = |name: &str, default: &str| -> Result<String, String> {
            match get(name).filter(|v| !v.is_empty() && v != default) {
                None => Ok(default.to_string()),
                Some(_) if !test => Err(format!(
                    "{name} est reservee aux tests (CIPHERA_INSTALL_TEST=1) : refusee"
                )),
                Some(v) if !v.starts_with("https://") => {
                    Err(format!("{name} doit commencer par https://"))
                }
                Some(v) if !v.bytes().all(release::url_byte) => {
                    Err(format!("{name} contient des caracteres interdits"))
                }
                Some(v) => Ok(v.trim_end_matches('/').to_string()),
            }
        };
        let pubkeys = match get("CIPHERA_TEST_MINISIGN_PUBKEY").filter(|v| !v.is_empty()) {
            None => embedded_pubkeys(),
            Some(_) if !test => {
                return Err("CIPHERA_TEST_MINISIGN_PUBKEY est reservee aux tests \
                     (CIPHERA_INSTALL_TEST=1) : refusee"
                    .into());
            }
            Some(k)
                if k.starts_with("RW")
                    && k.len() == 56
                    && k.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)) =>
            {
                vec![k]
            }
            Some(_) => return Err("CIPHERA_TEST_MINISIGN_PUBKEY invalide".into()),
        };
        Ok(Sources {
            release_base_url: url("CIPHERA_RELEASE_BASE_URL", DEFAULT_BASE_URL)?,
            tools_base_url: url("CIPHERA_TOOLS_BASE_URL", DEFAULT_TOOLS_BASE_URL)?,
            pubkeys,
            target: target.to_string(),
        })
    }
}

/// Appels au systeme, remplaces par un systeme simule dans les tests.
pub trait System: Sync {
    /// Telecharge `url` (https) dans `dest`.
    fn download(&self, url: &str, dest: &Path) -> io::Result<()>;
    /// Sauvegarde de la base (sortie de `pg_dump` compressee zstd) dans `dest`.
    fn pg_dump(&self, dest: &Path) -> io::Result<()>;
    /// Derniere migration appliquee (`_sqlx_migrations`).
    fn latest_migration(&self) -> io::Result<u32>;
    /// Remplace la base (schema et donnees) par `sql`, sortie de `pg_dump`
    /// deja decompressee et verifiee (fin presente), le plan de controle
    /// arrete ; une seule transaction.
    fn restore_db(&self, sql: &Path) -> io::Result<()>;
    /// Arrete le plan de controle (avant une restauration de la base).
    fn stop_services(&self) -> io::Result<()>;
    /// Relit les unites et redemarre les services de Ciphera.
    fn restart_services(&self) -> io::Result<()>;
    /// `GET /api/health` repond-il 200 (panneau servi, base jointe) avant
    /// `timeout` ?
    fn wait_healthy(&self, timeout: Duration) -> bool;
    /// Services de [`SERVICES`] actifs avant la mise a jour, avec leur
    /// invocation systemd (`InvocationID`).
    fn running_services(&self) -> io::Result<Vec<(String, String)>>;
    /// Chaque service de `before` tourne de nouveau (nouvelle invocation :
    /// nouveaux binaires) et reste actif, sans redemarrage automatique,
    /// pendant `window`. Sinon, la raison.
    fn services_settled(&self, before: &[(String, String)], window: Duration)
    -> Result<(), String>;
    /// Unites de `units` activees (`systemctl is-enabled` : `enabled`).
    fn enabled_units(&self, units: &[String]) -> io::Result<Vec<String>>;
    /// Desactive, relit (`daemon-reload`), puis active et demarre des unites.
    fn apply_units(&self, change: &UnitChange) -> io::Result<()>;
    /// Etape de post-installation du nouveau `ciphera-cli` (`post-install
    /// --from <ancienne> --to <nouvelle>`), fichiers en place, avant le
    /// redemarrage : ce que la nouvelle version doit ajuster sur la machine.
    /// Un echec ramene la version precedente.
    fn post_install(&self, from: Version, to: Version) -> io::Result<()>;
    /// Version majeure d'`apparmor_parser` (`--version`) ; `None` si
    /// illisible.
    fn apparmor_parser_major(&self) -> Option<u32>;
    /// Secondes depuis l'epoque Unix.
    fn now(&self) -> u64;
}

/// Version majeure dans la sortie de `apparmor_parser --version`
/// (`AppArmor parser version 4.0.1`).
pub fn parse_apparmor_parser_major(text: &str) -> Option<u32> {
    let version = text
        .lines()
        .next()?
        .trim()
        .strip_prefix("AppArmor parser version ")?;
    version.split('.').next()?.parse().ok()
}

/// Variante du profil de buildkitd pour un `apparmor_parser` de version
/// majeure `major`, comme `install.sh` : AppArmor 4 a partir de 4, AppArmor 3
/// sinon (version illisible comprise : l'ABI 3.0 est aussi lue par les
/// parsers 4).
pub fn apparmor_variant(major: Option<u32>) -> &'static str {
    match major {
        Some(m) if m >= 4 => APPARMOR_PROFILE,
        _ => APPARMOR_PROFILE_ABI3,
    }
}

/// Origine de la version a installer.
#[derive(Clone, Debug)]
pub enum Mode {
    /// Demande deposee par le plan de controle (ou reprise seule si aucune).
    Request,
    /// Ligne de commande (root) : `--version`, avec `--allow-downgrade`
    /// pour accepter une version inferieure ou egale, qui exige
    /// `--restore-db` (sauvegarde de la base prise sous cette version).
    Manual {
        version: Version,
        allow_downgrade: bool,
        restore_db: Option<PathBuf>,
    },
    /// Demarrage de la machine (`ciphera-updater-recover.service`, avant
    /// les services) : reprise d'une mise a jour interrompue seulement, sans
    /// arreter ni relancer de service.
    Recover,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Aucune demande, rien a reprendre.
    NoRequest,
    /// Mise a jour interrompue annulee (aucune demande ensuite).
    Recovered,
    /// Reprise (`--recover`) sans effet : le verrou est tenu par un updater
    /// en cours, qui a la charge du journal (voir [`run`]).
    Busy,
    Updated {
        from: Version,
        to: Version,
    },
}

#[derive(Debug)]
pub enum UpdateError {
    /// Demande invalide (HMAC, date, format) : ignoree, rien n'est fait.
    Ignored(String),
    /// Version refusee (inferieure ou egale a l'installee).
    Refused(String),
    /// Telechargement ou verification en echec : rien n'a change.
    Verification(String),
    /// Sauvegarde de la base impossible : rien n'a change.
    Backup(String),
    /// Nouvelle version installee puis retiree (healthcheck en echec).
    RolledBack(String),
    /// Erreur locale (fichiers, verrou).
    Io(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ignored(m) => write!(f, "demande ignoree : {m}"),
            Self::Refused(m) => write!(f, "mise a jour refusee : {m}"),
            Self::Verification(m) => write!(f, "verification en echec, rien n'est modifie : {m}"),
            Self::Backup(m) => write!(
                f,
                "sauvegarde de la base impossible, rien n'est modifie : {m}"
            ),
            Self::RolledBack(m) => write!(f, "retour a la version precedente : {m}"),
            Self::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for UpdateError {}

fn io_err(what: &str) -> impl Fn(io::Error) -> UpdateError + '_ {
    move |e| UpdateError::Io(format!("{what} : {e}"))
}

/// Applique une mise a jour (voir la documentation du module).
pub fn run(
    mode: Mode,
    sys: &dyn System,
    paths: &Paths,
    sources: &Sources,
) -> Result<Outcome, UpdateError> {
    // 1. Version demandee.
    let live = !matches!(mode, Mode::Recover);
    let (target, allow_downgrade, restore_db) = match mode {
        Mode::Manual {
            version,
            allow_downgrade,
            restore_db,
        } => (Some(version), allow_downgrade, restore_db),
        Mode::Request => (read_request(sys, paths)?, false, None),
        Mode::Recover => (None, false, None),
    };
    let given = match (allow_downgrade, target) {
        (true, Some(to)) => Some(check_downgrade_dump(paths, restore_db.as_deref(), to)?),
        _ if restore_db.is_some() => {
            return Err(UpdateError::Refused(
                "--restore-db n'est accepte qu'avec --allow-downgrade".into(),
            ));
        }
        _ => None,
    };
    if let Some(to) = target {
        check_newer(installed(paths), to, allow_downgrade)?;
    }

    // 2. Verrou (relache a la fin du processus, meme en cas de panique).
    // La reprise ne l'attend jamais : `ciphera-updater-recover.service` est
    // tire par chaque demarrage des services (`RequiredBy=`), y compris le
    // `systemctl restart` de l'updater en cours, qui tient le verrou et
    // attend ce demarrage. Verrou tenu = un updater vivant a la charge du
    // journal : la reprise s'arrete aussitot, avec succes.
    let Some(_lock) = lock(paths, live)? else {
        eprintln!("mise a jour en cours (verrou tenu) : reprise laissee a l'updater actif");
        return Ok(Outcome::Busy);
    };

    // 3. Reprise d'une mise a jour interrompue. Au demarrage (Recover), les
    // services ne tournent pas encore : ni arret ni redemarrage.
    let mut recovered = false;
    if paths.state_dir.join(JOURNAL).exists() {
        eprintln!("mise a jour interrompue detectee : retour a la version precedente");
        restore(sys, paths, live)?;
        recovered = true;
    }
    let Some(to) = target else {
        return Ok(if recovered {
            Outcome::Recovered
        } else {
            Outcome::NoRequest
        });
    };
    let from = installed(paths);
    check_newer(from, to, allow_downgrade)?;

    // 4. Telechargement et verification, sans rien modifier.
    let staging = paths.state_dir.join(STAGING);
    reset_dir(&staging)?;
    let staged = release::fetch(sys, &staging, sources, to).map_err(UpdateError::Verification);
    let result =
        staged.and_then(|staged| install(sys, paths, &staging, &staged, from, to, given.as_ref()));
    let _ = fs::remove_dir_all(&staging);
    result
}

/// Fiche d'une sauvegarde : version de Ciphera et derniere migration
/// appliquee au moment du `pg_dump`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupMeta {
    pub version: String,
    pub migration: u32,
}

/// Sauvegarde donnee a `--restore-db`, verifiee.
struct GivenDump {
    path: PathBuf,
    meta: BackupMeta,
}

fn meta_path(dump: &Path) -> Option<PathBuf> {
    let name = dump.file_name()?.to_str()?;
    let stem = name.strip_suffix(BACKUP_SUFFIX)?;
    Some(dump.with_file_name(format!("{stem}{META_SUFFIX}")))
}

/// `--allow-downgrade` exige une sauvegarde de la base prise sous la
/// version visee : dans `backup_dir` (root 0700), fichier ordinaire (pas de
/// lien), fiche presente et de cette version, contenu complet.
fn check_downgrade_dump(
    paths: &Paths,
    dump: Option<&Path>,
    to: Version,
) -> Result<GivenDump, UpdateError> {
    let refused = |m: String| Err(UpdateError::Refused(m));
    let Some(dump) = dump else {
        return refused(
            "--allow-downgrade exige --restore-db <sauvegarde.sql.zst> : une version \
             anterieure ne demarre pas sur la base migree par la version installee. \
             Donner une sauvegarde prise sous la version visee (/var/backups/ciphera) ; \
             ce qui a ete ecrit depuis cette sauvegarde sera perdu"
                .into(),
        );
    };
    match fs::symlink_metadata(dump) {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return refused(format!(
                "sauvegarde {} : fichier ordinaire attendu (lien refuse)",
                dump.display()
            ));
        }
        Err(_) => return refused(format!("sauvegarde {} introuvable", dump.display())),
    }
    // Chemin resolu, garde jusqu'a la restauration : un dossier lien sur le
    // chemin donne ne peut plus etre redirige entre la verification et
    // l'usage (`backup_dir` et ses parents sont a root).
    let given = dump;
    let Ok(dump) = fs::canonicalize(given) else {
        return refused(format!("sauvegarde {} introuvable", given.display()));
    };
    let dump = dump.as_path();
    let inside = match (dump.parent(), fs::canonicalize(&paths.backup_dir)) {
        (Some(dir), Ok(backups)) => dir == backups,
        _ => false,
    };
    let named = dump
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(BACKUP_SUFFIX) && !n.starts_with('.'));
    if !inside || !named {
        return refused(format!(
            "sauvegarde {} refusee : seules les sauvegardes <date>.sql.zst de \
             /var/backups/ciphera (root 0700) sont utilisables",
            dump.display()
        ));
    }
    let Some(meta) = meta_path(dump).and_then(|m| read_meta(&m).ok()) else {
        return refused(format!(
            "fiche de la sauvegarde {} absente ou illisible (<date>.meta.json) : \
                 version et migrations inconnues",
            dump.display()
        ));
    };
    if meta.version != to.to_string() {
        return refused(format!(
            "sauvegarde prise sous {}, pas sous {to} : un binaire plus ancien migrerait \
             cette base, ce qui a ete ecrit depuis serait perdu sans le dire",
            meta.version
        ));
    }
    if let Err(e) = decode_dump(dump, &mut io::sink()) {
        return refused(format!("sauvegarde {} invalide : {e}", dump.display()));
    }
    Ok(GivenDump {
        path: dump.to_path_buf(),
        meta,
    })
}

fn read_meta(path: &Path) -> io::Result<BackupMeta> {
    let mut file = open_nofollow(path)?;
    let mut buf = Vec::new();
    (&mut file).take(4096).read_to_end(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

/// Ouvre un fichier ordinaire sans suivre de lien ni bloquer.
fn open_nofollow(path: &Path) -> io::Result<File> {
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    let file = opts.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("pas un fichier ordinaire"));
    }
    Ok(file)
}

/// Decompresse entierement la sauvegarde `dump` vers `out` et verifie la
/// fin de `pg_dump` : une sauvegarde coupee, abimee ou interrompue echoue.
fn decode_dump(dump: &Path, out: &mut dyn Write) -> io::Result<()> {
    struct Tail<'a> {
        out: &'a mut dyn Write,
        tail: Vec<u8>,
    }
    impl Write for Tail<'_> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let n = self.out.write(buf)?;
            self.tail.extend_from_slice(&buf[..n]);
            let keep = DUMP_TAIL_BYTES;
            if self.tail.len() > 4 * keep {
                self.tail.drain(..self.tail.len() - keep);
            }
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.out.flush()
        }
    }
    let file = open_nofollow(dump)?;
    let mut tail = Tail {
        out,
        tail: Vec::new(),
    };
    zstd::stream::copy_decode(file, &mut tail)?;
    tail.flush()?;
    if !dump_complete(&String::from_utf8_lossy(&tail.tail)) {
        return Err(io::Error::other(
            "fin de pg_dump absente (sauvegarde incomplete)",
        ));
    }
    Ok(())
}

/// Verifie une sauvegarde `<ts>.sql.zst` sans l'ecrire nulle part :
/// decompression complete et fin de `pg_dump` (voir [`dump_complete`]).
pub fn verify_dump(dump: &Path) -> io::Result<()> {
    decode_dump(dump, &mut io::sink())
}

/// Fin d'une sortie complete de `pg_dump` (format texte) : en partant de la
/// fin, seules des lignes vides, `--` et `\unrestrict <cle>` (pg_dump 17.6
/// et suivants) precedent la ligne [`DUMP_TRAILER`].
pub fn dump_complete(tail: &str) -> bool {
    for line in tail.lines().rev() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line == "--" || line.starts_with("\\unrestrict ") {
            continue;
        }
        return line == DUMP_TRAILER;
    }
    false
}

/// Verifie `dump` en le decompressant dans `<state_dir>/restore.sql` (root
/// 0600), puis le rejoue ; rien n'est touche si la verification echoue.
fn prepare_restore(paths: &Paths, dump: &Path) -> Result<PathBuf, UpdateError> {
    let sql = paths.state_dir.join(RESTORE_SQL);
    let _ = fs::remove_file(&sql);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = opts
        .open(&sql)
        .and_then(|mut f| decode_dump(dump, &mut f).and_then(|()| f.sync_all()));
    if let Err(e) = result {
        let _ = fs::remove_file(&sql);
        return Err(UpdateError::Io(format!(
            "sauvegarde {} invalide, base non restauree : {e}",
            dump.display()
        )));
    }
    Ok(sql)
}

fn install(
    sys: &dyn System,
    paths: &Paths,
    staging: &Path,
    staged: &release::Staged,
    from: Version,
    to: Version,
    given: Option<&GivenDump>,
) -> Result<Outcome, UpdateError> {
    // Retour arriere : la sauvegarde donnee porte les migrations de la
    // release visee.
    if let Some(g) = given
        && g.meta.migration != staged.migration
    {
        return Err(UpdateError::Refused(format!(
            "sauvegarde {} : derniere migration {} au lieu de {} pour {to}",
            g.path.display(),
            g.meta.migration,
            staged.migration
        )));
    }

    // Outils de l'hote appeles par les nouvelles unites (`nft` pour le
    // filtrage des builds) : absents, l'unite ne demarrerait pas. Refus
    // avant toute modification.
    check_host_tools(paths, staged)?;
    // Services en marche avant la mise a jour : ils doivent tourner de
    // nouveau, sur les nouveaux binaires, et rester stables.
    let running = sys
        .running_services()
        .map_err(io_err("etat des services"))?;

    // 5. Sauvegarde de la base, puis anciennes sauvegardes retirees.
    let dump = backup(sys, paths, from)?;
    let mut keep = vec![dump.as_path()];
    keep.extend(given.map(|g| g.path.as_path()));
    prune_backups(paths, &keep);

    // 6. Remplacement (journal d'abord), puis unites activees ou
    // desactivees selon l'archive, puis etape du nouveau binaire.
    let version_file = staging.join(VERSION_FILE);
    fs::write(&version_file, format!("{to}\n")).map_err(io_err("version"))?;
    let units_file = staging.join(UNITS_FILE);
    let listed: String = staged.units.iter().map(|u| format!("{u}\n")).collect();
    fs::write(&units_file, listed).map_err(io_err("registre des unites"))?;
    let (units, change) = unit_changes(sys, paths, staged)?;
    let apparmor = apparmor_source(sys, paths, staged);
    let plan = plan(paths, staged, apparmor, &version_file, &units_file);
    if let Err(e) = replace(paths, &plan, from, to, &dump, &units) {
        if paths.state_dir.join(JOURNAL).exists()
            && let Err(r) = restore(sys, paths, true)
        {
            eprintln!("retour arriere en echec : {r}");
        }
        return Err(e);
    }
    let prepared = if change.is_empty() {
        Ok(())
    } else {
        sys.apply_units(&change)
            .map_err(|e| format!("activation des unites : {e}"))
    }
    .and_then(|()| {
        sys.post_install(from, to)
            .map_err(|e| format!("post-installation de {to} : {e}"))
    });
    if let Err(e) = prepared {
        restore(sys, paths, true)?;
        return Err(UpdateError::RolledBack(format!(
            "{e} ; {from} remise en place"
        )));
    }

    // 7. Base de la version visee (retour arriere), puis redemarrage et
    // healthcheck (le journal note `started` avant les renommages).
    if let Some(g) = given {
        let restored = prepare_restore(paths, &g.path).and_then(|sql| {
            let r = sys
                .stop_services()
                .and_then(|()| sys.restore_db(&sql))
                .map_err(|e| {
                    UpdateError::Io(format!("restauration de {} : {e}", g.path.display()))
                });
            let _ = fs::remove_file(&sql);
            r
        });
        if let Err(e) = restored {
            restore(sys, paths, true)?;
            return Err(UpdateError::RolledBack(format!(
                "{e} ; {from} remise en place"
            )));
        }
    }
    // Succes : panneau servi et base jointe, puis chaque service qui
    // tournait avant relance sur les nouveaux binaires et stable.
    let s = HEALTH_TIMEOUT.as_secs();
    let verdict = match sys.restart_services() {
        Ok(()) if sys.wait_healthy(HEALTH_TIMEOUT) => sys.services_settled(&running, SETTLE_WINDOW),
        Ok(()) => Err(format!(
            "/api/health ne repond pas (ou sans la base) en {s} s"
        )),
        Err(e) => Err(format!("redemarrage des services en echec : {e}")),
    };
    if let Err(why) = verdict {
        restore(sys, paths, true)?;
        if !sys.wait_healthy(HEALTH_TIMEOUT) {
            eprintln!("attention : le panneau ne repond pas non plus apres le retour arriere");
        }
        return Err(UpdateError::RolledBack(format!(
            "{to} installee mais {why} : {from} remise en place et base restauree depuis \
             {dump} (les modifications faites pendant la mise a jour, jusqu'a {s} s, sont \
             perdues)",
            dump = dump.display(),
        )));
    }
    // Unites retirees de la release : fichiers supprimes une fois le succes
    // acquis (gardes jusque-la pour le retour arriere).
    for u in &units.removed {
        if let Err(e) = fs::remove_file(paths.unit_dir.join(u)) {
            eprintln!("attention : unite retiree {u} non supprimee : {e}");
        }
    }
    if !units.removed.is_empty()
        && let Err(e) = sys.apply_units(&UnitChange::default())
    {
        eprintln!("attention : {e}");
    }
    let _ = fs::remove_file(paths.state_dir.join(JOURNAL));
    let _ = fs::remove_dir_all(paths.state_dir.join(PREVIOUS));
    Ok(Outcome::Updated { from, to })
}

/// Unites de l'archive comparees aux unites installees par Ciphera
/// ([`installed_units`]) : nouvelles (activees, demarrees si `WantedBy=`),
/// retirees (desactivees et arretees, etat d'activation note pour le
/// retour arriere), conservees (etat d'activation garde : une unite que
/// l'administrateur a desactivee le reste). Une unite deja presente mais
/// hors registre garde aussi son etat.
fn unit_changes(
    sys: &dyn System,
    paths: &Paths,
    staged: &release::Staged,
) -> Result<(UnitJournal, UnitChange), UpdateError> {
    let old = installed_units(paths);
    let mut journal = UnitJournal::default();
    let mut change = UnitChange {
        stop: true,
        ..UnitChange::default()
    };
    for u in &old {
        if !staged.units.contains(u) {
            journal.removed.push(u.clone());
            change.disable.push(u.clone());
        }
    }
    journal.previous_enabled = sys
        .enabled_units(&journal.removed)
        .map_err(io_err("etat des unites retirees"))?;
    for u in &staged.units {
        if old.contains(u) || paths.unit_dir.join(u).exists() {
            continue;
        }
        let text = fs::read_to_string(staged.pkg.join("systemd").join(u))
            .map_err(io_err("unite de la release"))?;
        let (enabled, wanted) = install_section(&text);
        if !enabled {
            continue;
        }
        change.enable.push(u.clone());
        journal.added.push(u.clone());
        if wanted {
            change.start.push(u.clone());
        }
    }
    Ok((journal, change))
}

/// Agent installe, dont le mode `egress` charge le filtrage par nft.
const AGENT_BIN: &str = "/usr/local/bin/ciphera-agent";
const NFT_BIN: &str = "/usr/sbin/nft";

/// Paquet du systeme qui fournit un outil de l'hote (message d'erreur).
fn package_of(tool: &str) -> &str {
    match tool {
        NFT_BIN => "nftables",
        _ => "le paquet qui le fournit",
    }
}

/// Programmes lances par les unites de la release (`ExecStart*=`, prefixes
/// `+-!@:` retires) hors de Ciphera (`/usr/local`), et nft pour
/// `ciphera-agent egress` : chacun doit exister et etre executable sur
/// l'hote. Toutes les unites sont activees : meme une
/// unite dont une `ConditionPathExists=` est fausse a cet instant (l'agent,
/// Docker arrete ou pas encore installe) demarrera plus tard, donc l'outil
/// est exige dans tous les cas.
fn check_host_tools(paths: &Paths, staged: &release::Staged) -> Result<(), UpdateError> {
    let on_host = |p: &str| paths.root.join(p.trim_start_matches('/'));
    for unit in &staged.units {
        let path = staged.pkg.join("systemd").join(unit);
        let text = fs::read_to_string(&path).map_err(io_err("unite de la release"))?;
        for line in text.lines() {
            let Some((key, value)) = line.trim_start().split_once('=') else {
                continue;
            };
            if !key.trim_end().starts_with("ExecStart") {
                continue;
            }
            let mut words = value
                .trim_start()
                .trim_start_matches(['+', '-', '!', '@', ':'])
                .split_whitespace();
            let program = words.next().unwrap_or("");
            // Filtrage de la sortie : `ciphera-agent egress` lance nft.
            let program = if program == AGENT_BIN && words.next() == Some("egress") {
                NFT_BIN
            } else {
                program
            };
            if !program.starts_with('/') || program.starts_with("/usr/local/") {
                continue;
            }
            if !is_executable(&on_host(program)) {
                return Err(UpdateError::Refused(format!(
                    "{unit} exige {program}, absent de cette machine : installer {} puis relancer \
                     la mise a jour (rien n'est modifie)",
                    package_of(program)
                )));
            }
        }
    }
    Ok(())
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// Version installee : `<share_dir>/VERSION` (ecrit par `install.sh` et par
/// l'updater), sinon celle de ce binaire (livre avec les autres).
pub fn installed(paths: &Paths) -> Version {
    fs::read_to_string(paths.share_dir.join(VERSION_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| Version::from_pkg(env!("CARGO_PKG_VERSION")))
        .unwrap_or(Version {
            major: 0,
            minor: 0,
            patch: 0,
        })
}

fn check_newer(from: Version, to: Version, allow_downgrade: bool) -> Result<(), UpdateError> {
    if to > from || allow_downgrade {
        Ok(())
    } else {
        Err(UpdateError::Refused(format!(
            "{to} n'est pas superieure a la version installee {from} \
             (retour arriere : ciphera-cli updater --version {to} --allow-downgrade, en root)"
        )))
    }
}

/// Lit puis retire la demande ; `None` sans demande.
fn read_request(sys: &dyn System, paths: &Paths) -> Result<Option<Version>, UpdateError> {
    let bytes = match open_request(&paths.request) {
        Ok(mut file) => {
            let mut buf = Vec::new();
            let read = (&mut file)
                .take(MAX_REQUEST_BYTES + 1)
                .read_to_end(&mut buf);
            drop(file);
            let _ = fs::remove_file(&paths.request);
            read.map_err(|_| UpdateError::Ignored("demande illisible".into()))?;
            if buf.len() as u64 > MAX_REQUEST_BYTES {
                return Err(UpdateError::Ignored("demande trop longue".into()));
            }
            buf
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            let _ = fs::remove_file(&paths.request);
            return Err(UpdateError::Ignored(
                "demande illisible (fichier ordinaire attendu)".into(),
            ));
        }
    };
    let secrets = SecretBox::load(&paths.config_dir.join("age.key"))
        .map_err(|e| UpdateError::Io(format!("cle de l'instance : {e}")))?;
    let key = request_key(&secrets);
    UpdateRequest::from_json(&bytes)
        .and_then(|req| req.verify(&*key, sys.now()))
        .map(Some)
        .map_err(|e| UpdateError::Ignored(e.to_string()))
}

/// Ouvre la demande sans suivre de lien symbolique ni bloquer (une FIFO
/// deposee a sa place ne fige pas l'updater) ; fichier ordinaire de 4 Kio
/// au plus seulement.
fn open_request(path: &Path) -> io::Result<File> {
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    let file = opts.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::other("pas un fichier ordinaire"));
    }
    if meta.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::other("demande trop longue"));
    }
    Ok(file)
}

/// Verrou exclusif. `wait` : attendre qu'il se libere ; sinon `None` s'il
/// est deja tenu.
/// Le fichier est en 0600 (root) : un autre compte ne peut pas l'ouvrir
/// pour tenir le verrou (la reprise croirait un updater actif), meme s'il
/// a ete cree avant par une execution manuelle avec un umask permissif.
/// Son dossier (`/run/ciphera`, normalement cree par systemd-tmpfiles) est
/// cree s'il manque avec le mode de `ciphera.tmpfiles.conf`.
fn lock(paths: &Paths, wait: bool) -> Result<Option<File>, UpdateError> {
    if let Some(dir) = paths.lock.parent() {
        create_dirs(dir, LOCK_DIR_MODE).map_err(io_err("dossier du verrou"))?;
    }
    let mut opts = fs::OpenOptions::new();
    opts.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = opts.open(&paths.lock).map_err(io_err("verrou"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(io_err("droits du verrou"))?;
    }
    if wait {
        file.lock().map_err(io_err("verrou"))?;
        return Ok(Some(file));
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(io_err("verrou")(e)),
    }
}

fn reset_dir(dir: &Path) -> Result<(), UpdateError> {
    match fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err("dossier de travail")(e)),
    }
    create_private_dir(dir)
}

/// Dossier 0700 (root) ; ses parents manquants en 0755.
fn create_private_dir(dir: &Path) -> Result<(), UpdateError> {
    create_dirs(dir, 0o755).map_err(io_err("creation d'un dossier"))?;
    set_mode(dir, 0o700).map_err(io_err("droits d'un dossier"))
}

/// Mode du dossier du verrou (`/run/ciphera`, `ciphera.tmpfiles.conf`).
const LOCK_DIR_MODE: u32 = 0o751;
/// Mode des dossiers de destination crees (binaires, unites, partage).
const PUBLIC_DIR_MODE: u32 = 0o755;

/// Cree `dir` et ses parents manquants avec exactement `mode` (pas celui
/// que laisserait le umask du shell root qui lance l'updater). Les dossiers
/// deja presents ne sont pas modifies.
fn create_dirs(dir: &Path, mode: u32) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.as_os_str().is_empty() || fs::symlink_metadata(d).is_ok() {
            break;
        }
        missing.push(d);
        cur = d.parent();
    }
    for d in missing.into_iter().rev() {
        match fs::create_dir(d) {
            Ok(()) => set_mode(d, mode)?,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// `pg_dump` vers `<backup_dir>/<AAAAMMJJTHHMMSSZ>.sql.zst` (0600),
/// verifie (fin de `pg_dump`), avec sa fiche `<ts>.meta.json` (version
/// installee, derniere migration appliquee).
fn backup(sys: &dyn System, paths: &Paths, from: Version) -> Result<PathBuf, UpdateError> {
    create_private_dir(&paths.backup_dir)?;
    let ts = utc_stamp(sys.now());
    let name = format!("{ts}{BACKUP_SUFFIX}");
    let dest = paths.backup_dir.join(&name);
    let partial = paths.backup_dir.join(format!(".{name}.partial"));
    let meta_file = paths.backup_dir.join(format!("{ts}{META_SUFFIX}"));
    let result = File::create(&partial)
        .and_then(|f| {
            drop(f);
            set_mode(&partial, 0o600)
        })
        .and_then(|()| sys.pg_dump(&partial))
        .and_then(|()| decode_dump(&partial, &mut io::sink()))
        .and_then(|()| sys.latest_migration())
        .and_then(|migration| {
            let meta = BackupMeta {
                version: from.to_string(),
                migration,
            };
            let json = serde_json::to_vec(&meta).map_err(io::Error::other)?;
            write_synced(&meta_file, &json)?;
            set_mode(&meta_file, 0o600)
        })
        .and_then(|()| fs::rename(&partial, &dest))
        .and_then(|()| sync_dir(&paths.backup_dir));
    if let Err(e) = result {
        let _ = fs::remove_file(&partial);
        let _ = fs::remove_file(&meta_file);
        return Err(UpdateError::Backup(e.to_string()));
    }
    eprintln!("base sauvegardee : {}", dest.display());
    Ok(dest)
}

/// Garde les [`KEEP_BACKUPS`] sauvegardes les plus recentes (noms dates,
/// tries) ; `keep` n'est jamais retire. Les autres fichiers restent.
fn prune_backups(paths: &Paths, keep: &[&Path]) {
    let Ok(dir) = fs::read_dir(&paths.backup_dir) else {
        return;
    };
    let mut dumps: Vec<PathBuf> = dir
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .is_some_and(|n| n.ends_with(BACKUP_SUFFIX) && !n.starts_with('.'))
        })
        .collect();
    dumps.sort();
    let excess = dumps.len().saturating_sub(KEEP_BACKUPS);
    for old in dumps.into_iter().take(excess) {
        let same = |k: &&Path| {
            *k == old
                || matches!(
                    (fs::canonicalize(k), fs::canonicalize(&old)),
                    (Ok(a), Ok(b)) if a == b
                )
        };
        if keep.iter().any(same) {
            continue;
        }
        match fs::remove_file(&old) {
            Ok(()) => eprintln!("ancienne sauvegarde retiree : {}", old.display()),
            Err(e) => eprintln!("sauvegarde {} non retiree : {e}", old.display()),
        }
        if let Some(meta) = meta_path(&old) {
            let _ = fs::remove_file(meta);
        }
    }
}

/// Date UTC compacte (`20261123T101500Z`).
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Jours -> date civile (algorithme de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// Fichier a installer : source preparee, destination, droits.
struct Planned {
    src: PathBuf,
    dest: PathBuf,
    mode: u32,
}

/// Profil AppArmor de l'archive qui remplace le profil installe : la
/// variante du `apparmor_parser` de la machine. Rien si le profil n'est pas
/// installe (machine sans AppArmor) ou si l'archive n'a pas cette variante
/// (le profil installe reste alors en place).
fn apparmor_source(sys: &dyn System, paths: &Paths, staged: &release::Staged) -> Option<PathBuf> {
    if !paths.apparmor_dir.join(APPARMOR_PROFILE).is_file() {
        return None;
    }
    let variant = apparmor_variant(sys.apparmor_parser_major());
    if !staged.apparmor.iter().any(|a| a == variant) {
        eprintln!(
            "attention : profil AppArmor {variant} absent de l'archive, profil installe garde"
        );
        return None;
    }
    Some(staged.pkg.join("apparmor").join(variant))
}

fn plan(
    paths: &Paths,
    staged: &release::Staged,
    apparmor: Option<PathBuf>,
    version_file: &Path,
    units_file: &Path,
) -> Vec<Planned> {
    let file = |src: PathBuf, dest: PathBuf, mode| Planned { src, dest, mode };
    let mut plan: Vec<Planned> = BINARIES
        .iter()
        .map(|b| file(staged.pkg.join("bin").join(b), paths.bin_dir.join(b), 0o755))
        .collect();
    plan.extend(
        staged
            .tools
            .iter()
            .map(|(src, dest)| file(src.clone(), paths.tools_dir.join(dest), 0o755)),
    );
    plan.extend(staged.units.iter().map(|u| {
        file(
            staged.pkg.join("systemd").join(u),
            paths.unit_dir.join(u),
            0o644,
        )
    }));
    plan.push(file(
        staged.pkg.join("systemd").join(TMPFILES),
        paths.tmpfiles.clone(),
        0o644,
    ));
    // Profil AppArmor remplace seulement s'il est installe (AppArmor actif,
    // voir install.sh), par la variante du parser de la machine.
    if let Some(src) = apparmor {
        plan.push(file(src, paths.apparmor_dir.join(APPARMOR_PROFILE), 0o644));
    }
    plan.extend(staged.containers.iter().map(|c| {
        file(
            staged.pkg.join("containers").join(c),
            paths.share_dir.join("containers").join(c),
            0o644,
        )
    }));
    // Registre des unites (remis par le retour arriere, comme le reste).
    plan.push(file(
        units_file.to_path_buf(),
        paths.share_dir.join(UNITS_FILE),
        0o644,
    ));
    // En dernier : la version ne change qu'une fois tout le reste en place.
    plan.push(file(
        version_file.to_path_buf(),
        paths.share_dir.join(VERSION_FILE),
        0o644,
    ));
    plan
}

/// Journal d'une mise a jour en cours : de quoi tout remettre en place.
#[derive(Serialize, Deserialize)]
struct Journal {
    from: String,
    to: String,
    entries: Vec<JournalEntry>,
    /// Sauvegarde de la base prise juste avant le remplacement.
    dump: PathBuf,
    /// La nouvelle version a pu tourner (ou la base a ete remplacee) : le
    /// retour arriere restaure aussi la base.
    #[serde(default)]
    started: bool,
    /// Unites ajoutees ou retirees (activation a defaire au retour arriere).
    #[serde(default)]
    units: UnitJournal,
}

#[derive(Serialize, Deserialize)]
struct JournalEntry {
    dest: PathBuf,
    /// Copie du fichier remplace ; `None` : fichier nouveau, a retirer.
    backup: Option<PathBuf>,
}

/// `<dossier>/.<nom><suffixe>` : fichier temporaire a cote de `dest`.
fn sibling(dest: &Path, suffix: &str) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!(".{name}{suffix}"))
}

/// Copie `src` en `dest` avec `mode`, donnees ecrites sur disque.
fn copy_synced(src: &Path, dest: &Path, mode: u32) -> io::Result<()> {
    let _ = fs::remove_file(dest);
    let mut input = File::open(src)?;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)?;
    io::copy(&mut input, &mut out)?;
    out.sync_all()?;
    drop(out);
    set_mode(dest, mode)
}

fn write_synced(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = sibling(path, NEW_SUFFIX);
    let _ = fs::remove_file(&tmp);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    // Le renommage lui-meme sur disque (journal : avant tout redemarrage).
    match path.parent() {
        Some(dir) => sync_dir(dir),
        None => Ok(()),
    }
}

/// Ecrit le dossier sur disque (entrees creees, renommees ou retirees).
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

fn current_mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).map_or(0o644, |m| m.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        0o644
    }
}

fn replace(
    paths: &Paths,
    plan: &[Planned],
    from: Version,
    to: Version,
    dump: &Path,
    units: &UnitJournal,
) -> Result<(), UpdateError> {
    // Copies des fichiers actuels, puis journal : avant toute modification.
    let previous = paths.state_dir.join(PREVIOUS);
    reset_dir(&previous)?;
    let mut entries = Vec::with_capacity(plan.len());
    for (i, p) in plan.iter().enumerate() {
        let backup = match fs::symlink_metadata(&p.dest) {
            Ok(m) if m.is_file() => {
                let copy = previous.join(i.to_string());
                copy_synced(&p.dest, &copy, current_mode(&p.dest))
                    .map_err(io_err("copie des fichiers actuels"))?;
                Some(copy)
            }
            Ok(_) => {
                return Err(UpdateError::Io(format!(
                    "{} n'est pas un fichier ordinaire : refuse",
                    p.dest.display()
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_err("lecture des fichiers actuels")(e)),
        };
        entries.push(JournalEntry {
            dest: p.dest.clone(),
            backup,
        });
    }
    let journal = Journal {
        from: from.to_string(),
        to: to.to_string(),
        entries,
        dump: dump.to_path_buf(),
        started: false,
        units: units.clone(),
    };
    write_journal(paths, &journal).map_err(io_err("journal"))?;

    // Nouveaux fichiers prepares a cote des anciens, puis renommes.
    for p in plan {
        if let Some(dir) = p.dest.parent() {
            create_dirs(dir, PUBLIC_DIR_MODE).map_err(io_err("dossier de destination"))?;
        }
        copy_synced(&p.src, &sibling(&p.dest, NEW_SUFFIX), p.mode)
            .map_err(io_err("preparation des nouveaux fichiers"))?;
    }
    // Des le premier renommage, un nouveau binaire peut tourner (relance
    // automatique) et migrer la base : le journal le note, sur disque, avant.
    mark_started(paths).map_err(io_err("journal"))?;
    for p in plan {
        fs::rename(sibling(&p.dest, NEW_SUFFIX), &p.dest)
            .map_err(io_err("remplacement des fichiers"))?;
    }
    sync_dirs(plan.iter().filter_map(|p| p.dest.parent()));
    eprintln!("fichiers de {to} en place (precedents : {from})");
    Ok(())
}

/// Journal illisible : mis de cote (`journal.json.corrupt`) avec une
/// erreur visible, pour ne pas bloquer les mises a jour suivantes.
fn read_journal(paths: &Paths) -> Result<Journal, UpdateError> {
    let path = paths.state_dir.join(JOURNAL);
    let bytes = fs::read(&path).map_err(io_err("journal"))?;
    serde_json::from_slice(&bytes).map_err(|_| {
        let corrupt = paths.state_dir.join(format!("{JOURNAL}.corrupt"));
        let moved = fs::rename(&path, &corrupt).and_then(|()| sync_dir(&paths.state_dir));
        UpdateError::Io(match moved {
            Ok(()) => format!(
                "journal de mise a jour illisible, mis de cote dans {} : verifier a la main \
                 les fichiers de Ciphera (copies dans {}) et la version installee",
                corrupt.display(),
                paths.state_dir.join(PREVIOUS).display()
            ),
            Err(e) => format!(
                "journal de mise a jour illisible ({}) : {e}",
                path.display()
            ),
        })
    })
}

fn write_journal(paths: &Paths, journal: &Journal) -> io::Result<()> {
    let json = serde_json::to_vec(journal).map_err(io::Error::other)?;
    write_synced(&paths.state_dir.join(JOURNAL), &json)
}

/// Note dans le journal que la base peut changer a partir d'ici.
fn mark_started(paths: &Paths) -> io::Result<()> {
    let mut journal = read_journal(paths).map_err(|e| io::Error::other(e.to_string()))?;
    journal.started = true;
    write_journal(paths, &journal)
}

fn sync_dirs<'a>(dirs: impl Iterator<Item = &'a Path>) {
    #[cfg(unix)]
    for dir in dirs {
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = dirs;
}

/// Remet en place les fichiers du journal et, si la nouvelle version a pu
/// tourner, la base sauvegardee avant la mise a jour ; retire le journal.
/// `live` : services en marche (arretes avant, relances apres) ; sinon
/// demarrage de la machine, avant les services. Le journal reste si un
/// fichier ou la base n'a pas pu etre remis : la prochaine execution
/// reessaie.
fn restore(sys: &dyn System, paths: &Paths, live: bool) -> Result<(), UpdateError> {
    let path = paths.state_dir.join(JOURNAL);
    let journal = read_journal(paths)?;
    // Sauvegarde verifiee avant de toucher a quoi que ce soit.
    let sql = if journal.started {
        Some(prepare_restore(paths, &journal.dump).map_err(|e| {
            UpdateError::Io(format!(
                "{e} ; rien n'est modifie, journal garde ({}) : restaurer la base a la main",
                path.display()
            ))
        })?)
    } else {
        None
    };
    if live
        && sql.is_some()
        && let Err(e) = sys.stop_services()
    {
        if let Some(sql) = &sql {
            let _ = fs::remove_file(sql);
        }
        return Err(UpdateError::Io(format!(
            "arret des services impossible ({e}) : base non restauree, journal garde \
             (relancer `systemctl start ciphera-updater.service`)"
        )));
    }
    // Unites nouvelles desactivees (arretees si en marche) avant que leurs
    // fichiers ne soient retires.
    let u = &journal.units;
    if !u.added.is_empty()
        && let Err(e) = sys.apply_units(&UnitChange {
            disable: u.added.clone(),
            stop: live,
            ..UnitChange::default()
        })
    {
        eprintln!("desactivation des unites nouvelles en echec : {e}");
    }
    let mut failed = false;
    for e in &journal.entries {
        let _ = fs::remove_file(sibling(&e.dest, NEW_SUFFIX));
        let result = match &e.backup {
            Some(copy) => {
                let tmp = sibling(&e.dest, RESTORE_SUFFIX);
                copy_synced(copy, &tmp, current_mode(copy)).and_then(|()| fs::rename(&tmp, &e.dest))
            }
            None => match fs::remove_file(&e.dest) {
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        if let Err(err) = result {
            eprintln!("retour arriere de {} impossible : {err}", e.dest.display());
            failed = true;
        }
    }
    sync_dirs(journal.entries.iter().filter_map(|e| e.dest.parent()));
    if failed {
        return Err(UpdateError::Io(
            "retour arriere incomplet : relancer `systemctl start ciphera-updater.service`".into(),
        ));
    }
    if let Some(sql) = &sql {
        let restored = sys.restore_db(sql);
        let _ = fs::remove_file(sql);
        if let Err(e) = restored {
            return Err(UpdateError::Io(format!(
                "restauration de la base depuis {} en echec : {e} (le journal est garde : \
                 relancer `systemctl start ciphera-updater.service`)",
                journal.dump.display()
            )));
        }
        eprintln!("base restauree depuis {}", journal.dump.display());
    }
    // Unites retirees qui etaient activees : reactivees, et redemarrees si
    // les services tournent. Les autres gardent leur etat.
    {
        let change = UnitChange {
            enable: u.previous_enabled.clone(),
            start: if live {
                u.previous_enabled.clone()
            } else {
                Vec::new()
            },
            ..UnitChange::default()
        };
        if (!change.is_empty() || !u.added.is_empty())
            && let Err(e) = sys.apply_units(&change)
        {
            eprintln!("reactivation des unites precedentes en echec : {e}");
        }
    }
    if live && let Err(e) = sys.restart_services() {
        eprintln!("redemarrage des services apres le retour arriere en echec : {e}");
    }
    let _ = fs::remove_file(&path);
    let _ = sync_dir(&paths.state_dir);
    let _ = fs::remove_dir_all(paths.state_dir.join(PREVIOUS));
    eprintln!(
        "{} remise en place (au lieu de {})",
        journal.from, journal.to
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossiers manquants crees avec le mode demande (pas celui du umask),
    /// dossiers existants laisses tels quels.
    #[cfg(unix)]
    #[test]
    fn missing_dirs_get_the_requested_mode() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ciphera-dirs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let leaf = root.join("a/b/c");
        create_dirs(&leaf, 0o751).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        for d in [root.join("a"), root.join("a/b"), leaf.clone()] {
            assert_eq!(mode(&d), 0o751, "{}", d.display());
        }
        assert_eq!(mode(&root), 0o700, "dossier existant modifie");
        create_dirs(&leaf, 0o700).unwrap();
        assert_eq!(mode(&leaf), 0o751, "dossier existant modifie");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(1_800_000_000), "20270115T080000Z");
        assert_eq!(utc_stamp(951_782_400), "20000229T000000Z");
        assert_eq!(utc_stamp(1_795_426_496), "20261123T093456Z");
    }

    /// `install.sh` du depot ne porte aucune cle : `install/release-script.sh`
    /// y injecte les cles de `install/ciphera.pub` (celles integrees ici) a la
    /// release.
    #[test]
    fn embedded_keys_are_injected_into_install_sh_at_release() {
        let install = include_str!("../../../install/install.sh");
        let keys = embedded_pubkeys();
        assert!(!keys.is_empty() && keys.len() <= 3, "{keys:?}");
        for key in &keys {
            assert!(key.starts_with("RW") && key.len() == 56, "{key}");
            assert!(
                !install.contains(key.as_str()),
                "cle ecrite en dur dans install.sh"
            );
        }
        let lines: Vec<&str> = install
            .lines()
            .filter(|l| l.starts_with("CIPHERA_PUBKEYS="))
            .collect();
        assert_eq!(lines, ["CIPHERA_PUBKEYS='@CIPHERA_RELEASE_PUBKEYS@'"]);
    }
}
