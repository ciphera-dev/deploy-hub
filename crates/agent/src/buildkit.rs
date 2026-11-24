//! Un buildkitd par projet, demarre a la demande par le build.
//!
//! - conteneur `ciphera-buildkitd-<12 hex de l'UUID du projet>` (`infra::
//!   buildkit_spec`), meme identite (`ciphera-build`), memes limites et meme
//!   `buildkitd.toml` pour tous les projets ;
//! - etat dans `<etat>/p/<uuid>` (0700, `ciphera-build`) et socket dans
//!   `<sockets>/<uuid>` (setgid au groupe `ciphera-agent`, comme le dossier
//!   parent) : l'agent n'a pas la main sur ces dossiers (ils sont au compte
//!   `ciphera-build`), un conteneur jetable (`DockerFs`) les cree et les
//!   supprime sous le bon compte ;
//! - un seul build a la fois sur le noeud (`build::BUILD_SLOTS`, tenu par
//!   l'appelant) : avant de demarrer le buildkitd d'un projet, celui d'un
//!   autre projet est arrete (jamais un buildkitd qui sert un build) ;
//! - un buildkitd inactif depuis `IDLE_TIMEOUT` est arrete ; son etat reste ;
//! - l'etat d'un projet est supprime avec le projet (`remove_project`) et
//!   quand il n'a pas servi depuis `STALE_AFTER` (balayage horaire). Le
//!   dernier usage est un fichier `<uuid>` du dossier `buildkit_usage_dir`
//!   (l'etat lui-meme est illisible pour l'agent) ;
//! - au demarrage : buildkitd de projets restes en marche arretes, restes des
//!   conteneurs jetables supprimes, etat du buildkitd partage d'avant
//!   supprime (une fois).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, RemoveContainerOptions,
};
use futures_util::StreamExt;
use shared::ids::ProjectId;

use crate::docker::{
    BollardDocker, ContainerId, DockerError, DockerOps, Health, MANAGED_LABEL, ROLE_LABEL,
};
use crate::infra::{self, BUILDKIT_PROJECT_LABEL, BUILDKITD_ROLE, InfraConfig, InfraError};

/// Un buildkitd sans build depuis ce delai est arrete.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Un etat sans build depuis ce delai est supprime.
pub const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);
/// Intervalle de la surveillance de l'inactivite.
pub const IDLE_CHECK_EVERY: Duration = Duration::from_secs(60);
/// Intervalle du balayage des etats perimes.
pub const SWEEP_EVERY: Duration = Duration::from_secs(3600);
/// Attente du socket d'un buildkitd demarre.
pub const SOCKET_WAIT: Duration = Duration::from_secs(120);
/// Delai d'un conteneur jetable (creation ou suppression de dossiers).
const HELPER_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Octets d'un chemin de socket Unix (`sun_path` : 108, NUL compris).
const MAX_SOCKET_PATH: usize = 107;
/// Caracteres gardes par ligne du journal de buildkitd rapporte a l'agent.
const MAX_LOG_CHARS: usize = 300;
/// Role du conteneur jetable (balaye au demarrage).
const HELPER_ROLE: &str = "buildkit-helper";
/// Marque : l'etat du buildkitd partage d'avant a ete supprime.
const LEGACY_MARKER: &str = ".legacy-state-removed";

#[derive(Debug)]
pub enum BuildkitError {
    Infra(InfraError),
    Docker(DockerError),
    /// Dossiers du projet (creation, suppression) ou fichier d'usage.
    Fs(String),
    /// Socket absent ou muet (etat et journal du conteneur : journal de
    /// l'agent seulement, jamais dans ce message).
    SocketTimeout(PathBuf),
    /// Un autre projet a un build en cours sur son buildkitd (sans le
    /// nommer : le message finit dans le journal d'un autre projet).
    Busy,
    /// Docker refuse de demarrer buildkitd sous son profil AppArmor
    /// (installe mais pas charge, ou refuse) : buildkitd n'est pas demarre,
    /// jamais de repli sans profil.
    AppArmor {
        profile: String,
        detail: String,
    },
}

