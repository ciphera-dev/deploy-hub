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
    HealthStatusEnum, HostConfig, HostConfigLogConfig, Mount, MountPointTypeEnum, MountTypeEnum,
    NetworkConnectRequest, NetworkCreateRequest, NetworkingConfig, RestartPolicy as DockerRestart,
    RestartPolicyNameEnum,
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
    /// Image demandee a la creation (reference telle que donnee a Docker).
    pub image: String,
    /// Arrete pour de bon (`created`, `exited`, `dead`). Faux pour un
    /// conteneur en marche, en pause, en redemarrage ou d'etat inconnu.
    pub stopped: bool,
}

/// Configuration de creation d'un conteneur gere, relue par `inspect`
/// (`DockerOps::snapshot`) pour le recreer a l'identique
/// (`DockerOps::recreate`) : image, env, cmd, entrypoint, etiquettes,
/// reseaux et alias, limites, utilisateur, montages. Gardee en memoire
/// seulement, jamais ecrite (elle porte l'env) ; `Debug` n'en montre ni
/// l'env ni la configuration.
#[derive(Clone)]
pub struct ContainerSnapshot {
    /// Conteneur relu.
    pub id: ContainerId,
    /// Nom sans le `/` de tete, repris tel quel.
    pub name: String,
    /// Identifiant de l'image (`sha256:...`) du conteneur relu.
    pub image: String,
    /// En marche au moment de la lecture (redemarre apres recreation).
    pub running: bool,
    /// Corps de creation : configuration, `HostConfig`, reseau principal.
    pub body: ContainerCreateBody,
    /// Autres reseaux (relies apres la creation), avec leurs alias.
    pub networks: Vec<(String, EndpointSettings)>,
}

impl fmt::Debug for ContainerSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerSnapshot")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("image", &self.image)
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

/// Reglages d'un point d'acces reseau a redonner a la creation : alias
/// (sans l'identifiant court de l'ancien conteneur), adresses demandees,
/// liens, options ; jamais l'adresse, la passerelle ni l'adresse MAC
/// attribuees par Docker.
fn endpoint_for_create(e: &EndpointSettings, short_id: &str) -> EndpointSettings {
    EndpointSettings {
        ipam_config: e.ipam_config.clone(),
        links: e.links.clone(),
        aliases: e.aliases.as_ref().map(|a| {
            a.iter()
                .filter(|x| x.as_str() != short_id)
                .cloned()
                .collect()
        }),
        driver_opts: e.driver_opts.clone(),
        gw_priority: e.gw_priority,
        ..Default::default()
    }
}

/// Destination d'un montage `Binds` (`source:destination[:options]`).
fn bind_destination(bind: &str) -> Option<&str> {
    bind.split(':').nth(1)
}

/// `ContainerSnapshot` d'un conteneur gere relu par `inspect` (`NotManaged`
/// sinon). Les volumes anonymes (`VOLUME` de l'image) sont remontes par leur
/// nom : le conteneur recree retrouve leurs donnees. Le nom d'hote attribue
/// par Docker (identifiant court) n'est pas repris.
pub fn snapshot_of(info: ContainerInspectResponse) -> Result<ContainerSnapshot, DockerError> {
    let id = info
        .id
        .clone()
        .filter(|i| !i.is_empty())
        .ok_or_else(|| DockerError::Api("conteneur sans identifiant".into()))?;
    let name = info
        .name
        .as_deref()
        .map(|n| n.trim_start_matches('/').to_string())
        .filter(|n| valid_name(n))
        .ok_or_else(|| DockerError::Api(format!("conteneur {id} : nom illisible")))?;
    check_managed(&name, info.config.as_ref().and_then(|c| c.labels.as_ref()))?;
    let short = id.get(..12).unwrap_or(&id).to_string();
    let config = info.config.unwrap_or_default();
    let mut host = info.host_config.unwrap_or_default();
    // Volumes anonymes : par leur nom, sauf une destination deja montee.
    let mut taken: HashSet<String> = host
        .mounts
        .iter()
        .flatten()
        .filter_map(|m| m.target.clone())
        .collect();
    taken.extend(
        host.binds
            .iter()
            .flatten()
            .filter_map(|b| bind_destination(b).map(str::to_string)),
    );
    for m in info.mounts.iter().flatten() {
        if m.typ != Some(MountPointTypeEnum::VOLUME) {
            continue;
        }
        let (Some(volume), Some(dest)) = (m.name.as_ref(), m.destination.as_ref()) else {
            continue;
        };
        if taken.insert(dest.clone()) {
            host.mounts.get_or_insert_with(Vec::new).push(Mount {
                target: Some(dest.clone()),
                source: Some(volume.clone()),
                typ: Some(MountTypeEnum::VOLUME),
                read_only: m.rw.map(|rw| !rw),
                ..Default::default()
            });
        }
    }
    let mut endpoints: Vec<(String, EndpointSettings)> = info
        .network_settings
        .and_then(|n| n.networks)
        .unwrap_or_default()
        .into_iter()
        .map(|(net, e)| {
            let e = endpoint_for_create(&e, &short);
            (net, e)
        })
        .collect();
    // Reseau principal (`NetworkMode`) a la creation, les autres ensuite.
    endpoints.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(i) = endpoints
        .iter()
        .position(|(net, _)| Some(net.as_str()) == host.network_mode.as_deref())
    {
        let main = endpoints.remove(i);
        endpoints.insert(0, main);
    }
    let first = (!endpoints.is_empty()).then(|| endpoints.remove(0));
    let hostname = config.hostname.filter(|h| *h != short);
    let body = ContainerCreateBody {
        hostname,
        domainname: config.domainname,
        user: config.user,
        exposed_ports: config.exposed_ports,
        tty: config.tty,
        open_stdin: config.open_stdin,
        stdin_once: config.stdin_once,
        env: config.env,
        cmd: config.cmd,
        healthcheck: config.healthcheck,
        args_escaped: config.args_escaped,
        image: config.image,
        volumes: config.volumes,
        working_dir: config.working_dir,
        entrypoint: config.entrypoint,
        network_disabled: config.network_disabled,
        labels: config.labels,
        stop_signal: config.stop_signal,
        stop_timeout: config.stop_timeout,
        shell: config.shell,
        host_config: Some(host),
        networking_config: first.map(|(net, e)| NetworkingConfig {
            endpoints_config: Some(HashMap::from([(net, e)])),
        }),
        ..Default::default()
    };
    Ok(ContainerSnapshot {
        id: ContainerId(id),
        name,
        image: info.image.unwrap_or_default(),
        running: info.state.and_then(|s| s.running).unwrap_or(false),
        body,
        networks: endpoints,
    })
}

/// Prefixes des images internes : (provisoires, finales). Les anciens noms
/// (`ciphera-staging/`, `ciphera/`) restent reconnus pour les images des
/// deploiements anterieurs.
const IMAGE_PREFIXES: [(&str, &str); 2] = [
    (
        shared::images::STAGING_PREFIX,
        shared::images::INTERNAL_PREFIX,
    ),
    (
        shared::images::LEGACY_STAGING_PREFIX,
        shared::images::LEGACY_PREFIX,
    ),
];

