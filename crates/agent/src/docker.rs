//! Docker, pilote uniquement par son API (bollard, socket Unix
//! `/var/run/docker.sock`) : aucune commande shell, aucun `exec`.
//!
//! `ContainerSpec` decrit un conteneur ; `create_body` le traduit en requete
//! Docker. Les specs d'application (`ContainerSpec::app`) ne peuvent porter
//! ni port publie, ni montage, ni privilege : ces champs n'y existent pas.
//! Tous les conteneurs tournent avec `no-new-privileges`, sauf buildkitd
//! rootless (exception documentee dans `infra`).
//!
//! L'agent ne touche qu'a ce qu'il a cree : un conteneur ou un reseau du meme
//! nom sans l'etiquette `dev.ciphera.managed=true` n'est ni remplace, ni
//! arrete, ni adopte (`DockerError::NotManaged`).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::models::{
    ContainerCreateBody, ContainerInspectResponse, ContainerStateStatusEnum, EndpointSettings,
    HealthStatusEnum, HostConfig, HostConfigLogConfig, Mount, MountTypeEnum, NetworkCreateRequest,
    NetworkingConfig, RestartPolicy as DockerRestart, RestartPolicyNameEnum,
};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, ListContainersOptions, ListImagesOptions,
    ListNetworksOptions, ListVolumesOptions, LogsOptions, RemoveContainerOptions,
    RemoveImageOptions, RemoveVolumeOptions, RestartContainerOptions, StatsOptions,
    StopContainerOptions,
};
use futures_util::stream::{self, BoxStream, StreamExt};
use proto::{ImageRef, LogStream, LogTarget, NodeMetrics};
use sha2::{Digest, Sha256};
use shared::ids::{DeploymentId, EnvironmentId, ProjectId};

/// Etiquette posee sur tout ce que Ciphera cree (conteneurs, reseaux).
pub const MANAGED_LABEL: &str = "dev.ciphera.managed";
/// Empreinte de la spec d'un conteneur : une spec differente le recree.
pub const SPEC_LABEL: &str = "dev.ciphera.spec";
/// Deploiement porte par un conteneur d'application.
pub const DEPLOYMENT_LABEL: &str = "dev.ciphera.deployment";
/// Projet Compose stable de l'environnement (services d'un ensemble Compose).
pub const COMPOSE_LABEL: &str = "dev.ciphera.compose";
/// Projet Ciphera d'un conteneur.
pub const PROJECT_LABEL: &str = "dev.ciphera.project";
/// Role d'un conteneur de Ciphera hors application (`PREPARE_ROLE` : bac a
/// sable de `railpack prepare`). Reserve : refuse dans un fichier Compose.
pub const ROLE_LABEL: &str = "dev.ciphera.role";
/// Bac a sable de `railpack prepare` (balaye au demarrage de l'agent).
pub const PREPARE_ROLE: &str = "prepare";
/// Reseau interne (`internal: true`) de PostgreSQL.
pub const INFRA_NETWORK: &str = "ciphera-internal";
/// Reseau de buildkitd seul : sortie vers Internet (images de base, git),
/// jamais relie a PostgreSQL ni aux applications. Son pont porte le meme
/// nom (`BRIDGE_NAME_OPTION`) : `ciphera-agent.service` filtre sa sortie
/// (ni l'hote, ni les plages privees ou de metadonnees) par ce nom.
pub const BUILD_NETWORK: &str = "ciphera-build";
/// Option Docker du nom de l'interface du pont (15 caracteres au plus).
pub const BRIDGE_NAME_OPTION: &str = "com.docker.network.bridge.name";
/// Prefixe des reseaux de projet, ou tournent les applications.
pub const PROJECT_NETWORK_PREFIX: &str = "ciphera-p-";
/// Longueur maximale d'une ligne de journal transmise (le reste est coupe).
pub const MAX_LOG_LINE: usize = 16 * 1024;
const STOP_TIMEOUT_SECS: i32 = 10;
/// Suppression forcee : essais au plus, et pause entre deux (voir
/// `force_remove`), soit 30 s d'attente au plus en plus de celle de Docker.
const REMOVE_ATTEMPTS: u32 = 4;
const REMOVE_RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(10);
/// Processus au plus par conteneur d'application (bombe fork).
pub const APP_PIDS_LIMIT: i64 = 1024;
/// Capacites retirees a tout conteneur d'application (simple ou Compose).
pub const APP_CAP_DROP: [&str; 4] = ["NET_RAW", "MKNOD", "AUDIT_WRITE", "SETPCAP"];
/// `tmpfs` de `/tmp` quand la racine est en lecture seule : 64 Mio.
pub const APP_TMP_TMPFS: &str = "rw,nosuid,nodev,size=64m";
/// Memoire plancher par conteneur d'application, en octets.
pub const APP_MIN_MEMORY: u64 = 256 << 20;

/// Plafonds de ressources de tout conteneur d'application (application
/// simple et services Compose), calcules sur le noeud : memoire = la moitie
/// de la RAM, 256 Mio au moins ; CPU = tous sauf un, un au moins ; au plus
/// `APP_PIDS_LIMIT` processus. Point unique des deux chemins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppLimits {
    /// Memoire, en octets.
    pub memory: u64,
    /// CPU, en milliardiemes de CPU.
    pub nano_cpus: u64,
}

impl AppLimits {
    pub fn compute(total_memory: u64, cpus: usize) -> Self {
        let cpus = u64::try_from(cpus.saturating_sub(1).max(1)).unwrap_or(1);
        Self {
            memory: (total_memory / 2).max(APP_MIN_MEMORY),
            nano_cpus: cpus.saturating_mul(1_000_000_000),
        }
    }

    /// Plafonds du noeud, lus une fois (RAM illisible : le plancher).
    pub fn node() -> Self {
        static NODE: std::sync::OnceLock<AppLimits> = std::sync::OnceLock::new();
        *NODE.get_or_init(|| {
            let memory = sysinfo::System::new().map_or(0, |mut s| {
                s.refresh_memory();
                s.total_memory()
            });
            let cpus = std::thread::available_parallelism().map_or(1, usize::from);
            Self::compute(memory, cpus)
        })
    }
}
/// Version de la traduction spec -> requete Docker, comprise dans
/// l'empreinte : a incrementer quand `create_body` change, pour recreer les
/// conteneurs existants.
const SPEC_VERSION: &str = "ciphera-spec-v5";
/// Journaux de chaque conteneur cree par Ciphera (applications, services
/// Compose, PostgreSQL, buildkitd, bac a sable de prepare) : pilote
/// `json-file` borne a `LOG_MAX_FILE` fichiers de `LOG_MAX_SIZE` (30 Mio au
/// plus par conteneur). Sans rotation, une application bavarde remplit le
/// disque de Docker, qui porte aussi la base.
pub const LOG_DRIVER: &str = "json-file";
pub const LOG_MAX_SIZE: &str = "10m";
pub const LOG_MAX_FILE: &str = "3";

/// `log_config` Docker des conteneurs de Ciphera.
pub fn log_config() -> HostConfigLogConfig {
    HostConfigLogConfig {
        typ: Some(LOG_DRIVER.to_string()),
        config: Some(HashMap::from([
            ("max-size".to_string(), LOG_MAX_SIZE.to_string()),
            ("max-file".to_string(), LOG_MAX_FILE.to_string()),
        ])),
    }
}

/// Identifiant d'un conteneur Docker.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContainerId(pub String);

/// Conteneur gere par Ciphera, tel que liste par Docker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedContainer {
    pub id: ContainerId,
    /// Nom sans le `/` de tete.
    pub name: String,
    pub labels: HashMap<String, String>,
}

/// Etiquette d'image d'un deploiement : `ciphera/<projet>-<env>:<id>`,
/// `ciphera/<projet>-<env>/<service>:<id>` (images finales du build) ou
/// `ciphera-staging/<id>[-<service>]:build` (noms provisoires). Une image
/// d'un autre deploiement (rollback) n'en fait jamais partie.
pub fn is_deployment_image(tag: &str, deployment: DeploymentId) -> bool {
    let id = deployment.to_string();
    if let Some(rest) = tag.strip_prefix("ciphera-staging/") {
        return rest
            .strip_suffix(":build")
            .and_then(|name| name.strip_prefix(id.as_str()))
            .is_some_and(|tail| tail.is_empty() || tail.starts_with('-'));
    }
    let Some(rest) = tag.strip_prefix("ciphera/") else {
        return false;
    };
    match rest.rsplit_once(':') {
        Some((repo, t)) => t == id && !repo.is_empty() && repo.split('/').count() <= 2,
        None => false,
    }
}

/// Etat d'un conteneur vu par Docker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// En marche, healthcheck en cours de demarrage.
    Starting,
    /// En marche, healthcheck reussi.
    Healthy,
    /// En marche, healthcheck en echec.
    Unhealthy,
    /// En marche, sans healthcheck.
    Running,
    /// Cree mais arrete (ou en redemarrage).
    Stopped,
    /// Le conteneur n'existe plus.
    Missing,
}

impl Health {
    /// Sain : healthcheck reussi, ou en marche sans healthcheck.
    pub fn is_healthy(self) -> bool {
        matches!(self, Self::Healthy | Self::Running)
    }
}

/// Une ligne de journal d'un conteneur (la cible est fixee par l'appelant).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerLog {
    pub stream: LogStream,
    /// Millisecondes depuis l'epoque Unix (horodatage de Docker).
    pub timestamp_ms: i64,
    pub line: String,
}

impl ContainerLog {
    pub fn into_line(self, target: LogTarget) -> proto::LogLine {
        proto::LogLine {
            target,
            stream: self.stream,
            timestamp_ms: self.timestamp_ms,
            line: self.line,
        }
    }
}