impl fmt::Display for BuildkitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Infra(e) => write!(f, "{e}"),
            Self::Docker(e) => write!(f, "{e}"),
            Self::Fs(m) => write!(f, "etat de buildkitd : {m}"),
            Self::SocketTimeout(_) => {
                write!(
                    f,
                    "buildkitd du projet n'a pas demarre (voir le journal de l'agent)"
                )
            }
            Self::Busy => write!(f, "un autre build utilise deja buildkitd"),
            Self::AppArmor { profile, detail } => write!(
                f,
                "profil AppArmor {profile} non charge ou refuse : buildkitd n'est pas demarre \
                 (aucun repli sans profil) ; le recharger en root \
                 (`apparmor_parser -r -W {}`) ou relancer install.sh ({detail})",
                infra::BUILDKIT_APPARMOR_FILE
            ),
        }
    }
}

impl std::error::Error for BuildkitError {}

impl From<InfraError> for BuildkitError {
    fn from(e: InfraError) -> Self {
        Self::Infra(e)
    }
}

impl From<DockerError> for BuildkitError {
    fn from(e: DockerError) -> Self {
        Self::Docker(e)
    }
}

/// Dossiers d'un projet, au compte `ciphera-build`.
pub trait ProjectFs: Send + Sync + 'static {
    /// Cree l'etat (0700) et le dossier du socket (setgid) du projet.
    fn create(&self, project: ProjectId) -> impl Future<Output = Result<(), BuildkitError>> + Send;
    /// Supprime l'etat et le dossier du socket du projet.
    fn remove(&self, project: ProjectId) -> impl Future<Output = Result<(), BuildkitError>> + Send;
    /// Supprime l'etat du buildkitd partage d'avant (tout sauf `p`).
    fn remove_legacy_state(&self) -> impl Future<Output = Result<(), BuildkitError>> + Send;
}

/// Utilisation en cours des buildkitd, partagee avec les jetons.
#[derive(Default)]
struct Usage {
    in_use: HashMap<ProjectId, usize>,
    last_used: HashMap<ProjectId, Instant>,
    provisioned: HashSet<ProjectId>,
    /// Projets dont le buildkitd a ete demarre par ce processus et pas encore
    /// arrete : seuls a arreter (les autres le sont deja, voir `startup`).
    running: HashSet<ProjectId>,
}

fn locked(usage: &Mutex<Usage>) -> MutexGuard<'_, Usage> {
    usage.lock().unwrap_or_else(|e| e.into_inner())
}

/// Date de dernier usage : fichier `path`, cree s'il manque.
fn touch(path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.set_modified(SystemTime::now())
}

/// Un build tient le buildkitd de son projet : tant que le jeton vit, il
/// n'est ni arrete pour inactivite ni remplace par celui d'un autre projet.
struct UseGuard {
    usage: Arc<Mutex<Usage>>,
    project: ProjectId,
    marker: PathBuf,
}

impl Drop for UseGuard {
    fn drop(&mut self) {
        {
            let mut u = locked(&self.usage);
            if let Some(n) = u.in_use.get_mut(&self.project) {
                *n = n.saturating_sub(1);
            }
            u.last_used.insert(self.project, Instant::now());
        }
        if let Err(e) = touch(&self.marker) {
            tracing::warn!("usage de l'etat de buildkitd : {e}");
        }
    }
}

/// buildkitd pret pour un build : son socket, et le jeton d'utilisation.
pub struct BuildkitLease {
    socket: Option<PathBuf>,
    _guard: Option<UseGuard>,
}

impl BuildkitLease {
    /// Sans buildkitd propre au projet : le socket de `BuildConfig` est
    /// utilise tel quel.
    pub fn shared() -> Self {
        Self {
            socket: None,
            _guard: None,
        }
    }

    /// Socket du buildkitd du projet.
    pub fn socket(&self) -> Option<&Path> {
        self.socket.as_deref()
    }
}

