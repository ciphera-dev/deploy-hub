//! Pipeline de build contre un vrai Docker et le vrai buildkitd rootless
//! (lance par `infra::ensure_infra`, comme en production) : ignores par
//! defaut, executes par le job `agent-docker` de la CI
//! (`cargo test -p ciphera-agent -- --ignored`). Exigent `buildctl` et
//! `railpack` dans `/usr/local/lib/ciphera/bin` (versions de install.sh),
//! `git` et le greffon `docker compose`, et le reseau (images de base,
//! frontend Railpack, depot public pour le clone). La ou le noyau restreint
//! les espaces de noms utilisateur, buildkitd tourne sous le profil AppArmor
//! applique `ciphera-buildkitd` (charge par le job).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::query_parameters::{
    ListContainersOptions, RemoveContainerOptions, RemoveImageOptions, RemoveVolumeOptions,
};
use ciphera_agent::build::argv::AllowedBin;
use ciphera_agent::build::local::LocalBackend;
use ciphera_agent::build::railpack;
use ciphera_agent::build::sandbox::{PREPARE_NETWORK, PrepareConfig};
use ciphera_agent::build::{
    self, BuildBackend, BuildConfig, BuildError, BuiltImages, ProgressSink, Workdir, clone,
};
use ciphera_agent::buildkit::{BuildkitManager, DockerFs, ProductionBuildkit};
use ciphera_agent::deploy::compose::ComposePlan;
use ciphera_agent::docker::{BUILD_NETWORK, BollardDocker, INFRA_NETWORK};
use ciphera_agent::infra::{self, BuildkitAccount, InfraConfig, SubIds};
use proto::{AgentToControl, BuildMethod, BuildSource, BuildSpec, ImageRef, Stage, StageStatus};
use shared::ids::{DeploymentId, EnvironmentId, ProjectId};
use tokio::sync::mpsc;