#[derive(Debug)]
pub enum DockerError {
    /// Nom ou identifiant refuse (il finirait dans un chemin de l'API).
    InvalidName(String),
    /// Reseau existant avec un autre reglage `internal` (ou un autre nom de
    /// pont) que celui attendu, impossible a recreer.
    NetworkMismatch(String),
    /// Conteneur ou reseau du meme nom sans l'etiquette `dev.ciphera.managed`
    /// (cree par quelqu'un d'autre) : laisse tel quel.
    NotManaged(String),
    /// Image absente apres le telechargement.
    Pull(String),
    /// Erreur de l'API Docker (sans contenu sensible).
    Api(String),
    /// Metriques du systeme illisibles.
    Metrics(String),
}

impl fmt::Display for DockerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(n) => write!(f, "nom Docker refuse : {n:?}"),
            Self::NetworkMismatch(n) => write!(
                f,
                "le reseau {n} existe deja avec un autre reglage (`internal` ou nom du pont) : refuse"
            ),
            Self::NotManaged(n) => write!(
                f,
                "{n} existe deja sans l'etiquette {MANAGED_LABEL}=true : pas cree par Ciphera, \
                 laisse tel quel (le renommer ou le supprimer a la main)"
            ),
            Self::Pull(i) => write!(f, "telechargement de l'image {i} impossible"),
            Self::Api(m) => write!(f, "API Docker : {m}"),
            Self::Metrics(m) => write!(f, "metriques du noeud : {m}"),
        }
    }
}

impl std::error::Error for DockerError {}

fn api(e: BollardError) -> DockerError {
    DockerError::Api(e.to_string())
}

fn is_status(e: &BollardError, code: u16) -> bool {
    matches!(e, BollardError::DockerResponseServerError { status_code, .. } if *status_code == code)
}

/// Docker a envoye SIGKILL mais n'a pas encore vu le conteneur sortir :
/// PID 1 zombie (« is zombie and can not be killed ») ou sortie non
/// recue. Passager : le conteneur sort une fois son espace de noms vide.
fn kill_pending(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError { status_code: 500, message }
            if message.contains("is zombie") || message.contains("did not receive an exit event")
    )
}

/// Nom de conteneur ou de reseau acceptable : `[A-Za-z0-9][A-Za-z0-9_.-]*`,
/// 128 caracteres au plus. Rien qui puisse sortir du chemin de l'API
/// (`/`, `?`, `..`) ni passer pour une option.
pub fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    name.len() <= 128 && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

fn check_name(name: &str) -> Result<(), DockerError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(DockerError::InvalidName(name.to_string()))
    }
}

/// Chemin d'un montage de l'hote ou du conteneur : absolu, sans `:` ni `,`
/// (separateurs de la syntaxe `source:cible:options`) ni caractere de
/// controle.
pub fn valid_bind_path(path: &str) -> bool {
    path.starts_with('/')
        && !path
            .bytes()
            .any(|b| b == b':' || b == b',' || b.is_ascii_control())
}

/// `Ok` si `labels` porte `dev.ciphera.managed=true` ; sinon `NotManaged`.
pub fn check_managed(
    name: &str,
    labels: Option<&HashMap<String, String>>,
) -> Result<(), DockerError> {
    if labels
        .and_then(|l| l.get(MANAGED_LABEL))
        .is_some_and(|v| v == "true")
    {
        Ok(())
    } else {
        Err(DockerError::NotManaged(name.to_string()))
    }
}

/// Reference d'image acceptable (`registre/depot:tag[@sha256:…]`) :
/// `[A-Za-z0-9._/:@-]`, commence par un caractere alphanumerique, aucun
/// segment `.` ou `..`, 512 caracteres au plus. Elle finit dans le chemin de
/// `GET /images/{image}/json` : ni `?`, ni `#`, ni `%`, ni remontee.
pub fn valid_image(image: &str) -> bool {
    image.len() <= 512
        && image
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric())
        && image.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b':' | b'@' | b'-')
        })
        && !image
            .split('/')
            .any(|seg| seg == "." || seg == ".." || seg.is_empty())
}

/// Alias DNS d'un service sur le reseau de son projet : `<service>.internal`,
/// pour un nom en etiquette DNS (`[a-z0-9]`, `-` a l'interieur, 63
/// caracteres au plus) ; sinon aucun.
pub fn internal_alias(service: &str) -> Option<String> {
    let b = service.as_bytes();
    let ok = (1..=63).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && b[0] != b'-'
        && b[b.len() - 1] != b'-';
    ok.then(|| format!("{service}.internal"))
}

/// Reseau du projet : `ciphera-p-<project_id>`.
pub fn project_network(project: ProjectId) -> String {
    format!("{PROJECT_NETWORK_PREFIX}{project}")
}

/// Projet d'un reseau `ciphera-p-<project_id>` (nom exact, sinon aucun).
pub fn network_project(name: &str) -> Option<ProjectId> {
    let project: ProjectId = name.strip_prefix(PROJECT_NETWORK_PREFIX)?.parse().ok()?;
    (project_network(project) == name).then_some(project)
}

/// Etiquette posee par Compose sur ses conteneurs et ses volumes.
pub const COMPOSE_PROJECT_KEY: &str = "com.docker.compose.project";
/// Prefixe des projets Compose d'un environnement (`compose_project_name`).
pub const COMPOSE_PROJECT_PREFIX: &str = "cp-";

/// Projet Compose stable d'un environnement :
/// `cp-<UUID du projet sans tirets>-<8 premiers caracteres de l'UUID de
/// l'environnement>`. Tire des identifiants, jamais des noms : les volumes
/// d'un projet supprime ne reviennent pas a un nouveau projet du meme nom,
/// et ceux d'un projet se retrouvent par `compose_project_prefix`.
pub fn compose_project_name(project: ProjectId, environment: EnvironmentId) -> String {
    let env = environment.0.simple().to_string();
    format!("{}{}", compose_project_prefix(project), &env[..8])
}

/// Debut du nom de tous les projets Compose de `project`.
pub fn compose_project_prefix(project: ProjectId) -> String {
    format!("{COMPOSE_PROJECT_PREFIX}{}-", project.0.simple())
}

/// Le volume appartient a un projet Compose de `prefix`
/// (`compose_project_prefix`) : etiquette de Compose et nom
/// (`<projet Compose>_<cle>`) concordants.
pub fn is_project_volume(name: &str, labels: &HashMap<String, String>, prefix: &str) -> bool {
    labels.get(COMPOSE_PROJECT_KEY).is_some_and(|p| {
        p.starts_with(prefix)
            && name
                .strip_prefix(p.as_str())
                .is_some_and(|rest| rest.starts_with('_'))
    })
}

/// Montage d'un conteneur.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MountSpec {
    /// Volume nomme gere par Docker.
    Volume { name: String, target: String },
    /// Chemin de l'hote (conteneurs d'infrastructure seulement), etiquete
    /// pour SELinux (`z`, sans effet ailleurs).
    Bind {
        source: PathBuf,
        target: String,
        read_only: bool,
    },
}

/// Ce qu'il faut faire tourner. Champs prives : seules `ContainerSpec::app`
/// et le module `infra` en construisent.
#[derive(Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    pub(crate) name: String,
    pub(crate) image: String,
    pub(crate) network: String,
    pub(crate) cmd: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    /// Valeurs d'environnement publiques (infrastructure) : comprises dans
    /// l'empreinte. Pour une application, elles peuvent etre secretes : seuls
    /// les noms y entrent (une nouvelle valeur vient avec un nouveau
    /// deploiement, donc un nouveau conteneur).
    pub(crate) env_is_public: bool,
    /// `uid:gid` ; sans valeur, l'utilisateur de l'image.
    pub(crate) user: Option<String>,
    pub(crate) mounts: Vec<MountSpec>,
    /// Capacites retirees (`ALL` : toutes).
    pub(crate) cap_drop: Vec<String>,
    /// Capacites rendues apres `cap_drop` (buildkitd rootless seulement :
    /// `SETUID` et `SETGID` de newuidmap/newgidmap ; jamais une application).
    pub(crate) cap_add: Vec<String>,
    /// `no-new-privileges` : partout sauf buildkitd rootless (voir `infra`).
    pub(crate) no_new_privileges: bool,
    /// Options de securite supplementaires (buildkitd rootless seulement).
    pub(crate) security_opt: Vec<String>,
    /// `systempaths=unconfined` : ni chemins masques ni chemins en lecture
    /// seule dans /proc (buildkitd rootless seulement, voir `infra`).
    pub(crate) unmask_system_paths: bool,
    /// `--init` : init de Docker (tini) en PID 1, qui relaie les signaux et
    /// recolte les processus orphelins (buildkitd seulement : rootlesskit,
    /// programme Go multi-thread, n'est pas fait pour etre PID 1).
    pub(crate) init: bool,
    pub(crate) pids_limit: Option<i64>,
    /// Plafond de memoire en octets (buildkitd seulement).
    pub(crate) memory: Option<i64>,
    /// Plafond de CPU en milliardiemes de CPU (`--cpus` x 1e9 ; buildkitd
    /// seulement).
    pub(crate) nano_cpus: Option<i64>,
    pub(crate) labels: Vec<(String, String)>,
    /// Alias DNS sur `network` (`<service>.internal` d'une application).
    pub(crate) aliases: Vec<String>,
    /// Racine en lecture seule, avec un `tmpfs` sur `/tmp` (APP_TMP_TMPFS).
    pub(crate) read_only_rootfs: bool,
}

impl fmt::Debug for ContainerSpec {
    /// Sans les valeurs d'environnement (secrets des applications).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let env: Vec<&str> = self.env.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("ContainerSpec")
            .field("name", &self.name)
            .field("image", &self.image)
            .field("network", &self.network)
            .field("env_keys", &env)
            .finish_non_exhaustive()
    }
}

/// Serialisation canonique pour l'empreinte : chaque champ prefixe de sa
/// longueur, chaque liste de son nombre d'elements (aucune ambiguite).
#[derive(Default)]
struct Canonical(Vec<u8>);