/// Nom d'image provisoire (`<id>[-<service>]`) ou finale
/// (`<projet>-<env>[/<service>]:<id>`), prefixe retire. `None` : pas une
/// image interne.
fn internal_image_name(tag: &str) -> Option<(bool, &str)> {
    IMAGE_PREFIXES.iter().find_map(|(staging, final_prefix)| {
        // Le prefixe provisoire est plus long que le final du meme jeu
        // (`ciphera.invalid/staging/`) : teste en premier.
        tag.strip_prefix(staging)
            .map(|rest| (true, rest))
            .or_else(|| tag.strip_prefix(final_prefix).map(|rest| (false, rest)))
    })
}

/// Etiquette d'image d'un deploiement :
/// `ciphera.invalid/<projet>-<env>:<id>`,
/// `ciphera.invalid/<projet>-<env>/<service>:<id>` (images finales du build)
/// ou `ciphera.invalid/staging/<id>[-<service>]:build` (noms provisoires) ;
/// aussi sous les anciens prefixes `ciphera/` et `ciphera-staging/`. Une
/// image d'un autre deploiement (rollback) n'en fait jamais partie.
pub fn is_deployment_image(tag: &str, deployment: DeploymentId) -> bool {
    let id = deployment.to_string();
    match internal_image_name(tag) {
        Some((true, rest)) => rest
            .strip_suffix(":build")
            .and_then(|name| name.strip_prefix(id.as_str()))
            .is_some_and(|tail| tail.is_empty() || tail.starts_with('-')),
        Some((false, rest)) => match rest.rsplit_once(':') {
            Some((repo, t)) => {
                t == id
                    && !repo.is_empty()
                    && repo.split('/').count() <= 2
                    && repo.split('/').all(|s| !s.is_empty())
            }
            None => false,
        },
        None => false,
    }
}

