//! Deploiement sans interruption, annulation et retrait des deploiements.
//!
//! `ControlToAgent::Deploy` (etapes rapportees par `DeployProgress`) :
//!
//! 1. `Start` : le nouveau conteneur demarre sur le reseau de son projet
//!    `ciphera-p-<project_id>` (projet Compose : voir `compose`), a cote de
//!    l'ancien ;
//! 2. `Healthcheck` : `GET <chemin>` sur son adresse du reseau du projet,
//!    `tries` essais (5 par defaut), chacun apres `interval_secs` secondes
//!    (5 par defaut) ; sans port expose, le conteneur doit etre en marche ;
//! 3. `Switch` : il devient le deploiement servi de son service et la table
//!    du proxy est recalculee (upstream bascule, voir `routes`) ;
//! 4. l'ancien deploiement servi du service est arrete apres `drain`
//!    (10 s), le temps que ses requetes en cours se terminent.
//!
//! Un echec a `Start`, `Healthcheck` ou `Switch` supprime le nouveau
//! conteneur ; l'ancien continue de servir, les routes ne changent pas, et
//! l'etape est rapportee `Failed`.
//!
//! Projet Compose, dans cet ordre :
//! 1. `Start` lance les services nouveaux (sans conteneur dans le projet
//!    Compose stable) et ceux dont le service expose depend (`depends_on`),
//!    dans l'ordre de `depends_on`, puis le service expose ;
//! 2. `Healthcheck` sonde le service expose ;
//! 3. sonde reussie : `Switch` met a jour les autres services, puis bascule.
//!
//! Avant de relancer un service deja en place, l'agent relit par `inspect`
//! la configuration de creation de ses conteneurs (`ContainerSnapshot`),
//! gardee en memoire seulement (elle porte l'env : jamais ecrite).
//!
//! Sonde en echec, autre echec ou annulation avant la bascule : les
//! services nouveaux lances par ce deploiement sont retires, et les
//! services deja en place qu'il a relances sont restaures (nouveaux
//! conteneurs supprimes, anciens recrees a l'identique puis demarres dans
//! l'ordre de `depends_on`, journal « services dependants restaures ») :
//! l'ensemble servi reste intact. Docker refuse : la restauration reste a
//! faire (reprise par l'appel suivant, `RemoveDeployment` sans accuse
//! jusque-la) et l'issue du deploiement le dit (`NOT_RESTORED`). Une
//! restauration ne touche jamais un service repris entre-temps : ensemble
//! servi change (bascule d'un autre deploiement) ou autre deploiement du
//! meme projet Compose en cours ou en attente sur ce service ; elle est
//! alors abandonnee (journal). Mises a jour, restaurations et bascules d'un
//! meme projet Compose stable passent une a une (verrou par projet).
//! Les services sans lien avec le service expose ne sont
//! jamais touches avant la reussite de la sonde.
//! Les services deja en place relances sont notes (noms seuls) dans l'etat
//! local jusqu'a l'issue : si l'agent s'arrete avant, leur configuration
//! d'avant est perdue et le demarrage suivant met le deploiement en echec
//! (`NOT_RESTORED`) sans le relancer.
//! Les services nouveaux sont aussi notes dans l'etat local jusqu'a la
//! bascule : si l'agent s'arrete avant l'issue, le demarrage suivant les
//! retire (`sweep`). Un retrait qui echoue garde la note (reprise). La
//! bascule d'un deploiement Compose oublie, dans la transaction qui le rend
//! servi, toute note visant un service de son ensemble ; et un service de
//! l'ensemble d'un deploiement servi n'est jamais retire (garde finale).
//!
//! La bascule exige une table de routes du plan de controle recue depuis le
//! demarrage (sinon `Switch` echoue, « routes not synced ») : l'ancien
//! deploiement n'est jamais arrete sans que la table qui contient le nouvel
//! upstream soit partie vers le proxy.
//!
//! `Stop` pendant un build ou un deploiement, avant la bascule, l'interrompt
//! (processus tues) et nettoie : dossier du build, conteneur `railpack
//! prepare`, nouveau conteneur, images du deploiement ; rapporte
//! `Cancelled`. Une fois la bascule commencee, `Stop` arrive trop tard : il
//! est ignore (le drainage de l'ancien continue) et l'issue de la bascule est
//! rapportee de nouveau ; de meme pour le deploiement servi. Sur un autre
//! deploiement termine, `Stop` arrete ses conteneurs. `RemoveDeployment`
//! supprime conteneurs, images et ensemble Compose d'un deploiement
//! (retention, suppression d'un projet) puis l'accuse par
//! `DeploymentRemoved` ; si Docker echoue, l'etat local est garde, sans
//! accuse, et l'ordre suivant recommence. Au demarrage et toutes les
//! heures, les conteneurs et images de deploiements absents de l'etat local
//! sont supprimes (`sweep_orphans`). `Restart` redemarre les
//! conteneurs du deploiement servi (et son ensemble Compose).
//!
//! `RemoveProject` (suppression d'un projet) : le projet est marque retire
//! (tout nouvel ordre de build ou de deploiement pour lui est refuse), ses
//! builds et deploiements en cours sont annules et nettoyes, puis ses
//! conteneurs (etiquette `dev.ciphera.project`), son etat local, ses images
//! de deploiement, avec `purge_volumes` ses volumes Compose, et son reseau
//! `ciphera-p-<id>` sont supprimes. Un balayage horaire retire les reseaux
//! de projet sans conteneur dont le projet n'est plus connu de l'agent.
//!
//! Livraison au moins une fois : l'issue definitive de chaque build et
//! deploiement est gardee dans l'etat local ; un ordre rejoue pour un
//! deploiement conclu la rapporte de nouveau sans rien refaire. Un `Deploy`
//! arrive pendant le build du meme deploiement attend sa fin
//! (`BUILD_WAIT` au plus, sinon `Start` echoue) ; un ordre en double d'une
//! operation en cours est signale dans le journal du deploiement
//! (l'operation en cours rapporte son issue). `State` donne a chaque
//! connexion les deploiements servis, les operations en cours et les issues
//! recentes.

pub mod compose;
pub mod probe;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proto::{
    AgentState, AgentToControl, BuildSpec, DeploySpec, DeploymentOutcome, LogStream, Route,
    RunningDeployment, Stage, StageStatus,
};
use shared::ids::{DeploymentId, ProjectId};
use tokio::sync::mpsc;

use self::compose::{
    COMPOSE_LABEL, COMPOSE_PROJECT_LABEL, COMPOSE_SERVICE_LABEL, ComposePlan, ComposeRunner,
    PROJECT_LABEL,
};
use self::probe::{HttpProbe, valid_healthcheck};
use crate::build::argv::ValidatedArg;
use crate::build::{self, BuildBackend, BuildConfig, BuildError, ProgressSink};
use crate::docker::{
    ContainerId, ContainerSnapshot, ContainerSpec, DEPLOYMENT_LABEL, DockerError, DockerOps,
    PREPARE_ROLE, ROLE_LABEL, image_deployment, network_project, project_network,
};
use crate::ops::{Busy, Cancel, OpKind, Operations, Ticket};
use crate::routes::{RouteError, RouteSink, Router};
use crate::state::{DeploymentRecord, StateDb, StateError};

/// Drainage de l'ancien deploiement apres la bascule.
pub const DRAIN: Duration = Duration::from_secs(10);
/// Attente au plus d'un build en cours par un `Deploy` du meme deploiement.
pub const BUILD_WAIT: Duration = Duration::from_secs(60);
/// Intervalle du balayage des reseaux de projet vides.
pub const NETWORK_SWEEP_EVERY: Duration = Duration::from_secs(3600);
/// Message d'un ordre recu pour un projet supprime.
const PROJECT_REMOVED: &str = "projet supprime de ce serveur : ordre refuse";
/// Journal d'un deploiement Compose en echec ou annule : les services deja
/// en place qu'il avait relances sont revenus a leur etat servi.
pub const RESTORED: &str = "services dépendants restaurés";
/// Issue d'un deploiement Compose interrompu par un arret de l'agent apres
/// la mise a jour de services deja en place (configuration d'avant perdue).
pub const NOT_RESTORED: &str = "services dépendants non restaurés : redéployez la version servie";
/// Dossiers transitoires des deploiements Compose (fichier donne a
/// `docker compose`), sous le StateDirectory 0700 de l'agent.
pub const DEFAULT_DEPLOYS_DIR: &str = "/var/lib/ciphera-agent/deploys";

/// Reglages des deploiements.
#[derive(Clone, Debug)]
pub struct DeployConfig {
    /// Dossier des builds (nettoye a l'annulation et au demarrage).
    pub builds_dir: String,
    /// Dossier des deploiements Compose.
    pub deploys_dir: String,
    pub drain: Duration,
    /// Ecoute locale du panneau (`AgentConfig::panel_upstream`), seul amont
    /// admis pour la route du panneau.
    pub panel_upstream: Option<SocketAddr>,
}

impl DeployConfig {
    pub fn production() -> Self {
        Self {
            builds_dir: build::DEFAULT_BUILDS_DIR.to_string(),
            deploys_dir: DEFAULT_DEPLOYS_DIR.to_string(),
            drain: DRAIN,
            panel_upstream: None,
        }
    }
}

#[derive(Debug)]
pub enum DeployError {
    /// Ordre invalide (healthcheck, service Compose...).
    Invalid(String),
    Docker(DockerError),
    State(StateError),
    Compose(String),
    Healthcheck(String),
    Routes(RouteError),
    /// Dossier du deploiement (fichier Compose).
    Workdir(String),
}