impl Canonical {
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&(b.len() as u64).to_be_bytes());
        self.0.extend_from_slice(b);
    }

    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }

    fn count(&mut self, n: usize) {
        self.0.extend_from_slice(&(n as u64).to_be_bytes());
    }

    fn flag(&mut self, b: bool) {
        self.0.push(u8::from(b));
    }

    fn opt_str(&mut self, s: Option<&str>) {
        self.flag(s.is_some());
        if let Some(s) = s {
            self.str(s);
        }
    }

    fn list(&mut self, items: &[String]) {
        self.count(items.len());
        for i in items {
            self.str(i);
        }
    }
}

/// Etiquettes reservees de portee (`deployment`, `compose`, `project`, `role`) de
/// tout conteneur cree par l'agent. Toujours toutes presentes, vides si sans
/// objet : une etiquette absente se lirait dans l'image, et le cycle de vie
/// (redemarrer, arreter, supprimer) s'etendrait alors a un autre projet.
/// Point unique pour l'application simple et les services Compose.
pub fn scope_labels(
    deployment: Option<DeploymentId>,
    compose: &str,
    project: Option<ProjectId>,
) -> [(&'static str, String); 4] {
    [
        (
            DEPLOYMENT_LABEL,
            deployment.map(|d| d.to_string()).unwrap_or_default(),
        ),
        (COMPOSE_LABEL, compose.to_string()),
        (
            PROJECT_LABEL,
            project.map(|p| p.to_string()).unwrap_or_default(),
        ),
        (ROLE_LABEL, String::new()),
    ]
}

impl ContainerSpec {
    /// Conteneur d'application : reseau du projet, aucun port publie (le
    /// proxy du noeud le joint par le reseau), aucun montage, aucun
    /// privilege, capacites par defaut de Docker sans `NET_RAW` ni `MKNOD`
    /// et sans ajout, au plus `APP_PIDS_LIMIT` processus, redemarrage
    /// automatique.
    pub fn app(
        deployment: DeploymentId,
        project: ProjectId,
        image: &ImageRef,
        env: Vec<(String, String)>,
    ) -> Self {
        let limits = AppLimits::node();
        Self {
            name: format!("ciphera-d-{deployment}"),
            image: image.0.clone(),
            network: project_network(project),
            cmd: Vec::new(),
            env,
            env_is_public: false,
            user: None,
            mounts: Vec::new(),
            cap_drop: APP_CAP_DROP.iter().map(|c| (*c).to_string()).collect(),
            cap_add: Vec::new(),
            no_new_privileges: true,
            security_opt: Vec::new(),
            unmask_system_paths: false,
            init: false,
            pids_limit: Some(APP_PIDS_LIMIT),
            memory: i64::try_from(limits.memory).ok(),
            nano_cpus: i64::try_from(limits.nano_cpus).ok(),
            labels: scope_labels(Some(deployment), "", Some(project))
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            aliases: Vec::new(),
            read_only_rootfs: false,
        }
    }

    /// Utilisateur du conteneur (`uid[:gid]` numerique, deja valide par
    /// l'appelant) ; sans valeur, l'utilisateur de l'image.
    pub fn with_run_as(mut self, run_as: Option<&str>) -> Self {
        self.user = run_as.map(str::to_string);
        self
    }

    /// Racine en lecture seule et `/tmp` en `tmpfs` (APP_TMP_TMPFS).
    pub fn with_read_only_rootfs(mut self, on: bool) -> Self {
        self.read_only_rootfs = on;
        self
    }

    /// Rend l'application joignable sous `<service>.internal` sur le reseau
    /// du projet. Nom hors DNS (voir `internal_alias`) : aucun alias.
    pub fn with_service_alias(mut self, service: &str) -> Self {
        match internal_alias(service) {
            Some(alias) => self.aliases = vec![alias],
            None => tracing::warn!("service {service:?} : nom hors DNS, pas d'alias .internal"),
        }
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Utilisateur demande (`uid[:gid]`), s'il y en a un.
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub fn read_only_rootfs(&self) -> bool {
        self.read_only_rootfs
    }

    pub fn image(&self) -> &str {
        &self.image
    }

    pub fn network(&self) -> &str {
        &self.network
    }

    /// Environnement du conteneur (valeurs secretes d'une application : a ne
    /// jamais journaliser ni enregistrer).
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// Etiquettes propres a la spec (sans `MANAGED_LABEL` ni `SPEC_LABEL`,
    /// posees a la creation).
    pub fn labels(&self) -> &[(String, String)] {
        &self.labels
    }

    /// Empreinte de la spec (etiquette `SPEC_LABEL`) : SHA-256 en
    /// hexadecimal d'une serialisation canonique, stable d'un binaire a
    /// l'autre. Les valeurs d'environnement d'une application n'y entrent
    /// pas (l'etiquette est lisible par `docker inspect`).
    pub fn fingerprint(&self) -> String {
        let mut c = Canonical::default();
        c.str(SPEC_VERSION);
        c.str(&self.name);
        c.str(&self.image);
        c.str(&self.network);
        c.list(&self.cmd);
        c.flag(self.env_is_public);
        c.count(self.env.len());
        for (k, v) in &self.env {
            c.str(k);
            if self.env_is_public {
                c.str(v);
            }
        }
        c.opt_str(self.user.as_deref());
        c.count(self.mounts.len());
        for m in &self.mounts {
            match m {
                MountSpec::Volume { name, target } => {
                    c.str("volume");
                    c.str(name);
                    c.str(target);
                }
                MountSpec::Bind {
                    source,
                    target,
                    read_only,
                } => {
                    c.str("bind");
                    c.str(&source.to_string_lossy());
                    c.str(target);
                    c.flag(*read_only);
                }
            }
        }
        c.list(&self.cap_drop);
        c.list(&self.cap_add);
        c.flag(self.no_new_privileges);
        c.list(&self.security_opt);
        c.flag(self.unmask_system_paths);
        c.flag(self.pids_limit.is_some());
        c.0.extend_from_slice(&self.pids_limit.unwrap_or(0).to_be_bytes());
        // Absents tant qu'aucun plafond n'est pose : l'empreinte des
        // conteneurs sans plafond ne change pas.
        if self.memory.is_some() || self.nano_cpus.is_some() {
            c.str("resources");
            c.0.extend_from_slice(&self.memory.unwrap_or(0).to_be_bytes());
            c.0.extend_from_slice(&self.nano_cpus.unwrap_or(0).to_be_bytes());
        }
        // Absent sans init : l'empreinte des autres conteneurs ne change pas.
        if self.init {
            c.str("init");
        }
        // Absent tant que la racine est en ecriture : l'empreinte ne change pas.
        if self.read_only_rootfs {
            c.str("read-only-rootfs");
        }
        // Absent pour l'infrastructure : son empreinte ne change pas.
        if !self.aliases.is_empty() {
            c.list(&self.aliases);
        }
        let mut labels = self.labels.clone();
        labels.sort();
        c.count(labels.len());
        for (k, v) in &labels {
            c.str(k);
            c.str(v);
        }
        Sha256::digest(&c.0)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Noms, image et chemins de montage acceptables pour l'API Docker.
    fn validate(&self) -> Result<(), DockerError> {
        check_name(&self.name)?;
        check_name(&self.network)?;
        if !valid_image(&self.image) {
            return Err(DockerError::InvalidName(self.image.clone()));
        }
        for m in &self.mounts {
            if let MountSpec::Bind { source, target, .. } = m {
                let source = source.to_string_lossy();
                for path in [source.as_ref(), target.as_str()] {
                    if !valid_bind_path(path) {
                        return Err(DockerError::InvalidName(path.to_string()));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Vrai si l'utilisateur de la configuration d'un conteneur est root : sans
/// valeur (utilisateur de l'image non precise : root), `0`, `root`, ou un
/// uid 0 (`0:gid`, `root:groupe`). Un autre nom ne se resout pas ici : il
/// n'est pas compte comme root.
pub fn user_is_root(user: Option<&str>) -> bool {
    let name = user.unwrap_or("").split(':').next().unwrap_or("");
    name.is_empty() || name == "root" || name.bytes().all(|b| b == b'0')
}

/// Requete de creation Docker pour `spec`. Les chemins de l'hote passent par
/// `binds` (seule syntaxe qui accepte `z`, l'etiquetage SELinux partage) ;
/// les volumes nommes par `mounts`.
pub fn create_body(spec: &ContainerSpec) -> ContainerCreateBody {
    let mut labels: HashMap<String, String> = spec.labels.iter().cloned().collect();
    // Etiquettes reservees : posees par l'agent, jamais celles de l'image.
    for (key, value) in scope_labels(None, "", None) {
        labels.entry(key.to_string()).or_insert(value);
    }
    labels.insert(MANAGED_LABEL.to_string(), "true".to_string());
    labels.insert(SPEC_LABEL.to_string(), spec.fingerprint());
    let mut mounts: Vec<Mount> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    for m in &spec.mounts {
        match m {
            MountSpec::Volume { name, target } => mounts.push(Mount {
                typ: Some(MountTypeEnum::VOLUME),
                source: Some(name.clone()),
                target: Some(target.clone()),
                ..Default::default()
            }),
            MountSpec::Bind {
                source,
                target,
                read_only,
            } => binds.push(format!(
                "{}:{}:{},z",
                source.to_string_lossy(),
                target,
                if *read_only { "ro" } else { "rw" }
            )),
        }
    }
    let mut security_opt = Vec::new();
    if spec.no_new_privileges {
        security_opt.push("no-new-privileges:true".to_string());
    }
    security_opt.extend(spec.security_opt.iter().cloned());
    ContainerCreateBody {
        image: Some(spec.image.clone()),
        cmd: (!spec.cmd.is_empty()).then(|| spec.cmd.clone()),
        env: (!spec.env.is_empty())
            .then(|| spec.env.iter().map(|(k, v)| format!("{k}={v}")).collect()),
        user: spec.user.clone(),
        labels: Some(labels),
        host_config: Some(HostConfig {
            network_mode: Some(spec.network.clone()),
            mounts: (!mounts.is_empty()).then_some(mounts),
            binds: (!binds.is_empty()).then_some(binds),
            security_opt: (!security_opt.is_empty()).then_some(security_opt),
            cap_drop: (!spec.cap_drop.is_empty()).then(|| spec.cap_drop.clone()),
            cap_add: (!spec.cap_add.is_empty()).then(|| spec.cap_add.clone()),
            pids_limit: spec.pids_limit,
            memory: spec.memory,
            // Pas de swap en plus de la memoire.
            memory_swap: spec.memory,
            nano_cpus: spec.nano_cpus,
            // Equivalent API de `--security-opt systempaths=unconfined`
            // (option traduite par le client docker, pas par le demon).
            masked_paths: spec.unmask_system_paths.then(Vec::new),
            readonly_paths: spec.unmask_system_paths.then(Vec::new),
            init: spec.init.then_some(true),
            readonly_rootfs: spec.read_only_rootfs.then_some(true),
            tmpfs: spec
                .read_only_rootfs
                .then(|| HashMap::from([("/tmp".to_string(), APP_TMP_TMPFS.to_string())])),
            publish_all_ports: Some(false),
            log_config: Some(log_config()),
            restart_policy: Some(DockerRestart {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            ..Default::default()
        }),
        networking_config: (!spec.aliases.is_empty()).then(|| NetworkingConfig {
            endpoints_config: Some(HashMap::from([(
                spec.network.clone(),
                EndpointSettings {
                    aliases: Some(spec.aliases.clone()),
                    ..Default::default()
                },
            )])),
        }),
        ..Default::default()
    }
}

/// Requete de creation d'un reseau : `ciphera-internal` est interne (aucune
/// sortie, aucune entree depuis l'exterieur) ; `ciphera-build` et les reseaux
/// de projet ne le sont pas (buildkitd et les applications sortent sur
/// Internet).
pub fn network_create_request(name: &str) -> NetworkCreateRequest {
    NetworkCreateRequest {
        name: name.to_string(),
        driver: Some("bridge".to_string()),
        internal: Some(name == INFRA_NETWORK),
        attachable: Some(false),
        labels: Some(HashMap::from([(
            MANAGED_LABEL.to_string(),
            "true".to_string(),
        )])),
        options: bridge_name(name)
            .map(|bridge| HashMap::from([(BRIDGE_NAME_OPTION.to_string(), bridge.to_string())])),
        ..Default::default()
    }
}

/// Nom d'interface d'un reseau existant (`BRIDGE_NAME_OPTION`), s'il en a un.
pub fn bridge_name_of(network: &bollard::models::NetworkInspect) -> Option<&str> {
    network
        .options
        .as_ref()
        .and_then(|o| o.get(BRIDGE_NAME_OPTION))
        .map(String::as_str)
}

/// Nom impose a l'interface du pont de `network` (filtrage de sortie) :
/// seulement pour `ciphera-build`.
fn bridge_name(network: &str) -> Option<&'static str> {
    (network == BUILD_NETWORK).then_some(BUILD_NETWORK)
}

/// Operations Docker de l'agent. Implementees par `BollardDocker`, simulees
/// dans les tests.
pub trait DockerOps: Send + Sync + 'static {
    /// Fait tourner `spec` : rien a faire si un conteneur de ce nom tourne
    /// deja avec la meme spec (demarre s'il est arrete) ; sinon il est
    /// remplace, apres le telechargement de l'image (un echec laisse l'ancien
    /// en marche). Un conteneur du meme nom non gere par Ciphera n'est pas
    /// touche (`NotManaged`).
    fn run(
        &self,
        spec: &ContainerSpec,
    ) -> impl Future<Output = Result<ContainerId, DockerError>> + Send;
    /// Arrete un conteneur gere par Ciphera (`NotManaged` sinon).
    fn stop(&self, id: &ContainerId) -> impl Future<Output = Result<(), DockerError>> + Send;
    /// Redemarre un conteneur gere par Ciphera (`NotManaged` sinon) ; absent :
    /// rien a faire.
    fn restart(&self, id: &ContainerId) -> impl Future<Output = Result<(), DockerError>> + Send;
    /// Sante d'un conteneur gere par Ciphera (`NotManaged` sinon).
    fn health(&self, id: &ContainerId) -> impl Future<Output = Result<Health, DockerError>> + Send;
    /// Le conteneur tourne-t-il en root (utilisateur effectif de sa
    /// configuration, voir `user_is_root`) ? Absent : non.
    fn runs_as_root(
        &self,
        id: &ContainerId,
    ) -> impl Future<Output = Result<bool, DockerError>> + Send;
    /// Journaux du conteneur (les `tail` dernieres lignes, puis la suite si
    /// `follow`). Le flux s'arrete a la premiere erreur.
    fn logs(
        &self,
        id: &ContainerId,
        tail: u32,
        follow: bool,
    ) -> impl Future<Output = BoxStream<'static, ContainerLog>> + Send;
    /// Metriques du noeud (sans les conteneurs : voir `container_usage`).
    fn stats(&self) -> impl Future<Output = Result<NodeMetrics, DockerError>> + Send;
    /// CPU (pourcentage d'un coeur) et memoire (octets, hors cache) d'un
    /// conteneur gere par Ciphera (`NotManaged` sinon) ; `None` s'il est
    /// absent ou arrete.
    fn container_usage(
        &self,
        id: &ContainerId,
    ) -> impl Future<Output = Result<Option<(f32, u64)>, DockerError>> + Send;
    /// Cree le reseau s'il manque (interne pour `ciphera-internal`) ; refuse
    /// un reseau existant non gere par Ciphera (`NotManaged`, jamais adopte)
    /// ou dont le reglage `internal` differe.
    fn ensure_network(&self, name: &str) -> impl Future<Output = Result<(), DockerError>> + Send;
    /// Supprime (de force) un conteneur gere par Ciphera ; absent : rien a
    /// faire ; non gere : `NotManaged`, laisse tel quel.
    fn remove(&self, id: &ContainerId) -> impl Future<Output = Result<(), DockerError>> + Send;
    /// Conteneurs geres par Ciphera (arretes compris) portant toutes les
    /// etiquettes `labels`.
    fn list_managed(
        &self,
        labels: &[(&str, &str)],
    ) -> impl Future<Output = Result<Vec<ManagedContainer>, DockerError>> + Send;
    /// Adresse d'un conteneur gere sur son reseau de projet (`ciphera-p-*`) :
    /// `None` s'il n'est sur aucun (ou sur plusieurs), sans adresse ou absent.
    /// Seule source des upstreams du proxy.
    fn project_address(
        &self,
        id: &ContainerId,
    ) -> impl Future<Output = Result<Option<IpAddr>, DockerError>> + Send;
    /// Retire les etiquettes d'image d'un deploiement (`is_deployment_image`) ;
    /// une image encore utilisee par un conteneur reste.
    fn remove_deployment_images(
        &self,
        deployment: DeploymentId,
    ) -> impl Future<Output = Result<(), DockerError>> + Send;
    /// Supprime le reseau gere `name` ; absent : rien a faire ; non gere :
    /// `NotManaged`, laisse tel quel. Docker le refuse tant qu'un conteneur
    /// y est attache.
    fn remove_network(&self, name: &str) -> impl Future<Output = Result<(), DockerError>> + Send;
    /// Noms des reseaux de projet geres (`ciphera-p-<uuid>`).
    fn project_networks(&self) -> impl Future<Output = Result<Vec<String>, DockerError>> + Send;
    /// Supprime les volumes des projets Compose de `project`
    /// (`is_project_volume`) ; leur nombre. Un volume encore utilise fait
    /// echouer l'appel.
    fn remove_project_volumes(
        &self,
        project: ProjectId,
    ) -> impl Future<Output = Result<usize, DockerError>> + Send;
}

/// Adresse sur l'unique reseau de projet parmi `networks` (nom -> adresse).
pub fn project_ip<'a>(
    networks: impl Iterator<Item = (&'a str, Option<&'a str>)>,
) -> Option<IpAddr> {
    let mut found = None;
    for (name, ip) in networks {
        if !name.starts_with(PROJECT_NETWORK_PREFIX) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(ip.and_then(|ip| ip.parse::<IpAddr>().ok()));
    }
    found.flatten().filter(|ip| !ip.is_unspecified())
}

/// CPU (pourcentage d'un coeur, comme `docker stats`) et memoire hors
/// cache de fichiers (cgroup v2 `inactive_file`, v1 `total_inactive_file`)
/// d'une lecture de `/containers/{id}/stats`.
pub fn container_usage_of(stats: &bollard::models::ContainerStatsResponse) -> (f32, u64) {
    let total = |c: &Option<bollard::models::ContainerCpuStats>| {
        c.as_ref()
            .and_then(|c| c.cpu_usage.as_ref())
            .and_then(|u| u.total_usage)
            .unwrap_or(0)
    };
    let system = |c: &Option<bollard::models::ContainerCpuStats>| {
        c.as_ref().and_then(|c| c.system_cpu_usage).unwrap_or(0)
    };
    let cpus = stats
        .cpu_stats
        .as_ref()
        .and_then(|c| c.online_cpus)
        .unwrap_or(1)
        .max(1);
    let cpu_delta = total(&stats.cpu_stats).saturating_sub(total(&stats.precpu_stats));
    let system_delta = system(&stats.cpu_stats).saturating_sub(system(&stats.precpu_stats));
    let cpu = if system_delta == 0 {
        0.0
    } else {
        (cpu_delta as f64 / system_delta as f64 * f64::from(cpus) * 100.0) as f32
    };
    let memory = stats.memory_stats.as_ref().map_or(0, |m| {
        let cache = m
            .stats
            .as_ref()
            .and_then(|s| {
                s.get("inactive_file")
                    .or_else(|| s.get("total_inactive_file"))
            })
            .copied()
            .unwrap_or(0);
        m.usage.unwrap_or(0).saturating_sub(cache)
    });
    (cpu, memory)
}

/// Sondes du systeme pour les metriques (CPU mesure entre deux appels).
struct Probe {
    system: sysinfo::System,
    disks: sysinfo::Disks,
    networks: sysinfo::Networks,
}

impl Probe {
    /// Premiere mesure du CPU prise ici : la suivante (30 s plus tard) donne
    /// une vraie moyenne au lieu de 0.
    fn new() -> Result<Self, sysinfo::Error> {
        let mut system = sysinfo::System::new()?;
        system.refresh_cpu_usage();
        Ok(Self {
            system,
            disks: sysinfo::Disks::new_with_refreshed_list()?,
            networks: sysinfo::Networks::new_with_refreshed_list()?,
        })
    }

    fn sample(&mut self) -> NodeMetrics {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh(true);
        self.networks.refresh(true);
        let (disk_total, disk_free) = disk_totals(
            self.disks
                .iter()
                .map(|d| (d.name(), d.total_space(), d.available_space())),
        );
        let (rx, tx) = self.networks.iter().fold((0u64, 0u64), |(r, t), (_, n)| {
            (
                r.saturating_add(n.total_received()),
                t.saturating_add(n.total_transmitted()),
            )
        });
        NodeMetrics {
            cpu_percent: self.system.global_cpu_usage(),
            mem_used_bytes: self.system.used_memory(),
            mem_total_bytes: self.system.total_memory(),
            disk_used_bytes: disk_total.saturating_sub(disk_free),
            disk_total_bytes: disk_total,
            net_rx_bytes: rx,
            net_tx_bytes: tx,
            containers: Vec::new(),
        }
    }
}

/// `(total, libre)` des disques `(peripherique, total, libre)` : un meme
/// peripherique monte plusieurs fois (bind mounts, sous-volumes) n'est compte
/// qu'une fois.
fn disk_totals<'a>(disks: impl Iterator<Item = (&'a std::ffi::OsStr, u64, u64)>) -> (u64, u64) {
    let mut seen = HashSet::new();
    disks
        .filter(|(name, _, _)| seen.insert(*name))
        .fold((0u64, 0u64), |(t, f), (_, total, free)| {
            (t.saturating_add(total), f.saturating_add(free))
        })
}

/// `DockerOps` par l'API Docker locale.
#[derive(Clone)]
pub struct BollardDocker {
    docker: Docker,
    /// Creees au demarrage (ou au premier appel de `stats` si elles ont
    /// echoue).
    probe: Arc<Mutex<Option<Probe>>>,
}

impl fmt::Debug for BollardDocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BollardDocker").finish_non_exhaustive()
    }
}

impl BollardDocker {
    /// Socket Docker par defaut (`/var/run/docker.sock`) ; `DOCKER_HOST`
    /// n'est pas lu. Aucune requete n'est envoyee ici. Les sondes des
    /// metriques sont creees tout de suite (premiere mesure du CPU).
    pub fn connect() -> Result<Self, DockerError> {
        let docker = Docker::connect_with_socket_defaults().map_err(api)?;
        let probe = Probe::new()
            .inspect_err(|e| tracing::warn!("metriques du noeud : {e} (nouvel essai plus tard)"))
            .ok();
        Ok(Self {
            docker,
            probe: Arc::new(Mutex::new(probe)),
        })
    }

    /// Conteneur `name_or_id` s'il existe et est gere par Ciphera ; `None`
    /// s'il n'existe pas ; `NotManaged` s'il n'est pas a nous.
    async fn inspect_managed(
        &self,
        name_or_id: &str,
    ) -> Result<Option<ContainerInspectResponse>, DockerError> {
        match self.docker.inspect_container(name_or_id, None).await {
            Ok(info) => {
                check_managed(
                    name_or_id,
                    info.config.as_ref().and_then(|c| c.labels.as_ref()),
                )?;
                Ok(Some(info))
            }
            Err(e) if is_status(&e, 404) => Ok(None),
            Err(e) => Err(api(e)),
        }
    }

    async fn pull_if_missing(&self, image: &str) -> Result<(), DockerError> {
        match self.docker.inspect_image(image).await {
            Ok(_) => return Ok(()),
            Err(e) if is_status(&e, 404) => {}
            Err(e) => return Err(api(e)),
        }
        tracing::info!("telechargement de l'image {image}");
        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };
        let mut pull = self.docker.create_image(Some(options), None, None);
        while let Some(step) = pull.next().await {
            step.map_err(|e| {
                tracing::warn!("image {image} : {e}");
                DockerError::Pull(image.to_string())
            })?;
        }
        Ok(())
    }

    /// Suppression forcee ; si Docker n'a pas vu le conteneur sortir apres
    /// son SIGKILL (PID 1 zombie le temps que l'espace de noms des processus
    /// se vide), nouvel essai, `REMOVE_ATTEMPTS` au plus.
    async fn force_remove(&self, id: &str) -> Result<(), DockerError> {
        let mut attempt = 1;
        loop {
            let options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            match self.docker.remove_container(id, Some(options)).await {
                Ok(()) => return Ok(()),
                Err(e) if is_status(&e, 404) => return Ok(()),
                Err(e) if attempt < REMOVE_ATTEMPTS && kill_pending(&e) => {
                    tracing::warn!(
                        "conteneur {id} pas encore sorti apres SIGKILL (essai {attempt}/{REMOVE_ATTEMPTS}) : {e}"
                    );
                    attempt += 1;
                    tokio::time::sleep(REMOVE_RETRY_DELAY).await;
                }
                Err(e) => return Err(api(e)),
            }
        }
    }
}

impl DockerOps for BollardDocker {
    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerId, DockerError> {
        spec.validate()?;
        let fingerprint = spec.fingerprint();
        let existing = self.inspect_managed(&spec.name).await?;
        if let Some(existing) = &existing {
            let same = existing
                .config
                .as_ref()
                .and_then(|c| c.labels.as_ref())
                .and_then(|l| l.get(SPEC_LABEL))
                .is_some_and(|f| *f == fingerprint);
            let id = existing.id.clone().unwrap_or_else(|| spec.name.clone());
            if same {
                let running = existing
                    .state
                    .as_ref()
                    .and_then(|s| s.running)
                    .unwrap_or(false);
                if !running {
                    self.docker.start_container(&id, None).await.map_err(api)?;
                }
                return Ok(ContainerId(id));
            }
        }
        // Image d'abord : si le telechargement echoue, l'ancien conteneur
        // continue de tourner.
        self.pull_if_missing(&spec.image).await?;
        if let Some(existing) = existing {
            tracing::info!("conteneur {} : spec modifiee, recreation", spec.name);
            // Par identifiant : jamais un conteneur recree entre-temps sous
            // le meme nom par quelqu'un d'autre.
            self.force_remove(existing.id.as_deref().unwrap_or(&spec.name))
                .await?;
        }
        let options = CreateContainerOptions {
            name: Some(spec.name.clone()),
            ..Default::default()
        };
        let created = self
            .docker
            .create_container(Some(options), create_body(spec))
            .await
            .map_err(api)?;
        self.docker
            .start_container(&created.id, None)
            .await
            .map_err(api)?;
        Ok(ContainerId(created.id))
    }

    async fn stop(&self, id: &ContainerId) -> Result<(), DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(()); // n'existe plus : rien a arreter
        };
        let target = info.id.unwrap_or_else(|| id.0.clone());
        let options = StopContainerOptions {
            t: Some(STOP_TIMEOUT_SECS),
            ..Default::default()
        };
        match self.docker.stop_container(&target, Some(options)).await {
            Ok(()) => Ok(()),
            // 304 : deja arrete ; 404 : supprime entre-temps.
            Err(e) if is_status(&e, 304) || is_status(&e, 404) => Ok(()),
            Err(e) => Err(api(e)),
        }
    }

    async fn restart(&self, id: &ContainerId) -> Result<(), DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(()); // n'existe plus : rien a redemarrer
        };
        let target = info.id.unwrap_or_else(|| id.0.clone());
        let options = RestartContainerOptions {
            t: Some(STOP_TIMEOUT_SECS),
            ..Default::default()
        };
        match self.docker.restart_container(&target, Some(options)).await {
            Ok(()) => Ok(()),
            // 404 : supprime entre-temps.
            Err(e) if is_status(&e, 404) => Ok(()),
            Err(e) => Err(api(e)),
        }
    }

    async fn health(&self, id: &ContainerId) -> Result<Health, DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(Health::Missing);
        };
        let Some(state) = info.state else {
            return Ok(Health::Stopped);
        };
        if state.status != Some(ContainerStateStatusEnum::RUNNING) {
            return Ok(Health::Stopped);
        }
        Ok(match state.health.and_then(|h| h.status) {
            Some(HealthStatusEnum::HEALTHY) => Health::Healthy,
            Some(HealthStatusEnum::UNHEALTHY) => Health::Unhealthy,
            Some(HealthStatusEnum::STARTING) => Health::Starting,
            _ => Health::Running,
        })
    }

    async fn runs_as_root(&self, id: &ContainerId) -> Result<bool, DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(false);
        };
        Ok(user_is_root(info.config.and_then(|c| c.user).as_deref()))
    }

    async fn logs(
        &self,
        id: &ContainerId,
        tail: u32,
        follow: bool,
    ) -> BoxStream<'static, ContainerLog> {
        if check_name(&id.0).is_err() {
            return stream::empty().boxed();
        }
        let options = LogsOptions {
            follow,
            stdout: true,
            stderr: true,
            timestamps: true,
            tail: tail.to_string(),
            ..Default::default()
        };
        let assembler = Arc::new(Mutex::new(LogAssembler::default()));
        let tail = assembler.clone();
        self.docker
            .logs(&id.0, Some(options))
            .take_while(|r| std::future::ready(r.is_ok()))
            .filter_map(|r| std::future::ready(r.ok()))
            .flat_map(move |out| {
                use bollard::container::LogOutput;
                let (stream, bytes) = match out {
                    LogOutput::StdErr { message } => (LogStream::Stderr, message),
                    LogOutput::StdOut { message } | LogOutput::Console { message } => {
                        (LogStream::Stdout, message)
                    }
                    LogOutput::StdIn { .. } => return stream::iter(Vec::new()),
                };
                let mut guard = assembler
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                stream::iter(guard.push(stream, &bytes))
            })
            .chain(
                stream::once(async move {
                    let mut guard = tail.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    stream::iter(guard.finish())
                })
                .flatten(),
            )
            .boxed()
    }

    async fn stats(&self) -> Result<NodeMetrics, DockerError> {
        let probe = self.probe.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = probe
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let probe = match guard.as_mut() {
                Some(p) => p,
                None => {
                    guard.insert(Probe::new().map_err(|e| DockerError::Metrics(e.to_string()))?)
                }
            };
            Ok(probe.sample())
        })
        .await
        .map_err(|e| DockerError::Metrics(e.to_string()))?
    }

    async fn container_usage(&self, id: &ContainerId) -> Result<Option<(f32, u64)>, DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(None);
        };
        let running = info
            .state
            .as_ref()
            .is_some_and(|s| s.status == Some(ContainerStateStatusEnum::RUNNING));
        if !running {
            return Ok(None);
        }
        // Une seule lecture, CPU mesure par Docker entre deux echantillons.
        let options = StatsOptions {
            stream: false,
            one_shot: false,
        };
        let target = info.id.as_deref().unwrap_or(&id.0);
        let Some(stats) = self.docker.stats(target, Some(options)).next().await else {
            return Ok(None);
        };
        let stats = stats.map_err(api)?;
        Ok(Some(container_usage_of(&stats)))
    }

    async fn ensure_network(&self, name: &str) -> Result<(), DockerError> {
        check_name(name)?;
        let internal = name == INFRA_NETWORK;
        let bridge = bridge_name(name);
        let check = |existing: bollard::models::NetworkInspect| {
            check_managed(name, existing.labels.as_ref())?;
            if existing.internal.unwrap_or(false) == internal && bridge_name_of(&existing) == bridge
            {
                Ok(())
            } else {
                Err(DockerError::NetworkMismatch(name.to_string()))
            }
        };
        match self.docker.inspect_network(name, None).await {
            Ok(existing) => {
                check_managed(name, existing.labels.as_ref())?;
                if bridge.is_some()
                    && existing.internal.unwrap_or(false) == internal
                    && bridge_name_of(&existing) != bridge
                {
                    // Reseau d'une version precedente : recree avec le nom
                    // de pont attendu par le filtrage de sortie.
                    self.recreate_network(name, &existing).await?;
                } else {
                    return check(existing);
                }
            }
            Err(e) if is_status(&e, 404) => {}
            Err(e) => return Err(api(e)),
        }
        match self
            .docker
            .create_network(network_create_request(name))
            .await
        {
            Ok(_) => Ok(()),
            // Cree entre-temps : on le reverifie.
            Err(e) if is_status(&e, 409) => {
                check(self.docker.inspect_network(name, None).await.map_err(api)?)
            }
            Err(e) => Err(api(e)),
        }
    }

    async fn remove(&self, id: &ContainerId) -> Result<(), DockerError> {
        self.remove_impl(id).await
    }

    async fn list_managed(
        &self,
        labels: &[(&str, &str)],
    ) -> Result<Vec<ManagedContainer>, DockerError> {
        self.list_managed_impl(labels).await
    }

    async fn project_address(&self, id: &ContainerId) -> Result<Option<IpAddr>, DockerError> {
        self.project_address_impl(id).await
    }

    async fn remove_deployment_images(&self, deployment: DeploymentId) -> Result<(), DockerError> {
        self.remove_deployment_images_impl(deployment).await
    }

    async fn remove_network(&self, name: &str) -> Result<(), DockerError> {
        check_name(name)?;
        match self.docker.inspect_network(name, None).await {
            Ok(existing) => check_managed(name, existing.labels.as_ref())?,
            Err(e) if is_status(&e, 404) => return Ok(()),
            Err(e) => return Err(api(e)),
        }
        match self.docker.remove_network(name).await {
            Ok(()) => Ok(()),
            Err(e) if is_status(&e, 404) => Ok(()),
            Err(e) => Err(api(e)),
        }
    }

    async fn project_networks(&self) -> Result<Vec<String>, DockerError> {
        let options = ListNetworksOptions {
            filters: Some(HashMap::from([
                ("label".to_string(), vec![format!("{MANAGED_LABEL}=true")]),
                ("name".to_string(), vec![PROJECT_NETWORK_PREFIX.to_string()]),
            ])),
        };
        let list = self
            .docker
            .list_networks(Some(options))
            .await
            .map_err(api)?;
        // Filtres de Docker reverifies (le filtre `name` est une sous-chaine).
        Ok(list
            .into_iter()
            .filter(|n| check_managed("", n.labels.as_ref()).is_ok())
            .filter_map(|n| n.name)
            .filter(|n| network_project(n).is_some())
            .collect())
    }

    async fn remove_project_volumes(&self, project: ProjectId) -> Result<usize, DockerError> {
        let prefix = compose_project_prefix(project);
        let options = ListVolumesOptions {
            filters: Some(HashMap::from([(
                "label".to_string(),
                vec![COMPOSE_PROJECT_KEY.to_string()],
            )])),
        };
        let volumes = self
            .docker
            .list_volumes(Some(options))
            .await
            .map_err(api)?
            .volumes
            .unwrap_or_default();
        let mut removed = 0;
        for v in volumes
            .iter()
            .filter(|v| is_project_volume(&v.name, &v.labels, &prefix))
        {
            check_name(&v.name)?;
            match self
                .docker
                .remove_volume(&v.name, Some(RemoveVolumeOptions { force: false }))
                .await
            {
                Ok(()) => removed += 1,
                Err(e) if is_status(&e, 404) => {}
                Err(e) => return Err(api(e)),
            }
        }
        Ok(removed)
    }
}