/// Deploiement proprietaire de l'etiquette d'image `tag` (voir
/// `is_deployment_image`), `None` pour toute autre image.
pub fn image_deployment(tag: &str) -> Option<DeploymentId> {
    let candidate = match internal_image_name(tag)? {
        (true, rest) => rest.strip_suffix(":build")?.get(..36)?,
        (false, rest) => rest.rsplit_once(':')?.1,
    };
    let id = candidate.parse::<DeploymentId>().ok()?;
    is_deployment_image(tag, id).then_some(id)
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

/// Volume de sortie d'un bac a sable `railpack prepare` (du deploiement
/// `deployment` s'il est donne), reconnu par ses seules etiquettes
/// reservees : gere, role `prepare`, deploiement valide.
pub fn is_prepare_volume(
    labels: &HashMap<String, String>,
    deployment: Option<DeploymentId>,
) -> bool {
    labels.get(MANAGED_LABEL).is_some_and(|v| v == "true")
        && labels.get(ROLE_LABEL).is_some_and(|v| v == PREPARE_ROLE)
        && labels
            .get(DEPLOYMENT_LABEL)
            .and_then(|d| d.parse::<DeploymentId>().ok())
            .is_some_and(|d| deployment.is_none_or(|want| want == d))
}

/// Debut du nom de tous les projets Compose de `project`.
pub fn compose_project_prefix(project: ProjectId) -> String {
    format!("{COMPOSE_PROJECT_PREFIX}{}-", project.0.simple())
}

/// Le volume appartient au projet de `prefix` (`compose_project_prefix`) :
/// volume cree par Compose (etiquette `com.docker.compose.project`) dont le
/// nom est `cp-<UUID du projet>-<8 caracteres hexa de l'environnement>_<cle>`.
/// Le nom fait foi, pas l'etiquette : Compose etiquette un volume avec le
/// projet de la commande qui le cree (`ciphera-d-<deploiement>` pour un
/// volume monte seulement par le service expose), alors que la politique
/// Compose et `ComposePlan` imposent le nom `<projet stable>_<cle>`.
pub fn is_project_volume(name: &str, labels: &HashMap<String, String>, prefix: &str) -> bool {
    if !labels.contains_key(COMPOSE_PROJECT_KEY) {
        return false;
    }
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    let Some((env, key)) = rest.split_once('_') else {
        return false;
    };
    env.len() == 8
        && env
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && !key.is_empty()
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
    /// `follow`) ; `LogStart::Beginning` pour un conteneur lu depuis sa
    /// creation. Le flux s'arrete a la premiere erreur ; une ligne peut-etre
    /// incomplete (erreur, conteneur en marche, fenetre `tail` ouverte sur la
    /// suite d'une ligne) n'est jamais envoyee (`assemble_logs`).
    fn logs(
        &self,
        id: &ContainerId,
        start: LogStart,
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
    /// Comme `remove`, avec les volumes anonymes du conteneur (`VOLUME` de
    /// l'image sans volume nomme) : retrait d'un projet avec ses donnees.
    fn remove_with_volumes(
        &self,
        id: &ContainerId,
    ) -> impl Future<Output = Result<(), DockerError>> + Send;
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
    /// Supprime les volumes de sortie de `railpack prepare`
    /// (`is_prepare_volume`, du deploiement `deployment` s'il est donne)
    /// qu'aucun conteneur n'utilise : un volume encore monte est laisse.
    /// Renvoie leur nombre.
    fn remove_prepare_volumes(
        &self,
        deployment: Option<DeploymentId>,
    ) -> impl Future<Output = Result<usize, DockerError>> + Send;
    /// Deploiements qui ont au moins une etiquette d'image
    /// (`image_deployment`), sans doublon.
    fn image_deployments(
        &self,
    ) -> impl Future<Output = Result<Vec<DeploymentId>, DockerError>> + Send;
    /// Configuration de creation du conteneur gere `id` (`snapshot_of`,
    /// `NotManaged` sinon) ; `None` s'il n'existe plus.
    fn snapshot(
        &self,
        id: &ContainerId,
    ) -> impl Future<Output = Result<Option<ContainerSnapshot>, DockerError>> + Send;
    /// Recree le conteneur `snap` a l'identique, sous son nom, et le demarre
    /// s'il tournait ; renvoie son identifiant. Le conteneur relu existe
    /// encore : rien n'est recree. Un autre conteneur gere du meme nom est
    /// supprime d'abord ; non gere : `NotManaged`, laisse tel quel.
    fn recreate(
        &self,
        snap: &ContainerSnapshot,
    ) -> impl Future<Output = Result<ContainerId, DockerError>> + Send;
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

/// Delai d'un telechargement d'image (registre lent ou muet).
pub const PULL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);
/// Plafond de la taille annoncee d'une image telechargee (somme des couches),
/// le meme que celui d'une image construite.
pub const MAX_PULL_BYTES: u64 = 10 << 30;

/// Telechargement d'image refuse ou interrompu.
#[derive(Debug)]
pub enum PullError {
    /// Image interne de Ciphera : jamais telechargee (`shared::images`).
    Internal,
    /// `PULL_TIMEOUT` depasse.
    Timeout,
    /// Taille annoncee au-dela du plafond.
    TooLarge,
    /// Erreur rapportee par Docker.
    Failed(String),
}

impl fmt::Display for PullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal => {
                write!(
                    f,
                    "image interne absente (jamais telechargee d'un registre)"
                )
            }
            Self::Timeout => write!(f, "delai depasse ({} min)", PULL_TIMEOUT.as_secs() / 60),
            Self::TooLarge => write!(f, "taille annoncee au-dela de {} Gio", MAX_PULL_BYTES >> 30),
            Self::Failed(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for PullError {}

/// Image telechargeable : jamais une image interne de Ciphera
/// (`ciphera.invalid/…`, anciens `ciphera/…` et `ciphera-staging/…`, avec ou
/// sans le registre de Docker Hub). Une image interne absente est une
/// erreur, pas un telechargement.
pub fn check_pullable(image: &str) -> Result<(), PullError> {
    if shared::images::is_internal_ref(image) {
        Err(PullError::Internal)
    } else {
        Ok(())
    }
}

/// Lit le flux d'un telechargement (`create_image`) en `timeout` au plus, et
/// s'arrete des que la taille annoncee (plus grande taille annoncee ou deja
/// recue par couche, additionnee sur les couches, arithmetique saturee,
/// taille compressee) depasse
/// `max_bytes`. Abandonner le flux ferme la requete : Docker interrompt le
/// telechargement. `status` recoit chaque etape (journal du build).
pub async fn bounded_pull<S, F, Fut>(
    steps: S,
    timeout: std::time::Duration,
    max_bytes: u64,
    mut status: F,
) -> Result<(), PullError>
where
    S: futures_util::Stream<Item = Result<bollard::models::CreateImageInfo, BollardError>>,
    F: FnMut(String) -> Fut,
    Fut: Future<Output = ()>,
{
    let read = async {
        let mut steps = std::pin::pin!(steps);
        let mut layers: HashMap<String, u64> = HashMap::new();
        while let Some(step) = steps.next().await {
            let info = step.map_err(|e| PullError::Failed(e.to_string()))?;
            // Taille annoncee, ou deja recue si le registre n'en annonce
            // pas : une couche sans `total` est bornee par son `current`.
            let announced = info.progress_detail.as_ref().and_then(|p| {
                let size = |v: Option<i64>| v.and_then(|v| u64::try_from(v).ok());
                match (size(p.current), size(p.total)) {
                    (None, None) => None,
                    (c, t) => Some(c.unwrap_or(0).max(t.unwrap_or(0))),
                }
            });
            if let (Some(id), Some(size)) = (info.id.as_ref(), announced) {
                let layer = layers.entry(id.clone()).or_insert(0);
                *layer = (*layer).max(size);
                let total = layers.values().fold(0u64, |acc, s| acc.saturating_add(*s));
                if total > max_bytes {
                    return Err(PullError::TooLarge);
                }
            }
            if let Some(s) = info.status {
                status(s).await;
            }
        }
        Ok(())
    };
    tokio::time::timeout(timeout, read)
        .await
        .unwrap_or(Err(PullError::Timeout))
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

    /// Telecharge `image` si elle manque : jamais une image interne
    /// (`check_pullable`), en `PULL_TIMEOUT` et `MAX_PULL_BYTES` annonces au
    /// plus (`bounded_pull`).
    async fn pull_if_missing(&self, image: &str) -> Result<(), DockerError> {
        match self.docker.inspect_image(image).await {
            Ok(_) => return Ok(()),
            Err(e) if is_status(&e, 404) => {}
            Err(e) => return Err(api(e)),
        }
        let failed = |e: PullError| {
            tracing::warn!("image {image} : {e}");
            DockerError::Pull(image.to_string())
        };
        check_pullable(image).map_err(failed)?;
        tracing::info!("telechargement de l'image {image}");
        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };
        let pull = self.docker.create_image(Some(options), None, None);
        bounded_pull(pull, PULL_TIMEOUT, MAX_PULL_BYTES, |_| async {})
            .await
            .map_err(failed)
    }

    /// Suppression forcee ; si Docker n'a pas vu le conteneur sortir apres
    /// son SIGKILL (PID 1 zombie le temps que l'espace de noms des processus
    /// se vide), nouvel essai, `REMOVE_ATTEMPTS` au plus.
    async fn force_remove(&self, id: &str) -> Result<(), DockerError> {
        self.force_remove_opts(id, false).await
    }

    /// `force_remove`, avec les volumes anonymes du conteneur si `volumes`.
    async fn force_remove_opts(&self, id: &str, volumes: bool) -> Result<(), DockerError> {
        let mut attempt = 1;
        loop {
            let options = RemoveContainerOptions {
                force: true,
                v: volumes,
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
        start: LogStart,
        follow: bool,
    ) -> BoxStream<'static, ContainerLog> {
        if check_name(&id.0).is_err() {
            return stream::empty().boxed();
        }
        // La ligne en attente a la fin du flux n'est complete que si le
        // conteneur est sorti (`log_complete`) : execution relevee avant la
        // lecture, comparee a celle d'apres.
        let before = run_mark(&self.docker, &id.0).await;
        // TTY (ou conteneur illisible) : sans horodatage (`logs_options`).
        let tty = before.as_ref().is_none_or(|m| m.tty);
        let options = logs_options(start, follow, tty);
        let docker = self.docker.clone();
        let name = id.0.clone();
        let exited = move || async move {
            let after = run_mark(&docker, &name).await;
            log_complete(before.as_ref(), after.as_ref(), follow)
        };
        assemble_logs(
            self.docker.logs(&id.0, Some(options)),
            start.window(),
            exited,
        )
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

    async fn snapshot(&self, id: &ContainerId) -> Result<Option<ContainerSnapshot>, DockerError> {
        check_name(&id.0)?;
        self.inspect_managed(&id.0)
            .await?
            .map(snapshot_of)
            .transpose()
    }

    async fn recreate(&self, snap: &ContainerSnapshot) -> Result<ContainerId, DockerError> {
        check_name(&snap.name)?;
        if let Some(existing) = self.inspect_managed(&snap.name).await? {
            let id = existing.id.unwrap_or_else(|| snap.name.clone());
            if id == snap.id.0 {
                // Jamais remplace : rien a recreer.
                return Ok(snap.id.clone());
            }
            self.force_remove(&id).await?;
        }
        let mut body = snap.body.clone();
        // La reference d'origine si elle designe toujours la meme image,
        // sinon l'image elle-meme (par son identifiant).
        let same_ref = match body.image.as_deref() {
            Some(r) if valid_image(r) => match self.docker.inspect_image(r).await {
                Ok(img) => img.id.as_deref() == Some(snap.image.as_str()),
                Err(e) if is_status(&e, 404) => false,
                Err(e) => return Err(api(e)),
            },
            _ => false,
        };
        if !same_ref {
            body.image = Some(snap.image.clone());
        }
        let options = CreateContainerOptions {
            name: Some(snap.name.clone()),
            ..Default::default()
        };
        let created = self
            .docker
            .create_container(Some(options), body)
            .await
            .map_err(api)?;
        for (net, endpoint) in &snap.networks {
            let request = NetworkConnectRequest {
                container: created.id.clone(),
                endpoint_config: Some(endpoint.clone()),
            };
            self.docker
                .connect_network(net, request)
                .await
                .map_err(api)?;
        }
        if snap.running {
            self.docker
                .start_container(&created.id, None)
                .await
                .map_err(api)?;
        }
        Ok(ContainerId(created.id))
    }

    async fn remove_with_volumes(&self, id: &ContainerId) -> Result<(), DockerError> {
        check_name(&id.0)?;
        let Some(info) = self.inspect_managed(&id.0).await? else {
            return Ok(());
        };
        self.force_remove_opts(info.id.as_deref().unwrap_or(&id.0), true)
            .await
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

    async fn remove_prepare_volumes(
        &self,
        deployment: Option<DeploymentId>,
    ) -> Result<usize, DockerError> {
        let mut labels = vec![
            format!("{MANAGED_LABEL}=true"),
            format!("{ROLE_LABEL}={PREPARE_ROLE}"),
        ];
        if let Some(d) = deployment {
            labels.push(format!("{DEPLOYMENT_LABEL}={d}"));
        }
        // `dangling` : volumes qu'aucun conteneur (meme arrete) ne monte.
        let options = ListVolumesOptions {
            filters: Some(HashMap::from([
                ("label".to_string(), labels),
                ("dangling".to_string(), vec!["true".to_string()]),
            ])),
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
            .filter(|v| is_prepare_volume(&v.labels, deployment))
        {
            check_name(&v.name)?;
            // Sans `force` : Docker refuse (409) un volume encore monte.
            match self
                .docker
                .remove_volume(&v.name, Some(RemoveVolumeOptions { force: false }))
                .await
            {
                Ok(()) => removed += 1,
                Err(e) if is_status(&e, 404) || is_status(&e, 409) => {}
                Err(e) => return Err(api(e)),
            }
        }
        Ok(removed)
    }

    async fn remove_project_volumes(&self, project: ProjectId) -> Result<usize, DockerError> {
        let prefix = compose_project_prefix(project);
        let options = ListVolumesOptions {
            filters: Some(HashMap::from([
                ("label".to_string(), vec![COMPOSE_PROJECT_KEY.to_string()]),
                ("name".to_string(), vec![prefix.clone()]),
            ])),
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

    async fn image_deployments(&self) -> Result<Vec<DeploymentId>, DockerError> {
        let images = self
            .docker
            .list_images(Some(ListImagesOptions::default()))
            .await
            .map_err(api)?;
        let mut out: Vec<DeploymentId> = images
            .iter()
            .flat_map(|i| i.repo_tags.iter())
            .filter_map(|t| image_deployment(t))
            .collect();
        out.sort_unstable();
        out.dedup();
        Ok(out)
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
                let stopped = matches!(
                    c.state,
                    Some(
                        bollard::models::ContainerSummaryStateEnum::CREATED
                            | bollard::models::ContainerSummaryStateEnum::EXITED
                            | bollard::models::ContainerSummaryStateEnum::DEAD
                    )
                );
                Some(ManagedContainer {
                    image: c.image.unwrap_or_default(),
                    stopped,
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
/// Une ligne sans `\n` est retenue jusqu'au `\n` ou a la fin propre du flux
/// d'un conteneur sorti (`finish`), sans vidage sur delai ; apres une erreur
/// ou sur un conteneur encore en marche, elle est ecartee (`discard`) : echec
/// ferme, car un vidage coupe pourrait separer un secret en deux morceaux
/// jamais masques (une barre de progression sans retour a la ligne
/// n'apparait donc qu'a la fin).
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

    /// Fin propre du flux d'un conteneur sorti : la ligne sans retour a la
    /// ligne final est complete et part. Dans tout autre cas, `discard`.
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

    /// Fin du flux sans garantie que la ligne en attente soit complete
    /// (erreur, conteneur encore en marche) : elle est ecartee, coupee sur
    /// une frontiere de trame, elle pourrait laisser voir le debut d'un
    /// secret jamais masque. `INCOMPLETE_LINE`, sans aucune donnee, la
    /// remplace.
    pub fn discard(&mut self) -> Vec<ContainerLog> {
        let mut out = Vec::new();
        for (i, stream) in [LogStream::Stdout, LogStream::Stderr]
            .into_iter()
            .enumerate()
        {
            let line = std::mem::take(&mut self.pending[i]);
            self.over[i] = false;
            if !line.is_empty() {
                out.push(ContainerLog {
                    stream,
                    timestamp_ms: now_ms(),
                    line: INCOMPLETE_LINE.to_string(),
                });
            }
        }
        out
    }
}

/// Remplace une ligne ecartee, sans aucune donnee : ligne en attente
/// (`LogAssembler::discard`) ou premiere ligne d'une fenetre peut-etre
/// coupee (`LogWindow`).
pub const INCOMPLETE_LINE: &str = "[ligne incomplète ignorée]";

/// Debut d'une lecture de journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStart {
    /// Les `n` derniers messages (puis la suite, en suivi).
    Tail(u32),
    /// Tout le journal que Docker garde encore (conteneur neuf) : seule la
    /// premiere ligne de chaque flux est perdue (`LogWindow`).
    Beginning,
}

impl fmt::Display for LogStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tail(n) => write!(f, "{n}"),
            Self::Beginning => f.write_str("all"),
        }
    }
}

impl LogStart {
    /// Valeur `tail` demandee a Docker : un message de plus que les `n`
    /// voulus, celui dont la ligne est ecartee (voir `LogWindow`).
    pub fn docker_tail(self) -> String {
        match self {
            Self::Tail(n) => (u64::from(n) + 1).to_string(),
            Self::Beginning => "all".to_string(),
        }
    }

    /// Fenetre lue avec `docker_tail` : toujours peut-etre coupee.
    pub fn window(self) -> LogWindow {
        LogWindow::Cut {
            extra: matches!(self, Self::Tail(_)),
        }
    }
}

/// Debut de la fenetre d'une lecture. `tail` compte les messages de Docker,
/// trames partielles comprises, et la rotation du journal (json-file,
/// `max-file`) en retire les plus anciens : toute lecture d'un vrai journal
/// peut s'ouvrir sur la suite d'une ligne, qui commencerait par la fin d'un
/// secret jamais masque (`redact_line` ne masque que des valeurs entieres).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogWindow {
    /// Flux complet connu (tests en memoire) : rien n'est ecarte.
    Whole,
    /// Peut-etre coupee : la premiere ligne de chaque flux est remplacee par
    /// `INCOMPLETE_LINE`. Avec `extra` (un message demande en plus), celle
    /// faite de la seule premiere trame est simplement ecartee.
    Cut { extra: bool },
}

/// Filtre de la premiere ligne de chaque flux selon `LogWindow` ; note
/// aussi les trames de console (TTY), decoupees n'importe ou.
struct WindowFilter {
    window: LogWindow,
    started: bool,
    seen: [bool; 2],
    console: bool,
}

impl WindowFilter {
    fn new(window: LogWindow) -> Self {
        Self {
            window,
            started: false,
            seen: [false; 2],
            console: false,
        }
    }

    /// Trame `bytes` du flux `stream` ; renvoie les lignes a envoyer.
    fn frame(
        &mut self,
        assembler: &mut LogAssembler,
        stream: LogStream,
        bytes: &[u8],
    ) -> Vec<ContainerLog> {
        let first_frame = !self.started;
        self.started = true;
        let lines = assembler.push(stream, bytes);
        let mut out = Vec::with_capacity(lines.len());
        for (k, line) in lines.into_iter().enumerate() {
            out.extend(self.filter(line, first_frame && k == 0));
        }
        out
    }

    fn filter(&mut self, mut line: ContainerLog, only_first_frame: bool) -> Option<ContainerLog> {
        let i = LogAssembler::slot(line.stream);
        let first = !self.seen[i];
        self.seen[i] = true;
        match self.window {
            LogWindow::Cut { extra } if first => {
                if extra && only_first_frame {
                    return None;
                }
                line.line = INCOMPLETE_LINE.to_string();
                Some(line)
            }
            _ => Some(line),
        }
    }

    /// Fin du flux : `lines` de l'assembleur (`finish`, ou repères de
    /// `discard` si `markers`).
    fn end(&mut self, lines: Vec<ContainerLog>, markers: bool) -> Vec<ContainerLog> {
        if markers {
            return lines;
        }
        lines
            .into_iter()
            .filter_map(|line| self.filter(line, false))
            .collect()
    }
}

/// Lignes rassemblees du flux `docker logs` `frames` (lu avec la fenetre
/// `window`), arrete a la premiere erreur. Sur une fin propre, `exited` dit
/// si le journal est complet (lecture sans suivi d'un conteneur sorti, voir
/// `log_complete`) : la ligne en attente part alors (`finish`). Apres une
/// erreur, conteneur encore en marche, fin d'un suivi ou trames de console
/// (TTY), elle est ecartee (`discard`) : echec ferme.
pub fn assemble_logs<S, E, X, F>(
    frames: S,
    window: LogWindow,
    exited: X,
) -> BoxStream<'static, ContainerLog>
where
    S: futures_util::Stream<Item = Result<bollard::container::LogOutput, E>> + Send + 'static,
    E: Send + 'static,
    X: FnOnce() -> F + Send + 'static,
    F: Future<Output = bool> + Send + 'static,
{
    use bollard::container::LogOutput;
    struct State<S, X> {
        frames: Option<std::pin::Pin<Box<S>>>,
        exited: Option<X>,
        assembler: LogAssembler,
        filter: WindowFilter,
        ready: std::collections::VecDeque<ContainerLog>,
    }
    let state = State {
        frames: Some(Box::pin(frames)),
        exited: Some(exited),
        assembler: LogAssembler::default(),
        filter: WindowFilter::new(window),
        ready: Default::default(),
    };
    stream::unfold(state, |mut st| async move {
        loop {
            if let Some(line) = st.ready.pop_front() {
                return Some((line, st));
            }
            let frames = st.frames.as_mut()?;
            let lines = match frames.next().await {
                Some(Ok(LogOutput::StdErr { message })) => {
                    st.filter
                        .frame(&mut st.assembler, LogStream::Stderr, &message)
                }
                Some(Ok(LogOutput::StdOut { message })) => {
                    st.filter
                        .frame(&mut st.assembler, LogStream::Stdout, &message)
                }
                Some(Ok(LogOutput::Console { message })) => {
                    st.filter.console = true;
                    st.filter
                        .frame(&mut st.assembler, LogStream::Stdout, &message)
                }
                Some(Ok(LogOutput::StdIn { .. })) => Vec::new(),
                Some(Err(_)) => {
                    st.frames = None;
                    let discarded = st.assembler.discard();
                    st.filter.end(discarded, true)
                }
                None => {
                    st.frames = None;
                    let complete = match st.exited.take() {
                        Some(exited) if !st.filter.console => exited().await,
                        _ => false,
                    };
                    if complete {
                        let finished = st.assembler.finish();
                        st.filter.end(finished, false)
                    } else {
                        let discarded = st.assembler.discard();
                        st.filter.end(discarded, true)
                    }
                }
            };
            st.ready.extend(lines);
        }
    })
    .boxed()
}

/// Etat d'une execution du conteneur.
#[derive(PartialEq)]
struct RunMark {
    /// `exited` ou `dead`.
    exited: bool,
    started_at: String,
    finished_at: String,
    /// Conteneur a TTY (`Config.Tty`).
    tty: bool,
}

/// Execution courante du conteneur ; `None` s'il reste illisible.
async fn run_mark(docker: &Docker, name: &str) -> Option<RunMark> {
    let info = docker.inspect_container(name, None).await.ok()?;
    let tty = info.config.and_then(|c| c.tty).unwrap_or(false);
    let state = info.state?;
    Some(RunMark {
        tty,
        exited: matches!(
            state.status,
            Some(ContainerStateStatusEnum::EXITED | ContainerStateStatusEnum::DEAD)
        ),
        started_at: state.started_at.unwrap_or_default(),
        finished_at: state.finished_at.unwrap_or_default(),
    })
}

/// Options de `docker logs`. Pour un conteneur a TTY, sans horodatage :
/// Docker y ecrit une ligne longue en messages partiels que bollard ne
/// recoupe qu'au `\n` (un seul element `Console`) ; avec `timestamps`,
/// l'horodatage du message suivant resterait au milieu de la ligne et
/// separerait un secret en deux morceaux jamais masques. Les lignes prennent
/// alors l'heure de reception (`split_log_chunk`).
fn logs_options(start: LogStart, follow: bool, tty: bool) -> LogsOptions {
    LogsOptions {
        follow,
        stdout: true,
        stderr: true,
        timestamps: !tty,
        tail: start.docker_tail(),
        ..Default::default()
    }
}

/// Journal complet a la fin propre d'une lecture : jamais en suivi (le flux
/// peut finir conteneur en marche, puis le conteneur sortir avant
/// l'inspection) ; sans suivi, conteneur deja sorti avant la lecture (`before`)
/// et meme execution, inchangee, apres.
fn log_complete(before: Option<&RunMark>, after: Option<&RunMark>, follow: bool) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => !follow && before.exited && before == after,
        _ => false,
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
    // Annee lue telle quelle : arithmetique verifiee, une annee absurde
    // donne `None`, jamais un debordement.
    let y = if month <= 2 {
        year.checked_sub(1)?
    } else {
        year
    };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era.checked_mul(146_097)?.checked_add(doe - 719_468)?;
    days.checked_mul(24)?
        .checked_add(h)?
        .checked_mul(60)?
        .checked_add(m)?
        .checked_mul(60_000)?
        .checked_add(sec * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_volumes_are_recognized_by_reserved_labels_only() {
        let id = DeploymentId::new();
        let labels = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let d = id.to_string();
        let ok = labels(&[
            (MANAGED_LABEL, "true"),
            (ROLE_LABEL, PREPARE_ROLE),
            (DEPLOYMENT_LABEL, &d),
        ]);
        assert!(is_prepare_volume(&ok, None));
        assert!(is_prepare_volume(&ok, Some(id)));
        assert!(!is_prepare_volume(&ok, Some(DeploymentId::new())));
        for bad in [
            labels(&[(ROLE_LABEL, PREPARE_ROLE), (DEPLOYMENT_LABEL, &d)]),
            labels(&[(MANAGED_LABEL, "true"), (DEPLOYMENT_LABEL, &d)]),
            labels(&[(MANAGED_LABEL, "true"), (ROLE_LABEL, PREPARE_ROLE)]),
            labels(&[
                (MANAGED_LABEL, "true"),
                (ROLE_LABEL, PREPARE_ROLE),
                (DEPLOYMENT_LABEL, "pas-un-uuid"),
            ]),
            labels(&[(COMPOSE_PROJECT_KEY, "cp-x")]),
        ] {
            assert!(!is_prepare_volume(&bad, None), "{bad:?}");
        }
    }

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

    /// Ligne de 36 Ko, secret a cheval sur l'octet 16 384, en trames
    /// partielles horodatees de 16 Kio (la derniere finit par `\n`).
    fn straddling_frames(secret: &str) -> (String, Vec<bytes::Bytes>) {
        let line = format!("{}{secret}{}", "a".repeat(16_384 - 20), "b".repeat(20_000));
        let chunks: Vec<&[u8]> = line.as_bytes().chunks(16_384).collect();
        let frames = chunks
            .iter()
            .enumerate()
            .map(|(n, chunk)| {
                let mut frame = format!("2026-11-21T10:20:3{n}.123456789Z ").into_bytes();
                frame.extend_from_slice(chunk);
                if n + 1 == chunks.len() {
                    frame.push(b'\n');
                }
                bytes::Bytes::from(frame)
            })
            .collect();
        (line, frames)
    }

    fn stdout(message: bytes::Bytes) -> Result<bollard::container::LogOutput, &'static str> {
        Ok(bollard::container::LogOutput::StdOut { message })
    }

    /// Une ligne laisse voir un fragment du secret (4 caracteres ou plus).
    fn leaks(lines: &[ContainerLog], secret: &str) -> bool {
        lines
            .iter()
            .any(|l| (0..=secret.len() - 4).any(|i| l.line.contains(&secret[i..i + 4])))
    }

    #[tokio::test]
    async fn partial_line_before_a_stream_error_is_never_sent() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let (_, frames) = straddling_frames(secret);
        // Premiere trame (debut du secret), puis erreur ; la suite ne doit
        // pas etre lue.
        let items = vec![
            stdout(b"2026-11-21T10:20:29Z avant\n".as_slice().into()),
            stdout(frames[0].clone()),
            Err("coupure"),
            stdout(frames[1].clone()),
            stdout(frames[2].clone()),
        ];
        let lines = assembled(items, LogWindow::Whole, true).await;
        assert!(!leaks(&lines, secret), "debut du secret envoye");
        let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(shown, ["avant", INCOMPLETE_LINE]);
    }

    #[tokio::test]
    async fn partial_line_of_a_running_container_is_never_sent() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let (_, frames) = straddling_frames(secret);
        // Lecture sans suivi pendant l'ecriture : fin propre du flux apres
        // deux trames, conteneur encore en marche.
        let items = vec![stdout(frames[0].clone()), stdout(frames[1].clone())];
        let lines = assembled(items, LogWindow::Whole, false).await;
        assert!(!leaks(&lines, secret), "ligne partielle envoyee");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].line, INCOMPLETE_LINE);
    }

    #[tokio::test]
    async fn exited_container_lines_are_all_sent_on_a_clean_end() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let (line, frames) = straddling_frames(secret);
        let mut items: Vec<_> = frames.into_iter().map(stdout).collect();
        // Derniere ligne sans `\n` : complete, le conteneur est sorti.
        items.push(stdout(b"2026-11-21T10:20:40Z fin".as_slice().into()));
        let lines = assembled(items, LogWindow::Whole, true).await;
        let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(shown, [line.as_str(), "fin"]);
    }

    fn stderr(message: &'static [u8]) -> Result<bollard::container::LogOutput, &'static str> {
        Ok(bollard::container::LogOutput::StdErr {
            message: message.into(),
        })
    }

    async fn assembled(
        items: Vec<Result<bollard::container::LogOutput, &'static str>>,
        window: LogWindow,
        exited: bool,
    ) -> Vec<ContainerLog> {
        assemble_logs(stream::iter(items), window, move || async move { exited })
            .collect()
            .await
    }

    /// Fenetre `tail` ouverte sur la suite d'une ligne (trame qui commence
    /// par la fin du secret) : elle n'est jamais envoyee.
    #[tokio::test]
    async fn tail_read_starting_on_a_continuation_frame_never_sends_a_secret_suffix() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let (_, frames) = straddling_frames(secret);
        // `LogStart::Tail(3)` : 4 messages demandes et rendus. Le premier
        // (le message en plus) est une ligne entiere de l'autre flux.
        let items = vec![
            stderr(b"2026-11-21T10:20:29Z err\n"),
            stdout(frames[1].clone()),
            stdout(frames[2].clone()),
            stdout(b"2026-11-21T10:20:40Z suivante\n".as_slice().into()),
        ];
        let lines = assembled(items, LogStart::Tail(3).window(), true).await;
        assert!(!leaks(&lines, secret), "fin du secret envoyee");
        let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(shown, [INCOMPLETE_LINE, "suivante"]);
    }

    /// Journal tourne (json-file, `max-file`) : moins de messages que
    /// demande, mais le plus ancien garde est la suite d'une ligne. Ni
    /// `Tail` ni `Beginning` n'envoient la fin du secret.
    #[tokio::test]
    async fn rotated_log_starting_on_a_continuation_frame_never_sends_a_secret_suffix() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let (_, frames) = straddling_frames(secret);
        for start in [LogStart::Tail(2000), LogStart::Beginning] {
            let items = vec![
                stdout(frames[1].clone()),
                stdout(frames[2].clone()),
                stdout(b"2026-11-21T10:20:40Z suivante\n".as_slice().into()),
            ];
            let lines = assembled(items, start.window(), true).await;
            assert!(!leaks(&lines, secret), "fin du secret envoyee ({start})");
            let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
            assert_eq!(shown, [INCOMPLETE_LINE, "suivante"], "{start}");
        }
    }

    /// `Beginning` (conteneur neuf) : la premiere ligne de chaque flux est
    /// remplacee par le repere, jamais ecartee sans trace.
    #[tokio::test]
    async fn beginning_read_marks_the_first_line_of_each_stream() {
        assert_eq!(LogStart::Beginning.docker_tail(), "all");
        assert_eq!(LogStart::Tail(5).docker_tail(), "6");
        let items = vec![
            stdout(b"2026-11-21T10:20:29Z premiere\n".as_slice().into()),
            stderr(b"2026-11-21T10:20:30Z err\n"),
            stdout(b"2026-11-21T10:20:31Z deuxieme\n".as_slice().into()),
        ];
        let lines = assembled(items, LogStart::Beginning.window(), true).await;
        let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(shown, [INCOMPLETE_LINE, INCOMPLETE_LINE, "deuxieme"]);
    }

    /// Trames de console (TTY) decoupees n'importe ou : la ligne en attente
    /// est ecartee meme conteneur sorti.
    #[tokio::test]
    async fn console_pending_line_is_discarded_even_after_exit() {
        let items = vec![Ok(bollard::container::LogOutput::Console {
            message: b"2026-11-21T10:20:29Z secret-coupe".as_slice().into(),
        })];
        let lines = assembled(items, LogWindow::Whole, true).await;
        let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(shown, [INCOMPLETE_LINE]);
    }

    /// Conteneur a TTY : Docker ecrit la ligne longue en deux messages
    /// partiels que bollard rend en un seul element `Console` (recoupe au
    /// `\n`). Lue avec les options de `logs`, la ligne garde le secret
    /// entier.
    #[tokio::test]
    async fn tty_long_line_is_read_without_timestamps_and_the_secret_stays_whole() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        let (line, _) = straddling_frames(secret);
        let (a, b) = line.split_at(16_384);
        let start = LogStart::Tail(10);
        let options = logs_options(start, true, true);
        // Ce que Docker ecrit avec ces options.
        let (first, long) = if options.timestamps {
            (
                "2026-11-21T10:20:29Z debut\n".to_string(),
                format!("2026-11-21T10:20:30.1Z {a}2026-11-21T10:20:30.2Z {b}\n"),
            )
        } else {
            ("debut\n".to_string(), format!("{a}{b}\n"))
        };
        let console = |s: String| {
            Ok(bollard::container::LogOutput::Console {
                message: s.into_bytes().into(),
            })
        };
        let items = vec![console(first), console(long)];
        let lines = assembled(items, start.window(), false).await;
        // Entier, le secret sera masque par le plan de controle ; coupe par
        // un horodatage, son debut ne le serait pas.
        assert!(
            lines.iter().all(|l| l.line.contains(secret)),
            "secret coupe par un horodatage"
        );
        let shown: Vec<_> = lines.iter().map(|l| l.line.as_str()).collect();
        assert_eq!(shown, [line.as_str()]);
        // Sans TTY, les horodatages restent demandes.
        assert!(logs_options(start, true, false).timestamps);
    }

    #[test]
    fn pending_line_is_complete_only_for_an_exited_container_read_without_follow() {
        let run = |exited: bool, finished: &str| RunMark {
            exited,
            tty: false,
            started_at: "2026-11-21T10:00:00Z".to_string(),
            finished_at: finished.to_string(),
        };
        let running = run(false, "0001-01-01T00:00:00Z");
        let done = run(true, "2026-11-21T10:20:31.5Z");
        // Suivi fini proprement conteneur en marche, sorti avant
        // l'inspection : jamais complet.
        assert!(!log_complete(Some(&running), Some(&done), true));
        assert!(!log_complete(Some(&done), Some(&done), true));
        // Sans suivi : deja sorti avant la lecture, inchange apres.
        assert!(log_complete(Some(&done), Some(&done), false));
        assert!(!log_complete(Some(&running), Some(&done), false));
        assert!(!log_complete(Some(&running), Some(&running), false));
        let restarted = RunMark {
            started_at: "2026-11-21T10:30:00Z".to_string(),
            ..run(true, "2026-11-21T10:40:00Z")
        };
        assert!(!log_complete(Some(&done), Some(&restarted), false));
        assert!(!log_complete(None, Some(&done), false));
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
    fn internal_images_never_pulled() {
        for internal in [
            "ciphera.invalid/web-production:1",
            "ciphera.invalid/web-production/web:1",
            "ciphera.invalid/staging/x:build",
            "ciphera/web-production:1",
            "ciphera-staging/x:build",
            "docker.io/ciphera/web-production:1",
            "index.docker.io/ciphera-staging/x:build",
            "registry-1.docker.io/ciphera.invalid/x",
        ] {
            assert!(
                matches!(check_pullable(internal), Err(PullError::Internal)),
                "{internal}"
            );
        }
        for user in [
            "nginx:1.27",
            "docker.io/library/postgres:17",
            "ghcr.io/ciphera/web:1",
        ] {
            assert!(check_pullable(user).is_ok(), "{user}");
        }
    }

    fn layer(id: &str, total: i64) -> Result<bollard::models::CreateImageInfo, BollardError> {
        progress(id, Some(0), Some(total))
    }

    fn progress(
        id: &str,
        current: Option<i64>,
        total: Option<i64>,
    ) -> Result<bollard::models::CreateImageInfo, BollardError> {
        Ok(bollard::models::CreateImageInfo {
            id: Some(id.into()),
            status: Some("Downloading".into()),
            progress_detail: Some(bollard::models::ProgressDetail { current, total }),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn pull_without_announced_size_is_capped_by_received_bytes() {
        // Registre qui n'annonce aucune taille : `current` compte.
        let over = stream::iter([
            progress("a", Some(60), None),
            progress("b", Some(30), None),
            progress("b", Some(41), None),
        ]);
        let result = bounded_pull(over, PULL_TIMEOUT, 100, |_| async {}).await;
        assert!(matches!(result, Err(PullError::TooLarge)), "{result:?}");
        let fits = stream::iter([progress("a", Some(60), None), progress("b", None, None)]);
        assert!(
            bounded_pull(fits, PULL_TIMEOUT, 100, |_| async {})
                .await
                .is_ok()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn pull_timeout() {
        assert_eq!(PULL_TIMEOUT, std::time::Duration::from_secs(10 * 60));
        // Registre qui ne repond plus apres une couche : abandon au delai.
        let steps = stream::iter([layer("a", 10)]).chain(stream::pending());
        let started = tokio::time::Instant::now();
        let result = bounded_pull(steps, PULL_TIMEOUT, MAX_PULL_BYTES, |_| async {}).await;
        assert!(matches!(result, Err(PullError::Timeout)), "{result:?}");
        assert_eq!(started.elapsed(), PULL_TIMEOUT);
    }

    #[tokio::test]
    async fn pull_announced_size_is_capped() {
        assert_eq!(MAX_PULL_BYTES, 10 << 30);
        // Taille par couche : la plus grande annoncee, pas la somme des
        // evenements d'une meme couche.
        let fits = stream::iter([layer("a", 60), layer("a", 60), layer("b", 40)]);
        assert!(
            bounded_pull(fits, PULL_TIMEOUT, 100, |_| async {})
                .await
                .is_ok()
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let over = stream::iter([layer("a", 60), layer("b", 41), layer("c", 1)]);
        let result = bounded_pull(over, PULL_TIMEOUT, 100, move |s| {
            log.lock().unwrap().push(s);
            async {}
        })
        .await;
        assert!(matches!(result, Err(PullError::TooLarge)), "{result:?}");
        // Arret des le depassement : la couche suivante n'est pas lue.
        assert_eq!(seen.lock().unwrap().len(), 1);
        // Taille absurde (negative ou enorme) : somme saturee, jamais de
        // debordement.
        let huge = stream::iter([
            layer("a", -5),
            layer("b", i64::MAX),
            layer("c", i64::MAX),
            layer("d", i64::MAX),
        ]);
        let result = bounded_pull(huge, PULL_TIMEOUT, u64::MAX - 1, |_| async {}).await;
        assert!(matches!(result, Err(PullError::TooLarge)), "{result:?}");
    }

    #[test]
    fn rfc3339_arithmetic_never_overflows() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_ms("2026-11-25T10:00:00.5Z"),
            Some(1_795_600_800_500)
        );
        for bad in [
            "9223372036854775807-12-31T23:59:59Z",
            "99999999999999999-01-01T00:00:00Z",
            "-1-01-01T00:00:00Z",
        ] {
            assert_eq!(parse_rfc3339_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn deployment_images_are_recognised_by_their_tag() {
        let id = DeploymentId::new();
        let other = DeploymentId::new();
        for tag in [
            format!("ciphera.invalid/web-production:{id}"),
            format!("ciphera.invalid/web-production/web:{id}"),
            format!("ciphera.invalid/staging/{id}:build"),
            format!("ciphera.invalid/staging/{id}-web:build"),
            // Anciens noms (deploiements anterieurs) : toujours reconnus.
            format!("ciphera/web-production:{id}"),
            format!("ciphera/web-production/web:{id}"),
            format!("ciphera-staging/{id}:build"),
            format!("ciphera-staging/{id}-web:build"),
        ] {
            assert!(is_deployment_image(&tag, id), "{tag}");
            assert!(!is_deployment_image(&tag, other), "{tag}");
        }
        assert_eq!(
            image_deployment(&format!("ciphera.invalid/web-production/web:{id}")),
            Some(id)
        );
        assert_eq!(
            image_deployment(&format!("ciphera.invalid/staging/{id}-web:build")),
            Some(id)
        );
        assert_eq!(
            image_deployment(&format!("ciphera-staging/{id}-web:build")),
            Some(id)
        );
        assert_eq!(
            image_deployment(&format!("ciphera.invalid/web-production:{}", id.0.simple())),
            None,
            "forme canonique seulement"
        );
        for tag in [
            format!("ciphera.invalid/web-production:{id}x"),
            format!("docker.io/ciphera/web:{id}"),
            format!("ciphera.invalid/a/b/c:{id}"),
            format!("ciphera.invalid/staging/{id}x:build"),
            format!("ciphera.invalid/staging/{id}:latest"),
            format!("ciphera.invalid:5000/web:{id}"),
            format!("ciphera/a/b/c:{id}"),
            format!("ciphera-staging/{id}x:build"),
            format!("ciphera-staging/{id}:latest"),
            "nginx:alpine".to_string(),
        ] {
            assert!(!is_deployment_image(&tag, id), "{tag}");
            assert_eq!(image_deployment(&tag), None, "{tag}");
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

    /// Reponse d'`inspect` d'un service Compose (forme de l'API Docker).
    fn compose_inspect() -> ContainerInspectResponse {
        serde_json::from_value(serde_json::json!({
            "Id": "0123456789abcdef0123",
            "Name": "/shop-production-cache-1",
            "Image": "sha256:aaaa",
            "State": {"Running": true},
            "Config": {
                "Hostname": "0123456789ab",
                "Image": "redis:7-alpine",
                "Env": ["CACHE_VERSION=1", "PATH=/usr/bin"],
                "Cmd": ["redis-server"],
                "User": "999",
                "Labels": {
                    "dev.ciphera.managed": "true",
                    "com.docker.compose.project": "shop-production",
                    "com.docker.compose.service": "cache"
                }
            },
            "HostConfig": {
                "NetworkMode": "ciphera-p-x",
                "Memory": 268435456,
                "Mounts": [{"Type": "volume", "Source": "shop-production_data", "Target": "/data"}]
            },
            "Mounts": [
                {"Type": "volume", "Name": "shop-production_data", "Destination": "/data", "RW": true},
                {"Type": "volume", "Name": "3f2a", "Destination": "/cache", "RW": true}
            ],
            "NetworkSettings": {"Networks": {
                "other": {"Aliases": ["x"], "IPAddress": "10.0.0.3"},
                "ciphera-p-x": {
                    "Aliases": ["cache", "cache.internal", "0123456789ab"],
                    "IPAddress": "172.18.0.4",
                    "MacAddress": "02:42:ac:12:00:04"
                }
            }}
        }))
        .unwrap()
    }

    #[test]
    fn snapshot_keeps_creation_config_without_runtime_values() {
        let snap = snapshot_of(compose_inspect()).unwrap();
        assert_eq!(snap.id.0, "0123456789abcdef0123");
        assert_eq!(snap.name, "shop-production-cache-1");
        assert_eq!(snap.image, "sha256:aaaa");
        assert!(snap.running);
        let body = &snap.body;
        assert_eq!(body.hostname, None, "nom d'hote attribue par Docker");
        assert_eq!(body.image.as_deref(), Some("redis:7-alpine"));
        assert_eq!(
            body.env.as_deref(),
            Some(&["CACHE_VERSION=1".to_string(), "PATH=/usr/bin".to_string()][..])
        );
        assert_eq!(body.user.as_deref(), Some("999"));
        assert_eq!(
            body.labels
                .as_ref()
                .unwrap()
                .get("com.docker.compose.service"),
            Some(&"cache".to_string())
        );
        let host = body.host_config.as_ref().unwrap();
        assert_eq!(host.memory, Some(268_435_456));
        let mounts = host.mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 2, "{mounts:?}");
        assert_eq!(mounts[1].source.as_deref(), Some("3f2a"), "volume anonyme");
        assert_eq!(mounts[1].target.as_deref(), Some("/cache"));
        let endpoints = body
            .networking_config
            .as_ref()
            .unwrap()
            .endpoints_config
            .as_ref()
            .unwrap();
        let main = endpoints.get("ciphera-p-x").expect("reseau principal");
        assert_eq!(
            main.aliases.as_deref(),
            Some(&["cache".to_string(), "cache.internal".to_string()][..])
        );
        assert_eq!(main.ip_address, None);
        assert_eq!(main.mac_address, None);
        assert_eq!(snap.networks.len(), 1);
        assert_eq!(snap.networks[0].0, "other");
        let shown = format!("{snap:?}");
        assert!(!shown.contains("CACHE_VERSION"), "{shown}");
    }

    #[test]
    fn snapshot_refuses_unmanaged_container() {
        let mut info = compose_inspect();
        info.config.as_mut().unwrap().labels = Some(HashMap::new());
        assert!(matches!(snapshot_of(info), Err(DockerError::NotManaged(_))));
    }
}
