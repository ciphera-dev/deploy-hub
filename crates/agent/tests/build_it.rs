//! Pipeline de build contre un vrai Docker et le vrai buildkitd rootless
//! (lance par `infra::ensure_infra`, comme en production) : ignores par
//! defaut, executes par le job `agent-docker` de la CI
//! (`cargo test -p ciphera-agent -- --ignored`). Exigent `buildctl` et
//! `railpack` dans `/usr/local/lib/ciphera/bin` (versions de install.sh),
//! `git` et le greffon `docker compose`, et le reseau (images de base,
//! frontend Railpack, depot public pour le clone).
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::query_parameters::{RemoveContainerOptions, RemoveImageOptions, RemoveVolumeOptions};
use ciphera_agent::build::local::LocalBackend;
use ciphera_agent::build::railpack;
use ciphera_agent::build::sandbox::{PREPARE_NETWORK, PrepareConfig};
use ciphera_agent::build::{self, BuildConfig, BuildError, BuiltImages, ProgressSink, Workdir};
use ciphera_agent::deploy::compose::ComposePlan;
use ciphera_agent::docker::{BUILD_NETWORK, BollardDocker, INFRA_NETWORK};
use ciphera_agent::infra::{self, BuildkitAccount, InfraConfig, SubIds};
use proto::{AgentToControl, BuildMethod, BuildSource, BuildSpec, ImageRef, Stage, StageStatus};
use shared::ids::{DeploymentId, ProjectId};
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(180);
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
        let dir = std::env::temp_dir().join(format!("ciphera-build-it-{}", DeploymentId::new()));
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
        for name in [infra::POSTGRES_CONTAINER, infra::BUILDKIT_CONTAINER] {
            let options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            let _ = api.remove_container(name, Some(options)).await;
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
        project_id: None,
        environment_id: None,
    }
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
    let docker = BollardDocker::connect().unwrap();
    let tmp = TempDir::new();
    let mut cleanup = Cleanup { images: Vec::new() };
    cleanup.run().await;

    let secret_dir = tmp.subdir("secret", 0o777);
    let password_file = secret_dir.join("db_password");
    std::fs::write(&password_file, format!("pw-{}", DeploymentId::new())).unwrap();
    std::fs::set_permissions(&password_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    // Socket de buildkitd : dossier setgid au groupe du test (comme
    // /run/ciphera-agent/buildkit au groupe ciphera-agent) pour que le
    // socket 0660 soit joignable par ce processus.
    let socket_dir = tmp.subdir("buildkit", 0o2777);
    let cfg = InfraConfig {
        postgres_secret_dir: secret_dir,
        postgres_socket_dir: tmp.subdir("pg", 0o777),
        postgres_user: (10001, 10001),
        buildkit_socket_dir: socket_dir.clone(),
        buildkit_apparmor_profile: infra::buildkit_apparmor_profile(),
        buildkit_account: Some(BuildkitAccount {
            user: (BUILD_UID, BUILD_UID),
            subuids: BUILD_SUBIDS,
            subgids: BUILD_SUBIDS,
        }),
        buildkit_state_dir: tmp.subdir("buildkit-state", 0o777),
        buildkit_identity_dir: tmp.0.join("buildkit-identity"),
    };
    infra::ensure_infra(&docker, &cfg).await.unwrap();
    let socket = socket_dir.join("buildkitd.sock");
    let start = Instant::now();
    while !socket.exists() {
        assert!(start.elapsed() < WAIT, "socket de buildkitd jamais cree");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let builds = tmp.subdir("builds", 0o700);
    let build_cfg = BuildConfig {
        builds_dir: builds.to_str().unwrap().to_string(),
        buildkit_socket: socket.to_str().unwrap().to_string(),
        askpass: std::env::current_exe().unwrap(),
        prepare: Some(prepare_config()),
    };
    let backend = LocalBackend::connect(&socket).unwrap();
    let project = format!("it{}", &DeploymentId::new().to_string()[..8]);

    // Dockerfile : image chargee et etiquetee, nom provisoire retire.
    let s = spec(
        &project,
        local_source(),
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
    );
    let tag = format!("ciphera/{project}-production:{}", s.deployment_id);
    cleanup.images.push(tag.clone());
    let (result, stages) = build_local(&build_cfg, &backend, &s, &fixture("dockerfile-app")).await;
    let built = result.unwrap_or_else(|e| panic!("dockerfile-app : {e} ({stages:?})"));
    assert_eq!(built.images, [ImageRef(tag.clone())]);
    assert!(image_exists(&api, &tag).await);
    assert!(!image_exists(&api, &format!("ciphera-staging/{}:build", s.deployment_id)).await);
    assert_eq!(stages.last(), Some(&(Stage::Image, StageStatus::Succeeded)));

    // Railpack (Node) : plan prepare puis frontend epingle.
    let s = spec(&project, local_source(), BuildMethod::Railpack);
    let tag = format!("ciphera/{project}-production:{}", s.deployment_id);
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
    let tag = format!("ciphera/{project}-production/web:{}", s.deployment_id);
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
    let tag = format!("ciphera/{project}-production:{}", s.deployment_id);
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
    assert!(!image_exists(&api, &format!("ciphera-staging/{}:build", s.deployment_id)).await);

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