impl BollardDocker {
    /// Supprime le reseau gere `name` pour le recreer : ses conteneurs (tous
    /// geres par Ciphera, sinon `NetworkMismatch` et rien n'est touche) sont
    /// supprimes d'abord ; l'agent les recree ensuite (buildkitd).
    async fn recreate_network(
        &self,
        name: &str,
        existing: &bollard::models::NetworkInspect,
    ) -> Result<(), DockerError> {
        let attached: Vec<String> = existing
            .containers
            .as_ref()
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default();
        let mut managed = Vec::new();
        for id in &attached {
            match self.inspect_managed(id).await {
                Ok(Some(info)) => managed.push(info.id.unwrap_or_else(|| id.clone())),
                Ok(None) => {}
                Err(DockerError::NotManaged(_)) => {
                    return Err(DockerError::NetworkMismatch(name.to_string()));
                }
                Err(e) => return Err(e),
            }
        }
        for id in &managed {
            self.force_remove(id).await?;
        }
        match self.docker.remove_network(name).await {
            Ok(()) => {}
            Err(e) if is_status(&e, 404) => {}
            Err(e) => return Err(api(e)),
        }
        tracing::info!("reseau {name} recree (nom du pont pour le filtrage de sortie)");
        Ok(())
    }

    async fn remove_image_tag(&self, tag: &str) -> Result<(), DockerError> {
        let options = RemoveImageOptions {
            force: false,
            noprune: false,
            ..Default::default()
        };
        match self.docker.remove_image(tag, Some(options), None).await {
            Ok(_) => Ok(()),
            Err(e) if is_status(&e, 404) => Ok(()),
            // 409 : encore utilisee par un conteneur (arrete compris).
            Err(e) if is_status(&e, 409) => {
                tracing::warn!("image {tag} encore utilisee : conservee");
                Ok(())
            }
            Err(e) => Err(api(e)),
        }
    }
}

