//! Pipeline de build sans shell : clone Git, plan Railpack, Dockerfile ou
//! Compose, construits par BuildKit (`ciphera-buildkitd`, client `buildctl`)
//! puis charges dans Docker.
//!
//! Etapes rapportees au plan de controle (`DeployProgress`) : `Clone`,
//! `Build`, `Image`. Une image n'est etiquetee
//! `ciphera/<projet>-<environnement>:<deployment_id>` qu'apres un chargement
//! reussi (nom provisoire `ciphera-staging/...` d'abord) ; en cas d'echec,
//! l'etape qui echoue est rapportee `Failed`, aucune image n'est etiquetee et
//! les images provisoires sont supprimees. Aucune etiquette « courante » ici
//! (la promotion vient avec les deploiements).
//!
//! Le dossier de build (`<builds_dir>/<deployment_id>`, 0700) est supprime a
//! la fin, que le build reussisse ou echoue (explicitement, et par `Drop` si
//! la tache est annulee).

pub mod argv;
pub mod clone;
pub mod compose;
pub mod compose_policy;
pub mod dockerfile;
pub mod local;
pub mod railpack;
pub mod sandbox;

use std::fmt;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use proto::{
    AgentToControl, BuildMethod, BuildSource, BuildSpec, GitToken, ImageRef, LogLine, LogStream,
    LogTarget, Stage, StageStatus,
};
use shared::ids::DeploymentId;
use tokio::sync::mpsc;

use self::argv::{AllowedBin, InvalidArg, ValidatedArg};
pub use self::compose_policy::PolicyViolation;

/// Dossier des builds (sous le StateDirectory 0700 de l'agent).
pub const DEFAULT_BUILDS_DIR: &str = "/var/lib/ciphera-agent/builds";
/// Socket de `ciphera-buildkitd` sur l'hote (voir `infra`).
pub const DEFAULT_BUILDKIT_SOCKET: &str = "/run/ciphera-agent/buildkit/buildkitd.sock";
/// Message d'un build demande sans buildkitd (agent en mode degrade).
pub const UNAVAILABLE: &str = "BuildKit indisponible : ciphera-buildkitd n'est pas lance (compte \
     ciphera-build absent ? relancer install.sh), builds impossibles";
/// Longueur maximale d'une ligne de journal de build transmise.
pub const MAX_LOG_LINE: usize = crate::docker::MAX_LOG_LINE;

/// Reglages du pipeline.
#[derive(Clone, Debug)]
pub struct BuildConfig {
    /// Dossier parent des dossiers de build (cree en 0700 s'il manque).
    pub builds_dir: String,
    /// Socket Unix de buildkitd.
    pub buildkit_socket: String,
    /// Programme `GIT_ASKPASS` : le binaire de l'agent lui-meme (mode cache
    /// `clone::askpass_main`).
    pub askpass: PathBuf,
    /// Bac a sable de `railpack prepare` ; `None` (compte `ciphera-build`
    /// absent) : les builds Railpack echouent.
    pub prepare: Option<sandbox::PrepareConfig>,
}

impl BuildConfig {
    /// Chemins de production ; `askpass` : le binaire en cours.
    pub fn production(prepare: Option<sandbox::PrepareConfig>) -> std::io::Result<Self> {
        Ok(Self {
            builds_dir: DEFAULT_BUILDS_DIR.to_string(),
            buildkit_socket: DEFAULT_BUILDKIT_SOCKET.to_string(),
            askpass: std::env::current_exe()?,
            prepare,
        })
    }
}

/// Images produites et etiquetees par un build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltImages {
    pub images: Vec<ImageRef>,
    /// Projet Compose : ensemble a deployer (JSON, `compose::bundle`).
    pub compose: Option<String>,
}