/// Le socket accepte une connexion (buildkitd ecoute) ; un fichier de socket
/// reste d'un buildkitd arrete n'est pas pret.
pub fn socket_ready(path: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::net::UnixStream::connect(path).is_ok()
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

pub struct BuildkitManager<D: DockerOps, F: ProjectFs> {
    docker: Arc<D>,
    fs: F,
    cfg: InfraConfig,
    usage: Arc<Mutex<Usage>>,
    /// Une operation a la fois (demarrage, arret, suppression).
    op: tokio::sync::Mutex<()>,
    started: tokio::sync::OnceCell<()>,
    socket_wait: Duration,
    ready: fn(&Path) -> bool,
    /// Longueur du chemin du socket verifiee (fausse avec un test de
    /// disponibilite simule : les chemins des tests sont longs).
    check_path: bool,
    /// Intervalle de verification de l'etat du conteneur pendant l'attente.
    health_every: Duration,
}

/// Gestionnaire de production.
pub type ProductionBuildkit = BuildkitManager<BollardDocker, DockerFs>;

impl<D: DockerOps, F: ProjectFs> BuildkitManager<D, F> {
    pub fn new(docker: Arc<D>, fs: F, cfg: InfraConfig) -> Self {
        Self {
            docker,
            fs,
            cfg,
            usage: Arc::default(),
            op: tokio::sync::Mutex::new(()),
            started: tokio::sync::OnceCell::new(),
            socket_wait: SOCKET_WAIT,
            ready: socket_ready,
            check_path: true,
            health_every: Duration::from_secs(2),
        }
    }

    /// Attente du socket et test de disponibilite (tests).
    #[must_use]
    pub fn with_readiness(mut self, wait: Duration, ready: fn(&Path) -> bool) -> Self {
        self.socket_wait = wait;
        self.ready = ready;
        self.check_path = false;
        self
    }

    /// Intervalle de verification du conteneur pendant l'attente du socket
    /// (tests).
    #[must_use]
    pub fn with_health_interval(mut self, every: Duration) -> Self {
        self.health_every = every;
        self
    }

    /// Les builds sont possibles (compte `ciphera-build` et plages presents).
    pub fn available(&self) -> bool {
        self.cfg.builds_available()
    }

    fn marker(&self, project: ProjectId) -> PathBuf {
        self.cfg.buildkit_usage_dir.join(project.to_string())
    }

    /// Projets qui ont un etat (un fichier d'usage), `project` compris.
    fn known_projects(&self, project: ProjectId) -> HashSet<ProjectId> {
        let mut known: HashSet<ProjectId> = std::fs::read_dir(&self.cfg.buildkit_usage_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse().ok())
            .collect();
        known.insert(project);
        known
    }

    /// Une fois, au premier usage : restes d'un arret de l'agent.
    async fn startup(&self) -> Result<(), BuildkitError> {
        infra::remove_legacy_buildkitd(self.docker.as_ref()).await?;
        for c in self
            .docker
            .list_managed(&[(ROLE_LABEL, BUILDKITD_ROLE)])
            .await?
        {
            tracing::info!("buildkitd {} reste en marche : arret", c.name);
            self.docker.stop(&c.id).await?;
        }
        for c in self
            .docker
            .list_managed(&[(ROLE_LABEL, HELPER_ROLE)])
            .await?
        {
            tracing::info!("conteneur jetable {} abandonne : supprime", c.name);
            self.docker.remove(&c.id).await?;
        }
        let marker = self.cfg.buildkit_usage_dir.join(LEGACY_MARKER);
        if !marker.exists() {
            self.fs.remove_legacy_state().await?;
            touch(&marker).map_err(|e| BuildkitError::Fs(e.to_string()))?;
        }
        Ok(())
    }

    /// buildkitd du projet en marche, son socket pret. A appeler avec le
    /// creneau de build du noeud en main (`build::BUILD_SLOTS`) : le
    /// buildkitd d'un autre projet est arrete, sauf s'il sert encore un
    /// build (`Busy`).
    pub async fn ensure(&self, project: ProjectId) -> Result<BuildkitLease, BuildkitError> {
        let _op = self.op.lock().await;
        self.started.get_or_try_init(|| self.startup()).await?;
        if locked(&self.usage)
            .in_use
            .iter()
            .any(|(p, n)| *p != project && *n > 0)
        {
            return Err(BuildkitError::Busy);
        }
        let provisioned = locked(&self.usage).provisioned.contains(&project);
        if !provisioned {
            self.fs.create(project).await?;
            locked(&self.usage).provisioned.insert(project);
        }
        touch(&self.marker(project)).map_err(|e| BuildkitError::Fs(e.to_string()))?;
        let projects = self.known_projects(project).len();
        infra::write_project_config(&self.cfg, project, projects)?;
        // Un seul buildkitd a la fois : arret de ceux que ce processus a
        // demarres pour un autre projet (jamais ceux deja arretes).
        let others: Vec<ProjectId> = locked(&self.usage)
            .running
            .iter()
            .copied()
            .filter(|p| *p != project)
            .collect();
        for other in others {
            self.stop_project(other).await?;
        }
        let socket = self.cfg.buildkit_project_socket(project);
        // Un socket Unix tient dans 107 octets : au-dela, ni buildctl ni
        // l'agent ne peuvent s'y connecter (chemin de production : ~80).
        // Refuse avant de demarrer le moindre conteneur.
        if self.check_path && socket.as_os_str().len() > MAX_SOCKET_PATH {
            return Err(BuildkitError::Fs(format!(
                "chemin du socket trop long ({} octets, {MAX_SOCKET_PATH} au plus)",
                socket.as_os_str().len(),
            )));
        }
        let spec = infra::buildkit_spec(&self.cfg, project, projects)?;
        let id = self
            .docker
            .run(&spec)
            .await
            .map_err(|e| self.start_error(e))?;
        locked(&self.usage).running.insert(project);
        self.wait_ready(&socket, &id).await?;
        {
            let mut u = locked(&self.usage);
            *u.in_use.entry(project).or_insert(0) += 1;
            u.last_used.insert(project, Instant::now());
        }
        Ok(BuildkitLease {
            socket: Some(socket),
            _guard: Some(UseGuard {
                usage: self.usage.clone(),
                project,
                marker: self.marker(project),
            }),
        })
    }

    /// Erreur du demarrage de buildkitd ; refus d'AppArmor sous le profil
    /// de buildkitd : `AppArmor` (message explicite, journalise).
    fn start_error(&self, e: DockerError) -> BuildkitError {
        match (&self.cfg.buildkit_apparmor_profile, e) {
            (Some(profile), DockerError::Api(detail))
                if detail.to_ascii_lowercase().contains("apparmor") =>
            {
                let e = BuildkitError::AppArmor {
                    profile: profile.clone(),
                    detail,
                };
                tracing::error!("{e}");
                e
            }
            (_, e) => e.into(),
        }
    }

    async fn wait_ready(&self, socket: &Path, id: &ContainerId) -> Result<(), BuildkitError> {
        let start = Instant::now();
        let mut down = 0;
        let mut last_check = Instant::now();
        while !(self.ready)(socket) {
            // Conteneur arrete ou disparu trois fois de suite (verifie toutes
            // les 2 s) : inutile d'attendre la fin du delai.
            if last_check.elapsed() >= self.health_every {
                last_check = Instant::now();
                let gone = matches!(
                    self.docker.health(id).await,
                    Ok(Health::Stopped | Health::Missing)
                );
                down = if gone { down + 1 } else { 0 };
            }
            if down >= 3 || start.elapsed() >= self.socket_wait {
                // Le detail (journal de buildkitd compris) reste dans le
                // journal de l'agent : le message du build est court.
                tracing::error!(
                    "buildkitd du projet n'a pas demarre ({}) : {}",
                    socket.display(),
                    self.diagnostic(socket, id).await
                );
                return Err(BuildkitError::SocketTimeout(socket.to_path_buf()));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    /// Etat du conteneur, du dossier du socket et dernieres lignes du journal
    /// de buildkitd, pour le message d'erreur.
    async fn diagnostic(&self, socket: &Path, id: &ContainerId) -> String {
        let state = match self.docker.health(id).await {
            Ok(h) => format!("conteneur {h:?}"),
            Err(e) => format!("conteneur illisible : {e}"),
        };
        let dir = socket.parent().map_or_else(
            || "dossier inconnu".to_string(),
            |d| {
                let mut names: Vec<String> = std::fs::read_dir(d)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                format!("dossier du socket : {names:?}")
            },
        );
        let mut lines: Vec<String> = Vec::new();
        let mut logs = self.docker.logs(id, 40, false).await;
        while let Some(l) = logs.next().await {
            lines.push(l.line.chars().take(MAX_LOG_CHARS).collect::<String>());
        }
        let start = lines.len().saturating_sub(25);
        format!("{state} ; {dir} ; journal : {}", lines[start..].join(" | "))
    }

    /// Arrete les buildkitd sans build depuis `max_idle` (leur etat reste) ;
    /// leur nombre.
    pub async fn stop_idle(&self, max_idle: Duration) -> usize {
        let _op = self.op.lock().await;
        let idle: Vec<ProjectId> = {
            let u = locked(&self.usage);
            u.last_used
                .iter()
                .filter(|(p, at)| {
                    u.in_use.get(p).copied().unwrap_or(0) == 0 && at.elapsed() >= max_idle
                })
                .map(|(p, _)| *p)
                .collect()
        };
        let mut stopped = 0;
        for project in idle {
            // Un arret refuse garde le projet : retente au passage suivant.
            match self.stop_project(project).await {
                Ok(()) => stopped += 1,
                Err(e) => tracing::warn!("buildkitd inactif du projet : {e}"),
            }
        }
        stopped
    }

    /// Arrete le buildkitd du projet (son etat reste) ; il n'est retire de
    /// ceux a surveiller qu'une fois arrete.
    async fn stop_project(&self, project: ProjectId) -> Result<(), BuildkitError> {
        let label = project.to_string();
        for c in self
            .docker
            .list_managed(&[(BUILDKIT_PROJECT_LABEL, label.as_str())])
            .await?
        {
            self.docker.stop(&c.id).await?;
            tracing::info!("buildkitd {} arrete", c.name);
        }
        let mut u = locked(&self.usage);
        u.running.remove(&project);
        u.last_used.remove(&project);
        Ok(())
    }

    /// Supprime le buildkitd, l'etat et le socket du projet (`RemoveProject`).
    pub async fn remove_project(&self, project: ProjectId) -> Result<(), BuildkitError> {
        let _op = self.op.lock().await;
        self.remove_locked(project).await
    }

    async fn remove_locked(&self, project: ProjectId) -> Result<(), BuildkitError> {
        let label = project.to_string();
        for c in self
            .docker
            .list_managed(&[(BUILDKIT_PROJECT_LABEL, label.as_str())])
            .await?
        {
            self.docker.remove(&c.id).await?;
        }
        self.fs.remove(project).await?;
        {
            let mut u = locked(&self.usage);
            u.in_use.remove(&project);
            u.last_used.remove(&project);
            u.provisioned.remove(&project);
            u.running.remove(&project);
        }
        for file in [
            self.marker(project),
            self.cfg.buildkit_project_config(project),
        ] {
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(BuildkitError::Fs(format!("{} : {e}", file.display()))),
            }
        }
        Ok(())
    }

    /// Supprime l'etat des projets sans build depuis `max_age` (ni en cours) ;
    /// leur nombre.
    pub async fn sweep_stale(&self, max_age: Duration) -> usize {
        let _op = self.op.lock().await;
        let now = SystemTime::now();
        let stale: Vec<ProjectId> = std::fs::read_dir(&self.cfg.buildkit_usage_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let project: ProjectId = e.file_name().to_str()?.parse().ok()?;
                let modified = e.metadata().ok()?.modified().ok()?;
                (now.duration_since(modified).ok()? >= max_age).then_some(project)
            })
            .filter(|p| locked(&self.usage).in_use.get(p).copied().unwrap_or(0) == 0)
            .collect();
        let mut removed = 0;
        for project in stale {
            match self.remove_locked(project).await {
                Ok(()) => {
                    tracing::info!("etat de buildkitd du projet {project} perime : supprime");
                    removed += 1;
                }
                Err(e) => tracing::warn!("etat de buildkitd du projet {project} : {e}"),
            }
        }
        removed
    }

    /// Surveillance : arret des buildkitd inactifs chaque minute, balayage
    /// des etats perimes chaque heure. Ne rend jamais la main.
    pub async fn maintain(self: Arc<Self>) {
        let mut idle = tokio::time::interval(IDLE_CHECK_EVERY);
        let mut sweep = tokio::time::interval(SWEEP_EVERY);
        loop {
            tokio::select! {
                _ = idle.tick() => { self.stop_idle(IDLE_TIMEOUT).await; }
                _ = sweep.tick() => { self.sweep_stale(STALE_AFTER).await; }
            }
        }
    }
}

/// Dossiers des projets crees et supprimes par un conteneur jetable : l'image
/// epinglee de buildkitd, sans reseau, systeme de fichiers en lecture seule,
/// sans capacite (sauf celles de la suppression), sous le compte
/// `ciphera-build` pour creer (les dossiers lui appartiennent) et sous root
/// pour supprimer (des fichiers appartiennent aux sous-uids de ses etapes).
pub struct DockerFs {
    docker: Docker,
    image: String,
    user: (u32, u32),
    state_root: PathBuf,
    socket_root: PathBuf,
}

/// Script de creation : `$P` est l'UUID du projet. Etat en 0700 ; dossier du
/// socket en 0770 qui herite du setgid et du groupe `ciphera-agent` du
/// parent (un `chmod 2770` par un compte hors de ce groupe retirerait le
/// setgid).
const CREATE_SCRIPT: &str = "set -eu; umask 077; mkdir -p /state/p \"/state/p/$P\"; \
     umask 007; mkdir -p \"/sock/$P\"";
/// Script de suppression d'un projet.
const REMOVE_SCRIPT: &str = "set -eu; rm -rf \"/state/p/$P\" \"/sock/$P\"";
/// Script de suppression de l'etat du buildkitd partage d'avant.
const LEGACY_SCRIPT: &str =
    "set -eu; for f in /state/* /state/.[!.]*; do [ \"$f\" = /state/p ] || rm -rf \"$f\"; done";

/// Supprime le conteneur jetable meme si la tache est annulee en cours
/// d'attente (le `Drop` lance la suppression sur le runtime).
struct RemoveOnDrop {
    docker: Docker,
    id: Option<String>,
}

impl RemoveOnDrop {
    fn disarm(&mut self) {
        self.id = None;
    }
}

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let (Some(id), Ok(handle)) = (self.id.take(), tokio::runtime::Handle::try_current()) else {
            return;
        };
        let docker = self.docker.clone();
        handle.spawn(async move {
            let options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            let _ = docker.remove_container(&id, Some(options)).await;
        });
    }
}