impl BollardDocker {
    pub(crate) async fn remove_impl(&self, id: &ContainerId) -> Result<(), DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(());
        };
        self.force_remove(info.id.as_deref().unwrap_or(&id.0)).await
    }

    pub(crate) async fn list_managed_impl(
        &self,
        labels: &[(&str, &str)],
    ) -> Result<Vec<ManagedContainer>, DockerError> {
        let mut filters = vec![format!("{MANAGED_LABEL}=true")];
        filters.extend(labels.iter().map(|(k, v)| format!("{k}={v}")));
        let options = ListContainersOptions {
            all: true,
            filters: Some(HashMap::from([("label".to_string(), filters)])),
            ..Default::default()
        };
        let list = self
            .docker
            .list_containers(Some(options))
            .await
            .map_err(api)?;
        Ok(list
            .into_iter()
            .filter_map(|c| {
                let labels = c.labels.unwrap_or_default();
                // Filtre de Docker reverifie : jamais un conteneur d'autrui.
                check_managed("", Some(&labels)).ok()?;
                Some(ManagedContainer {
                    id: ContainerId(c.id?),
                    name: c
                        .names
                        .unwrap_or_default()
                        .first()
                        .map(|n| n.trim_start_matches('/').to_string())
                        .unwrap_or_default(),
                    labels,
                })
            })
            .filter(|c| labels_match(&c.labels, labels))
            .collect())
    }

    pub(crate) async fn project_address_impl(
        &self,
        id: &ContainerId,
    ) -> Result<Option<IpAddr>, DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(None);
        };
        let networks = info
            .network_settings
            .and_then(|n| n.networks)
            .unwrap_or_default();
        Ok(project_ip(networks.iter().map(|(name, ep)| {
            (name.as_str(), ep.ip_address.as_deref())
        })))
    }

    pub(crate) async fn remove_deployment_images_impl(
        &self,
        deployment: DeploymentId,
    ) -> Result<(), DockerError> {
        let images = self
            .docker
            .list_images(Some(ListImagesOptions::default()))
            .await
            .map_err(api)?;
        for tag in images
            .iter()
            .flat_map(|i| i.repo_tags.iter())
            .filter(|t| is_deployment_image(t, deployment))
        {
            self.remove_image_tag(tag).await?;
        }
        Ok(())
    }
}