#[derive(Debug)]
pub enum BuildError {
    /// Ordre de build invalide (nom, URL, branche, chemin...).
    Invalid(InvalidArg),
    /// BuildKit indisponible (buildkitd non lance).
    Unavailable(&'static str),
    /// Dossier de build (creation, suppression).
    Workdir(String),
    /// Un outil a echoue (code de sortie, delai, lancement impossible).
    Tool { tool: &'static str, message: String },
    /// Fichier Compose refuse par la politique.
    Policy(PolicyViolation),
    /// Chemin de build absent ou hors du depot.
    Path(String),
    /// Fichier Compose illisible.
    Compose(String),
    /// Chargement, etiquetage ou telechargement d'image.
    Image(String),
    /// Un build de ce deploiement est deja en cours sur l'agent.
    AlreadyRunning,
    /// Build annule par `Stop` avant d'avoir conclu (rien n'est rapporte).
    Cancelled,
    /// Dockerfile qui demande un frontend non autorise (directive `syntax`).
    Frontend(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "ordre de build invalide : {e}"),
            Self::Unavailable(m) => write!(f, "{m}"),
            Self::Workdir(m) => write!(f, "dossier de build : {m}"),
            Self::Tool { tool, message } => write!(f, "{tool} : {message}"),
            Self::Policy(v) => write!(f, "{v}"),
            Self::Path(m) => write!(f, "chemin de build : {m}"),
            Self::Compose(m) => write!(f, "fichier Compose : {m}"),
            Self::Image(m) => write!(f, "image : {m}"),
            Self::AlreadyRunning => write!(f, "un build de ce deploiement est deja en cours"),
            Self::Cancelled => write!(f, "build annule"),
            Self::Frontend(m) => write!(f, "frontend de build refuse : {m}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<InvalidArg> for BuildError {
    fn from(e: InvalidArg) -> Self {
        Self::Invalid(e)
    }
}

impl From<PolicyViolation> for BuildError {
    fn from(v: PolicyViolation) -> Self {
        Self::Policy(v)
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
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

/// Messages d'un build vers le plan de controle (`DeployProgress`,
/// `LogLine`). Le jeton Git du build, s'il apparait dans une ligne ou un
/// message, est masque avant l'envoi.
/// Appele pour chaque etat d'etape avant son envoi (message deja masque) :
/// l'agent y enregistre les issues definitives.
pub type StageHook = std::sync::Arc<dyn Fn(Stage, &StageStatus) + Send + Sync>;

#[derive(Clone)]
pub struct ProgressSink {
    deployment_id: DeploymentId,
    tx: mpsc::Sender<AgentToControl>,
    secret: Option<GitToken>,
    hook: Option<StageHook>,
}

impl fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressSink")
            .field("deployment_id", &self.deployment_id)
            .finish_non_exhaustive()
    }
}

impl ProgressSink {
    pub fn new(deployment_id: DeploymentId, tx: mpsc::Sender<AgentToControl>) -> Self {
        Self {
            deployment_id,
            tx,
            secret: None,
            hook: None,
        }
    }

    pub fn deployment_id(&self) -> DeploymentId {
        self.deployment_id
    }

    /// Meme destination, `hook` appele avant chaque etat d'etape.
    pub fn with_hook(&self, hook: StageHook) -> Self {
        Self {
            hook: Some(hook),
            ..self.clone()
        }
    }

    fn redacting(&self, token: Option<&GitToken>) -> Self {
        Self {
            secret: token.cloned(),
            ..self.clone()
        }
    }

    fn scrub(&self, text: &str) -> String {
        match &self.secret {
            Some(t) if !t.expose().is_empty() => text.replace(t.expose(), "***"),
            _ => text.to_string(),
        }
    }

    /// Envoie l'etat d'une etape (lien coupe : ignore, l'etat sera
    /// reconcilie a la reconnexion).
    pub async fn stage(&self, stage: Stage, status: StageStatus) {
        let status = match status {
            StageStatus::Failed { message } => StageStatus::Failed {
                message: self.scrub(&message),
            },
            other => other,
        };
        if let Some(hook) = &self.hook {
            hook(stage, &status);
        }
        let _ = self
            .tx
            .send(AgentToControl::DeployProgress {
                deployment_id: self.deployment_id,
                stage,
                status,
            })
            .await;
    }

    /// Envoie une ligne de journal du build (coupee a `MAX_LOG_LINE`).
    pub async fn log(&self, stream: LogStream, line: &str) {
        let line = self.scrub(line);
        let _ = self
            .tx
            .send(AgentToControl::LogLine(LogLine {
                target: LogTarget::Deployment(self.deployment_id),
                stream,
                timestamp_ms: now_ms(),
                line: truncate(&line, MAX_LOG_LINE).to_string(),
            }))
            .await;
    }
}

/// Execution des outils et acces a Docker pour le pipeline. `LocalBackend`
/// (processus + API Docker) en production ; simule dans les tests.
pub trait BuildBackend: Send + Sync {
    /// Le socket de buildkitd existe (buildkitd lance par l'agent).
    fn buildkit_available(&self) -> bool;
    /// Lance `cmd` jusqu'au bout ; sorties en journal (`sink`), sauf la
    /// sortie standard si `capture` (alors renvoyee). Code non nul, delai
    /// depasse ou lancement impossible : `BuildError::Tool`.
    fn exec(
        &self,
        bin: AllowedBin,
        cmd: Command,
        sink: &ProgressSink,
        capture: bool,
    ) -> impl Future<Output = Result<Vec<u8>, BuildError>> + Send;
    /// Lance `buildctl` (`cmd`, sortie `type=docker`) et charge l'archive
    /// produite dans Docker, ou elle porte le nom provisoire `staging`. En
    /// cas d'echec, rien ne reste sous ce nom.
    fn build_and_load(
        &self,
        cmd: Command,
        staging: &ImageRef,
        sink: &ProgressSink,
    ) -> impl Future<Output = Result<(), BuildError>> + Send;
    /// Telecharge une image (source `Image`).
    fn pull(
        &self,
        image: &ImageRef,
        sink: &ProgressSink,
    ) -> impl Future<Output = Result<(), BuildError>> + Send;
    /// Etiquette `source` sous `target`.
    fn tag(
        &self,
        source: &ImageRef,
        target: &ImageRef,
    ) -> impl Future<Output = Result<(), BuildError>> + Send;
    /// Retire l'etiquette `image` (l'image disparait si c'etait la
    /// derniere) ; absente : rien a faire.
    fn remove_image(&self, image: &ImageRef)
    -> impl Future<Output = Result<(), BuildError>> + Send;
    /// Lance le conteneur jetable de `railpack prepare` sur le pont
    /// `ciphera-prepare` (cree s'il manque), journaux vers `sink`, jusqu'a
    /// sa fin ou `sandbox::PREPARE_TIMEOUT` ; le conteneur est supprime dans
    /// tous les cas. Code non nul ou delai depasse : `BuildError::Tool`.
    fn prepare(
        &self,
        container: &sandbox::PrepareContainer,
        sink: &ProgressSink,
    ) -> impl Future<Output = Result<(), BuildError>> + Send;
}

/// Dossier d'un build : `<builds_dir>/<deployment_id>` (0700), avec `src`
/// (le clone), `home` (HOME des outils) et `plan` (plan Railpack). Supprime
/// par `remove` ou, a defaut, a sa liberation.
#[derive(Debug)]
pub struct Workdir {
    root: String,
    deployment: DeploymentId,
    removed: bool,
}

#[cfg(unix)]
fn create_private_dir(path: &str) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &str) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

impl Workdir {
    /// Cree le dossier du build (un reste d'un build interrompu du meme
    /// deploiement est d'abord supprime).
    pub fn create(builds_dir: &str, deployment: DeploymentId) -> Result<Self, BuildError> {
        ValidatedArg::path(builds_dir)?;
        let io = |e: std::io::Error| BuildError::Workdir(e.to_string());
        if !Path::new(builds_dir).is_dir() {
            std::fs::create_dir_all(builds_dir).map_err(io)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(builds_dir, std::fs::Permissions::from_mode(0o700))
                .map_err(io)?;
        }
        let root = format!("{builds_dir}/{deployment}");
        if Path::new(&root).exists() {
            std::fs::remove_dir_all(&root).map_err(io)?;
        }
        create_private_dir(&root).map_err(io)?;
        let dir = Self {
            root,
            deployment,
            removed: false,
        };
        for sub in [dir.home(), dir.plan()] {
            create_private_dir(&sub).map_err(io)?;
        }
        Ok(dir)
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn deployment(&self) -> DeploymentId {
        self.deployment
    }

    /// Dossier du clone (cree par `git clone`).
    pub fn src(&self) -> String {
        format!("{}/src", self.root)
    }

    pub fn home(&self) -> String {
        format!("{}/home", self.root)
    }

    pub fn plan(&self) -> String {
        format!("{}/plan", self.root)
    }

    /// Supprime le dossier du build.
    pub fn remove(mut self) -> Result<(), BuildError> {
        self.removed = true;
        match std::fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(BuildError::Workdir(e.to_string())),
        }
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        if !self.removed {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Noms valides d'un build : projet, environnement, deploiement, et UUID du
/// projet et de l'environnement (cloisonnement des caches).
pub(crate) struct Targets {
    project: ValidatedArg,
    env: ValidatedArg,
    deployment: ValidatedArg,
    project_id: Option<ValidatedArg>,
    environment_id: Option<ValidatedArg>,
}

/// Cloisonnement des caches de BuildKit d'un build, par environnement :
/// `key` (`<UUID du projet>-<UUID de l'environnement>`, longueur fixe donc
/// sans ambiguite) prefixe chaque cache de montage d'un Dockerfile
/// (`BUILDKIT_CACHE_MOUNT_NS`) et chaque cache du frontend Railpack. Une
/// branche de preproduction ne remplit donc pas les caches de la production
/// du meme projet.
#[derive(Clone, Debug)]
pub struct CacheScope {
    pub key: ValidatedArg,
}

impl Targets {
    fn new(spec: &BuildSpec) -> Result<Self, BuildError> {
        Ok(Self {
            project: ValidatedArg::name(&spec.project)?,
            env: ValidatedArg::name(&spec.environment)?,
            deployment: ValidatedArg::name(&spec.deployment_id.to_string())?,
            project_id: spec
                .project_id
                .map(|id| ValidatedArg::name(&id.to_string()))
                .transpose()?,
            environment_id: spec
                .environment_id
                .map(|id| ValidatedArg::name(&id.to_string()))
                .transpose()?,
        })
    }

    /// `ciphera/<projet>-<env>`
    fn repo(&self) -> String {
        format!("ciphera/{}-{}", self.project.as_str(), self.env.as_str())
    }

    /// `ciphera/<projet>-<env>:<deployment_id>`
    pub(crate) fn main_tag(&self) -> ImageRef {
        ImageRef(format!("{}:{}", self.repo(), self.deployment.as_str()))
    }

    /// `ciphera/<projet>-<env>/<service>:<deployment_id>` (Compose).
    pub(crate) fn service_tag(&self, service: &ValidatedArg) -> ImageRef {
        ImageRef(format!(
            "{}/{}:{}",
            self.repo(),
            service.as_str(),
            self.deployment.as_str()
        ))
    }

    /// Nom provisoire du chargement (`ciphera-staging/<deployment_id>[-<service>]:build`).
    pub(crate) fn staging(&self, service: Option<&ValidatedArg>) -> ValidatedArg {
        let mut parts = vec![&STAGING_PREFIX, &self.deployment];
        if let Some(s) = service {
            parts.push(&DASH);
            parts.push(s);
        }
        parts.push(&STAGING_SUFFIX);
        ValidatedArg::concat(&parts)
    }

    /// Nom du projet Compose a la normalisation (`<projet>-<env>`). Jamais
    /// une cle de cache : `a` + `b-c` et `a-b` + `c` donnent le meme nom.
    pub(crate) fn compose_project(&self) -> ValidatedArg {
        ValidatedArg::concat(&[&self.project, &DASH, &self.env])
    }

    /// Caches du build, a partir des UUID du projet et de l'environnement.
    /// Ordre sans l'un d'eux (plan de controle anterieur au protocole v6) :
    /// refuse, plutot qu'un build dont les caches seraient partages.
    pub(crate) fn cache_scope(&self) -> Result<CacheScope, BuildError> {
        let (Some(project), Some(env)) = (&self.project_id, &self.environment_id) else {
            return Err(BuildError::Invalid(InvalidArg {
                kind: "identifiant de projet ou d'environnement",
                reason: "absent de l'ordre de build (plan de controle a mettre a jour)",
            }));
        };
        Ok(CacheScope {
            key: ValidatedArg::concat(&[project, &DASH, env]),
        })
    }
}

static STAGING_PREFIX: ValidatedArg = ValidatedArg::lit("ciphera-staging/");
static STAGING_SUFFIX: ValidatedArg = ValidatedArg::lit(":build");
static DASH: ValidatedArg = ValidatedArg::lit("-");

/// Image chargee sous un nom provisoire, a etiqueter sous son nom final.
pub(crate) struct Staged {
    pub(crate) staging: ImageRef,
    pub(crate) target: ImageRef,
}

/// Commande `buildctl` vers buildkitd : `frontend` (arguments du frontend
/// et des sources locales), sortie `type=docker` sous le nom provisoire.
/// Jamais d'entitlement (`--allow`) : `security.insecure` et `network.host`
/// restent refuses par buildkitd.
pub(crate) fn buildctl_command(
    socket: &ValidatedArg,
    frontend: Vec<ValidatedArg>,
    staging: &ValidatedArg,
    home: &ValidatedArg,
) -> Command {
    let mut args = vec![
        ValidatedArg::lit("--addr"),
        ValidatedArg::concat(&[&ValidatedArg::lit("unix://"), socket]),
        ValidatedArg::lit("build"),
        ValidatedArg::lit("--progress"),
        ValidatedArg::lit("plain"),
    ];
    args.extend(frontend);
    args.push(ValidatedArg::lit("--output"));
    args.push(ValidatedArg::concat(&[
        &ValidatedArg::lit("type=docker,name="),
        staging,
    ]));
    let mut cmd = argv::command(AllowedBin::Buildctl, &args);
    cmd.env("HOME", home.as_str());
    cmd
}

/// Chemin `candidate` resolu (liens symboliques compris) : il doit exister et
/// rester dans le depot (`root_canon`, forme canonique de `root`). Renvoie le
/// chemin a passer aux outils (`root` + chemin relatif, separateurs `/`) et
/// sa forme canonique.
pub(crate) fn confine(
    root: &str,
    root_canon: &Path,
    candidate: &Path,
) -> Result<(ValidatedArg, PathBuf), BuildError> {
    let canon = std::fs::canonicalize(candidate)
        .map_err(|_| BuildError::Path("introuvable dans le depot".into()))?;
    let rel = canon
        .strip_prefix(root_canon)
        .map_err(|_| BuildError::Path("hors du depot (lien symbolique ?)".into()))?;
    let mut out = root.to_string();
    for c in rel.components() {
        match c {
            Component::Normal(part) => {
                out.push('/');
                out.push_str(
                    part.to_str()
                        .ok_or_else(|| BuildError::Path("nom non UTF-8".into()))?,
                );
            }
            _ => return Err(BuildError::Path("hors du depot".into())),
        }
    }
    Ok((ValidatedArg::path(&out)?, canon))
}

/// Profondeur maximale parcourue par `check_symlinks`.
const MAX_DEPTH: usize = 64;

/// Refuse un depot qui contient un lien symbolique vers l'exterieur (ou
/// casse) : railpack et Compose lisent des fichiers du depot avec les droits
/// de l'agent, un lien vers ses secrets les ferait lire (et afficher dans les
/// journaux). `.git` est ignore (ecrit par git, jamais lu par les outils).
pub(crate) fn check_symlinks(src: &str) -> Result<(), BuildError> {
    let root = std::fs::canonicalize(src)
        .map_err(|e| BuildError::Path(format!("depot introuvable : {e}")))?;
    let mut stack = vec![(root.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(BuildError::Path("arborescence trop profonde".into()));
        }
        let entries = std::fs::read_dir(&dir).map_err(|e| BuildError::Path(e.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|e| BuildError::Path(e.to_string()))?;
            let path = entry.path();
            if depth == 0 && entry.file_name() == ".git" {
                continue;
            }
            let kind = entry
                .file_type()
                .map_err(|e| BuildError::Path(e.to_string()))?;
            if kind.is_symlink() {
                let inside = std::fs::canonicalize(&path).is_ok_and(|t| t.starts_with(&root));
                if !inside {
                    let rel = path.strip_prefix(&root).unwrap_or(&path);
                    return Err(BuildError::Path(format!(
                        "lien symbolique hors du depot ou casse : {}",
                        rel.display()
                    )));
                }
            } else if kind.is_dir() {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(())
}

/// Dossier racine de l'ordre (monorepo), valide avant le clone : relatif,
/// sans `..`, NUL ni caractere hors liste. `None` : racine du depot (champ
/// absent, vide ou `.`). `./a/b/` est lu `a/b`.
fn root_directory(spec: &BuildSpec) -> Result<Option<ValidatedArg>, BuildError> {
    let Some(value) = spec.root_directory.as_deref() else {
        return Ok(None);
    };
    let value = value.strip_prefix("./").unwrap_or(value);
    let value = value.trim_end_matches('/');
    if value.is_empty() || value == "." {
        return Ok(None);
    }
    let invalid = InvalidArg {
        kind: "dossier racine",
        reason: "chemin relatif au depot",
    };
    if value.contains('\0') || shared::validate::path(value).is_err() {
        return Err(invalid.into());
    }
    Ok(Some(ValidatedArg::path(value).map_err(|_| invalid)?))
}

/// Dossier du build dans le clone : `root` resolu (liens compris), qui doit
/// exister, etre un dossier et rester dans le clone. Renvoie le clone sans
/// dossier racine.
fn build_root(workdir: &Workdir, root: Option<&ValidatedArg>) -> Result<ValidatedArg, BuildError> {
    let src = workdir.src();
    let Some(root) = root else {
        return Ok(ValidatedArg::path(&src)?);
    };
    let candidate = Path::new(&src).join(root.as_str());
    if std::fs::symlink_metadata(&candidate).is_err() {
        return Err(BuildError::Path(format!(
            "dossier racine {} introuvable dans le depot",
            root.as_str()
        )));
    }
    let root_canon = std::fs::canonicalize(&src)
        .map_err(|e| BuildError::Path(format!("depot introuvable : {e}")))?;
    let (dir, canon) = confine(&src, &root_canon, &candidate).map_err(|_| {
        BuildError::Path(format!(
            "dossier racine {} hors du depot (lien symbolique ?)",
            root.as_str()
        ))
    })?;
    if !canon.is_dir() {
        return Err(BuildError::Path(format!(
            "dossier racine {} : pas un dossier",
            root.as_str()
        )));
    }
    Ok(dir)
}

/// Methode de build validee.
enum Method {
    Railpack,
    Dockerfile(ValidatedArg),
    Compose { path: ValidatedArg, service: String },
}

fn method(build: &BuildMethod) -> Result<Method, BuildError> {
    let repo_path = |p: &str| -> Result<ValidatedArg, BuildError> {
        shared::validate::path(p).map_err(|_| InvalidArg {
            kind: "chemin",
            reason: "relatif au depot",
        })?;
        Ok(ValidatedArg::path(p)?)
    };
    Ok(match build {
        BuildMethod::Railpack => Method::Railpack,
        BuildMethod::Dockerfile { path } => Method::Dockerfile(repo_path(path)?),
        BuildMethod::Compose { path, service } => {
            shared::validate::compose_service(service).map_err(|_| InvalidArg {
                kind: "service Compose",
                reason: "nom de service",
            })?;
            Method::Compose {
                path: repo_path(path)?,
                service: service.clone(),
            }
        }
    })
}

/// Suit l'etape en cours pour rapporter l'echec au bon endroit.
struct Stages<'a> {
    sink: &'a ProgressSink,
    current: Option<Stage>,
}

impl<'a> Stages<'a> {
    fn new(sink: &'a ProgressSink) -> Self {
        Self {
            sink,
            current: None,
        }
    }

    async fn start(&mut self, stage: Stage) {
        self.current = Some(stage);
        self.sink.stage(stage, StageStatus::Started).await;
    }

    async fn succeed(&mut self) {
        if let Some(stage) = self.current.take() {
            self.sink.stage(stage, StageStatus::Succeeded).await;
        }
    }

    async fn finish<T>(&mut self, result: &Result<T, BuildError>) {
        if let (Err(e), Some(stage)) = (result, self.current.take()) {
            self.sink
                .stage(
                    stage,
                    StageStatus::Failed {
                        message: e.to_string(),
                    },
                )
                .await;
        }
    }
}

/// Deploiements dont un build tourne sur cet agent.
static IN_FLIGHT: std::sync::Mutex<Vec<DeploymentId>> = std::sync::Mutex::new(Vec::new());

/// Build en cours d'un deploiement : un second ordre du meme deploiement est
/// refuse tant que ce jeton vit (il partagerait son dossier de build).
#[derive(Debug)]
pub struct InFlight(DeploymentId);

impl InFlight {
    pub fn claim(deployment: DeploymentId) -> Result<Self, BuildError> {
        let mut running = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains(&deployment) {
            return Err(BuildError::AlreadyRunning);
        }
        running.push(deployment);
        Ok(Self(deployment))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        let mut running = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        running.retain(|d| *d != self.0);
    }
}

/// Build complet d'un ordre : source `Image` (telechargement + etiquette) ou
/// `Git` (clone, build, chargement, etiquette). Toujours rapporte au plan de
/// controle ; le dossier de build est supprime dans tous les cas. Un ordre
/// pour un deploiement deja en cours est refuse sans rien rapporter ni
/// toucher (le build en cours rapporte le sien).
pub async fn run<B: BuildBackend>(
    spec: &BuildSpec,
    cfg: &BuildConfig,
    backend: &B,
    sink: &ProgressSink,
) -> Result<BuiltImages, BuildError> {
    run_with(spec, cfg, backend, sink, |_| Ok(())).await
}

/// `run`, avec `finalize` appele une fois le build fini et son dossier
/// supprime, avant le dernier `Succeeded` : la reussite n'est rapportee
/// qu'une fois tout conclu (ensemble Compose enregistre, operation rendue).
/// `Err(BuildError::Cancelled)` : rien n'est rapporte (le `Stop` rapporte).
pub async fn run_with<B, F>(
    spec: &BuildSpec,
    cfg: &BuildConfig,
    backend: &B,
    sink: &ProgressSink,
    finalize: F,
) -> Result<BuiltImages, BuildError>
where
    B: BuildBackend,
    F: FnOnce(&BuiltImages) -> Result<(), BuildError>,
{
    let _running = InFlight::claim(spec.deployment_id)?;
    let sink = sink.redacting(spec.git_token.as_ref());
    let mut stages = Stages::new(&sink);
    let mut result = run_stages(spec, cfg, backend, &sink, &mut stages).await;
    if let Ok(built) = &result {
        match finalize(built) {
            Ok(()) => stages.succeed().await,
            Err(BuildError::Cancelled) => return Err(BuildError::Cancelled),
            Err(e) => result = Err(e),
        }
    }
    stages.finish(&result).await;
    result
}

async fn run_stages<B: BuildBackend>(
    spec: &BuildSpec,
    cfg: &BuildConfig,
    backend: &B,
    sink: &ProgressSink,
    stages: &mut Stages<'_>,
) -> Result<BuiltImages, BuildError> {
    match &spec.source {
        BuildSource::Image { reference } => {
            stages.start(Stage::Image).await;
            let targets = Targets::new(spec)?;
            image_source(backend, sink, reference, &targets).await?;
            // `Image` reussi rapporte par l'appelant, une fois conclu.
            Ok(BuiltImages {
                images: vec![targets.main_tag()],
                compose: None,
            })
        }
        BuildSource::Git {
            url,
            branch,
            commit_sha,
        } => {
            if !backend.buildkit_available() {
                stages.start(Stage::Build).await;
                return Err(BuildError::Unavailable(UNAVAILABLE));
            }
            stages.start(Stage::Clone).await;
            let checkout = clone::Checkout::new(url, branch, commit_sha)?;
            let targets = Targets::new(spec)?;
            targets.cache_scope()?;
            let method = method(&spec.build)?;
            let root = root_directory(spec)?;
            let workdir = Workdir::create(&cfg.builds_dir, spec.deployment_id)?;
            let result = async {
                clone::run(
                    backend,
                    sink,
                    &checkout,
                    &workdir,
                    &cfg.askpass,
                    spec.git_token.as_ref(),
                )
                .await?;
                stages.succeed().await;
                build_stages(
                    cfg,
                    backend,
                    sink,
                    stages,
                    &workdir,
                    &targets,
                    &method,
                    root.as_ref(),
                )
                .await
            }
            .await;
            conclude(result, workdir.remove())
        }
    }
}

/// Resultat d'un build une fois son dossier supprime : un echec de
/// suppression apres un succes (images deja etiquetees) n'est qu'un
/// avertissement (le prochain build du deploiement supprime ce reste).
fn conclude(
    result: Result<BuiltImages, BuildError>,
    removed: Result<(), BuildError>,
) -> Result<BuiltImages, BuildError> {
    if let Err(e) = removed {
        tracing::warn!("{e}");
    }
    result
}

/// Build d'un depot deja present dans `workdir.src()` (sans clone) : etapes
/// `Build` et `Image`, avec le meme rapport que `run`. Le dossier reste a
/// l'appelant.
pub async fn build_checkout<B: BuildBackend>(
    spec: &BuildSpec,
    cfg: &BuildConfig,
    backend: &B,
    sink: &ProgressSink,
    workdir: &Workdir,
) -> Result<BuiltImages, BuildError> {
    let sink = sink.redacting(spec.git_token.as_ref());
    let mut stages = Stages::new(&sink);
    stages.start(Stage::Build).await;
    let result = async {
        let targets = Targets::new(spec)?;
        targets.cache_scope()?;
        let method = method(&spec.build)?;
        let root = root_directory(spec)?;
        if !backend.buildkit_available() {
            return Err(BuildError::Unavailable(UNAVAILABLE));
        }
        build_stages(
            cfg,
            backend,
            &sink,
            &mut stages,
            workdir,
            &targets,
            &method,
            root.as_ref(),
        )
        .await
    }
    .await;
    if result.is_ok() {
        stages.succeed().await;
    }
    stages.finish(&result).await;
    result
}

async fn image_source<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    reference: &ImageRef,
    targets: &Targets,
) -> Result<(), BuildError> {
    if shared::validate::image(&reference.0).is_err() || !crate::docker::valid_image(&reference.0) {
        return Err(BuildError::Invalid(InvalidArg {
            kind: "image",
            reason: "reference d'image",
        }));
    }
    backend.pull(reference, sink).await?;
    backend.tag(reference, &targets.main_tag()).await
}

#[allow(clippy::too_many_arguments)]
async fn build_stages<B: BuildBackend>(
    cfg: &BuildConfig,
    backend: &B,
    sink: &ProgressSink,
    stages: &mut Stages<'_>,
    workdir: &Workdir,
    targets: &Targets,
    method: &Method,
    root: Option<&ValidatedArg>,
) -> Result<BuiltImages, BuildError> {
    if stages.current != Some(Stage::Build) {
        stages.start(Stage::Build).await;
    }
    let mut staged = Vec::new();
    let mut tagged = Vec::new();
    let mut bundle = None;
    let result = async {
        let socket = ValidatedArg::path(&cfg.buildkit_socket)?;
        check_symlinks(&workdir.src())?;
        let src = build_root(workdir, root)?;
        match method {
            Method::Railpack => {
                let prepare = cfg.prepare.as_ref();
                railpack::build(
                    backend,
                    sink,
                    prepare,
                    workdir,
                    &src,
                    targets,
                    &socket,
                    &mut staged,
                )
                .await?
            }
            Method::Dockerfile(path) => {
                dockerfile::build(
                    backend,
                    sink,
                    workdir,
                    &src,
                    targets,
                    &socket,
                    path,
                    &mut staged,
                )
                .await?
            }
            Method::Compose { path, service } => {
                bundle = Some(
                    compose::build(
                        backend,
                        sink,
                        workdir,
                        &src,
                        targets,
                        &socket,
                        path,
                        service,
                        &mut staged,
                    )
                    .await?,
                )
            }
        }
        stages.succeed().await;
        stages.start(Stage::Image).await;
        for s in &staged {
            backend.tag(&s.staging, &s.target).await?;
            tagged.push(s.target.clone());
        }
        Ok::<(), BuildError>(())
    }
    .await;
    // Noms provisoires retires dans tous les cas ; en cas d'echec, les
    // etiquettes finales deja posees aussi (aucune image partielle).
    for s in &staged {
        let _ = backend.remove_image(&s.staging).await;
    }
    if let Err(e) = result {
        for t in &tagged {
            let _ = backend.remove_image(t).await;
        }
        return Err(e);
    }
    // Etape `Image` encore ouverte : reussite rapportee par l'appelant.
    Ok(BuiltImages {
        images: tagged,
        compose: bundle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workdir_removal_failure_after_success_keeps_the_success() {
        let ok = || {
            Ok(BuiltImages {
                images: vec![ImageRef("ciphera/web-production:x".into())],
                compose: None,
            })
        };
        let failed_removal = || Err(BuildError::Workdir("occupe".into()));
        // Images etiquetees : le succes reste un succes (avertissement seul).
        let built = conclude(ok(), failed_removal()).expect("succes conserve");
        assert_eq!(built.images.len(), 1);
        // Un echec du build reste l'erreur rapportee.
        let err = conclude(Err(BuildError::Path("x".into())), failed_removal()).unwrap_err();
        assert!(matches!(err, BuildError::Path(_)), "{err}");
        conclude(ok(), Ok(())).unwrap();
    }
}