impl std::fmt::Display for DeployError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) => write!(f, "ordre de deploiement invalide : {m}"),
            Self::Docker(e) => write!(f, "{e}"),
            Self::State(e) => write!(f, "{e}"),
            Self::Compose(m) => write!(f, "Compose : {m}"),
            Self::Healthcheck(m) => write!(f, "healthcheck en echec : {m}"),
            Self::Routes(e) => write!(f, "{e}"),
            Self::Workdir(m) => write!(f, "dossier du deploiement : {m}"),
        }
    }
}

impl std::error::Error for DeployError {}

impl From<DockerError> for DeployError {
    fn from(e: DockerError) -> Self {
        Self::Docker(e)
    }
}

impl From<StateError> for DeployError {
    fn from(e: StateError) -> Self {
        Self::State(e)
    }
}

/// Conteneur demarre par l'etape `Start`.
struct Started {
    container: ContainerId,
}

/// Deploiement Compose en cours : ce qu'il reste a mettre a jour apres la
/// sonde, et ce qu'il a deja touche (retire ou signale s'il echoue ou est
/// annule avant la bascule).
struct ComposeRun {
    project: ProjectId,
    stable: ValidatedArg,
    file: ValidatedArg,
    dir: ValidatedArg,
    /// Services non exposes de l'ensemble (tous).
    services: Vec<String>,
    /// Services deja en place a mettre a jour apres la sonde.
    later: Vec<ValidatedArg>,
    /// Services nouveaux lances par ce deploiement.
    created: Vec<String>,
    /// Services deja en place relances par ce deploiement, dans l'ordre de
    /// `depends_on`.
    updated: Vec<Updated>,
    /// Deploiement en cours (faux : en echec, restauration a reprendre).
    active: bool,
}

/// Service deja en place relance par un deploiement Compose, et ses
/// conteneurs tels qu'ils etaient avant (memoire seulement).
struct Updated {
    service: String,
    before: Vec<ContainerSnapshot>,
    /// Deploiements servis du projet Compose stable a la lecture : une
    /// bascule depuis rend la restauration caduque.
    served: Vec<DeploymentId>,
}

/// Restauration d'un service : faite, abandonnee (service repris par un
/// autre deploiement, raison), ou a reprendre (echec de Docker ou de
/// l'etat local).
enum Restore {
    Done,
    Skipped(String),
    Pending,
}