/// `labels` porte chaque paire de `wanted`.
fn labels_match(labels: &HashMap<String, String>, wanted: &[(&str, &str)]) -> bool {
    wanted
        .iter()
        .all(|(k, v)| labels.get(*k).is_some_and(|x| x == v))
}

/// Une ligne de journal rassemblee depasse rarement 16 Kio ; au-dela
/// de cette taille (horodatage non compte), la suite est ecartee. Le plan de
/// controle masque la ligne entiere puis la coupe a une taille bien plus
/// courte : aucun secret ne peut etre coupe en deux dans la partie visible.
pub const MAX_ASSEMBLED_LINE: usize = 64 * 1024;

/// Rassemble les morceaux d'une meme ligne : Docker coupe les longues lignes
/// en plusieurs messages partiels de 16 Kio (aucun sauf le dernier ne finit
/// par `\n`) et, avec `timestamps`, prefixe CHAQUE message de son propre
/// horodatage : celui des morceaux suivants est retire avant la
/// concatenation, sinon il couperait un secret en deux. Un etat par flux
/// (sortie standard et erreur).
///
/// Une ligne sans `\n` est retenue jusqu'au `\n` ou a la fin du flux, sans
/// vidage sur delai : echec ferme, car un vidage coupe pourrait separer un
/// secret en deux morceaux jamais masques (une barre de progression sans
/// retour a la ligne n'apparait donc qu'a la fin).
#[derive(Default)]
pub struct LogAssembler {
    pending: [Vec<u8>; 2],
    /// Plafond atteint : la fin de la ligne en cours est ecartee.
    over: [bool; 2],
}