const BUILD_UID: u32 = 10003;
const BUILD_SUBIDS: SubIds = SubIds {
    start: 400000,
    count: 65536,
};
/// Depot public minuscule et stable, et son commit de `master`.
const PUBLIC_REPO: &str = "https://github.com/octocat/Hello-World";
const PUBLIC_COMMIT: &str = "7fd1a60b01f91b314f59955a4e4d4e80d8edf11d";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        // Court : le chemin du socket d'un projet (`<tmp>/buildkit/<uuid>/
        // buildkitd.sock`) doit tenir dans les 107 octets d'un socket Unix.
        let dir =
            std::env::temp_dir().join(format!("cbit-{}", &DeploymentId::new().to_string()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn subdir(&self, name: &str, mode: u32) -> PathBuf {
        let p = self.0.join(name);
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Conteneurs, volume et reseaux de l'infrastructure, et images du test,
/// supprimes en fin de test meme si une assertion echoue.
struct Cleanup {
    images: Vec<String>,
}

impl Cleanup {
    async fn run(&self) {
        let api = Docker::connect_with_socket_defaults().unwrap();
        // PostgreSQL, buildkitd partage d'avant et buildkitd de chaque projet.
        let all = api
            .list_containers(Some(ListContainersOptions {
                all: true,
                ..Default::default()
            }))
            .await
            .unwrap_or_default();
        let names = all
            .iter()
            .flat_map(|c| c.names.clone().unwrap_or_default())
            .map(|n| n.trim_start_matches('/').to_string())
            .filter(|n| n == infra::POSTGRES_CONTAINER || n.starts_with("ciphera-buildkitd"));
        for name in names {
            let options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            let _ = api.remove_container(&name, Some(options)).await;
        }
        let _ = api
            .remove_volume(
                infra::POSTGRES_VOLUME,
                Some(RemoveVolumeOptions { force: true }),
            )
            .await;
        for network in [INFRA_NETWORK, BUILD_NETWORK, PREPARE_NETWORK] {
            let _ = api.remove_network(network).await;
        }
        for image in &self.images {
            let options = RemoveImageOptions {
                force: true,
                ..Default::default()
            };
            let _ = api.remove_image(image, Some(options), None).await;
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(self.run()));
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Bac a sable de production (outils de /usr/local/lib/ciphera/bin, image
/// epinglee de buildkitd), au compte du test.
fn prepare_config() -> PrepareConfig {
    let image = infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit").unwrap();
    PrepareConfig::production(image, (BUILD_UID, BUILD_UID))
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn spec(project: &str, source: BuildSource, build: BuildMethod) -> BuildSpec {
    BuildSpec {
        deployment_id: DeploymentId::new(),
        project: project.into(),
        environment: "production".into(),
        source,
        build,
        git_token: None,
        root_directory: None,
        project_id: Some(ProjectId::new()),
        environment_id: Some(EnvironmentId::new()),
    }
}

/// Build Dockerfile du depot local `repo` pour le projet `ids` (memes noms
/// de projet et d'environnement pour tous : seuls les UUID different).
fn dockerfile_spec(project: &str, ids: (ProjectId, EnvironmentId)) -> BuildSpec {
    BuildSpec {
        project_id: Some(ids.0),
        environment_id: Some(ids.1),
        ..spec(
            project,
            local_source(),
            BuildMethod::Dockerfile {
                path: "Dockerfile".into(),
            },
        )
    }
}

/// Reglages d'infrastructure de production dans `tmp` (sockets en setgid,
/// compte du test).
fn infra_config(tmp: &TempDir) -> InfraConfig {
    let secret_dir = tmp.subdir("secret", 0o777);
    let password_file = secret_dir.join("db_password");
    std::fs::write(&password_file, format!("pw-{}", DeploymentId::new())).unwrap();
    std::fs::set_permissions(&password_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    // Socket de buildkitd : dossier setgid au groupe du test (comme
    // /run/ciphera-agent/buildkit au groupe ciphera-agent) pour que le
    // dossier d'un projet et son socket 0660 soient joignables par ce
    // processus.
    // Profil AppArmor applique la ou le noyau restreint les espaces de noms
    // utilisateur (job agent-docker, Ubuntu 24.04) : les builds Dockerfile,
    // Railpack et Compose passent sous ce profil.
    if Path::new("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").exists() {
        assert!(
            infra::buildkit_apparmor_profile().is_some(),
            "{} absent (voir le job agent-docker)",
            infra::BUILDKIT_APPARMOR_FILE
        );
    }
    InfraConfig {
        postgres_secret_dir: secret_dir,
        postgres_socket_dir: tmp.subdir("pg", 0o777),
        postgres_user: (10001, 10001),
        buildkit_socket_dir: tmp.subdir("buildkit", 0o2777),
        buildkit_apparmor_profile: infra::buildkit_apparmor_profile(),
        buildkit_account: Some(BuildkitAccount {
            user: (BUILD_UID, BUILD_UID),
            subuids: BUILD_SUBIDS,
            subgids: BUILD_SUBIDS,
        }),
        buildkit_state_dir: tmp.subdir("buildkit-state", 0o777),
        buildkit_identity_dir: tmp.0.join("buildkit-identity"),
        buildkit_usage_dir: tmp.0.join("buildkit-usage"),
        buildkit_resources: infra::BuildkitResources::detect(&tmp.0),
    }
}

/// Gestionnaire de production (un buildkitd par projet) dans `cfg`.
fn manager_for(cfg: &InfraConfig) -> Arc<ProductionBuildkit> {
    let fs = DockerFs::connect(
        cfg,
        infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit").unwrap(),
        (BUILD_UID, BUILD_UID),
    )
    .unwrap();
    Arc::new(BuildkitManager::new(
        Arc::new(BollardDocker::connect().unwrap()),
        fs,
        cfg.clone(),
    ))
}

/// Infrastructure de production (`infra::ensure_infra`) dans `tmp` et
/// gestionnaire de buildkitd par projet, prets a construire : reglages du
/// pipeline, backend et gestionnaire.
async fn start_buildkit(tmp: &TempDir) -> (BuildConfig, LocalBackend, Arc<ProductionBuildkit>) {
    let (build_cfg, backend, manager, _cfg) = start_buildkit_with_config(tmp).await;
    (build_cfg, backend, manager)
}

async fn start_buildkit_with_config(
    tmp: &TempDir,
) -> (
    BuildConfig,
    LocalBackend,
    Arc<ProductionBuildkit>,
    InfraConfig,
) {
    let docker = BollardDocker::connect().unwrap();
    let cfg = infra_config(tmp);
    infra::ensure_infra(&docker, &cfg).await.unwrap();
    let manager = manager_for(&cfg);
    let builds = tmp.subdir("builds", 0o700);
    // Le socket de `BuildConfig` n'est pas lu : chaque projet a le sien.
    let unused = tmp.0.join("unused.sock");
    let build_cfg = BuildConfig {
        builds_dir: builds.to_str().unwrap().to_string(),
        buildkit_socket: unused.to_str().unwrap().to_string(),
        askpass: std::env::current_exe().unwrap(),
        prepare: Some(prepare_config()),
    };
    let backend = LocalBackend::connect(&unused)
        .unwrap()
        .with_buildkit(manager.clone());
    (build_cfg, backend, manager, cfg)
}

/// Sortie standard de l'image `tag` lancee sans reseau.
fn run_image(tag: &str) -> String {
    let out = std::process::Command::new("docker")
        .args(["run", "--rm", "--network", "none", tag])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{tag} : {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// `buildctl build` lance directement (hors du pipeline et de ses
/// controles) vers buildkitd : succes et sortie d'erreur.
fn raw_buildctl(socket: &Path, home: &Path, args: &[&str]) -> (bool, String) {
    let out = std::process::Command::new(format!(
        "{}/buildctl",
        ciphera_agent::build::argv::TOOLS_DIR
    ))
    .arg("--addr")
    .arg(format!("unix://{}", socket.display()))
    .args(["build", "--progress", "plain"])
    .args(args)
    .env("HOME", home)
    .output()
    .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn local_source() -> BuildSource {
    // Non lu par `build_checkout` (le depot est deja dans le dossier).
    BuildSource::Git {
        url: PUBLIC_REPO.into(),
        branch: "master".into(),
        commit_sha: PUBLIC_COMMIT.into(),
    }
}

/// Construit un depot local (copie de `repo`) ; renvoie le resultat et les
/// etapes rapportees.
async fn build_local(
    cfg: &BuildConfig,
    backend: &LocalBackend,
    spec: &BuildSpec,
    repo: &Path,
) -> (Result<BuiltImages, BuildError>, Vec<(Stage, StageStatus)>) {
    let workdir = Workdir::create(&cfg.builds_dir, spec.deployment_id).unwrap();
    copy_dir(repo, Path::new(&workdir.src()));
    let (tx, rx) = mpsc::channel(100_000);
    let sink = ProgressSink::new(spec.deployment_id, tx);
    let result = build::build_checkout(spec, cfg, backend, &sink, &workdir).await;
    workdir.remove().unwrap();
    drop(sink);
    (result, collect(rx).await)
}

async fn collect(mut rx: mpsc::Receiver<AgentToControl>) -> Vec<(Stage, StageStatus)> {
    let mut out = Vec::new();
    while let Some(m) = rx.recv().await {
        if let AgentToControl::DeployProgress { stage, status, .. } = m {
            out.push((stage, status));
        }
    }
    out
}

async fn image_exists(api: &Docker, image: &str) -> bool {
    api.inspect_image(image).await.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn fixtures_build_with_buildkit_and_load_into_docker() {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let tmp = TempDir::new();
    let mut cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (build_cfg, backend, _manager) = start_buildkit(&tmp).await;
    let builds = PathBuf::from(&build_cfg.builds_dir);
    let project = format!("it{}", &DeploymentId::new().to_string()[..8]);

    // Dockerfile : image chargee et etiquetee, nom provisoire retire.
    let s = spec(
        &project,
        local_source(),
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
    );
    let tag = format!("ciphera.invalid/{project}-production:{}", s.deployment_id);
    cleanup.images.push(tag.clone());
    let (result, stages) = build_local(&build_cfg, &backend, &s, &fixture("dockerfile-app")).await;
    let built = result.unwrap_or_else(|e| panic!("dockerfile-app : {e} ({stages:?})"));
    assert_eq!(built.images, [ImageRef(tag.clone())]);
    assert!(image_exists(&api, &tag).await);
    assert!(
        !image_exists(
            &api,
            &format!("ciphera.invalid/staging/{}:build", s.deployment_id)
        )
        .await
    );
    assert_eq!(stages.last(), Some(&(Stage::Image, StageStatus::Succeeded)));

    // Railpack (Node) : plan prepare puis frontend epingle.
    let s = spec(&project, local_source(), BuildMethod::Railpack);
    let tag = format!("ciphera.invalid/{project}-production:{}", s.deployment_id);
    cleanup.images.push(tag.clone());
    let (result, stages) = build_local(&build_cfg, &backend, &s, &fixture("node-app")).await;
    let built = result.unwrap_or_else(|e| panic!("node-app : {e} ({stages:?})"));
    assert_eq!(built.images, [ImageRef(tag.clone())]);
    assert!(image_exists(&api, &tag).await);

    // Compose : seul le service qui a une section build est construit.
    let s = spec(
        &project,
        local_source(),
        BuildMethod::Compose {
            path: "compose.yaml".into(),
            service: "web".into(),
        },
    );
    let tag = format!(
        "ciphera.invalid/{project}-production/web:{}",
        s.deployment_id
    );
    cleanup.images.push(tag.clone());
    let (result, stages) = build_local(&build_cfg, &backend, &s, &fixture("compose-app")).await;
    let built = result.unwrap_or_else(|e| panic!("compose-app : {e} ({stages:?})"));
    assert_eq!(built.images, [ImageRef(tag.clone())]);
    assert!(image_exists(&api, &tag).await);
    // Ensemble a deployer : sortie reelle de `docker compose config`, image
    // construite a la place du build, accepte par le plan de deploiement.
    let bundle: serde_json::Value =
        serde_json::from_str(built.compose.as_deref().expect("ensemble Compose")).unwrap();
    assert_eq!(bundle["services"]["web"]["image"], tag.as_str());
    assert!(bundle["services"]["web"].get("build").is_none());
    let plan = ComposePlan::new(&bundle, "web", ProjectId::new(), s.deployment_id)
        .unwrap_or_else(|e| panic!("plan Compose : {e}\n{bundle}"));
    let order: Vec<&str> = plan.order.iter().map(|a| a.as_str()).collect();
    assert_eq!(order, ["cache"]);

    // Build en echec : etape Build en echec, aucune image (ni finale, ni
    // provisoire).
    let broken = tmp.subdir("broken", 0o755);
    std::fs::write(
        broken.join("Dockerfile"),
        "FROM docker.io/library/busybox:1.37\nRUN exit 3\n",
    )
    .unwrap();
    let s = spec(
        &project,
        local_source(),
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
    );
    let tag = format!("ciphera.invalid/{project}-production:{}", s.deployment_id);
    let (result, stages) = build_local(&build_cfg, &backend, &s, &broken).await;
    assert!(matches!(result, Err(BuildError::Tool { .. })), "{result:?}");
    assert!(
        matches!(
            stages.last(),
            Some((Stage::Build, StageStatus::Failed { .. }))
        ),
        "{stages:?}"
    );
    assert!(!image_exists(&api, &tag).await);
    assert!(
        !image_exists(
            &api,
            &format!("ciphera.invalid/staging/{}:build", s.deployment_id)
        )
        .await
    );

    // Clone reel d'un depot public au commit demande (sans jeton), puis
    // echec propre au build (ce depot n'a pas de Dockerfile) ; dossier du
    // build supprime.
    let s = spec(
        &project,
        local_source(),
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
    );
    let (tx, rx) = mpsc::channel(10_000);
    let sink = ProgressSink::new(s.deployment_id, tx);
    let result = build::run(&s, &build_cfg, &backend, &sink).await;
    drop(sink);
    let stages = collect(rx).await;
    assert!(matches!(result, Err(BuildError::Path(_))), "{result:?}");
    assert_eq!(
        stages[..2],
        [
            (Stage::Clone, StageStatus::Started),
            (Stage::Clone, StageStatus::Succeeded)
        ],
        "{stages:?}"
    );
    assert!(!builds.join(s.deployment_id.to_string()).exists());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et outils de build : job agent-docker de la CI"]
async fn railpack_prepare_runs_sandboxed_on_node_app() {
    use std::os::unix::fs::MetadataExt;
    let api = Docker::connect_with_socket_defaults().unwrap();
    let tmp = TempDir::new();
    let builds = tmp.subdir("builds", 0o700);
    let backend = LocalBackend::connect(tmp.0.join("absent.sock")).unwrap();
    let deployment = DeploymentId::new();
    let workdir = Workdir::create(builds.to_str().unwrap(), deployment).unwrap();
    copy_dir(&fixture("node-app"), Path::new(&workdir.src()));
    let (tx, rx) = mpsc::channel(100_000);
    let sink = ProgressSink::new(deployment, tx);
    let cfg = prepare_config();
    let result = railpack::prepare(&backend, &sink, Some(&cfg), &workdir, &workdir.src()).await;
    drop(sink);
    let logs: Vec<String> = {
        let mut rx = rx;
        let mut out = Vec::new();
        while let Some(m) = rx.recv().await {
            if let AgentToControl::LogLine(l) = m {
                out.push(l.line);
            }
        }
        out
    };
    result.unwrap_or_else(|e| panic!("prepare : {e}\n{}", logs.join("\n")));

    // Plan ecrit par le compte du bac a sable (ni root, ni l'agent).
    let plan = Path::new(&workdir.plan()).join("railpack-plan.json");
    let text = std::fs::read_to_string(&plan).unwrap();
    assert!(text.contains("node"), "{text}");
    assert_eq!(std::fs::metadata(&plan).unwrap().uid(), BUILD_UID);
    // Conteneur jetable supprime ; pont dedie gere par Ciphera, non interne.
    let name = format!("ciphera-prepare-{deployment}");
    assert!(api.inspect_container(&name, None).await.is_err());
    let network = api.inspect_network(PREPARE_NETWORK, None).await.unwrap();
    assert_eq!(network.internal, Some(false));
    assert_eq!(
        network
            .labels
            .unwrap_or_default()
            .get(ciphera_agent::docker::MANAGED_LABEL)
            .map(String::as_str),
        Some("true")
    );
    workdir.remove().unwrap();
    let _ = api.remove_network(PREPARE_NETWORK).await;
}

/// Railpack n'execute jamais d'autre mise que celui epingle : avec un faux
/// mise epingle (fichier vide, inexecutable), `prepare` echoue. Railpack
/// prend donc le mise monte ; et un mise qu'il telechargerait atterrirait
/// dans un tmpfs `noexec` (voir `sandbox`), donc ne pourrait pas tourner.
/// Le test precedent (vrai mise epingle) reussit : c'est lui qui sert.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et outils de build : job agent-docker de la CI"]
async fn railpack_prepare_runs_only_the_pinned_mise() {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let tmp = TempDir::new();
    let builds = tmp.subdir("builds", 0o700);
    let tools = tmp.subdir("tools", 0o755);
    let fake_mise = tools.join("mise-fake");
    std::fs::write(&fake_mise, "").unwrap();
    std::fs::set_permissions(&fake_mise, std::fs::Permissions::from_mode(0o755)).unwrap();
    let backend = LocalBackend::connect(tmp.0.join("absent.sock")).unwrap();
    let deployment = DeploymentId::new();
    let workdir = Workdir::create(builds.to_str().unwrap(), deployment).unwrap();
    copy_dir(&fixture("node-app"), Path::new(&workdir.src()));
    let (tx, rx) = mpsc::channel(100_000);
    let sink = ProgressSink::new(deployment, tx);
    let cfg = PrepareConfig {
        mise: fake_mise,
        ..prepare_config()
    };
    let result = railpack::prepare(&backend, &sink, Some(&cfg), &workdir, &workdir.src()).await;
    drop(sink);
    drop(rx);
    assert!(
        matches!(result, Err(BuildError::Tool { .. })),
        "un mise autre que celui epingle a servi : {result:?}"
    );
    assert!(
        !Path::new(&workdir.plan())
            .join("railpack-plan.json")
            .exists()
    );
    let name = format!("ciphera-prepare-{deployment}");
    assert!(api.inspect_container(&name, None).await.is_err());
    workdir.remove().unwrap();
    let _ = api.remove_network(PREPARE_NETWORK).await;
}

/// Copie du depot `repo` dans un nouveau dossier de `tmp`, son Dockerfile
/// precede de `prefix` (directive `syntax`, par exemple).
fn with_prefix(tmp: &TempDir, repo: &Path, prefix: &str) -> PathBuf {
    let dir = tmp.0.join(format!("repo-{}", DeploymentId::new()));
    copy_dir(repo, &dir);
    let dockerfile = dir.join("Dockerfile");
    let text = std::fs::read_to_string(&dockerfile).unwrap();
    std::fs::write(&dockerfile, format!("{prefix}{text}")).unwrap();
    dir
}

/// Cloisonnement des caches de montage, Dockerfile precede de `prefix` :
/// le projet A (environnement de production) ecrit un marqueur dans
/// `--mount=type=cache,id=x` et dans un cache sans id (chemin cible
/// `/root/.npm`). Ne le voient pas : le projet B (memes noms de projet et
/// d'environnement, autres UUID), y compris en visant le cache de A par un
/// id qui remonte, ni un autre environnement du meme projet. Temoin : un
/// autre build du meme environnement le voit.
async fn assert_cache_isolation(prefix: &str) {
    let tmp = TempDir::new();
    let mut cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (build_cfg, backend, _manager) = start_buildkit(&tmp).await;
    let project = format!("it{}", &DeploymentId::new().to_string()[..8]);
    let a = (ProjectId::new(), EnvironmentId::new());
    let b = (ProjectId::new(), EnvironmentId::new());
    let a_staging = (a.0, EnvironmentId::new());
    let writer = with_prefix(&tmp, &fixture("cache-writer"), prefix);
    let reader = with_prefix(&tmp, &fixture("cache-reader"), prefix);

    let mut build = async |ids: (ProjectId, EnvironmentId), repo: &Path, what: &str| {
        let s = dockerfile_spec(&project, ids);
        let tag = format!("ciphera.invalid/{project}-production:{}", s.deployment_id);
        cleanup.images.push(tag.clone());
        let (result, stages) = build_local(&build_cfg, &backend, &s, repo).await;
        result.unwrap_or_else(|e| panic!("{what} ({prefix:?}) : {e} ({stages:?})"));
        run_image(&tag)
    };

    build(a, &writer, "cache-writer (A)").await;
    assert_eq!(
        build(b, &reader, "cache-reader (B)").await,
        "absent\nabsent\n",
        "le projet B voit le cache du projet A"
    );
    assert_eq!(
        build(a_staging, &reader, "cache-reader (A, autre environnement)").await,
        "absent\nabsent\n",
        "un autre environnement de A voit le cache de sa production"
    );

    // B vise le cache de A par un id qui sort de son espace de noms :
    // BuildKit garde l'id tel quel (chaine opaque, sous l'espace de noms de
    // B), donc un cache vide.
    for id in [
        format!("../{}-{}/x", a.0, a.1),
        format!("/{}-{}/x", a.0, a.1),
    ] {
        let repo = tmp.0.join(format!("escape-{}", DeploymentId::new()));
        std::fs::create_dir(&repo).unwrap();
        std::fs::write(
            repo.join("Dockerfile"),
            format!(
                "{prefix}FROM docker.io/library/busybox:1.37\n\
                 RUN --mount=type=cache,id={id},target=/c \\\n    \
                 mkdir -p /srv && if [ -e /c/marker ]; then echo vu; else echo absent; fi > /srv/r.txt\n\
                 CMD [\"cat\", \"/srv/r.txt\"]\n"
            ),
        )
        .unwrap();
        assert_eq!(
            build(b, &repo, &format!("evasion {id}")).await,
            "absent\n",
            "id {id}"
        );
    }

    // Temoin : le meme environnement (memes UUID) retrouve ses caches.
    assert_eq!(
        build(a, &reader, "cache-reader (A)").await,
        "vu\nvu\n",
        "temoin : cache de A introuvable"
    );
}

/// Frontend `dockerfile.v0` integre a BuildKit.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn cache_mount_not_shared_between_projects() {
    assert_cache_isolation("").await;
}

/// Frontend telecharge `docker/dockerfile:1` (image flottante) : l'espace
/// de noms des caches lui est transmis et il l'applique.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn cache_mount_not_shared_through_docker_dockerfile_frontend() {
    assert_cache_isolation("# syntax=docker/dockerfile:1\n").await;
}

/// Un frontend arbitraire est refuse : par le pipeline (directive `syntax`
/// d'un Dockerfile, message clair, sans appel a BuildKit) et par buildkitd
/// lui-meme (`allowedRepositories` de sa configuration), en direct par
/// `gateway.v0` comme par la directive `syntax` d'un Dockerfile.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn foreign_frontend_refused() {
    let tmp = TempDir::new();
    let mut cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (build_cfg, backend, manager) = start_buildkit(&tmp).await;
    let project = format!("it{}", &DeploymentId::new().to_string()[..8]);

    let repo = tmp.subdir("foreign", 0o755);
    std::fs::write(
        repo.join("Dockerfile"),
        "# syntax=docker.io/someone/frontend\nFROM docker.io/library/busybox:1.37\n",
    )
    .unwrap();
    let s = dockerfile_spec(&project, (ProjectId::new(), EnvironmentId::new()));
    let tag = format!("ciphera.invalid/{project}-production:{}", s.deployment_id);
    cleanup.images.push(tag.clone());
    let (result, stages) = build_local(&build_cfg, &backend, &s, &repo).await;
    assert!(matches!(result, Err(BuildError::Frontend(_))), "{result:?}");
    assert!(
        matches!(
            stages.last(),
            Some((Stage::Build, StageStatus::Failed { message })) if message.contains("frontend")
        ),
        "{stages:?}"
    );
    let api = Docker::connect_with_socket_defaults().unwrap();
    assert!(!image_exists(&api, &tag).await);

    // buildkitd refuse de lui-meme une image existante qui n'est pas dans
    // sa liste (hors du pipeline, donc sans le controle de l'agent).
    let lease = manager.ensure(ProjectId::new()).await.unwrap();
    let socket = lease.socket().unwrap().to_path_buf();
    let home = tmp.subdir("raw-home", 0o700);
    let (ok, stderr) = raw_buildctl(
        &socket,
        &home,
        &[
            "--frontend",
            "gateway.v0",
            "--opt",
            "source=docker.io/library/busybox:1.37",
        ],
    );
    assert!(
        !ok && stderr.contains("not an allowed gateway source"),
        "{stderr}"
    );
    let raw = tmp.subdir("raw-dockerfile", 0o755);
    std::fs::write(
        raw.join("Dockerfile"),
        "# syntax=docker.io/library/busybox:1.37\nFROM docker.io/library/busybox:1.37\n",
    )
    .unwrap();
    let dir = raw.to_str().unwrap();
    let (ok, stderr) = raw_buildctl(
        &socket,
        &home,
        &[
            "--frontend",
            "dockerfile.v0",
            "--local",
            &format!("context={dir}"),
            "--local",
            &format!("dockerfile={dir}"),
        ],
    );
    assert!(
        !ok && stderr.contains("not an allowed gateway source"),
        "{stderr}"
    );
}

/// `docker/dockerfile:1` reste utilisable (directive `syntax`), cache de
/// montage compris.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn docker_dockerfile_frontend_allowed() {
    let tmp = TempDir::new();
    let mut cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (build_cfg, backend, _manager) = start_buildkit(&tmp).await;
    let project = format!("it{}", &DeploymentId::new().to_string()[..8]);
    let repo = tmp.subdir("syntax", 0o755);
    std::fs::write(
        repo.join("Dockerfile"),
        "# syntax=docker/dockerfile:1\n\
         FROM docker.io/library/busybox:1.37\n\
         RUN --mount=type=cache,target=/root/.npm echo ok > /root/.npm/ok\n",
    )
    .unwrap();
    let s = dockerfile_spec(&project, (ProjectId::new(), EnvironmentId::new()));
    let tag = format!("ciphera.invalid/{project}-production:{}", s.deployment_id);
    cleanup.images.push(tag.clone());
    let (result, stages) = build_local(&build_cfg, &backend, &s, &repo).await;
    let built = result.unwrap_or_else(|e| panic!("docker/dockerfile:1 : {e} ({stages:?})"));
    assert_eq!(built.images, [ImageRef(tag)]);
}

/// Etapes rapportees avec l'instant de leur reception.
async fn collect_timed(
    mut rx: mpsc::Receiver<AgentToControl>,
) -> Vec<(Stage, StageStatus, Instant)> {
    let mut out = Vec::new();
    while let Some(m) = rx.recv().await {
        if let AgentToControl::DeployProgress { stage, status, .. } = m {
            out.push((stage, status, Instant::now()));
        }
    }
    out
}

/// `build_local` avec l'instant de chaque etape.
async fn build_timed(
    cfg: &BuildConfig,
    backend: &LocalBackend,
    spec: &BuildSpec,
    repo: &Path,
) -> (
    Result<BuiltImages, BuildError>,
    Vec<(Stage, StageStatus, Instant)>,
) {
    let workdir = Workdir::create(&cfg.builds_dir, spec.deployment_id).unwrap();
    copy_dir(repo, Path::new(&workdir.src()));
    let (tx, rx) = mpsc::channel(100_000);
    let sink = ProgressSink::new(spec.deployment_id, tx);
    let collector = tokio::spawn(collect_timed(rx));
    let result = build::build_checkout(spec, cfg, backend, &sink, &workdir).await;
    workdir.remove().unwrap();
    drop(sink);
    (result, collector.await.unwrap())
}

fn instant_of(stages: &[(Stage, StageStatus, Instant)], stage: Stage, started: bool) -> Instant {
    stages
        .iter()
        .find(|(s, st, _)| {
            *s == stage
                && if started {
                    matches!(st, StageStatus::Started)
                } else {
                    matches!(st, StageStatus::Succeeded)
                }
        })
        .map(|(_, _, t)| *t)
        .unwrap_or_else(|| panic!("etape {stage:?} absente : {stages:?}"))
}

/// Deux builds lances ensemble : un seul tourne a la fois par noeud. Le
/// second n'ouvre son etape `Build` qu'apres la fin du `Build` du premier
/// (chaque `RUN` dort 6 s : sans serie, ils se chevaucheraient).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn builds_are_serialized() {
    let tmp = TempDir::new();
    let mut cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (build_cfg, backend, _manager) = start_buildkit(&tmp).await;
    let project = format!("it{}", &DeploymentId::new().to_string()[..8]);

    let mut specs = Vec::new();
    let mut repos = Vec::new();
    for n in 0..2 {
        let repo = tmp.subdir(&format!("slow-{n}"), 0o755);
        std::fs::write(
            repo.join("Dockerfile"),
            format!(
                "FROM docker.io/library/busybox:1.37\nRUN echo {} && sleep 6\n",
                DeploymentId::new()
            ),
        )
        .unwrap();
        let s = dockerfile_spec(&project, (ProjectId::new(), EnvironmentId::new()));
        cleanup.images.push(format!(
            "ciphera.invalid/{project}-production:{}",
            s.deployment_id
        ));
        specs.push(s);
        repos.push(repo);
    }
    let (a, b) = tokio::join!(
        build_timed(&build_cfg, &backend, &specs[0], &repos[0]),
        build_timed(&build_cfg, &backend, &specs[1], &repos[1]),
    );
    for (result, stages) in [&a, &b] {
        assert!(result.is_ok(), "{result:?} ({stages:?})");
    }
    let a_first = instant_of(&a.1, Stage::Build, true) <= instant_of(&b.1, Stage::Build, true);
    let (first, second) = if a_first { (&a.1, &b.1) } else { (&b.1, &a.1) };
    let first_done = instant_of(first, Stage::Build, false);
    let second_start = instant_of(second, Stage::Build, true);
    assert!(
        second_start >= first_done,
        "les deux builds se chevauchent : le second demarre {:?} avant la fin du premier",
        first_done.duration_since(second_start)
    );
}

/// Un outil annule (delai depasse) est tue avec ses descendants : le
/// processus lance en arriere-plan par l'outil ne survit pas.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et outils de build : job agent-docker de la CI"]
async fn build_timeout_kills_children() {
    let tmp = TempDir::new();
    let pid_file = tmp.0.join("child.pid");
    let backend = LocalBackend::connect(tmp.0.join("absent.sock")).unwrap();
    let (tx, _rx) = mpsc::channel(100);
    let sink = ProgressSink::new(DeploymentId::new(), tx);
    // L'outil lance un `sleep` en arriere-plan, note son pid, puis attend.
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c").arg(format!(
        "sleep 300 & echo $! > {} ; wait",
        pid_file.display()
    ));
    let run = BuildBackend::exec(&backend, AllowedBin::Git, cmd, &sink, false);
    let timed_out = tokio::time::timeout(Duration::from_secs(3), run).await;
    assert!(timed_out.is_err(), "l'outil devait depasser son delai");
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("pid du processus enfant")
        .trim()
        .parse()
        .unwrap();
    // Mort (ou zombie en attente d'etre recolte) peu apres l'arret du groupe.
    let start = Instant::now();
    loop {
        let alive = std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            !stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .trim_start()
                .starts_with('Z')
        });
        if !alive {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "le processus enfant {pid} a survecu a l'arret de l'outil"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `git` dans `dir` (identite explicite).
fn git_in(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new(AllowedBin::Git.program())
        .args(["-c", "user.name=t", "-c", "user.email=t@example.org"])
        .args(["-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} : {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Clone d'un depot plus gros que le plafond : arrete et refuse ; sous le
/// plafond, le meme depot passe. Le depot (fichier de 24 Mio peu
/// compressible) est genere localement et clone par `file://`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "git reel : job agent-docker de la CI"]
async fn clone_over_cap_fails() {
    let tmp = TempDir::new();
    let source = tmp.subdir("source", 0o755);
    git_in(&source, &["init", "-q", "-b", "main"]);
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let blob: Vec<u8> = (0..24usize << 20)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect();
    std::fs::write(source.join("gros.bin"), blob).unwrap();
    git_in(&source, &["add", "gros.bin"]);
    git_in(&source, &["commit", "-q", "-m", "gros"]);

    let backend = LocalBackend::connect(tmp.0.join("absent.sock")).unwrap();
    let (tx, _rx) = mpsc::channel(1000);
    let sink = ProgressSink::new(DeploymentId::new(), tx);
    let clone_cmd = |dest: &Path| {
        let mut cmd = std::process::Command::new(AllowedBin::Git.program());
        cmd.args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "-q",
            "--no-local",
        ])
        .arg(format!("file://{}", source.display()))
        .arg(dest)
        .env_clear()
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("PATH", "/usr/bin:/bin");
        cmd
    };

    let small = tmp.0.join("clone-small");
    let err = clone::exec_capped(&backend, clone_cmd(&small), &sink, &small, 8 << 20)
        .await
        .expect_err("clone au-dela du plafond");
    assert!(err.to_string().contains("trop volumineux"), "{err}");

    let ok = tmp.0.join("clone-ok");
    clone::exec_capped(&backend, clone_cmd(&ok), &sink, &ok, 256 << 20)
        .await
        .expect("clone sous le plafond");
    assert!(ok.join("gros.bin").is_file());
}

/// Le conteneur buildkitd du projet tourne (`Some(true)`), est arrete
/// (`Some(false)`) ou n'existe pas (`None`).
async fn buildkitd_running(api: &Docker, project: ProjectId) -> Option<bool> {
    let name = infra::buildkit_container_name(project);
    api.inspect_container(&name, None)
        .await
        .ok()
        .map(|i| i.state.and_then(|s| s.running).unwrap_or(false))
}

/// Dossier d'etat du projet present ? Lu par un conteneur root jetable (le
/// dossier est en 0700 au compte de buildkitd, illisible pour le test).
fn state_exists(state_root: &Path, project: ProjectId) -> bool {
    std::process::Command::new("docker")
        .args(["run", "--rm", "--network", "none", "-v"])
        .arg(format!("{}:/s:ro", state_root.display()))
        .arg("docker.io/library/busybox:1.37")
        .args(["test", "-d"])
        .arg(format!("/s/p/{project}"))
        .status()
        .unwrap()
        .success()
}

/// Dockerfile avec un cache de montage (`id=x`) : `CACHE_WRITER` y pose un
/// marqueur, `CACHE_READER` dit s'il le voit.
///
/// Les jetons sont coupes par `""` dans le Dockerfile : BuildKit recopie la
/// ligne de commande du `RUN` dans sa sortie, et un jeton ecrit en clair y
/// serait trouve quel que soit l'etat du cache. Le shell recolle `MARKER-...`
/// a l'affichage, seul le vrai resultat contient donc le jeton complet.
const CACHE_WRITER: &str = "FROM docker.io/library/busybox:1.37\n\
    RUN --mount=type=cache,id=x,target=/c echo marker > /c/marker\n";
const CACHE_READER: &str = "FROM docker.io/library/busybox:1.37\n\
    RUN --mount=type=cache,id=x,target=/c \
    if [ -e /c/marker ]; then echo MARK\"\"ER-SEEN; else echo MARK\"\"ER-ABSENT; fi\n";

/// `buildctl` direct sur `socket` avec le Dockerfile `dockerfile` et l'espace
/// de noms de cache impose `namespace` (hors du pipeline, qui le derive des
/// UUID : ici le meme pour deux projets).
fn raw_dockerfile_build(
    tmp: &TempDir,
    socket: &Path,
    dockerfile: &str,
    namespace: &str,
) -> (bool, String) {
    let dir = tmp.0.join(format!("raw-{}", DeploymentId::new()));
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.join("Dockerfile"), dockerfile).unwrap();
    let home = tmp.subdir(&format!("home-{}", DeploymentId::new()), 0o700);
    let d = dir.to_str().unwrap();
    raw_buildctl(
        socket,
        &home,
        &[
            "--frontend",
            "dockerfile.v0",
            "--local",
            &format!("context={d}"),
            "--local",
            &format!("dockerfile={d}"),
            "--opt",
            &format!("build-arg:BUILDKIT_CACHE_MOUNT_NS={namespace}"),
        ],
    )
}

/// Deux projets : deux conteneurs buildkitd, deux dossiers d'etat et deux
/// sockets distincts ; le buildkitd du premier est arrete quand le second
/// demarre (un seul a la fois sur le noeud).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn projects_use_distinct_buildkitd_and_state() {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let tmp = TempDir::new();
    let cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (_build_cfg, _backend, manager, cfg) = start_buildkit_with_config(&tmp).await;
    let (a, b) = (ProjectId::new(), ProjectId::new());

    let lease_a = manager.ensure(a).await.unwrap();
    let socket_a = lease_a.socket().unwrap().to_path_buf();
    assert_eq!(socket_a, cfg.buildkit_project_socket(a));
    assert_eq!(buildkitd_running(&api, a).await, Some(true));
    assert!(state_exists(&cfg.buildkit_state_dir, a));
    // Un build de A en cours : le buildkitd de B ne le remplace pas.
    assert!(manager.ensure(b).await.is_err());
    assert_eq!(buildkitd_running(&api, a).await, Some(true));
    drop(lease_a);

    let lease_b = manager.ensure(b).await.unwrap();
    let socket_b = lease_b.socket().unwrap().to_path_buf();
    assert_ne!(socket_a, socket_b);
    assert_eq!(buildkitd_running(&api, b).await, Some(true));
    assert_eq!(buildkitd_running(&api, a).await, Some(false));
    assert_ne!(
        infra::buildkit_container_name(a),
        infra::buildkit_container_name(b)
    );
    assert!(state_exists(&cfg.buildkit_state_dir, a));
    assert!(state_exists(&cfg.buildkit_state_dir, b));
}

/// Meme espace de noms de cache impose aux deux projets : le marqueur du
/// premier reste invisible au second (etats separes, pas seulement des
/// espaces de noms). Temoin : le premier projet retrouve son marqueur.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn cache_marker_invisible_even_with_same_namespace() {
    let tmp = TempDir::new();
    let cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (_build_cfg, _backend, manager) = start_buildkit(&tmp).await;
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let namespace = format!("forced-{}", DeploymentId::new());

    let lease = manager.ensure(a).await.unwrap();
    let (ok, stderr) =
        raw_dockerfile_build(&tmp, lease.socket().unwrap(), CACHE_WRITER, &namespace);
    assert!(ok, "marqueur (A) : {stderr}");
    drop(lease);

    let lease = manager.ensure(b).await.unwrap();
    let (ok, stderr) =
        raw_dockerfile_build(&tmp, lease.socket().unwrap(), CACHE_READER, &namespace);
    assert!(ok, "lecture (B) : {stderr}");
    assert!(stderr.contains("MARKER-ABSENT"), "{stderr}");
    assert!(
        !stderr.contains("MARKER-SEEN"),
        "B voit le cache de A : {stderr}"
    );
    drop(lease);

    let lease = manager.ensure(a).await.unwrap();
    let (ok, stderr) =
        raw_dockerfile_build(&tmp, lease.socket().unwrap(), CACHE_READER, &namespace);
    assert!(ok, "lecture (A) : {stderr}");
    assert!(stderr.contains("MARKER-SEEN"), "temoin : {stderr}");
}

/// Un buildkitd sans build est arrete (son etat reste) ; un buildkitd qui
/// sert un build ne l'est pas.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn idle_buildkitd_stopped() {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let tmp = TempDir::new();
    let cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (_build_cfg, _backend, manager, cfg) = start_buildkit_with_config(&tmp).await;
    let project = ProjectId::new();

    let lease = manager.ensure(project).await.unwrap();
    assert_eq!(manager.stop_idle(Duration::ZERO).await, 0);
    assert_eq!(buildkitd_running(&api, project).await, Some(true));
    drop(lease);
    // Pas assez ancien : reste.
    assert_eq!(manager.stop_idle(Duration::from_secs(3600)).await, 0);
    assert_eq!(buildkitd_running(&api, project).await, Some(true));
    assert_eq!(manager.stop_idle(Duration::ZERO).await, 1);
    assert_eq!(buildkitd_running(&api, project).await, Some(false));
    assert!(state_exists(&cfg.buildkit_state_dir, project));
    // Redemarre a la demande.
    let _lease = manager.ensure(project).await.unwrap();
    assert_eq!(buildkitd_running(&api, project).await, Some(true));
}

/// `RemoveProject` : conteneur, etat, socket et date d'usage supprimes ;
/// l'etat d'un autre projet reste. Etat perime (30 jours) : meme chose.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn project_state_removed_on_remove_project() {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let tmp = TempDir::new();
    let cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let (_build_cfg, _backend, manager, cfg) = start_buildkit_with_config(&tmp).await;
    let (gone, kept, stale) = (ProjectId::new(), ProjectId::new(), ProjectId::new());
    for p in [gone, kept, stale] {
        drop(manager.ensure(p).await.unwrap());
    }
    for p in [gone, kept, stale] {
        assert!(state_exists(&cfg.buildkit_state_dir, p));
    }

    manager.remove_project(gone).await.unwrap();
    assert_eq!(buildkitd_running(&api, gone).await, None);
    assert!(!state_exists(&cfg.buildkit_state_dir, gone));
    assert!(!cfg.buildkit_project_socket_dir(gone).exists());
    assert!(!cfg.buildkit_usage_dir.join(gone.to_string()).exists());
    assert!(!cfg.buildkit_project_config(gone).exists());
    assert!(state_exists(&cfg.buildkit_state_dir, kept));

    // Perime : son fichier d'usage date de plus de 30 jours.
    let marker = cfg.buildkit_usage_dir.join(stale.to_string());
    let old = std::time::SystemTime::now() - Duration::from_secs(31 * 24 * 3600);
    std::fs::File::options()
        .append(true)
        .open(&marker)
        .unwrap()
        .set_modified(old)
        .unwrap();
    // Dernier demarre : son buildkitd tourne encore. Le balayage l'arrete
    // proprement avant de le supprimer (une suppression forcee d'emblee
    // pouvait echouer : PID 1 zombie, « 0 » au lieu de « 1 »).
    assert_eq!(buildkitd_running(&api, stale).await, Some(true));
    assert_eq!(
        manager
            .sweep_stale(ciphera_agent::buildkit::STALE_AFTER)
            .await,
        1
    );
    assert_eq!(buildkitd_running(&api, stale).await, None);
    assert!(!state_exists(&cfg.buildkit_state_dir, stale));
    assert!(state_exists(&cfg.buildkit_state_dir, kept));
}

/// Le buildkitd partage d'avant (`ciphera-buildkitd`) est retire au
/// demarrage, et son etat avec lui ; l'etat des projets (`p`) reste.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel, buildkitd et outils de build : job agent-docker de la CI"]
async fn legacy_shared_buildkitd_removed() {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let docker = BollardDocker::connect().unwrap();
    let tmp = TempDir::new();
    let cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;
    let cfg = infra_config(&tmp);
    // Etat d'avant : fichiers directement dans le dossier d'etat.
    let legacy_state = cfg.buildkit_state_dir.join(".local");
    std::fs::create_dir(&legacy_state).unwrap();
    std::fs::write(legacy_state.join("old"), "x").unwrap();
    // Conteneur d'avant : gere par Ciphera, sous le nom partage.
    let image = infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit").unwrap();
    let body = bollard::models::ContainerCreateBody {
        image: Some(image),
        entrypoint: Some(vec!["sleep".into(), "600".into()]),
        labels: Some(std::collections::HashMap::from([(
            ciphera_agent::docker::MANAGED_LABEL.to_string(),
            "true".to_string(),
        )])),
        ..Default::default()
    };
    let options = bollard::query_parameters::CreateContainerOptions {
        name: Some(infra::LEGACY_BUILDKIT_CONTAINER.to_string()),
        ..Default::default()
    };
    let created = api.create_container(Some(options), body).await.unwrap();
    api.start_container(&created.id, None).await.unwrap();

    infra::ensure_infra(&docker, &cfg).await.unwrap();
    assert!(
        api.inspect_container(infra::LEGACY_BUILDKIT_CONTAINER, None)
            .await
            .is_err(),
        "buildkitd partage encore present"
    );
    // Premier usage : etat d'avant supprime, etat des projets garde.
    let manager = manager_for(&cfg);
    let project = ProjectId::new();
    drop(manager.ensure(project).await.unwrap());
    assert!(!legacy_state.exists());
    assert!(state_exists(&cfg.buildkit_state_dir, project));
}