/// Supprime un dossier s'il existe (sans suivre un lien symbolique).
fn remove_path(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            if let Err(e) = std::fs::remove_dir_all(path) {
                tracing::warn!("suppression de {} : {e}", path.display());
            }
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

#[cfg(unix)]
fn private_dir(path: &str) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn private_dir(path: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(unix)]
fn write_private(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// Deploiements de l'agent : Docker, etat local, table du proxy,
/// healthcheck HTTP et `docker compose`, tous injectes (simules dans les
/// tests).
pub struct Deployer<D, P, C, S: RouteSink> {
    docker: Arc<D>,
    db: StateDb,
    router: Router<S>,
    probe: P,
    compose: C,
    cfg: DeployConfig,
    ops: Operations,
    /// Projets retires (`RemoveProject`) depuis le demarrage : plus aucune
    /// operation n'y est lancee. Verrou tenu pendant l'inscription d'une
    /// operation, si bien qu'un retrait voit toutes celles deja lancees.
    removed: Mutex<HashSet<ProjectId>>,
    /// Deploiements Compose en cours, de `Start` a la bascule.
    compose_runs: Mutex<HashMap<DeploymentId, ComposeRun>>,
    /// Un verrou par projet Compose stable : mises a jour de ses services,
    /// restaurations et bascules passent une a une.
    stable_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl<D, P, C, S> Deployer<D, P, C, S>
where
    D: DockerOps,
    P: HttpProbe,
    C: ComposeRunner,
    S: RouteSink,
{
    pub fn new(
        docker: Arc<D>,
        db: StateDb,
        sink: S,
        probe: P,
        compose: C,
        cfg: DeployConfig,
    ) -> Self {
        Self {
            docker,
            db,
            router: Router::new(sink).with_panel(cfg.panel_upstream),
            probe,
            compose,
            cfg,
            ops: Operations::default(),
            removed: Mutex::new(HashSet::new()),
            compose_runs: Mutex::new(HashMap::new()),
            stable_locks: Mutex::new(HashMap::new()),
        }
    }

    /// Verrou du projet Compose stable `stable`.
    fn stable_lock(&self, stable: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.stable_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(stable.to_string())
            .or_default()
            .clone()
    }

    /// Services deja en place que le deploiement `id` n'a pas encore pu
    /// restaurer.
    fn unrestored(&self, id: DeploymentId) -> Vec<String> {
        self.compose_runs()
            .get(&id)
            .map(|r| r.updated.iter().map(|u| u.service.clone()).collect())
            .unwrap_or_default()
    }

    fn removed(&self) -> std::sync::MutexGuard<'_, HashSet<ProjectId>> {
        self.removed.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn compose_runs(&self) -> std::sync::MutexGuard<'_, HashMap<DeploymentId, ComposeRun>> {
        self.compose_runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lance l'operation `id`, sauf pour un projet retire (`None`).
    fn spawn_op<W, F>(
        &self,
        id: DeploymentId,
        kind: OpKind,
        project: Option<ProjectId>,
        work: W,
    ) -> Option<Result<(), Busy>>
    where
        W: FnOnce(Ticket) -> F,
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let removed = self.removed();
        if project.is_some_and(|p| removed.contains(&p)) {
            return None;
        }
        Some(self.ops.spawn_in(id, kind, project, work))
    }

    pub fn ops(&self) -> &Operations {
        &self.ops
    }

    /// Garde chaque table de routes du plan de controle dans `path`
    /// (reprise au demarrage, [`Deployer::restore_routes`]).
    pub fn with_routes_file(mut self, path: std::path::PathBuf) -> Self {
        self.router = self.router.with_store(path);
        self
    }

    /// Demarrage : table de routes gardee (ou deja recue) recalculee et
    /// envoyee au proxy ; `false` si aucune n'est connue.
    pub async fn restore_routes(&self) -> bool {
        self.router.restore(self.docker.as_ref(), &self.db).await
    }

    /// Etat envoye a chaque connexion : deploiements (sante relue dans
    /// Docker) et operations en cours.
    pub async fn state(&self) -> AgentState {
        crate::reconcile::state_fn(self.docker.clone(), self.db.clone(), self.ops.clone())().await
    }

    /// Nouvelle table de routes du plan de controle.
    pub async fn apply_routes(&self, routes: Vec<Route>) -> Result<(), RouteError> {
        self.router
            .set_desired(routes, self.docker.as_ref(), &self.db)
            .await
    }

    /// Meme destination que `sink` ; chaque etat definitif (voir
    /// `StageStatus::is_terminal`) est garde dans l'etat local avant
    /// l'envoi (message deja masque).
    fn recording(&self, sink: &ProgressSink) -> ProgressSink {
        let db = self.db.clone();
        let id = sink.deployment_id();
        sink.with_hook(Arc::new(move |stage, status| {
            if status.is_terminal(stage)
                && let Err(e) = db.put_outcome(id, stage, status)
            {
                tracing::error!("deploiement {id} : issue non enregistree : {e}");
            }
        }))
    }

    /// Issue definitive deja gardee pour `id`.
    fn outcome(&self, id: DeploymentId) -> Option<DeploymentOutcome> {
        self.db.outcome(id).unwrap_or_else(|e| {
            tracing::error!("deploiement {id} : issue illisible : {e}");
            None
        })
    }

    /// Deploiement en tache de fond (annulable par `stop` avant la
    /// bascule). Pendant le build du meme deploiement : attente de sa fin
    /// (`BUILD_WAIT` au plus, sinon `Start` echoue). En double d'un
    /// deploiement en cours : signale dans son journal.
    pub fn spawn_deploy(self: &Arc<Self>, spec: DeploySpec, outbox: mpsc::Sender<AgentToControl>) {
        let this = self.clone();
        tokio::spawn(async move { this.dispatch_deploy(spec, outbox).await });
    }

    async fn dispatch_deploy(
        self: Arc<Self>,
        spec: DeploySpec,
        outbox: mpsc::Sender<AgentToControl>,
    ) {
        let id = spec.deployment_id;
        let deadline = tokio::time::Instant::now() + BUILD_WAIT;
        loop {
            let (this, spec_, outbox_) = (self.clone(), spec.clone(), outbox.clone());
            let spawned = self.spawn_op(
                id,
                OpKind::Deploy,
                Some(spec.project_id),
                move |ticket| async move {
                    let sink = ProgressSink::new(id, outbox_);
                    if let Err(e) = this.run_op(&spec_, &sink, Some(ticket)).await {
                        tracing::warn!("deploiement {id} en echec : {e}");
                    }
                },
            );
            let busy = match spawned {
                Some(Ok(())) => return,
                Some(Err(busy)) => busy,
                None => {
                    tracing::info!("deploiement {id} : {PROJECT_REMOVED}");
                    ProgressSink::new(id, outbox)
                        .stage(
                            Stage::Start,
                            StageStatus::Failed {
                                message: PROJECT_REMOVED.into(),
                            },
                        )
                        .await;
                    return;
                }
            };
            if busy.kind == OpKind::Deploy {
                tracing::info!("deploiement {id} deja en cours : ordre en double ignore");
                ProgressSink::new(id, outbox)
                    .log(
                        LogStream::Stderr,
                        "deploiement deja en cours sur l'agent : ordre en double ignore",
                    )
                    .await;
                return;
            }
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() || !busy.wait(left).await {
                let message = format!(
                    "build du deploiement toujours en cours apres {} s : deploiement refuse",
                    BUILD_WAIT.as_secs()
                );
                tracing::warn!("deploiement {id} : {message}");
                self.recording(&ProgressSink::new(id, outbox))
                    .stage(Stage::Start, StageStatus::Failed { message })
                    .await;
                return;
            }
        }
    }

    /// Build en tache de fond (annulable par `stop`). La reussite n'est
    /// rapportee qu'une fois tout conclu : ensemble Compose et issue gardes,
    /// operation rendue (un `Deploy` qui suit trouve tout en place). Un
    /// build deja conclu rapporte son issue sans rien refaire.
    pub fn spawn_build<B: BuildBackend + 'static>(
        self: &Arc<Self>,
        spec: BuildSpec,
        cfg: BuildConfig,
        backend: Arc<B>,
        outbox: mpsc::Sender<AgentToControl>,
    ) {
        let id = spec.deployment_id;
        let project = spec.project_id;
        let this = self.clone();
        let log = outbox.clone();
        let spawned = self.spawn_op(id, OpKind::Build, project, move |ticket| async move {
            let sink = this.recording(&ProgressSink::new(id, outbox));
            if let Some(o) = this.outcome(id) {
                tracing::info!("build {id} deja conclu : issue rapportee de nouveau");
                sink.stage(o.stage, o.status).await;
                return;
            }
            let db = this.db.clone();
            let finalize = |built: &build::BuiltImages| {
                ticket
                    .finish(|| {
                        if let Some(bundle) = &built.compose {
                            db.put_bundle(id, bundle).map_err(|e| {
                                BuildError::Workdir(format!(
                                    "ensemble Compose non enregistre : {e}"
                                ))
                            })?;
                        }
                        db.put_outcome(id, Stage::Image, &StageStatus::Succeeded)
                            .map_err(|e| {
                                BuildError::Workdir(format!("issue non enregistree : {e}"))
                            })
                    })
                    .unwrap_or(Err(BuildError::Cancelled))
            };
            match build::run_with(&spec, &cfg, backend.as_ref(), &sink, finalize).await {
                Ok(built) => {
                    tracing::info!("build {id} : {} image(s) etiquetee(s)", built.images.len());
                }
                Err(BuildError::Cancelled) => tracing::info!("build {id} annule"),
                // Jamais le jeton Git : les erreurs du build le masquent.
                Err(e) => tracing::warn!("build {id} en echec : {e}"),
            }
        });
        let Some(spawned) = spawned else {
            tracing::info!("build {id} : {PROJECT_REMOVED}");
            tokio::spawn(async move {
                ProgressSink::new(id, log)
                    .stage(
                        Stage::Build,
                        StageStatus::Failed {
                            message: PROJECT_REMOVED.into(),
                        },
                    )
                    .await;
            });
            return;
        };
        if spawned.is_err() {
            tracing::info!("build {id} deja en cours : ordre en double ignore");
            tokio::spawn(async move {
                ProgressSink::new(id, log)
                    .log(
                        LogStream::Stderr,
                        "build deja en cours sur l'agent : ordre en double ignore",
                    )
                    .await;
            });
        }
    }

    /// Deploiement complet de `spec` (voir le module), etapes vers `sink`
    /// (sans operation enregistree : non annulable).
    pub async fn run(&self, spec: &DeploySpec, sink: &ProgressSink) -> Result<(), DeployError> {
        self.run_op(spec, sink, None).await
    }

    async fn run_op(
        &self,
        spec: &DeploySpec,
        sink: &ProgressSink,
        ticket: Option<Ticket>,
    ) -> Result<(), DeployError> {
        let sink = &self.recording(sink);
        let id = spec.deployment_id;
        if let Some(o) = self.outcome(id)
            && concludes_deploy(&o)
        {
            // Ordre rejoue : deploiement deja conclu.
            sink.stage(o.stage, o.status).await;
            return Ok(());
        }
        if let Some(r) = self.db.record(id)?
            && r.deployment.serving
        {
            // Ordre rejoue : deja servi.
            sink.stage(Stage::Switch, StageStatus::Succeeded).await;
            return Ok(());
        }
        sink.stage(Stage::Start, StageStatus::Started).await;
        let started = match self.start(spec, sink).await {
            Ok(s) => s,
            Err(e) => return self.fail(spec, Stage::Start, e, sink).await,
        };
        sink.stage(Stage::Start, StageStatus::Succeeded).await;

        sink.stage(Stage::Healthcheck, StageStatus::Started).await;
        if let Err(e) = self.healthcheck(spec, &started.container).await {
            return self.fail(spec, Stage::Healthcheck, e, sink).await;
        }
        let _ = self.db.set_healthy(id, true);
        sink.stage(Stage::Healthcheck, StageStatus::Succeeded).await;

        sink.stage(Stage::Switch, StageStatus::Started).await;
        // Sonde reussie : les autres services Compose, puis la bascule.
        if let Err(e) = self.update_later_services(id, &spec.env, sink).await {
            return self.fail(spec, Stage::Switch, e, sink).await;
        }
        // A partir d'ici, plus d'interruption : un `Stop` attend l'issue.
        if let Some(t) = &ticket
            && !t.enter_switch()
        {
            // Annule juste avant : le `Stop` nettoie et rapporte.
            return Ok(());
        }
        let previous = match self.switch(id).await {
            Ok(p) => p,
            Err(e) => return self.fail(spec, Stage::Switch, e, sink).await,
        };
        // Services Compose acquis au deploiement servi (notes oubliees par
        // la bascule, dans la transaction de `set_serving_compose`).
        self.compose_runs().remove(&id);
        remove_path(Path::new(&self.deploy_dir(id)));
        // Issue gardee avant de rendre la main (un `Stop` en attente la lit).
        let record = || {
            self.db
                .put_outcome(id, Stage::Switch, &StageStatus::Succeeded)
        };
        let recorded = match &ticket {
            Some(t) => t.finish(record).unwrap_or(Ok(())),
            None => record(),
        };
        if let Err(e) = recorded {
            tracing::error!("deploiement {id} : issue non enregistree : {e}");
        }
        sink.stage(Stage::Switch, StageStatus::Succeeded).await;

        if let Some(old) = previous {
            tokio::time::sleep(self.cfg.drain).await;
            self.stop_deployment_containers(old).await;
            sink.log(
                LogStream::Stdout,
                &format!("ancien deploiement {old} arrete apres le drainage"),
            )
            .await;
        }
        Ok(())
    }

    /// Echec d'une etape : nouveau conteneur et services Compose nouveaux
    /// supprimes, routes inchangees.
    async fn fail(
        &self,
        spec: &DeploySpec,
        stage: Stage,
        e: DeployError,
        sink: &ProgressSink,
    ) -> Result<(), DeployError> {
        self.discard(spec.deployment_id).await;
        self.undo_compose(spec.deployment_id, Some(sink)).await;
        remove_path(Path::new(&self.deploy_dir(spec.deployment_id)));
        let mut message = e.to_string();
        let unrestored = self.unrestored(spec.deployment_id);
        if !unrestored.is_empty() {
            message = format!("{message} ; {NOT_RESTORED} ({})", unrestored.join(", "));
        }
        sink.stage(stage, StageStatus::Failed { message }).await;
        Err(e)
    }

    async fn start(&self, spec: &DeploySpec, sink: &ProgressSink) -> Result<Started, DeployError> {
        if !valid_healthcheck(&spec.healthcheck) {
            return Err(DeployError::Invalid("healthcheck".into()));
        }
        if spec.internal_port == Some(0) {
            return Err(DeployError::Invalid("port 0".into()));
        }
        if let Some(bad) = spec.env.iter().find(|v| !valid_env_key(&v.key)) {
            // La cle seule : jamais la valeur.
            return Err(DeployError::Invalid(format!(
                "variable d'environnement {:?}",
                bad.key
            )));
        }
        if spec
            .run_as
            .as_deref()
            .is_some_and(|u| !proto::valid_run_as(u))
        {
            return Err(DeployError::Invalid("utilisateur (run_as)".into()));
        }
        self.docker
            .ensure_network(&project_network(spec.project_id))
            .await?;
        let container = match &spec.compose {
            None => {
                let env = spec
                    .env
                    .iter()
                    .map(|v| (v.key.clone(), v.value.expose().to_string()))
                    .collect();
                let mut app =
                    ContainerSpec::app(spec.deployment_id, spec.project_id, &spec.image, env)
                        .with_run_as(spec.run_as.as_deref())
                        .with_read_only_rootfs(spec.read_only_rootfs);
                if let Some(name) = &spec.service_name {
                    app = app.with_service_alias(name);
                }
                self.docker.run(&app).await?
            }
            // Dossier du fichier Compose garde jusqu'a la bascule (mise a
            // jour des autres services) ; retire par la bascule ou l'echec.
            Some(c) => self.start_compose(spec, &c.service, c.bundle, sink).await?,
        };
        let runs_as_root = self.docker.runs_as_root(&container).await.unwrap_or(false);
        self.db.put_record(&DeploymentRecord {
            deployment: RunningDeployment {
                deployment_id: spec.deployment_id,
                service_id: spec.service_id,
                image: spec.image.clone(),
                healthy: false,
                serving: false,
                runs_as_root,
            },
            container: container.clone(),
            port: spec.internal_port,
            project: Some(spec.project_id),
        })?;
        Ok(Started { container })
    }

    fn deploy_dir(&self, id: DeploymentId) -> String {
        format!("{}/{id}", self.cfg.deploys_dir)
    }

    async fn start_compose(
        &self,
        spec: &DeploySpec,
        service: &str,
        bundle: DeploymentId,
        sink: &ProgressSink,
    ) -> Result<ContainerId, DeployError> {
        let text = self.db.bundle(bundle)?.ok_or_else(|| {
            DeployError::Compose(format!("ensemble du deploiement {bundle} absent"))
        })?;
        let value = serde_json::from_str(&text)
            .map_err(|_| DeployError::Compose("ensemble illisible".into()))?;
        let mut plan =
            ComposePlan::new(&value, service, spec.project_id, spec.deployment_id, bundle)
                .map_err(DeployError::Compose)?;
        plan.secure_exposed(spec.run_as.as_deref(), spec.read_only_rootfs)
            .map_err(DeployError::Compose)?;
        // Cles seules dans le fichier ; valeurs dans l'environnement de
        // `docker compose` (voir `deploy::compose`).
        plan.inject_env(spec.env.iter().map(|v| v.key.as_str()))
            .map_err(DeployError::Compose)?;
        let dir = self.deploy_dir(spec.deployment_id);
        let file = format!("{dir}/compose.json");
        let invalid = |e: crate::build::argv::InvalidArg| DeployError::Workdir(e.to_string());
        let dir_arg = ValidatedArg::path(&dir).map_err(invalid)?;
        let file_arg = ValidatedArg::path(&file).map_err(invalid)?;
        remove_path(Path::new(&dir));
        private_dir(&dir).map_err(|e| DeployError::Workdir(e.to_string()))?;
        let json =
            serde_json::to_vec(&plan.file).map_err(|e| DeployError::Compose(e.to_string()))?;
        write_private(&file, &json).map_err(|e| DeployError::Workdir(e.to_string()))?;
        // Avant la sonde : services nouveaux et dependances du service
        // expose, dans l'ordre de depends_on ; les autres apres la sonde.
        let existing = self.compose_services(&plan.stable, spec.project_id).await?;
        let (now, later): (Vec<ValidatedArg>, Vec<ValidatedArg>) =
            plan.order.iter().cloned().partition(|s| {
                !existing.contains(s.as_str()) || plan.exposed_needs.contains(s.as_str())
            });
        let id = spec.deployment_id;
        self.compose_runs().insert(
            id,
            ComposeRun {
                project: spec.project_id,
                stable: plan.stable.clone(),
                file: file_arg.clone(),
                dir: dir_arg.clone(),
                services: plan.order.iter().map(|s| s.as_str().to_string()).collect(),
                later,
                created: Vec::new(),
                updated: Vec::new(),
                active: true,
            },
        );
        for svc in &now {
            self.up_tracked(id, svc, existing.contains(svc.as_str()), &spec.env, sink)
                .await?;
        }
        self.compose
            .up(
                &plan.exposed_project,
                &file_arg,
                &plan.exposed,
                &dir_arg,
                &spec.env,
                sink,
            )
            .await
            .map_err(DeployError::Compose)?;
        let found = self
            .docker
            .list_managed(&[
                (COMPOSE_PROJECT_LABEL, plan.exposed_project.as_str()),
                (COMPOSE_SERVICE_LABEL, plan.exposed.as_str()),
                (DEPLOYMENT_LABEL, &spec.deployment_id.to_string()),
            ])
            .await?;
        found
            .into_iter()
            .next()
            .map(|c| c.id)
            .ok_or_else(|| DeployError::Compose("conteneur du service expose introuvable".into()))
    }

    /// Essais espaces de `interval_secs` : HTTP sur l'adresse du reseau du
    /// projet si un port est expose, sinon conteneur en marche.
    async fn healthcheck(
        &self,
        spec: &DeploySpec,
        container: &ContainerId,
    ) -> Result<(), DeployError> {
        let h = &spec.healthcheck;
        let interval = Duration::from_secs(u64::from(h.interval_secs));
        let mut last = String::from("aucun essai");
        for _ in 0..h.tries {
            tokio::time::sleep(interval).await;
            let attempt = match spec.internal_port {
                Some(port) => match self.docker.project_address(container).await {
                    Ok(Some(ip)) => self.probe.check(SocketAddr::new(ip, port), &h.path).await,
                    Ok(None) => Err("pas d'adresse sur le reseau du projet".to_string()),
                    Err(e) => Err(e.to_string()),
                },
                None => match self.docker.health(container).await {
                    Ok(health) if health.is_healthy() => Ok(()),
                    Ok(health) => Err(format!("conteneur {health:?}")),
                    Err(e) => Err(e.to_string()),
                },
            };
            match attempt {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(DeployError::Healthcheck(format!(
            "{} essai(s) : {last}",
            h.tries
        )))
    }

    /// Bascule : `id` devient le deploiement servi de son service, table du
    /// proxy recalculee. Renvoie l'ancien deploiement servi. En cas
    /// d'echec, l'ancien redevient servi.
    async fn switch(&self, id: DeploymentId) -> Result<Option<DeploymentId>, DeployError> {
        let compose = self
            .compose_runs()
            .get(&id)
            .map(|r| (r.stable.as_str().to_string(), r.services.clone()));
        // Jamais pendant une restauration du meme projet Compose.
        let lock = compose.as_ref().map(|(s, _)| self.stable_lock(s));
        let _guard = match &lock {
            Some(l) => Some(l.lock().await),
            None => None,
        };
        let previous = self.db.set_serving_compose(
            id,
            compose.as_ref().map(|(s, v)| (s.as_str(), v.as_slice())),
        )?;
        // Les services de cet ensemble ne sont plus « nouveaux » pour aucun
        // autre deploiement en cours ou en echec.
        if let Some((stable, services)) = &compose {
            for (other, run) in self.compose_runs().iter_mut() {
                if *other != id && run.stable.as_str() == stable {
                    run.created.retain(|s| !services.contains(s));
                    run.updated.retain(|u| !services.contains(&u.service));
                }
            }
        }
        if let Err(e) = self.router.refresh(self.docker.as_ref(), &self.db).await {
            match previous {
                Some(old) => {
                    let _ = self.db.set_serving(old);
                }
                None => {
                    let _ = self.db.clear_serving(id);
                }
            }
            return Err(DeployError::Routes(e));
        }
        Ok(previous)
    }

    /// Supprime les conteneurs du deploiement `id` (jamais ceux d'un
    /// deploiement servi) et l'oublie.
    async fn discard(&self, id: DeploymentId) {
        if self
            .db
            .record(id)
            .ok()
            .flatten()
            .is_some_and(|r| r.deployment.serving)
        {
            return;
        }
        for c in self.containers_of(id).await {
            if let Err(e) = self.docker.remove(&c).await {
                tracing::warn!("deploiement {id} : suppression de {} : {e}", c.0);
            }
        }
        if let Err(e) = self.db.remove_deployment(id) {
            tracing::error!("{e}");
        }
    }

    /// Conteneurs des services non exposes du projet Compose stable `stable`
    /// du projet `project` (sans deploiement), `service` seul si donne.
    async fn compose_service_containers(
        &self,
        stable: &str,
        project: ProjectId,
        service: Option<&str>,
    ) -> Result<Vec<crate::docker::ManagedContainer>, DockerError> {
        let project = project.to_string();
        let mut labels = vec![(COMPOSE_LABEL, stable), (PROJECT_LABEL, project.as_str())];
        if let Some(s) = service {
            labels.push((COMPOSE_SERVICE_LABEL, s));
        }
        let list = self.docker.list_managed(&labels).await?;
        Ok(list
            .into_iter()
            .filter(|c| c.labels.get(DEPLOYMENT_LABEL).is_none_or(String::is_empty))
            .collect())
    }

    /// Services non exposes qui ont deja un conteneur (en marche ou non)
    /// dans le projet Compose stable `stable`.
    async fn compose_services(
        &self,
        stable: &ValidatedArg,
        project: ProjectId,
    ) -> Result<HashSet<String>, DeployError> {
        Ok(self
            .compose_service_containers(stable.as_str(), project, None)
            .await?
            .into_iter()
            .filter_map(|c| c.labels.get(COMPOSE_SERVICE_LABEL).cloned())
            .collect())
    }

    /// `docker compose up` d'un service non expose du deploiement Compose
    /// `id`, note avant le lancement (une annulation pendant `up` le
    /// retrouve) : mis a jour s'il etait `existing` (configuration de ses
    /// conteneurs relue avant, pour la restauration), sinon nouveau. Le
    /// service est aussi note dans l'etat local : apres un arret de l'agent
    /// avant l'issue, le demarrage retire un service nouveau, et met en
    /// echec un deploiement qui avait mis a jour un service deja en place
    /// ([`Deployer::sweep_interrupted_compose`]).
    async fn up_tracked(
        &self,
        id: DeploymentId,
        service: &ValidatedArg,
        existing: bool,
        env: &[proto::EnvVar],
        sink: &ProgressSink,
    ) -> Result<(), DeployError> {
        let (project, stable, file, dir) = {
            let runs = self.compose_runs();
            let run = runs
                .get(&id)
                .ok_or_else(|| DeployError::Compose("deploiement Compose inconnu".into()))?;
            (
                run.project,
                run.stable.clone(),
                run.file.clone(),
                run.dir.clone(),
            )
        };
        let name = service.as_str().to_string();
        // Jamais pendant une restauration ou une bascule du meme projet.
        let lock = self.stable_lock(stable.as_str());
        let _guard = lock.lock().await;
        if existing {
            let known = self
                .compose_runs()
                .get(&id)
                .is_some_and(|r| r.updated.iter().any(|u| u.service == name));
            if !known {
                // Relue avant tout changement ; un echec de Docker ici
                // arrete le deploiement avant que le service soit touche.
                let before = self
                    .snapshot_service(project, stable.as_str(), &name)
                    .await?;
                let served = self.db.compose_serving_deployments(stable.as_str())?;
                if let Some(run) = self.compose_runs().get_mut(&id) {
                    run.updated.push(Updated {
                        service: name.clone(),
                        before,
                        served,
                    });
                }
                self.db.put_compose_updated(id, stable.as_str(), &name)?;
            }
        } else {
            self.db
                .put_compose_created(id, project, stable.as_str(), &name)?;
            if let Some(run) = self.compose_runs().get_mut(&id)
                && !run.created.contains(&name)
            {
                run.created.push(name);
            }
        }
        self.compose
            .up(&stable, &file, service, &dir, env, sink)
            .await
            .map_err(DeployError::Compose)
    }

    /// Configuration de creation des conteneurs du service non expose
    /// `service` du projet Compose stable `stable` (memoire seulement).
    async fn snapshot_service(
        &self,
        project: ProjectId,
        stable: &str,
        service: &str,
    ) -> Result<Vec<ContainerSnapshot>, DeployError> {
        let mut out = Vec::new();
        for c in self
            .compose_service_containers(stable, project, Some(service))
            .await?
        {
            if let Some(snap) = self.docker.snapshot(&c.id).await? {
                out.push(snap);
            }
        }
        Ok(out)
    }

    /// Garde avant de restaurer le service `u` du deploiement `id` (verrou
    /// du projet tenu) : la raison de ne pas y toucher, s'il a ete repris
    /// (ensemble servi change, ou autre deploiement du meme projet Compose
    /// en cours ou qui l'a lance ou mis a jour). `Err` : etat illisible.
    fn restore_blocked(
        &self,
        id: DeploymentId,
        stable: &str,
        u: &Updated,
    ) -> Result<Option<String>, StateError> {
        for (other, run) in self.compose_runs().iter() {
            if *other == id || run.stable.as_str() != stable {
                continue;
            }
            if run.active || self.ops.is_running(*other) {
                return Ok(Some(format!("deploiement {other} en cours")));
            }
            if run.created.contains(&u.service)
                || run.updated.iter().any(|x| x.service == u.service)
            {
                return Ok(Some(format!("repris par le deploiement {other}")));
            }
        }
        let served = self.db.compose_serving_deployments(stable)?;
        if served != u.served {
            return Ok(Some("ensemble servi change depuis".into()));
        }
        Ok(None)
    }

    /// Remet le service `u` du projet Compose stable `stable` tel qu'il
    /// etait avant le deploiement `id` : conteneurs inconnus d'avant
    /// supprimes, conteneurs d'avant disparus recrees a l'identique (et
    /// demarres s'ils tournaient), conteneurs d'avant restes en place
    /// ramenes a leur etat (en marche ou arrete). `false` si Docker a
    /// echoue : ce qui reste a faire est garde dans `u`.
    async fn restore_service(
        &self,
        id: DeploymentId,
        project: ProjectId,
        stable: &str,
        u: &mut Updated,
    ) -> bool {
        let current = match self
            .compose_service_containers(stable, project, Some(&u.service))
            .await
        {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(
                    "deploiement {id} : service Compose {} illisible : {e}",
                    u.service
                );
                return false;
            }
        };
        let mut complete = true;
        for c in &current {
            if u.before.iter().any(|b| b.id == c.id) {
                continue;
            }
            if let Err(e) = self.docker.remove(&c.id).await {
                tracing::warn!(
                    "service Compose {} : suppression de {} : {e}",
                    u.service,
                    c.name
                );
                complete = false;
            }
        }
        if !complete {
            return false;
        }
        for snap in &mut u.before {
            let kept = current.iter().find(|c| c.id == snap.id);
            let done = match kept {
                Some(c) if snap.running && c.stopped => self.docker.restart(&c.id).await,
                Some(c) if !snap.running && !c.stopped => self.docker.stop(&c.id).await,
                Some(_) => Ok(()),
                None => self.docker.recreate(snap).await.map(|new| {
                    // Recree : une reprise ne le recree pas une seconde fois.
                    snap.id = new;
                }),
            };
            if let Err(e) = done {
                tracing::warn!(
                    "service Compose {} : restauration de {} : {e}",
                    u.service,
                    snap.name
                );
                complete = false;
            }
        }
        complete
    }

    /// Apres la sonde : mise a jour des services non exposes que `Start`
    /// n'a pas relances, dans l'ordre de `depends_on` (rien hors Compose).
    async fn update_later_services(
        &self,
        id: DeploymentId,
        env: &[proto::EnvVar],
        sink: &ProgressSink,
    ) -> Result<(), DeployError> {
        let later = match self.compose_runs().get(&id) {
            Some(run) => run.later.clone(),
            None => return Ok(()),
        };
        for svc in &later {
            self.up_tracked(id, svc, true, env, sink).await?;
        }
        Ok(())
    }

    /// Deploiement Compose `id` en echec ou annule avant la bascule : les
    /// services nouveaux qu'il a lances sont retires ; ceux deja en place
    /// qu'il a relances sont restaures dans l'ordre de `depends_on`
    /// (`restore_service`) et nommes dans son journal (`sink`). `false` si
    /// Docker a echoue : ce qui reste a faire reste note (memoire et etat
    /// local), un appel suivant reprend.
    async fn undo_compose(&self, id: DeploymentId, sink: Option<&ProgressSink>) -> bool {
        let Some(mut run) = self.compose_runs().remove(&id) else {
            return true;
        };
        let mut left = Vec::new();
        for service in &run.created {
            if !self
                .drop_created(id, run.project, run.stable.as_str(), service)
                .await
            {
                left.push(service.clone());
            }
        }
        let mut restored = Vec::new();
        let mut skipped = Vec::new();
        let mut pending = Vec::new();
        let lock = self.stable_lock(run.stable.as_str());
        for mut u in std::mem::take(&mut run.updated) {
            let outcome = {
                let _guard = lock.lock().await;
                match self.restore_blocked(id, run.stable.as_str(), &u) {
                    Ok(Some(reason)) => Restore::Skipped(reason),
                    Ok(None) => {
                        if self
                            .restore_service(id, run.project, run.stable.as_str(), &mut u)
                            .await
                        {
                            Restore::Done
                        } else {
                            Restore::Pending
                        }
                    }
                    Err(e) => {
                        tracing::error!("deploiement {id} : service Compose {} : {e}", u.service);
                        Restore::Pending
                    }
                }
            };
            match outcome {
                Restore::Pending => pending.push(u),
                done => {
                    if let Err(e) = self.db.remove_compose_updated(id, Some(&u.service)) {
                        tracing::warn!("deploiement {id} : service Compose {} : {e}", u.service);
                    }
                    match done {
                        Restore::Skipped(reason) => {
                            skipped.push(format!("{} ({reason})", u.service));
                        }
                        _ => restored.push(u.service),
                    }
                }
            }
        }
        if !skipped.is_empty() {
            let message = format!(
                "services dependants laisses tels quels, repris entre-temps : {}",
                skipped.join(", ")
            );
            tracing::warn!("deploiement {id} : {message}");
            if let Some(sink) = sink {
                sink.log(LogStream::Stderr, &message).await;
            }
        }
        if !restored.is_empty() {
            let message = format!("{RESTORED} : {}", restored.join(", "));
            tracing::info!("deploiement {id} : {message}");
            if let Some(sink) = sink {
                sink.log(LogStream::Stdout, &message).await;
            }
        }
        if !pending.is_empty() {
            let names: Vec<&str> = pending.iter().map(|u| u.service.as_str()).collect();
            let message = format!(
                "services dependants pas encore restaures (Docker), nouvel essai : {}",
                names.join(", ")
            );
            tracing::warn!("deploiement {id} : {message}");
            if let Some(sink) = sink {
                sink.log(LogStream::Stderr, &message).await;
            }
        }
        if left.is_empty() && pending.is_empty() {
            if let Err(e) = self.db.remove_compose_updated(id, None) {
                tracing::warn!("deploiement {id} : services Compose mis a jour : {e}");
            }
            return true;
        }
        run.created = left;
        run.updated = pending;
        run.later.clear();
        run.active = false;
        self.compose_runs().insert(id, run);
        false
    }

    /// Note « service nouveau » `(id, service)` : le service est retire puis
    /// la note oubliee ; `true` si c'est fait. Garde finale : un service de
    /// l'ensemble d'un deploiement servi n'est jamais retire (note
    /// seulement oubliee). Etat illisible ou echec de Docker : rien n'est
    /// oublie, `false`.
    async fn drop_created(
        &self,
        id: DeploymentId,
        project: ProjectId,
        stable: &str,
        service: &str,
    ) -> bool {
        match self.db.compose_service_served(stable, service) {
            Ok(true) => {
                tracing::info!(
                    "deploiement {id} : service Compose {service} servi par un autre deploiement : garde"
                );
            }
            Ok(false) => {
                if !self
                    .remove_compose_service(id, project, stable, service)
                    .await
                {
                    return false;
                }
            }
            Err(e) => {
                tracing::error!("deploiement {id} : service Compose {service} garde : {e}");
                return false;
            }
        }
        if let Err(e) = self.db.remove_compose_created(id, Some(service)) {
            tracing::warn!("deploiement {id} : service Compose {service} : {e}");
        }
        true
    }

    /// Notes « service nouveau » de `id` restees dans l'etat local (sans
    /// suivi en memoire : apres un redemarrage) : traitees comme par
    /// `undo_compose` ; `false` si l'une reste.
    async fn drop_created_notes(&self, id: DeploymentId) -> bool {
        let entries = match self.db.compose_created() {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("deploiement {id} : services Compose nouveaux illisibles : {e}");
                return false;
            }
        };
        let mut complete = true;
        for (_, project, stable, service) in entries.into_iter().filter(|e| e.0 == id) {
            complete &= self.drop_created(id, project, &stable, &service).await;
        }
        complete
    }

    /// Retire les conteneurs du service non expose `service` du projet
    /// Compose stable `stable` ; `false` si Docker a echoue.
    async fn remove_compose_service(
        &self,
        id: DeploymentId,
        project: ProjectId,
        stable: &str,
        service: &str,
    ) -> bool {
        let list = match self
            .compose_service_containers(stable, project, Some(service))
            .await
        {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("deploiement {id} : service Compose {service} illisible : {e}");
                return false;
            }
        };
        let mut complete = true;
        for c in list {
            if let Err(e) = self.docker.remove(&c.id).await {
                tracing::warn!("service Compose {} : suppression : {e}", c.name);
                complete = false;
            }
        }
        complete
    }

    /// Au demarrage (aucune operation en cours) : services Compose nouveaux
    /// lances par un deploiement que l'arret de l'agent a interrompu avant
    /// son issue, retires (en marche ou non). Ceux d'un deploiement servi
    /// lui appartiennent : seulement oublies. Sur un echec de Docker, la
    /// note reste (reprise au prochain demarrage). Un tel deploiement qui
    /// avait mis a jour un service deja en place ne peut plus le restaurer
    /// (configuration perdue avec la memoire) : il est mis en echec
    /// (`NOT_RESTORED`), un ordre rejoue rapporte cette issue sans rien
    /// relancer.
    async fn sweep_interrupted_compose(&self) {
        self.fail_interrupted_updates();
        let entries = match self.db.compose_created() {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("services Compose interrompus illisibles : {e}");
                return;
            }
        };
        for (id, project, stable, service) in entries {
            if self.ops.is_running(id) || self.compose_runs().contains_key(&id) {
                continue;
            }
            if self.is_serving(id) {
                if let Err(e) = self.db.remove_compose_created(id, Some(&service)) {
                    tracing::warn!("deploiement {id} : service Compose {service} : {e}");
                }
                continue;
            }
            if self.drop_created(id, project, &stable, &service).await {
                tracing::info!("service Compose {service} du deploiement interrompu {id} traite");
            }
        }
    }

    /// Notes « service mis a jour » sans suivi en memoire (deploiement
    /// interrompu par un arret de l'agent) : deploiement mis en echec
    /// (`NOT_RESTORED`) s'il n'est pas servi, puis notes oubliees.
    fn fail_interrupted_updates(&self) {
        let entries = match self.db.compose_updated() {
            Ok(e) => e,
            Err(e) => {
                tracing::error!("services Compose mis a jour illisibles : {e}");
                return;
            }
        };
        let mut ids: Vec<DeploymentId> = entries.into_iter().map(|e| e.0).collect();
        ids.dedup();
        for id in ids {
            if self.ops.is_running(id) || self.compose_runs().contains_key(&id) {
                continue;
            }
            if !self.is_serving(id) {
                tracing::error!("deploiement {id} interrompu : {NOT_RESTORED}");
                let failed = StageStatus::Failed {
                    message: NOT_RESTORED.into(),
                };
                // Remplace une issue deja gardee (echec, annulation) : la
                // restauration perdue ne doit jamais passer inapercue.
                if let Err(e) = self.db.put_outcome_forced(id, Stage::Start, &failed) {
                    tracing::error!("deploiement {id} : issue non enregistree : {e}");
                    continue;
                }
            }
            if let Err(e) = self.db.remove_compose_updated(id, None) {
                tracing::warn!("deploiement {id} : services Compose mis a jour : {e}");
            }
        }
    }

    async fn containers_of(&self, id: DeploymentId) -> Vec<ContainerId> {
        let label = id.to_string();
        match self
            .docker
            .list_managed(&[(DEPLOYMENT_LABEL, label.as_str())])
            .await
        {
            Ok(list) => list.into_iter().map(|c| c.id).collect(),
            Err(e) => {
                tracing::warn!("deploiement {id} : conteneurs illisibles : {e}");
                Vec::new()
            }
        }
    }

    async fn stop_deployment_containers(&self, id: DeploymentId) {
        for c in self.containers_of(id).await {
            if let Err(e) = self.docker.stop(&c).await {
                tracing::warn!("deploiement {id} : arret de {} : {e}", c.0);
            }
        }
        let _ = self.db.set_healthy(id, false);
    }

    /// `ControlToAgent::Stop`. Operation en cours avant la bascule :
    /// interrompue, nettoyee, `Cancelled` rapporte. Bascule commencee ou
    /// deploiement servi : trop tard, ignore, l'issue est rapportee de
    /// nouveau (le drainage de l'ancien continue). Autre deploiement
    /// termine : ses conteneurs sont arretes.
    pub async fn stop(&self, id: DeploymentId, outbox: &mpsc::Sender<AgentToControl>) {
        let sink = self.recording(&ProgressSink::new(id, outbox.clone()));
        // Annulation gardee sous le verrou du registre, avant que le creneau
        // soit libere : un Deploy qui attendait le build, ou un ordre rejoue
        // pendant le nettoyage, trouve l'issue et ne relance rien.
        let decide = |kind: OpKind| {
            let prior = self
                .outcome(id)
                .filter(|o| kind == OpKind::Build || concludes_deploy(o));
            if prior.is_none()
                && let Err(e) = self
                    .db
                    .put_outcome(id, kind.stage(), &StageStatus::Cancelled)
            {
                tracing::error!("deploiement {id} : annulation non enregistree : {e}");
            }
            prior
        };
        match self.ops.cancel(id, decide).await {
            Cancel::Cancelled(_, Some(o)) => {
                // Operation conclue juste avant l'interruption : son issue
                // fait foi.
                sink.stage(o.stage, o.status).await;
            }
            Cancel::Cancelled(kind, None) => {
                self.undo_compose(id, Some(&sink)).await;
                self.cleanup(id).await;
                let unrestored = self.unrestored(id);
                if unrestored.is_empty() {
                    sink.stage(kind.stage(), StageStatus::Cancelled).await;
                } else {
                    // Annule, mais l'ensemble servi n'est pas revenu : dit
                    // dans l'issue (qui remplace l'annulation gardee).
                    let failed = StageStatus::Failed {
                        message: format!(
                            "deploiement annule ; {NOT_RESTORED} ({})",
                            unrestored.join(", ")
                        ),
                    };
                    if let Err(e) = self.db.put_outcome_forced(id, kind.stage(), &failed) {
                        tracing::error!("deploiement {id} : issue non enregistree : {e}");
                    }
                    sink.stage(kind.stage(), failed).await;
                }
                tracing::info!("deploiement {id} annule");
            }
            Cancel::TooLate => {
                tracing::info!("deploiement {id} : arret demande apres la bascule, ignore");
                if let Some(o) = self.outcome(id) {
                    sink.stage(o.stage, o.status).await;
                }
            }
            Cancel::Idle if self.is_serving(id) => {
                tracing::info!("deploiement {id} servi : arret trop tard, ignore");
                sink.stage(Stage::Switch, StageStatus::Succeeded).await;
            }
            Cancel::Idle => self.stop_deployment_containers(id).await,
        }
    }

    fn is_serving(&self, id: DeploymentId) -> bool {
        self.db
            .record(id)
            .ok()
            .flatten()
            .is_some_and(|r| r.deployment.serving)
    }

    /// Restes d'une operation interrompue : conteneurs du deploiement
    /// (nouveau conteneur, bac a sable de `railpack prepare`), images,
    /// dossiers du build et du deploiement, ensemble Compose.
    async fn cleanup(&self, id: DeploymentId) {
        self.undo_compose(id, None).await;
        self.discard(id).await;
        if let Err(e) = self.docker.remove_deployment_images(id).await {
            tracing::warn!("deploiement {id} : images : {e}");
        }
        remove_path(Path::new(&format!("{}/{id}", self.cfg.builds_dir)));
        remove_path(Path::new(&self.deploy_dir(id)));
        let _ = self.db.remove_bundle(id);
    }

    /// `ControlToAgent::RemoveDeployment` : operation en cours interrompue,
    /// conteneurs, images, ensemble Compose et etat local supprimes. Le
    /// deploiement servi d'un projet Compose emporte aussi les services non
    /// exposes de son projet Compose. Routes recalculees s'il etait servi.
    ///
    /// Renvoie `true` si tout a ete supprime ou etait deja absent : l'etat
    /// local est alors oublie. Si Docker echoue (liste, conteneur ou image),
    /// l'etat local est garde et `false` renvoye : le prochain ordre refait
    /// la suppression.
    pub async fn remove(&self, id: DeploymentId) -> bool {
        let _ = self.ops.cancel(id, |_| ()).await;
        let compose_undone = self.undo_compose(id, None).await && self.drop_created_notes(id).await;
        let serving = self.is_serving(id);
        let label = id.to_string();
        let containers = match self
            .docker
            .list_managed(&[(DEPLOYMENT_LABEL, label.as_str())])
            .await
        {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(
                    "deploiement {id} : conteneurs illisibles, suppression reportee : {e}"
                );
                return false;
            }
        };
        let mut doomed: Vec<ContainerId> = containers.iter().map(|c| c.id.clone()).collect();
        if serving {
            doomed.extend(self.compose_siblings(id, &containers).await);
        }
        let mut complete = compose_undone;
        for c in &doomed {
            if let Err(e) = self.docker.remove(c).await {
                tracing::warn!("deploiement {id} : suppression de {} : {e}", c.0);
                complete = false;
            }
        }
        if let Err(e) = self.docker.remove_deployment_images(id).await {
            tracing::warn!("deploiement {id} : images : {e}");
            complete = false;
        }
        if complete {
            if let Err(e) = self
                .db
                .remove_deployment(id)
                .and(self.db.remove_bundle(id))
                .and(self.db.remove_compose_updated(id, None))
            {
                tracing::error!("deploiement {id} : etat local : {e}");
                complete = false;
            }
            remove_path(Path::new(&format!("{}/{id}", self.cfg.builds_dir)));
            remove_path(Path::new(&self.deploy_dir(id)));
        } else {
            tracing::warn!("deploiement {id} : suppression incomplete, etat local garde");
        }
        if serving && let Err(e) = self.router.refresh(self.docker.as_ref(), &self.db).await {
            tracing::warn!("{e}");
        }
        complete
    }

    /// [`Deployer::remove`] puis, s'il a abouti, accuse
    /// `AgentToControl::DeploymentRemoved` : le plan de controle ne tient le
    /// deploiement pour nettoye qu'a cet accuse.
    pub async fn remove_acked(&self, id: DeploymentId, outbox: &mpsc::Sender<AgentToControl>) {
        if self.remove(id).await {
            let _ = outbox
                .send(AgentToControl::DeploymentRemoved { deployment_id: id })
                .await;
        }
    }

    /// Balayage des restes (au demarrage et toutes les heures), sur la foi
    /// du seul etat local, donc prudent :
    /// - seuls les conteneurs **arretes** etiquetes d'un deploiement absent
    ///   de l'etat local (ni enregistre, ni en cours) sont supprimes ; un
    ///   conteneur en marche n'est jamais touche (journalise) : le plan de
    ///   controle, qui fait foi, le retire a la reconciliation s'il le faut ;
    /// - images de deploiement dont l'etat local ne garde rien (ni
    ///   enregistrement, ni issue, ni ensemble Compose, ni operation) et
    ///   qu'aucun conteneur restant n'utilise ;
    /// - etat local sans aucun deploiement alors que des conteneurs
    ///   etiquetes existent (etat neuf ou recree, sauvegarde ancienne) :
    ///   rien n'est supprime, une alerte est journalisee ;
    /// - etat local illisible : rien n'est supprime.
    pub async fn sweep_orphans(&self) {
        self.sweep_orphans_in(None).await;
    }

    /// [`Deployer::sweep_orphans`] limite aux conteneurs du projet `project`
    /// et aux images de leurs deploiements. Pour les tests sur un Docker
    /// partage seulement.
    #[doc(hidden)]
    pub async fn sweep_orphans_of_project(&self, project: ProjectId) {
        self.sweep_orphans_in(Some(project)).await;
    }

    async fn sweep_orphans_in(&self, project: Option<ProjectId>) {
        // Docker lu avant l'etat local : un objet cree ensuite par une
        // operation n'est pas dans la liste, un objet liste dont l'operation
        // a commence entre-temps est vu par `in_progress`.
        let project_label = project.map(|p| p.to_string());
        let scope: Vec<(&str, &str)> = project_label
            .as_deref()
            .map(|p| vec![(PROJECT_LABEL, p)])
            .unwrap_or_default();
        let containers = match self.docker.list_managed(&scope).await {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!("balayage des restes : conteneurs illisibles : {e}");
                return;
            }
        };
        // `project` : seules les images des conteneurs supprimes sont visees.
        let images = match project {
            Some(_) => Vec::new(),
            None => match self.docker.image_deployments().await {
                Ok(ids) => ids,
                Err(e) => {
                    tracing::warn!("balayage des restes : images illisibles : {e}");
                    Vec::new()
                }
            },
        };
        // Operations lues avant l'etat local : une operation terminee entre
        // les deux lectures a deja ecrit son enregistrement ou son issue.
        let in_progress = self.ops.in_progress();
        let records = match self.db.deployments() {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("balayage des restes saute, etat local illisible : {e}");
                return;
            }
        };
        let (Ok(outcomes), Ok(bundles)) = (self.db.outcomes(), self.db.bundle_ids()) else {
            tracing::error!("balayage des restes saute, etat local illisible");
            return;
        };
        let labelled: Vec<(DeploymentId, &crate::docker::ManagedContainer)> = containers
            .iter()
            .filter_map(|c| container_deployment(c).map(|id| (id, c)))
            .collect();
        if records.is_empty() && !labelled.is_empty() {
            tracing::error!(
                "balayage des restes saute : etat local sans aucun deploiement alors que {} \
                 conteneur(s) de deploiement existent (etat neuf, recree ou restaure ?) ; \
                 rien n'est supprime, le plan de controle reconcilie",
                labelled.len()
            );
            return;
        }
        let recorded: std::collections::HashSet<DeploymentId> = records
            .iter()
            .map(|r| r.deployment.deployment_id)
            .chain(in_progress.iter().copied())
            .collect();
        let mut known = recorded.clone();
        known.extend(outcomes.iter().map(|o| o.deployment_id));
        known.extend(bundles);

        let mut doomed_images: Vec<DeploymentId> = Vec::new();
        let mut removed: std::collections::HashSet<&ContainerId> = std::collections::HashSet::new();
        // Projets dont le retrait (`RemoveProject`) est en cours ou a
        // reprendre : leurs conteneurs (et volumes) relevent de `remove_project`.
        let retiring: HashSet<ProjectId> = self.removed().clone();
        for (id, c) in &labelled {
            let id = *id;
            if recorded.contains(&id) || self.ops.is_running(id) {
                continue;
            }
            if c.labels
                .get(PROJECT_LABEL)
                .and_then(|p| p.parse::<ProjectId>().ok())
                .is_some_and(|p| retiring.contains(&p))
            {
                continue;
            }
            if !c.stopped {
                tracing::warn!(
                    "conteneur {} du deploiement {id} absent de l'etat local mais en marche : \
                     garde (retire par le plan de controle s'il le faut)",
                    c.name
                );
                continue;
            }
            tracing::info!(
                "conteneur arrete {} du deploiement {id} absent de l'etat local : supprime",
                c.name
            );
            match self.docker.remove(&c.id).await {
                Ok(()) => {
                    removed.insert(&c.id);
                    if project.is_some() && !known.contains(&id) && !doomed_images.contains(&id) {
                        doomed_images.push(id);
                    }
                }
                Err(e) => tracing::warn!("{} : {e}", c.name),
            }
        }
        // Deploiements dont une image sert encore a un conteneur restant (par
        // son image ou son etiquette) : leurs images restent.
        let mut in_use: std::collections::HashSet<DeploymentId> = std::collections::HashSet::new();
        for c in containers.iter().filter(|c| !removed.contains(&c.id)) {
            in_use.extend(image_deployment(&c.image));
            in_use.extend(container_deployment(c));
        }
        for id in images {
            if !known.contains(&id) && !doomed_images.contains(&id) {
                doomed_images.push(id);
            }
        }
        for id in doomed_images {
            if in_use.contains(&id) || self.ops.is_running(id) {
                continue;
            }
            tracing::info!("images du deploiement {id} absent de l'etat local : supprimees");
            if let Err(e) = self.docker.remove_deployment_images(id).await {
                tracing::warn!("deploiement {id} : images : {e}");
            }
        }
    }

    /// `ControlToAgent::RemoveProject` : plus aucune operation du projet
    /// n'est lancee ; celles en cours sont annulees (un build en cours est
    /// interrompu, son buildkitd libere) et nettoyees ; puis conteneurs du
    /// projet (etiquette `dev.ciphera.project`), etat local et images de ses
    /// deploiements, avec `purge_volumes` ses volumes Compose, et son reseau
    /// sont supprimes. Routes recalculees si un deploiement servi part.
    /// L'etat local n'est efface qu'une fois tout retire de Docker : sur un
    /// echec, il reste (le retrait reprend au prochain ordre).
    /// Rejouable : `Ok` quand plus rien ne reste. L'etat de buildkitd du
    /// projet est supprime par l'appelant, apres (`BuildkitManager`).
    pub async fn remove_project(
        &self,
        project: ProjectId,
        purge_volumes: bool,
    ) -> Result<(), DeployError> {
        self.removed().insert(project);
        for id in self.ops.of_project(project) {
            if let Cancel::Cancelled(..) = self.ops.cancel(id, |_| ()).await {
                self.cleanup(id).await;
                tracing::info!("projet {project} supprime : operation {id} annulee");
            }
        }
        // Restaurations en attente du projet : sans objet, le projet part
        // (configurations gardees en memoire oubliees avec elles).
        let pending: Vec<DeploymentId> = self
            .compose_runs()
            .iter()
            .filter(|(_, r)| r.project == project)
            .map(|(id, _)| *id)
            .collect();
        for id in pending {
            self.compose_runs().remove(&id);
            let _ = self.db.remove_compose_updated(id, None);
        }
        // Docker d'abord : sur un echec (conteneur, volumes, reseau), l'etat
        // local du projet reste et l'erreur empeche l'accuse ; l'ordre
        // revient a la reconnexion et reprend tout.
        let label = project.to_string();
        let mut failure: Option<DeployError> = None;
        for c in self
            .docker
            .list_managed(&[(PROJECT_LABEL, label.as_str())])
            .await?
        {
            // Avec la purge, les volumes anonymes des conteneurs aussi.
            let removed = if purge_volumes {
                self.docker.remove_with_volumes(&c.id).await
            } else {
                self.docker.remove(&c.id).await
            };
            if let Err(e) = removed {
                tracing::warn!("projet {project} : suppression de {} : {e}", c.name);
                failure = Some(e.into());
            }
        }
        if let Some(e) = failure {
            return Err(e);
        }
        if purge_volumes {
            let n = self.docker.remove_project_volumes(project).await?;
            tracing::info!("projet {project} : {n} volume(s) supprime(s)");
        }
        self.docker
            .remove_network(&project_network(project))
            .await?;
        let mut serving = false;
        for r in self
            .db
            .deployments()?
            .into_iter()
            .filter(|r| r.project == Some(project))
        {
            let id = r.deployment.deployment_id;
            serving |= r.deployment.serving;
            if let Err(e) = self.docker.remove_deployment_images(id).await {
                tracing::warn!("deploiement {id} : images : {e}");
            }
            self.db.remove_deployment(id)?;
            let _ = self.db.remove_bundle(id);
            let _ = self.db.remove_compose_updated(id, None);
        }
        if serving && let Err(e) = self.router.refresh(self.docker.as_ref(), &self.db).await {
            tracing::warn!("{e}");
        }
        tracing::info!("projet {project} supprime de ce serveur");
        Ok(())
    }

    /// Balayage : reseaux de projet (`ciphera-p-<id>`) sans conteneur dont
    /// le projet n'a ni deploiement dans l'etat local ni operation en cours
    /// (projet supprime alors que l'agent etait absent, restes) ; leur
    /// nombre. Docker refuse de retirer un reseau encore utilise.
    pub async fn sweep_networks(&self) -> usize {
        let networks = match self.docker.project_networks().await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("reseaux de projet illisibles : {e}");
                return 0;
            }
        };
        let known: HashSet<ProjectId> = match self.db.deployments() {
            Ok(list) => list.into_iter().filter_map(|r| r.project).collect(),
            Err(e) => {
                tracing::warn!("balayage des reseaux saute : {e}");
                return 0;
            }
        };
        let mut removed = 0;
        for name in networks {
            let Some(project) = network_project(&name) else {
                continue;
            };
            if known.contains(&project) || !self.ops.of_project(project).is_empty() {
                continue;
            }
            let label = project.to_string();
            match self
                .docker
                .list_managed(&[(PROJECT_LABEL, label.as_str())])
                .await
            {
                Ok(list) if list.is_empty() => {}
                _ => continue,
            }
            match self.docker.remove_network(&name).await {
                Ok(()) => {
                    tracing::info!("reseau de projet vide {name} supprime");
                    removed += 1;
                }
                Err(e) => tracing::warn!("reseau de projet {name} : {e}"),
            }
        }
        removed
    }

    /// Services non exposes de l'ensemble Compose du deploiement `id` : les
    /// conteneurs du projet `project` (celui de l'etat local du deploiement,
    /// jamais une etiquette de conteneur) et de l'ensemble stable `stable`
    /// (celui des conteneurs `containers`) qui ne portent aucun deploiement.
    /// Les conteneurs exposes des autres deploiements (anciens, gardes pour
    /// le retour arriere) portent les memes etiquettes de projet et
    /// d'ensemble : ils ne sont jamais touches.
    async fn compose_siblings(
        &self,
        id: DeploymentId,
        containers: &[crate::docker::ManagedContainer],
    ) -> Vec<ContainerId> {
        let Some(project) = self.db.record(id).ok().flatten().and_then(|r| r.project) else {
            return Vec::new();
        };
        let project = project.to_string();
        let mut out: Vec<ContainerId> = Vec::new();
        for c in containers {
            let Some(stable) = c.labels.get(COMPOSE_LABEL).filter(|s| !s.is_empty()) else {
                continue;
            };
            if c.labels.get(PROJECT_LABEL) != Some(&project) {
                continue;
            }
            if let Ok(others) = self
                .docker
                .list_managed(&[
                    (COMPOSE_LABEL, stable.as_str()),
                    (PROJECT_LABEL, project.as_str()),
                ])
                .await
            {
                for o in others {
                    let unowned = o.labels.get(DEPLOYMENT_LABEL).is_none_or(String::is_empty);
                    if unowned && !containers.iter().any(|c| c.id == o.id) && !out.contains(&o.id) {
                        out.push(o.id);
                    }
                }
            }
        }
        out
    }

    /// `ControlToAgent::Restart` : redemarre les conteneurs du deploiement
    /// servi `id` et les autres services de son ensemble Compose, puis
    /// recalcule la table du proxy (Docker peut changer leur adresse).
    /// Deploiement non servi, ou operation en cours : ignore.
    pub async fn restart(&self, id: DeploymentId) {
        if !self.is_serving(id) || self.ops.is_running(id) {
            tracing::info!("deploiement {id} non servi : redemarrage ignore");
            return;
        }
        let targets = self.serving_set(id).await;
        for c in &targets {
            if let Err(e) = self.docker.restart(c).await {
                tracing::warn!("deploiement {id} : redemarrage de {} : {e}", c.0);
            }
        }
        tracing::info!("deploiement {id} redemarre ({} conteneurs)", targets.len());
        if let Err(e) = self.router.refresh(self.docker.as_ref(), &self.db).await {
            tracing::warn!("{e}");
        }
    }

    /// Conteneurs du deploiement `id` et autres services de son ensemble
    /// Compose (ceux que `Restart` et `StopContainers` visent).
    async fn serving_set(&self, id: DeploymentId) -> Vec<ContainerId> {
        let label = id.to_string();
        let containers = self
            .docker
            .list_managed(&[(DEPLOYMENT_LABEL, label.as_str())])
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("deploiement {id} : conteneurs illisibles : {e}");
                Vec::new()
            });
        let mut targets: Vec<ContainerId> = self.compose_siblings(id, &containers).await;
        targets.extend(containers.into_iter().map(|c| c.id));
        targets
    }

    /// `ControlToAgent::StopContainers` : arrete les conteneurs du
    /// deploiement servi `id` et les autres services de son ensemble
    /// Compose, sans le retirer (il reste servi : `restart` le relance), puis
    /// recalcule la table du proxy (plus d'adresse, plus d'upstream).
    /// Deploiement non servi, ou operation en cours : ignore.
    pub async fn stop_containers(&self, id: DeploymentId) {
        if !self.is_serving(id) || self.ops.is_running(id) {
            tracing::info!("deploiement {id} non servi : arret ignore");
            return;
        }
        let targets = self.serving_set(id).await;
        for c in &targets {
            if let Err(e) = self.docker.stop(c).await {
                tracing::warn!("deploiement {id} : arret de {} : {e}", c.0);
            }
        }
        let _ = self.db.set_healthy(id, false);
        tracing::info!("deploiement {id} arrete ({} conteneurs)", targets.len());
        if let Err(e) = self.router.refresh(self.docker.as_ref(), &self.db).await {
            tracing::warn!("{e}");
        }
    }

    /// Au demarrage (aucune operation en cours) : restes d'un arret brutal,
    /// conteneurs `railpack prepare`, dossiers de build et de deploiement,
    /// services Compose nouveaux d'un deploiement interrompu.
    /// Bacs a sable reconnus par leurs etiquettes reservees (gere, role
    /// `prepare`, deploiement valide, hors de tout projet Compose), jamais
    /// par leur nom.
    pub async fn sweep(&self) {
        match self
            .docker
            .list_managed(&[(ROLE_LABEL, PREPARE_ROLE)])
            .await
        {
            Ok(list) => {
                let sandbox = |c: &&crate::docker::ManagedContainer| {
                    c.labels.get(ROLE_LABEL).map(String::as_str) == Some(PREPARE_ROLE)
                        && c.labels
                            .get(DEPLOYMENT_LABEL)
                            .is_some_and(|d| d.parse::<DeploymentId>().is_ok())
                        && !c.labels.contains_key(COMPOSE_PROJECT_LABEL)
                };
                for c in list.iter().filter(sandbox) {
                    tracing::info!("bac a sable abandonne {} supprime", c.name);
                    if let Err(e) = self.docker.remove(&c.id).await {
                        tracing::warn!("{} : {e}", c.name);
                    }
                }
            }
            Err(e) => tracing::warn!("nettoyage au demarrage : {e}"),
        }
        for dir in [&self.cfg.builds_dir, &self.cfg.deploys_dir] {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                tracing::info!("dossier abandonne {} supprime", entry.path().display());
                remove_path(&entry.path());
            }
        }
        self.sweep_interrupted_compose().await;
    }
}

/// Cle d'une variable d'environnement : `^[A-Z_][A-Z0-9_]{0,127}$`.
pub fn valid_env_key(key: &str) -> bool {
    let b = key.as_bytes();
    (1..=128).contains(&b.len())
        && (b[0].is_ascii_uppercase() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// Deploiement de l'etiquette `dev.ciphera.deployment` d'un conteneur, sous
/// sa forme canonique seulement (celle que pose l'agent).
fn container_deployment(c: &crate::docker::ManagedContainer) -> Option<DeploymentId> {
    let label = c.labels.get(DEPLOYMENT_LABEL)?;
    label
        .parse::<DeploymentId>()
        .ok()
        .filter(|id| id.to_string() == *label)
}

/// L'issue conclut le deploiement (pas seulement son build) : echec ou
/// annulation a toute etape, ou bascule reussie.
fn concludes_deploy(o: &DeploymentOutcome) -> bool {
    match o.status {
        StageStatus::Failed { .. } | StageStatus::Cancelled => true,
        StageStatus::Succeeded => o.stage == Stage::Switch,
        StageStatus::Started => false,
    }
}