impl LogAssembler {
    fn slot(stream: LogStream) -> usize {
        match stream {
            LogStream::Stdout => 0,
            LogStream::Stderr => 1,
        }
    }

    /// Ajoute une trame ; renvoie les lignes terminees.
    pub fn push(&mut self, stream: LogStream, bytes: &[u8]) -> Vec<ContainerLog> {
        let i = Self::slot(stream);
        let mut out = Vec::new();
        for piece in bytes.split_inclusive(|b| *b == b'\n') {
            if !self.over[i] {
                // Chaque message porte son horodatage : on ne garde que
                // celui du debut de ligne.
                let piece = if self.pending[i].is_empty() {
                    piece
                } else {
                    strip_timestamp(piece)
                };
                // Marge pour l'horodatage place devant la ligne.
                let room = (MAX_ASSEMBLED_LINE + 64).saturating_sub(self.pending[i].len());
                let take = piece.len().min(room);
                self.pending[i].extend_from_slice(&piece[..take]);
                self.over[i] = take < piece.len();
            }
            if piece.ends_with(b"\n") {
                let line = std::mem::take(&mut self.pending[i]);
                self.over[i] = false;
                out.extend(split_log_chunk(stream, &line));
            }
        }
        out
    }

    /// Fin du flux : lignes sans retour a la ligne final.
    pub fn finish(&mut self) -> Vec<ContainerLog> {
        let mut out = Vec::new();
        for (i, stream) in [LogStream::Stdout, LogStream::Stderr]
            .into_iter()
            .enumerate()
        {
            let line = std::mem::take(&mut self.pending[i]);
            self.over[i] = false;
            out.extend(split_log_chunk(stream, &line));
        }
        out
    }
}

/// `piece` sans son horodatage Docker de tete (`<RFC 3339> `), s'il en a un.
fn strip_timestamp(piece: &[u8]) -> &[u8] {
    let Some(space) = piece.iter().take(64).position(|b| *b == b' ') else {
        return piece;
    };
    match std::str::from_utf8(&piece[..space]) {
        Ok(ts) if parse_rfc3339_ms(ts).is_some() => &piece[space + 1..],
        _ => piece,
    }
}