impl DockerFs {
    /// `image` : image epinglee de buildkitd ; `user` : `ciphera-build`.
    pub fn connect(
        cfg: &InfraConfig,
        image: String,
        user: (u32, u32),
    ) -> Result<Self, DockerError> {
        Ok(Self {
            docker: Docker::connect_with_socket_defaults()
                .map_err(|e| DockerError::Api(e.to_string()))?,
            image,
            user,
            state_root: cfg.buildkit_state_dir.clone(),
            socket_root: cfg.buildkit_socket_dir.clone(),
        })
    }

    fn err(e: impl fmt::Display) -> BuildkitError {
        BuildkitError::Fs(e.to_string())
    }

    async fn ensure_image(&self) -> Result<(), BuildkitError> {
        match self.docker.inspect_image(&self.image).await {
            Ok(_) => return Ok(()),
            Err(BollardError::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(e) => return Err(Self::err(e)),
        }
        let options = CreateImageOptions {
            from_image: Some(self.image.clone()),
            ..Default::default()
        };
        let mut steps = self.docker.create_image(Some(options), None, None);
        while let Some(step) = steps.next().await {
            step.map_err(Self::err)?;
        }
        Ok(())
    }

    /// Lance `script` (avec `P=<projet>`) dans un conteneur jetable.
    async fn helper(
        &self,
        script: &str,
        project: Option<ProjectId>,
        user: &str,
        caps: &[&str],
    ) -> Result<(), BuildkitError> {
        self.ensure_image().await?;
        let labels = [
            (MANAGED_LABEL, "true".to_string()),
            (ROLE_LABEL, HELPER_ROLE.to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let bind = |from: &Path, to: &str| format!("{}:{to}:rw,z", from.to_string_lossy());
        let body = ContainerCreateBody {
            image: Some(self.image.clone()),
            entrypoint: Some(vec!["/bin/sh".to_string(), "-c".to_string()]),
            cmd: Some(vec![script.to_string()]),
            env: Some(vec![
                format!("P={}", project.map(|p| p.to_string()).unwrap_or_default()),
                "PATH=/usr/bin:/bin".to_string(),
            ]),
            user: Some(user.to_string()),
            labels: Some(labels),
            network_disabled: Some(true),
            host_config: Some(HostConfig {
                network_mode: Some("none".to_string()),
                binds: Some(vec![
                    bind(&self.state_root, "/state"),
                    bind(&self.socket_root, "/sock"),
                ]),
                readonly_rootfs: Some(true),
                cap_drop: Some(vec!["ALL".to_string()]),
                cap_add: (!caps.is_empty()).then(|| caps.iter().map(|c| c.to_string()).collect()),
                security_opt: Some(vec!["no-new-privileges:true".to_string()]),
                pids_limit: Some(64),
                memory: Some(128 << 20),
                privileged: Some(false),
                publish_all_ports: Some(false),
                log_config: Some(crate::docker::log_config()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let created = self
            .docker
            .create_container(Some(CreateContainerOptions::default()), body)
            .await
            .map_err(Self::err)?;
        // Annulation (build abandonne) : le conteneur est quand meme supprime.
        let mut guard = RemoveOnDrop {
            docker: self.docker.clone(),
            id: Some(created.id.clone()),
        };
        let outcome = tokio::time::timeout(HELPER_TIMEOUT, async {
            self.docker
                .start_container(&created.id, None)
                .await
                .map_err(Self::err)?;
            match self.docker.wait_container(&created.id, None).next().await {
                Some(Ok(r)) => Ok(r.status_code),
                Some(Err(BollardError::DockerContainerWaitError { code, .. })) => Ok(code),
                Some(Err(e)) => Err(Self::err(e)),
                None => Err(Self::err("attente interrompue")),
            }
        })
        .await;
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        let _ = self
            .docker
            .remove_container(&created.id, Some(options))
            .await;
        guard.disarm();
        match outcome {
            Err(_) => Err(Self::err("delai depasse")),
            Ok(Err(e)) => Err(e),
            Ok(Ok(0)) => Ok(()),
            Ok(Ok(code)) => Err(Self::err(format!("echec (code {code})"))),
        }
    }
}

impl ProjectFs for DockerFs {
    async fn create(&self, project: ProjectId) -> Result<(), BuildkitError> {
        let (uid, gid) = self.user;
        self.helper(CREATE_SCRIPT, Some(project), &format!("{uid}:{gid}"), &[])
            .await
    }

    async fn remove(&self, project: ProjectId) -> Result<(), BuildkitError> {
        self.helper(
            REMOVE_SCRIPT,
            Some(project),
            "0:0",
            &["DAC_OVERRIDE", "DAC_READ_SEARCH", "FOWNER"],
        )
        .await
    }

    async fn remove_legacy_state(&self) -> Result<(), BuildkitError> {
        self.helper(
            LEGACY_SCRIPT,
            None,
            "0:0",
            &["DAC_OVERRIDE", "DAC_READ_SEARCH", "FOWNER"],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_quote_the_project_variable() {
        for s in [CREATE_SCRIPT, REMOVE_SCRIPT] {
            assert!(!s.contains(" $P"), "{s}");
        }
    }
}
