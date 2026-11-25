//! Tests contre un vrai Docker (socket `/var/run/docker.sock`) : ignores
//! par defaut, executes par le job Linux de la CI
//! (`cargo test -p ciphera-agent -- --ignored`). Ils telechargent les images
//! epinglees de PostgreSQL et BuildKit, plus busybox et nginx. A partir
//! d'Ubuntu 23.10, le profil AppArmor `ciphera-buildkitd` doit etre charge
//! (voir le job `agent-docker`) : il est verifie en mode applique.
#![cfg(unix)]

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::models::{ContainerCreateBody, HostConfig, NetworkCreateRequest};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, RemoveContainerOptions, RemoveVolumeOptions,
    TopOptions,
};
use ciphera_agent::buildkit::{BuildkitManager, DockerFs};
use ciphera_agent::docker::{
    self, BUILD_NETWORK, BollardDocker, ContainerId, ContainerSpec, DockerError, DockerOps, Health,
    INFRA_NETWORK, LogStart, MANAGED_LABEL,
};
use ciphera_agent::infra::{self, BuildkitAccount, InfraConfig, SubIds};
use futures_util::StreamExt;
use proto::ImageRef;
use shared::ids::{DeploymentId, ProjectId};

const PROBE_IMAGE: &str = "docker.io/library/busybox:1.37";
/// Present la ou le noyau restreint les espaces de noms utilisateur
/// (Ubuntu >= 23.10) : le profil AppArmor de buildkitd y est charge.
const USERNS_RESTRICT: &str = "/proc/sys/kernel/apparmor_restrict_unprivileged_userns";
/// Contexte AppArmor des processus de buildkitd (`/proc/<pid>/attr/current`).
const ENFORCED: &str = "ciphera-buildkitd (enforce)";
const APP_IMAGE: &str = "docker.io/library/nginx:alpine";
const WAIT: Duration = Duration::from_secs(180);
/// Nom complet (point final) : sans lui, busybox nslookup essaie aussi les
/// domaines de recherche du resolv.conf de l'hote, que le DNS de Docker ne
/// peut pas relayer depuis un reseau interne (SERVFAIL, code 1 alors que le
/// nom se resout).
const POSTGRES_FQDN: &str = "ciphera-postgres.";
/// Compte de buildkitd pour le test (aucun compte de l'hote : les fichiers
/// d'identite montes suffisent a rootlesskit et newuidmap).
const BUILD_UID: u32 = 10002;
const BUILD_SUBIDS: SubIds = SubIds {
    start: 300000,
    count: 65536,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        // Court : le chemin du socket d'un projet doit tenir dans les 107
        // octets d'un socket Unix.
        let dir =
            std::env::temp_dir().join(format!("cdit-{}", &DeploymentId::new().to_string()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Sous-dossier de mode `mode` (setgid compris).
    fn subdir(&self, name: &str, mode: u32) -> PathBuf {
        let p = self.0.join(name);
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    /// Sous-dossier ouvert a tous : le compte du conteneur (uid arbitraire)
    /// doit pouvoir y creer son socket. Reserve au test.
    fn open_subdir(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        std::fs::create_dir(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o777)).unwrap();
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Nettoyage Docker en fin de test, meme si une assertion echoue
/// (conteneurs, puis volumes, puis reseaux). Exige le runtime multi-thread.
#[derive(Default)]
struct Cleanup {
    containers: Vec<String>,
    volumes: Vec<String>,
    networks: Vec<String>,
}

impl Cleanup {
    fn infra() -> Self {
        Self {
            containers: vec![
                infra::POSTGRES_CONTAINER.into(),
                infra::LEGACY_BUILDKIT_CONTAINER.into(),
            ],
            volumes: vec![infra::POSTGRES_VOLUME.into()],
            networks: vec![INFRA_NETWORK.into(), BUILD_NETWORK.into()],
        }
    }

    async fn run(&self) {
        let api = api();
        for name in &self.containers {
            remove_container(&api, name).await;
        }
        for volume in &self.volumes {
            let _ = api
                .remove_volume(volume, Some(RemoveVolumeOptions { force: true }))
                .await;
        }
        for network in &self.networks {
            let _ = api.remove_network(network).await;
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(self.run()));
    }
}

fn api() -> Docker {
    Docker::connect_with_socket_defaults().unwrap()
}

async fn pull(api: &Docker, image: &str) {
    let options = CreateImageOptions {
        from_image: Some(image.to_string()),
        ..Default::default()
    };
    let mut steps = api.create_image(Some(options), None, None);
    while let Some(step) = steps.next().await {
        step.unwrap();
    }
}

async fn remove_container(api: &Docker, name: &str) {
    let options = RemoveContainerOptions {
        force: true,
        ..Default::default()
    };
    let _ = api.remove_container(name, Some(options)).await;
}

async fn wait_for_file(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < WAIT, "{} jamais cree", path.display());
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Conteneur busybox sans l'etiquette de Ciphera (cree « a la main »),
/// eventuellement sur `network`, qui execute `cmd`.
async fn foreign_container(
    api: &Docker,
    name: &str,
    network: Option<&str>,
    cmd: &[&str],
) -> String {
    let body = ContainerCreateBody {
        image: Some(PROBE_IMAGE.into()),
        cmd: Some(cmd.iter().map(|s| s.to_string()).collect()),
        host_config: Some(HostConfig {
            network_mode: network.map(str::to_string),
            ..Default::default()
        }),
        ..Default::default()
    };
    let options = CreateContainerOptions {
        name: Some(name.to_string()),
        ..Default::default()
    };
    let created = api.create_container(Some(options), body).await.unwrap();
    api.start_container(&created.id, None).await.unwrap();
    created.id
}

fn ip_on(info: &bollard::models::ContainerInspectResponse, network: &str) -> String {
    info.network_settings
        .as_ref()
        .and_then(|n| n.networks.as_ref())
        .and_then(|n| n.get(network).cloned())
        .and_then(|e| e.ip_address)
        .filter(|ip| !ip.is_empty())
        .unwrap_or_else(|| panic!("pas d'adresse sur {network}"))
}

/// Lance `cmd` dans un conteneur jetable construit comme une application du
/// projet (meme spec : reseau du projet, sans port, sans privilege), ou
/// rattache a `network` s'il est donne ; renvoie son code de sortie.
async fn probe(api: &Docker, project: ProjectId, network: Option<&str>, cmd: &[&str]) -> i64 {
    let spec = ContainerSpec::app(
        DeploymentId::new(),
        project,
        &ImageRef(PROBE_IMAGE.into()),
        Vec::new(),
    );
    let mut body = docker::create_body(&spec);
    body.cmd = Some(cmd.iter().map(|s| s.to_string()).collect());
    let hc = body.host_config.as_mut().unwrap();
    hc.restart_policy = None;
    if let Some(network) = network {
        hc.network_mode = Some(network.to_string());
    }
    let options = CreateContainerOptions {
        name: Some(spec.name().to_string()),
        ..Default::default()
    };
    let created = api.create_container(Some(options), body).await.unwrap();
    api.start_container(&created.id, None).await.unwrap();
    let code = match api.wait_container(&created.id, None).next().await {
        Some(Ok(r)) => r.status_code,
        Some(Err(BollardError::DockerContainerWaitError { code, .. })) => code,
        other => panic!("attente du conteneur : {other:?}"),
    };
    remove_container(api, &created.id).await;
    code
}

/// Build du Dockerfile `dockerfile` par le socket de buildkitd, avec
/// `buildctl` de l'image (dans le conteneur, utilisateur de l'image), sans
/// cache, sortie des etapes en clair. Renvoie le code de sortie et la sortie.
async fn buildctl_build(api: &Docker, container: &str, dockerfile: &str) -> (i64, String) {
    let script = "set -e; d=$(mktemp -d); \
        printf '%s\\n' \"$DOCKERFILE\" > \"$d/Dockerfile\"; \
        buildctl --addr unix:///run/buildkit/buildkitd.sock build --no-cache --progress=plain \
          --frontend dockerfile.v0 --local context=\"$d\" --local dockerfile=\"$d\"";
    let env = format!("DOCKERFILE={dockerfile}");
    exec_sh(api, container, script, &[env.as_str()]).await
}

/// Code de sortie et sortie de `sh -c <script>` dans `container`
/// (utilisateur et profil AppArmor du conteneur), avec `env`.
async fn exec_sh(api: &Docker, container: &str, script: &str, env: &[&str]) -> (i64, String) {
    let exec = api
        .create_exec(
            container,
            CreateExecOptions {
                cmd: Some(vec!["sh", "-c", script]),
                env: Some(env.to_vec()),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut output = String::new();
    if let StartExecResults::Attached {
        output: mut out, ..
    } = api.start_exec(&exec.id, None).await.unwrap()
    {
        while let Some(Ok(chunk)) = out.next().await {
            output.push_str(&chunk.to_string());
        }
    }
    let code = api
        .inspect_exec(&exec.id)
        .await
        .unwrap()
        .exit_code
        .unwrap_or(-1);
    (code, output)
}

/// `(uid reel de l'hote, nom)` de chaque processus du conteneur buildkitd :
/// PID donnes par Docker (`top`), uid et nom lus dans `/proc/<pid>/status`
/// de l'hote (independant de la variante de `ps`).
async fn buildkitd_process_uids(api: &Docker, container: &str) -> Vec<(u32, String)> {
    let top = api
        .top_processes(container, None::<TopOptions>)
        .await
        .unwrap();
    let titles = top.titles.unwrap_or_default();
    let pid = titles
        .iter()
        .position(|t| t == "PID")
        .unwrap_or_else(|| panic!("colonne PID absente : {titles:?}"));
    top.processes
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| {
            // Un processus peut finir entre `top` et la lecture.
            let status = std::fs::read_to_string(format!("/proc/{}/status", p[pid])).ok()?;
            let field = |key: &str| {
                status
                    .lines()
                    .find_map(|l| l.strip_prefix(key))
                    .map(str::trim)
                    .unwrap_or_else(|| panic!("{key} absent de /proc/{}/status", p[pid]))
                    .to_string()
            };
            let uid = field("Uid:")
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap();
            Some((uid, field("Name:")))
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job Linux de la CI (cargo test -p ciphera-agent -- --ignored)"]
async fn app_container_cannot_reach_infra_network() {
    let api = api();
    let docker = BollardDocker::connect().unwrap();
    let project = ProjectId::new();
    let project_net = docker::project_network(project);
    let listener = format!("ciphera-it-listener-{}", DeploymentId::new());
    // Declare avant le garde : supprime apres lui (conteneurs d'abord).
    let tmp = TempDir::new();
    let mut cleanup = Cleanup::infra();
    cleanup.containers.push(listener.clone());
    cleanup.networks.push(project_net.clone());
    cleanup.run().await;
    pull(&api, PROBE_IMAGE).await;

    let password = format!("pw-{}", DeploymentId::new());
    let secret_dir = tmp.open_subdir("secret");
    let password_file = secret_dir.join("db_password");
    std::fs::write(&password_file, &password).unwrap();
    std::fs::set_permissions(&password_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let cfg = InfraConfig {
        postgres_secret_dir: secret_dir,
        postgres_socket_dir: tmp.open_subdir("pg"),
        postgres_user: (10001, 10001),
        // Setgid (comme /run/ciphera-agent/buildkit) : le dossier d'un projet
        // et son socket heritent du groupe du test.
        buildkit_socket_dir: tmp.subdir("buildkit", 0o2777),
        buildkit_apparmor_profile: infra::buildkit_apparmor_profile(),
        buildkit_account: Some(BuildkitAccount {
            user: (BUILD_UID, BUILD_UID),
            subuids: BUILD_SUBIDS,
            subgids: BUILD_SUBIDS,
        }),
        buildkit_state_dir: tmp.open_subdir("buildkit-state"),
        buildkit_identity_dir: tmp.0.join("buildkit-identity"),
        buildkit_usage_dir: tmp.0.join("buildkit-usage"),
        buildkit_resources: infra::BuildkitResources::detect(&tmp.0),
    };

    // Un ciphera-buildkitd cree a la main (sans etiquette) n'est ni remplace
    // ni arrete ni supprime : conteneur intact.
    let foreign = foreign_container(
        &api,
        infra::LEGACY_BUILDKIT_CONTAINER,
        None,
        &["sleep", "600"],
    )
    .await;
    infra::ensure_infra(&docker, &cfg).await.unwrap();
    let still = api
        .inspect_container(infra::LEGACY_BUILDKIT_CONTAINER, None)
        .await
        .unwrap();
    assert_eq!(still.id.as_deref(), Some(foreign.as_str()));
    assert_eq!(still.state.and_then(|s| s.running), Some(true));
    let id = ContainerId(foreign.clone());
    assert!(matches!(
        docker.health(&id).await,
        Err(DockerError::NotManaged(_))
    ));
    assert!(matches!(
        docker.stop(&id).await,
        Err(DockerError::NotManaged(_))
    ));
    remove_container(&api, &foreign).await;

    infra::ensure_infra(&docker, &cfg).await.unwrap();
    let first = api
        .inspect_container(infra::POSTGRES_CONTAINER, None)
        .await
        .unwrap();
    // Idempotent : meme spec, meme conteneur.
    infra::ensure_infra(&docker, &cfg).await.unwrap();
    let again = api
        .inspect_container(infra::POSTGRES_CONTAINER, None)
        .await
        .unwrap();
    assert_eq!(first.id, again.id);

    // PostgreSQL initialise avec le fichier de mot de passe ; sockets Unix.
    wait_for_file(&cfg.postgres_socket_dir.join(".s.PGSQL.5432")).await;

    // buildkitd du projet, demarre a la demande.
    let docker_arc = std::sync::Arc::new(BollardDocker::connect().unwrap());
    let fs = DockerFs::connect(
        &cfg,
        infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit").unwrap(),
        (BUILD_UID, BUILD_UID),
    )
    .unwrap();
    let manager = BuildkitManager::new(docker_arc, fs, cfg.clone());
    let build_project = ProjectId::new();
    let bk_name = infra::buildkit_container_name(build_project);
    cleanup.containers.push(bk_name.clone());
    let lease = manager.ensure(build_project).await.unwrap();
    assert_eq!(
        lease.socket(),
        Some(cfg.buildkit_project_socket(build_project).as_path())
    );
    wait_for_file(&cfg.buildkit_project_socket(build_project)).await;

    // Le mot de passe n'apparait pas dans la configuration du conteneur.
    let config = again.config.clone().unwrap();
    let env = config.env.unwrap_or_default();
    assert!(!env.iter().any(|e| e.contains(&password)), "{env:?}");
    assert!(!format!("{again:?}").contains(&password));
    for name in [infra::POSTGRES_CONTAINER, bk_name.as_str()] {
        let info = api.inspect_container(name, None).await.unwrap();
        let ports = info
            .network_settings
            .and_then(|n| n.ports)
            .unwrap_or_default();
        assert!(ports.values().all(|b| b.is_none()), "{name} : {ports:?}");
        assert!(info.image.is_some());
        assert_ne!(
            info.host_config.and_then(|h| h.privileged),
            Some(true),
            "{name}"
        );
    }

    let net = api.inspect_network(INFRA_NETWORK, None).await.unwrap();
    assert_eq!(net.internal, Some(true));
    let build_net = api.inspect_network(BUILD_NETWORK, None).await.unwrap();
    assert_eq!(build_net.internal, Some(false));
    // buildkitd seul sur ciphera-build, absent de ciphera-internal.
    let bk = api.inspect_container(&bk_name, None).await.unwrap();
    let bk_nets: Vec<String> = bk
        .network_settings
        .and_then(|n| n.networks)
        .unwrap_or_default()
        .into_keys()
        .collect();
    assert_eq!(bk_nets, [BUILD_NETWORK]);
    // systempaths=unconfined vu par Docker ; bac a sable des processus actif.
    let bk_hc = bk.host_config.clone().unwrap();
    // Sans cela, Docker liste ses chemins masques par defaut (/proc/kcore…).
    assert!(bk_hc.masked_paths.unwrap_or_default().is_empty());
    assert!(
        !bk.config
            .and_then(|c| c.cmd)
            .unwrap_or_default()
            .iter()
            .any(|a| a.contains("no-process-sandbox"))
    );

    // Compte dedie vu par Docker et par le noyau : tous les processus de
    // buildkitd (rootlesskit, buildkitd) sous l'uid configure de l'hote,
    // jamais root ni 1000 ; seules capacites SETUID et SETGID.
    assert_eq!(
        bk_hc.cap_add.clone().unwrap_or_default(),
        ["SETUID", "SETGID"]
    );
    assert_eq!(bk_hc.cap_drop.clone().unwrap_or_default(), ["ALL"]);
    // Init de Docker en PID 1 (relais des signaux, orphelins recoltes).
    assert_eq!(bk_hc.init, Some(true));
    // Plafonds de ressources vus par Docker.
    assert_eq!(bk_hc.pids_limit, Some(infra::BUILDKIT_PIDS_LIMIT));
    assert_eq!(bk_hc.memory, Some(cfg.buildkit_resources.memory as i64));
    assert_eq!(
        bk_hc.nano_cpus,
        Some(cfg.buildkit_resources.nano_cpus as i64)
    );
    let uids = buildkitd_process_uids(&api, &bk_name).await;
    assert!(
        uids.iter().any(|(_, comm)| comm == "buildkitd"),
        "buildkitd absent : {uids:?}"
    );
    assert!(
        uids.iter().all(|(uid, _)| *uid == BUILD_UID),
        "processus hors du compte de buildkitd : {uids:?}"
    );
    let pid = api
        .inspect_container(&bk_name, None)
        .await
        .unwrap()
        .state
        .and_then(|s| s.pid)
        .expect("pid de buildkitd");
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    assert!(
        status.lines().any(|l| l == "CapBnd:	00000000000000c0"),
        "capacites de buildkitd :
{status}"
    );

    // BuildKit rootless repond sur son socket et construit (image de base
    // tiree par ciphera-build, etape RUN).
    let (code, output) = buildctl_build(
        &api,
        &bk_name,
        "FROM docker.io/library/busybox:1.37\nRUN echo ok > /ok",
    )
    .await;
    assert_eq!(code, 0, "build par le socket de buildkitd :\n{output}");

    // Profil AppArmor applique : la ou le noyau restreint les espaces de
    // noms utilisateur (job agent-docker, Ubuntu 24.04), le job charge le
    // profil comme install.sh.
    if Path::new(USERNS_RESTRICT).exists() {
        assert_eq!(
            cfg.buildkit_apparmor_profile.as_deref(),
            Some(infra::BUILDKIT_APPARMOR_PROFILE),
            "{} absent (voir le job agent-docker)",
            infra::BUILDKIT_APPARMOR_FILE
        );
        let info = api.inspect_container(&bk_name, None).await.unwrap();
        assert_eq!(
            info.app_armor_profile.as_deref(),
            Some(infra::BUILDKIT_APPARMOR_PROFILE)
        );
        // Processus du conteneur (ici `docker exec`) sous le profil, en
        // mode applique.
        let (code, current) = exec_sh(&api, &bk_name, "cat /proc/self/attr/current", &[]).await;
        assert_eq!(code, 0, "{current}");
        assert_eq!(
            current.trim_matches(|c: char| c.is_whitespace() || c == '\0'),
            ENFORCED
        );
        // Etapes RUN (descendantes de buildkitd) : meme profil.
        let (code, output) = buildctl_build(
            &api,
            &bk_name,
            "FROM docker.io/library/busybox:1.37\nRUN cat /proc/self/attr/current",
        )
        .await;
        assert_eq!(code, 0, "{output}");
        assert!(
            output.contains(ENFORCED),
            "profil des etapes RUN :\n{output}"
        );
        // /proc sensible refuse aux etapes RUN, par le profil. BuildKit
        // masque /proc/kcore par /dev/null (peripherique caractere, 0666) :
        // sans le profil, la lecture reussirait ; avec, AppArmor refuse le
        // chemin /proc/kcore (EACCES, `Permission denied`). Le marqueur
        // prouve le masque, donc que le refus vient d'AppArmor et non des
        // droits du fichier (ecrit en deux morceaux : la ligne RUN affichee
        // par buildctl ne le contient pas).
        let (code, output) = buildctl_build(
            &api,
            &bk_name,
            "FROM docker.io/library/busybox:1.37\n\
             RUN if [ -c /proc/kcore ]; then echo kcore\"-masque\"; fi; head -c 1 /proc/kcore > /dev/null",
        )
        .await;
        assert_ne!(code, 0, "lecture de /proc/kcore acceptee :\n{output}");
        assert!(
            output.contains("kcore-masque"),
            "/proc/kcore non masque :\n{output}"
        );
        assert!(
            output.contains("/proc/kcore: Permission denied"),
            "refus attendu d'AppArmor :\n{output}"
        );
    }

    // Vraie ecoute TCP sur ciphera-internal (busybox httpd).
    foreign_container(
        &api,
        &listener,
        Some(INFRA_NETWORK),
        &[
            "sh",
            "-c",
            "mkdir -p /www && echo ok > /www/index.html && exec httpd -f -p 8080 -h /www",
        ],
    )
    .await;
    let listener_info = api.inspect_container(&listener, None).await.unwrap();
    let listener_ip = ip_on(&listener_info, INFRA_NETWORK);
    let url = format!("http://{listener_ip}:8080/");
    let fetch = ["wget", "-q", "-T", "3", "-O", "/dev/null", url.as_str()];

    // Controle positif : depuis ciphera-internal, nom et port joignables.
    let start = Instant::now();
    while probe(&api, project, Some(INFRA_NETWORK), &fetch).await != 0 {
        assert!(start.elapsed() < WAIT, "ecoute de controle injoignable");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(
        probe(
            &api,
            project,
            Some(INFRA_NETWORK),
            &["nslookup", "-type=a", POSTGRES_FQDN]
        )
        .await,
        0
    );

    // Application sur le reseau de son projet : ni le nom, ni l'adresse.
    docker.ensure_network(&project_net).await.unwrap();
    assert_ne!(
        probe(&api, project, None, &["nslookup", "-type=a", POSTGRES_FQDN]).await,
        0,
        "le nom ciphera-postgres se resout depuis une application"
    );
    assert_ne!(
        probe(&api, project, None, &fetch).await,
        0,
        "une ecoute TCP de ciphera-internal est joignable depuis une application"
    );
}

/// Une ligne de 36 Ko (secret de 40 caracteres sur l'octet 16 384) est
/// coupee par Docker en messages partiels horodates : l'agent la rend d'un
/// seul tenant, le secret intact.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job Linux de la CI (cargo test -p ciphera-agent -- --ignored)"]
async fn long_log_line_is_reassembled_with_the_secret_contiguous() {
    const SECRET: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
    let api = api();
    let docker = BollardDocker::connect().unwrap();
    pull(&api, PROBE_IMAGE).await;
    let name = format!("ciphera-it-longline-{}", DeploymentId::new());
    remove_container(&api, &name).await;
    let script = format!(
        "head -c 16364 /dev/zero | tr '\\000' a; printf %s {SECRET}; \
         head -c 20000 /dev/zero | tr '\\000' b; echo"
    );
    foreign_container(&api, &name, None, &["sh", "-c", &script]).await;
    // Lus pendant l'ecriture, les journaux s'arretent sur la derniere trame
    // partielle complete (32 Kio) : la fin de la ligne attend encore le `\n`
    // dans Docker. On ne les lit qu'une fois le conteneur sorti.
    let exit = tokio::time::timeout(WAIT, api.wait_container(&name, None).next())
        .await
        .expect("conteneur jamais sorti");
    assert!(
        matches!(exit, Some(Ok(ref r)) if r.status_code == 0),
        "sortie du conteneur : {exit:?}"
    );
    let id = ContainerId(name.clone());
    let lines: Vec<_> = docker
        .logs(&id, LogStart::Tail(100), false)
        .await
        .collect()
        .await;
    remove_container(&api, &name).await;
    let found = lines
        .iter()
        .find(|l| l.line.len() > 30_000)
        .unwrap_or_else(|| panic!("ligne attendue : {} lignes", lines.len()));
    assert!(
        found.line.contains(SECRET),
        "secret coupe dans la ligne rassemblee"
    );
    assert_eq!(found.line.len(), 16_364 + SECRET.len() + 20_000);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job Linux de la CI (cargo test -p ciphera-agent -- --ignored)"]
async fn bollard_runs_reports_health_logs_and_stops_an_app() {
    let api = api();
    let docker = BollardDocker::connect().unwrap();
    let project = ProjectId::new();
    let net = docker::project_network(project);
    let unlabelled = docker::project_network(ProjectId::new());
    let mismatched = docker::project_network(ProjectId::new());
    let squat_deployment = DeploymentId::new();
    let foreign_name = format!("ciphera-d-{squat_deployment}");
    let mut cleanup = Cleanup {
        containers: vec![foreign_name.clone()],
        volumes: Vec::new(),
        networks: vec![net.clone(), unlabelled.clone(), mismatched.clone()],
    };
    docker.ensure_network(&net).await.unwrap();
    docker.ensure_network(&net).await.unwrap();

    let spec = ContainerSpec::app(
        DeploymentId::new(),
        project,
        &ImageRef(APP_IMAGE.into()),
        vec![("CIPHERA_TEST".into(), "1".into())],
    );
    cleanup.containers.push(spec.name().to_string());
    let id = docker.run(&spec).await.unwrap();
    assert_eq!(
        docker.run(&spec).await.unwrap(),
        id,
        "meme spec : meme conteneur"
    );
    assert_eq!(docker.health(&id).await.unwrap(), Health::Running);

    // Vu par Docker : reseau du projet, no-new-privileges, aucun port,
    // NET_RAW et MKNOD retires, processus bornes.
    let info = api.inspect_container(&id.0, None).await.unwrap();
    let hc = info.host_config.unwrap();
    assert_eq!(hc.network_mode.as_deref(), Some(net.as_str()));
    assert!(
        hc.security_opt
            .unwrap_or_default()
            .contains(&"no-new-privileges:true".to_string())
    );
    assert!(hc.port_bindings.unwrap_or_default().is_empty());
    assert!(hc.mounts.unwrap_or_default().is_empty());
    let dropped = hc.cap_drop.unwrap_or_default();
    assert!(dropped.contains(&"NET_RAW".to_string()), "{dropped:?}");
    assert_eq!(hc.pids_limit, Some(docker::APP_PIDS_LIMIT));

    let start = Instant::now();
    loop {
        let lines: Vec<_> = docker
            .logs(&id, LogStart::Tail(100), false)
            .await
            .collect()
            .await;
        if lines.iter().any(|l| l.line.contains("start")) {
            assert!(lines.iter().all(|l| l.timestamp_ms > 1_700_000_000_000));
            break;
        }
        assert!(start.elapsed() < WAIT, "journaux attendus : {lines:?}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    let m = docker.stats().await.unwrap();
    assert!(m.mem_total_bytes > 0 && m.disk_total_bytes > 0, "{m:?}");

    docker.stop(&id).await.unwrap();
    assert_eq!(docker.health(&id).await.unwrap(), Health::Stopped);
    docker.stop(&id).await.unwrap();
    remove_container(&api, &id.0).await;
    assert_eq!(docker.health(&id).await.unwrap(), Health::Missing);

    // Conteneur du meme nom cree a la main : run refuse, il reste en place.
    pull(&api, PROBE_IMAGE).await;
    let foreign = foreign_container(&api, &foreign_name, Some(&net), &["sleep", "600"]).await;
    let squatted = ContainerSpec::app(
        squat_deployment,
        project,
        &ImageRef(APP_IMAGE.into()),
        Vec::new(),
    );
    assert_eq!(squatted.name(), foreign_name);
    assert!(matches!(
        docker.run(&squatted).await,
        Err(DockerError::NotManaged(_))
    ));
    let still = api.inspect_container(&foreign_name, None).await.unwrap();
    assert_eq!(still.id.as_deref(), Some(foreign.as_str()));

    // Reseau du meme nom cree a la main : jamais adopte.
    api.create_network(NetworkCreateRequest {
        name: unlabelled.clone(),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(matches!(
        docker.ensure_network(&unlabelled).await,
        Err(DockerError::NotManaged(_))
    ));
    // Reseau gere mais avec un autre reglage `internal` : refuse.
    api.create_network(NetworkCreateRequest {
        name: mismatched.clone(),
        internal: Some(true),
        labels: Some(HashMap::from([(
            MANAGED_LABEL.to_string(),
            "true".to_string(),
        )])),
        ..Default::default()
    })
    .await
    .unwrap();
    assert!(matches!(
        docker.ensure_network(&mismatched).await,
        Err(DockerError::NetworkMismatch(_))
    ));
    drop(cleanup);
}

/// Mise a niveau : un `ciphera-build` d'une version precedente (pont au nom
/// genere par Docker, que le filtrage de sortie ne reconnait pas) est
/// recree avec l'interface `ciphera-build` ; le conteneur gere qui y etait
/// rattache (buildkitd) est supprime pour etre recree par l'agent. Un
/// conteneur d'autrui rattache empeche la migration (jamais supprime).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job Linux de la CI (cargo test -p ciphera-agent -- --ignored)"]
async fn legacy_build_network_is_recreated_with_its_bridge_name() {
    let api = api();
    let docker = BollardDocker::connect().unwrap();
    let managed = format!("ciphera-it-old-bk-{}", DeploymentId::new());
    let foreign = format!("ciphera-it-foreign-{}", DeploymentId::new());
    let mut cleanup = Cleanup::infra();
    cleanup.containers.push(managed.clone());
    cleanup.containers.push(foreign.clone());
    cleanup.run().await;
    pull(&api, PROBE_IMAGE).await;
    let legacy = || NetworkCreateRequest {
        name: BUILD_NETWORK.into(),
        driver: Some("bridge".into()),
        labels: Some(HashMap::from([(
            MANAGED_LABEL.to_string(),
            "true".to_string(),
        )])),
        ..Default::default()
    };
    let bridge_name = |n: &bollard::models::NetworkInspect| {
        n.options
            .as_ref()
            .and_then(|o| o.get(docker::BRIDGE_NAME_OPTION).cloned())
    };

    // Un conteneur d'autrui rattache : refus, rien n'est supprime.
    api.create_network(legacy()).await.unwrap();
    foreign_container(&api, &foreign, Some(BUILD_NETWORK), &["sleep", "600"]).await;
    assert!(matches!(
        docker.ensure_network(BUILD_NETWORK).await,
        Err(DockerError::NetworkMismatch(_))
    ));
    assert!(api.inspect_container(&foreign, None).await.is_ok());
    remove_container(&api, &foreign).await;

    // Seul un conteneur gere rattache : supprime, reseau recree.
    let body = ContainerCreateBody {
        image: Some(PROBE_IMAGE.into()),
        cmd: Some(vec!["sleep".into(), "600".into()]),
        labels: Some(HashMap::from([(
            MANAGED_LABEL.to_string(),
            "true".to_string(),
        )])),
        host_config: Some(HostConfig {
            network_mode: Some(BUILD_NETWORK.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let options = CreateContainerOptions {
        name: Some(managed.clone()),
        ..Default::default()
    };
    let created = api.create_container(Some(options), body).await.unwrap();
    api.start_container(&created.id, None).await.unwrap();
    docker.ensure_network(BUILD_NETWORK).await.unwrap();
    let net = api.inspect_network(BUILD_NETWORK, None).await.unwrap();
    assert_eq!(bridge_name(&net).as_deref(), Some(BUILD_NETWORK));
    assert!(api.inspect_container(&managed, None).await.is_err());
    assert!(
        Path::new("/sys/class/net").join(BUILD_NETWORK).exists(),
        "interface {BUILD_NETWORK} absente"
    );
    // Deja a jour : rien ne change.
    docker.ensure_network(BUILD_NETWORK).await.unwrap();
    let again = api.inspect_network(BUILD_NETWORK, None).await.unwrap();
    assert_eq!(again.id, net.id);
    drop(cleanup);
}