/// Decoupe un bloc de journal Docker (`<horodatage RFC 3339> <texte>` par
/// ligne) ; les lignes trop longues sont coupees a `MAX_ASSEMBLED_LINE` octets.
pub fn split_log_chunk(stream: LogStream, bytes: &[u8]) -> Vec<ContainerLog> {
    String::from_utf8_lossy(bytes)
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|raw| {
            let raw = raw.strip_suffix('\r').unwrap_or(raw);
            let (timestamp_ms, text) = match raw.split_once(' ') {
                Some((ts, rest)) => match parse_rfc3339_ms(ts) {
                    Some(ms) => (ms, rest),
                    None => (now_ms(), raw),
                },
                None => match parse_rfc3339_ms(raw) {
                    Some(ms) => (ms, ""),
                    None => (now_ms(), raw),
                },
            };
            ContainerLog {
                stream,
                timestamp_ms,
                line: truncate(text, MAX_ASSEMBLED_LINE).to_string(),
            }
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `AAAA-MM-JJTHH:MM:SS[.fraction]Z` (format des horodatages de Docker) en
/// millisecondes Unix ; `None` pour toute autre forme.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (year, month, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.split(':');
    let (h, m, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    if t.next().is_some() || h > 23 || m > 59 || sec > 60 {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis: i64 = format!("{frac:0<3}")[..3].parse().ok()?;
    // Jours depuis l'epoque (algorithme de H. Hinnant, calendrier gregorien).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + m) * 60_000 + sec * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_usage_follows_docker_stats() {
        use bollard::models::{
            ContainerCpuStats, ContainerCpuUsage, ContainerMemoryStats, ContainerStatsResponse,
        };
        let cpu = |total, system| ContainerCpuStats {
            cpu_usage: Some(ContainerCpuUsage {
                total_usage: Some(total),
                ..Default::default()
            }),
            system_cpu_usage: Some(system),
            online_cpus: Some(4),
            ..Default::default()
        };
        let stats = ContainerStatsResponse {
            cpu_stats: Some(cpu(1_500, 20_000)),
            precpu_stats: Some(cpu(1_000, 10_000)),
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(300 << 20),
                stats: Some(HashMap::from([("inactive_file".to_string(), 100 << 20)])),
                ..Default::default()
            }),
            ..Default::default()
        };
        // 500 / 10 000 du temps systeme sur 4 coeurs : 20 % d'un coeur.
        let (cpu_percent, memory) = container_usage_of(&stats);
        assert!((cpu_percent - 20.0).abs() < 1e-3, "{cpu_percent}");
        assert_eq!(memory, 200 << 20);
        // Premiere lecture (pas d'echantillon precedent) : pas de division par zero.
        assert_eq!(
            container_usage_of(&ContainerStatsResponse::default()),
            (0.0, 0)
        );
    }

    #[test]
    fn docker_timestamps_are_parsed() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_ms("2026-11-21T10:20:30.123456789Z"),
            Some(1_795_256_430_123)
        );
        assert_eq!(
            parse_rfc3339_ms("2000-02-29T00:00:01.5Z"),
            Some(951_782_401_500)
        );
        for bad in [
            "",
            "hello",
            "2026-13-01T00:00:00Z",
            "2026-11-21T10:20:30",
            "2026-11-21T10:20:30.x1Z",
        ] {
            assert_eq!(parse_rfc3339_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn log_chunks_are_split_timestamped_and_bounded() {
        let long = "x".repeat(MAX_ASSEMBLED_LINE + 10);
        let chunk =
            format!("2026-11-21T10:20:30.000000001Z bonjour\r\n2026-11-21T10:20:31Z {long}\n");
        let lines = split_log_chunk(LogStream::Stderr, chunk.as_bytes());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line, "bonjour");
        assert_eq!(lines[0].timestamp_ms, 1_795_256_430_000);
        assert_eq!(lines[0].stream, LogStream::Stderr);
        assert_eq!(lines[1].line.len(), MAX_ASSEMBLED_LINE);
    }

    #[test]
    fn partial_docker_frames_are_assembled_before_sending() {
        let mut a = LogAssembler::default();
        // Premiere trame : horodatage et debut, sans `\n` ; une lettre
        // multi-octets est coupee entre deux trames.
        let e = "é".as_bytes();
        let mut first = b"2026-11-21T10:20:30Z debut-".to_vec();
        first.push(e[0]);
        assert!(a.push(LogStream::Stdout, &first).is_empty());
        // Une ligne de l'autre flux ne se melange pas.
        let other = a.push(LogStream::Stderr, b"2026-11-21T10:20:31Z err\n");
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].line, "err");
        assert!(a.push(LogStream::Stdout, &[e[1], b'-']).is_empty());
        let done = a.push(LogStream::Stdout, b"fin\nsuite\n");
        assert_eq!(done.len(), 2);
        assert_eq!(done[0].line, "debut-é-fin");
        assert_eq!(done[0].timestamp_ms, 1_795_256_430_000);
        assert_eq!(done[1].line, "suite");
        // Fin du flux : la ligne sans `\n` final part quand meme.
        assert!(a.push(LogStream::Stdout, b"reste").is_empty());
        let rest = a.finish();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].line, "reste");
    }

    #[test]
    fn every_partial_frame_carries_its_own_timestamp_and_the_secret_stays_whole() {
        // Docker (timestamps=true) horodate chaque message partiel de 16 Kio.
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let line = format!("{}{secret}{}", "a".repeat(16_384 - 20), "b".repeat(20_000));
        let mut a = LogAssembler::default();
        let mut out = Vec::new();
        let chunks: Vec<&[u8]> = line.as_bytes().chunks(16_384).collect();
        for (n, chunk) in chunks.iter().enumerate() {
            let mut frame = format!("2026-11-21T10:20:3{n}.123456789Z ").into_bytes();
            frame.extend_from_slice(chunk);
            if n + 1 == chunks.len() {
                frame.push(b'\n');
            }
            out.extend(a.push(LogStream::Stdout, &frame));
        }
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].line, line);
        assert_eq!(out[0].timestamp_ms, 1_795_256_430_123);
    }

    /// Une ligne rassemblee, masquee en entier puis coupee a 16 Kio comme le
    /// fait le plan de controle, ne laisse aucun fragment du secret ; l'ancien
    /// comportement (coupe de chaque trame avant l'envoi) en laissait un.
    #[test]
    fn secret_across_16k_boundary_not_leaked() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let mask_then_cut = |line: &str| {
            let mut s = line.replace(secret, "••••");
            if s.len() > 16 * 1024 {
                let mut end = 16 * 1024;
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
            }
            s
        };
        let fragments = |s: &str| (0..=secret.len() - 4).any(|i| s.contains(&secret[i..i + 4]));
        for shift in 0..40 {
            let line = format!(
                "{}{secret}{}",
                "a".repeat(16_384 - shift),
                "b".repeat(20_000)
            );
            let mut a = LogAssembler::default();
            let mut out = Vec::new();
            let chunks: Vec<&[u8]> = line.as_bytes().chunks(16_384).collect();
            let mut old = Vec::new();
            for (n, chunk) in chunks.iter().enumerate() {
                let mut frame = format!("2026-11-21T10:20:30.00000000{n}Z ").into_bytes();
                frame.extend_from_slice(chunk);
                if n + 1 == chunks.len() {
                    frame.push(b'\n');
                }
                // Ancien comportement : chaque trame coupee a 16 Kio, seule.
                for l in split_log_chunk(LogStream::Stdout, &frame) {
                    old.push(l.line.chars().take(16 * 1024).collect::<String>());
                }
                out.extend(a.push(LogStream::Stdout, &frame));
            }
            assert_eq!(out.len(), 1);
            let shown = mask_then_cut(&out[0].line);
            assert!(!fragments(&shown), "fragment visible (decalage {shift})");
            // Sans rassemblement, un morceau seul ne peut pas etre masque.
            if shift > 4 && shift < 36 {
                assert!(
                    old.iter().any(|l| fragments(&mask_then_cut(l))),
                    "l'ancien comportement doit fuir (decalage {shift})"
                );
            }
        }
    }

    #[test]
    fn assembled_line_is_bounded_and_the_tail_dropped() {
        let mut a = LogAssembler::default();
        let chunk = "y".repeat(40_000);
        assert!(a.push(LogStream::Stdout, chunk.as_bytes()).is_empty());
        assert!(a.push(LogStream::Stdout, chunk.as_bytes()).is_empty());
        let lines = a.push(LogStream::Stdout, b"fin\nsuivante\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line.len(), MAX_ASSEMBLED_LINE);
        assert_eq!(lines[1].line, "suivante");
    }

    #[test]
    fn disks_are_counted_once_per_device() {
        use std::ffi::OsStr;
        let disks = [
            (OsStr::new("/dev/sda1"), 100, 40),
            (OsStr::new("/dev/sda1"), 100, 40), // meme disque, autre point de montage
            (OsStr::new("/dev/sdb1"), 50, 10),
        ];
        assert_eq!(disk_totals(disks.into_iter()), (150, 50));
    }

    #[test]
    fn truncation_respects_utf8() {
        assert_eq!(truncate("ééé", 3), "é");
    }

    #[test]
    fn deployment_images_are_recognised_by_their_tag() {
        let id = DeploymentId::new();
        let other = DeploymentId::new();
        for tag in [
            format!("ciphera/web-production:{id}"),
            format!("ciphera/web-production/web:{id}"),
            format!("ciphera-staging/{id}:build"),
            format!("ciphera-staging/{id}-web:build"),
        ] {
            assert!(is_deployment_image(&tag, id), "{tag}");
            assert!(!is_deployment_image(&tag, other), "{tag}");
        }
        for tag in [
            format!("ciphera/web-production:{id}x"),
            format!("docker.io/ciphera/web:{id}"),
            format!("ciphera/a/b/c:{id}"),
            format!("ciphera-staging/{id}x:build"),
            format!("ciphera-staging/{id}:latest"),
            "nginx:alpine".to_string(),
        ] {
            assert!(!is_deployment_image(&tag, id), "{tag}");
        }
    }

    #[test]
    fn project_address_needs_exactly_one_project_network() {
        let one = [
            ("ciphera-p-x", Some("172.18.0.5")),
            ("bridge", Some("172.17.0.2")),
        ];
        assert_eq!(
            project_ip(one.iter().map(|(n, i)| (*n, *i))),
            Some("172.18.0.5".parse().unwrap())
        );
        let two = [
            ("ciphera-p-x", Some("172.18.0.5")),
            ("ciphera-p-y", Some("172.19.0.5")),
        ];
        assert_eq!(project_ip(two.iter().map(|(n, i)| (*n, *i))), None);
        let none = [("ciphera-internal", Some("172.20.0.5"))];
        assert_eq!(project_ip(none.iter().map(|(n, i)| (*n, *i))), None);
        let empty = [("ciphera-p-x", Some(""))];
        assert_eq!(project_ip(empty.iter().map(|(n, i)| (*n, *i))), None);
    }

    #[test]
    fn app_container_has_memory_cpu() {
        let limits = AppLimits::compute(8 << 30, 4);
        assert_eq!((limits.memory, limits.nano_cpus), (4 << 30, 3_000_000_000));
        // Petit noeud : plancher de 256 Mio et un CPU.
        let small = AppLimits::compute(1 << 29, 1);
        assert_eq!((small.memory, small.nano_cpus), (256 << 20, 1_000_000_000));
        // Le conteneur d'application les porte, avec la limite de processus.
        let node = AppLimits::node();
        let spec = ContainerSpec::app(
            DeploymentId::new(),
            ProjectId::new(),
            &ImageRef("x:1".into()),
            vec![],
        );
        assert_eq!(spec.memory, i64::try_from(node.memory).ok());
        assert_eq!(spec.nano_cpus, i64::try_from(node.nano_cpus).ok());
        assert!(spec.memory.is_some_and(|m| m >= 256 << 20));
        assert!(spec.nano_cpus.is_some_and(|c| c >= 1_000_000_000));
        assert_eq!(spec.pids_limit, Some(APP_PIDS_LIMIT));
        // Sans swap en plus de la memoire, comme les services Compose.
        let body = create_body(&spec);
        let host = body.host_config.unwrap();
        assert_eq!(host.memory, spec.memory);
        assert_eq!(host.memory_swap, spec.memory);
    }

    #[test]
    fn kill_pending_matches_only_a_pending_kill() {
        let err = |status_code: u16, message: &str| BollardError::DockerResponseServerError {
            status_code,
            message: message.to_string(),
        };
        assert!(kill_pending(&err(
            500,
            "could not kill container: container 387e9da67373 PID 16682 is zombie and can not be killed"
        )));
        assert!(kill_pending(&err(
            500,
            "tried to kill container, but did not receive an exit event"
        )));
        assert!(!kill_pending(&err(500, "driver failed")));
        assert!(!kill_pending(&err(409, "is zombie")));
    }

    #[test]
    fn run_as_applied() {
        let app = || {
            ContainerSpec::app(
                DeploymentId::new(),
                ProjectId::new(),
                &ImageRef("x:1".into()),
                vec![],
            )
        };
        assert_eq!(create_body(&app()).user, None);
        let spec = app().with_run_as(Some("1000:1000"));
        assert_eq!(create_body(&spec).user.as_deref(), Some("1000:1000"));
        assert_ne!(spec.fingerprint(), app().fingerprint());
    }

    #[test]
    fn read_only_rootfs_with_tmp() {
        let spec = ContainerSpec::app(
            DeploymentId::new(),
            ProjectId::new(),
            &ImageRef("x:1".into()),
            vec![],
        );
        let host = create_body(&spec).host_config.unwrap();
        assert_eq!((host.readonly_rootfs, host.tmpfs), (None, None));
        let ro = spec.clone().with_read_only_rootfs(true);
        let host = create_body(&ro).host_config.unwrap();
        assert_eq!(host.readonly_rootfs, Some(true));
        let tmpfs = host.tmpfs.unwrap();
        assert_eq!(tmpfs.len(), 1);
        assert_eq!(tmpfs["/tmp"], "rw,nosuid,nodev,size=64m");
        assert_ne!(ro.fingerprint(), spec.fingerprint());
    }

    #[test]
    fn extra_caps_dropped() {
        let spec = ContainerSpec::app(
            DeploymentId::new(),
            ProjectId::new(),
            &ImageRef("x:1".into()),
            vec![],
        );
        let dropped = create_body(&spec).host_config.unwrap().cap_drop.unwrap();
        for cap in ["NET_RAW", "MKNOD", "AUDIT_WRITE", "SETPCAP"] {
            assert!(dropped.iter().any(|c| c == cap), "{cap} manque");
        }
    }

    #[test]
    fn root_user_detection() {
        for root in [
            None,
            Some(""),
            Some("0"),
            Some("root"),
            Some("0:0"),
            Some("root:app"),
            Some("00"),
        ] {
            assert!(user_is_root(root), "{root:?}");
        }
        for other in [
            Some("1000"),
            Some("1000:0"),
            Some("node"),
            Some("65534:65534"),
        ] {
            assert!(!user_is_root(other), "{other:?}");
        }
    }

    #[test]
    fn same_spec_same_fingerprint() {
        let a = ContainerSpec::app(
            DeploymentId::new(),
            ProjectId::new(),
            &ImageRef("x:1".into()),
            vec![],
        );
        let mut b = a.clone();
        assert_eq!(a.fingerprint(), b.fingerprint());
        b.image = "x:2".into();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }
}
