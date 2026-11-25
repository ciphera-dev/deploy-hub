//! Deploiements contre un vrai Docker (bollard, `docker compose`, healthcheck
//! HTTP reel sur le reseau du projet) : ignores par defaut, executes par le
//! job `agent-docker` de la CI (`cargo test -p ciphera-agent -- --ignored`).
//! Le proxy est remplace par un enregistreur des tables calculees.
#![cfg(unix)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bollard::Docker;
use bollard::query_parameters::{CreateImageOptions, RemoveContainerOptions};
use ciphera_agent::build::ProgressSink;
use ciphera_agent::build::local::LocalBackend;
use ciphera_agent::deploy::compose::{COMPOSE_PROJECT_LABEL, CliCompose};
use ciphera_agent::deploy::probe::{HttpProbe, TcpProbe};
use ciphera_agent::deploy::{DeployConfig, Deployer};
use ciphera_agent::docker::{self, BollardDocker, DockerOps, Health};
use ciphera_agent::routes::RouteSink;
use ciphera_agent::state::StateDb;
use futures_util::StreamExt;
use proto::{
    AgentToControl, ComposeDeploy, DeploySpec, EnvValue, EnvVar, Healthcheck, ImageRef, Route,
    Stage, StageStatus, TlsMode,
};
use serde_json::json;
use shared::ids::{DeploymentId, EnvironmentId, ProjectId, ServiceId};
use tokio::sync::mpsc;

const APP_IMAGE: &str = "docker.io/library/nginx:alpine";
/// Sort aussitot (commande par defaut `sh` sans terminal) : jamais sain.
const BROKEN_IMAGE: &str = "docker.io/library/busybox:1.37";
const CACHE_IMAGE: &str = "docker.io/library/redis:7-alpine";

#[derive(Clone, Default)]
struct Routes(Arc<Mutex<Vec<Vec<Route>>>>);

impl RouteSink for Routes {
    fn apply(&self, routes: Vec<Route>) {
        self.0.lock().unwrap().push(routes);
    }
}

impl Routes {
    fn upstreams(&self, host: &str) -> Vec<SocketAddr> {
        self.0
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .iter()
            .find(|r| r.host == host)
            .unwrap()
            .upstreams
            .clone()
    }
}

type RealDeployer = Deployer<BollardDocker, TcpProbe, CliCompose<LocalBackend>, Routes>;

struct Bench {
    dir: PathBuf,
    db: StateDb,
    routes: Routes,
    deployer: RealDeployer,
    docker: Arc<BollardDocker>,
    containers: Vec<String>,
    networks: Vec<String>,
    compose_projects: Vec<String>,
    /// Volumes a supprimer a la fin.
    volumes: Vec<String>,
}

impl Bench {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("ciphera-deploy-it-{}", DeploymentId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = StateDb::open(&dir.join("state.db")).unwrap();
        let docker = Arc::new(BollardDocker::connect().unwrap());
        let routes = Routes::default();
        let cfg = DeployConfig {
            builds_dir: dir.join("builds").to_str().unwrap().into(),
            deploys_dir: dir.join("deploys").to_str().unwrap().into(),
            drain: Duration::from_secs(2),
            panel_upstream: None,
        };
        let backend = LocalBackend::connect(dir.join("absent.sock")).unwrap();
        let deployer = Deployer::new(
            docker.clone(),
            db.clone(),
            routes.clone(),
            TcpProbe::default(),
            CliCompose(Arc::new(backend)),
            cfg,
        );
        Self {
            dir,
            db,
            routes,
            deployer,
            docker,
            containers: Vec::new(),
            networks: Vec::new(),
            compose_projects: Vec::new(),
            volumes: Vec::new(),
        }
    }

    async fn deploy(&self, spec: &DeploySpec) -> (Result<(), String>, Vec<(Stage, StageStatus)>) {
        let (tx, mut rx) = mpsc::channel(100_000);
        let sink = ProgressSink::new(spec.deployment_id, tx);
        let result = self
            .deployer
            .run(spec, &sink)
            .await
            .map_err(|e| e.to_string());
        drop(sink);
        let mut stages = Vec::new();
        while let Some(m) = rx.recv().await {
            if let AgentToControl::DeployProgress { stage, status, .. } = m {
                stages.push((stage, status));
            }
        }
        (result, stages)
    }

    async fn cleanup(&self) {
        let api = Docker::connect_with_socket_defaults().unwrap();
        let mut names = self.containers.clone();
        for project in &self.compose_projects {
            if let Ok(list) = self
                .docker
                .list_managed(&[(COMPOSE_PROJECT_LABEL, project.as_str())])
                .await
            {
                names.extend(list.into_iter().map(|c| c.id.0));
            }
        }
        for name in names {
            let options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            let _ = api.remove_container(&name, Some(options)).await;
        }
        for network in &self.networks {
            let _ = api.remove_network(network).await;
        }
        for volume in &self.volumes {
            let _ = api
                .remove_volume(
                    volume,
                    None::<bollard::query_parameters::RemoveVolumeOptions>,
                )
                .await;
        }
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(self.cleanup()));
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn pull(image: &str) {
    let api = Docker::connect_with_socket_defaults().unwrap();
    let options = CreateImageOptions {
        from_image: Some(image.to_string()),
        ..Default::default()
    };
    let mut steps = api.create_image(Some(options), None, None);
    while let Some(step) = steps.next().await {
        step.unwrap();
    }
}

fn route(host: &str, service: ServiceId) -> Route {
    Route {
        host: host.into(),
        path_prefix: "/".into(),
        upstreams: Vec::new(),
        redirect_to: None,
        tls: TlsMode::None,
        service: Some(service),
    }
}

fn app(project: ProjectId, service: ServiceId, image: &str, tries: u8) -> DeploySpec {
    DeploySpec {
        deployment_id: DeploymentId::new(),
        service_id: service,
        image: ImageRef(image.into()),
        internal_port: Some(80),
        project_id: project,
        healthcheck: Healthcheck {
            path: "/".into(),
            tries,
            interval_secs: 1,
        },
        compose: None,
        service_name: Some("web".into()),
        env: Vec::new(),
        environment_id: None,
        run_as: None,
        read_only_rootfs: false,
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job agent-docker de la CI (cargo test -p ciphera-agent -- --ignored)"]
async fn real_docker_failed_v2_keeps_v1_then_healthy_v3_switches_and_stops_v1() {
    for image in [APP_IMAGE, BROKEN_IMAGE] {
        pull(image).await;
    }
    let mut bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench.networks.push(docker::project_network(project));
    bench
        .deployer
        .apply_routes(vec![route("app.example.test", web)])
        .await
        .unwrap();

    // v1 : sain, servi.
    let v1 = app(project, web, APP_IMAGE, 30);
    bench
        .containers
        .push(format!("ciphera-d-{}", v1.deployment_id));
    let (result, stages) = bench.deploy(&v1).await;
    result.unwrap_or_else(|e| panic!("v1 : {e} ({stages:?})"));
    let v1_addr = bench.routes.upstreams("app.example.test")[0];
    TcpProbe::default().check(v1_addr, "/").await.unwrap();

    // v2 : ne demarre jamais -> echec, v1 sert toujours, v2 supprime.
    let v2 = app(project, web, BROKEN_IMAGE, 3);
    let v2_name = format!("ciphera-d-{}", v2.deployment_id);
    bench.containers.push(v2_name.clone());
    let (result, stages) = bench.deploy(&v2).await;
    assert!(result.is_err(), "{stages:?}");
    assert!(
        matches!(
            stages.last(),
            Some((Stage::Healthcheck, StageStatus::Failed { .. }))
        ),
        "{stages:?}"
    );
    assert_eq!(bench.routes.upstreams("app.example.test"), vec![v1_addr]);
    TcpProbe::default().check(v1_addr, "/").await.unwrap();
    let api = Docker::connect_with_socket_defaults().unwrap();
    assert!(
        api.inspect_container(&v2_name, None).await.is_err(),
        "v2 supprime"
    );

    // v3 : sain -> bascule, puis v1 arrete apres le drainage.
    let v3 = app(project, web, APP_IMAGE, 30);
    bench
        .containers
        .push(format!("ciphera-d-{}", v3.deployment_id));
    let (result, stages) = bench.deploy(&v3).await;
    result.unwrap_or_else(|e| panic!("v3 : {e} ({stages:?})"));
    let v3_addr = bench.routes.upstreams("app.example.test")[0];
    assert_ne!(v3_addr, v1_addr);
    TcpProbe::default().check(v3_addr, "/").await.unwrap();
    let v1_record = bench.db.record(v1.deployment_id).unwrap().unwrap();
    assert!(!v1_record.deployment.serving);
    assert_eq!(
        bench.docker.health(&v1_record.container).await.unwrap(),
        Health::Stopped
    );
    // Upstream : adresse sur le reseau du projet.
    let info = api
        .inspect_container(&format!("ciphera-d-{}", v3.deployment_id), None)
        .await
        .unwrap();
    let networks = info.network_settings.unwrap().networks.unwrap();
    assert_eq!(
        networks.keys().cloned().collect::<Vec<_>>(),
        vec![docker::project_network(project)]
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et docker compose : job agent-docker de la CI"]
async fn real_compose_deploy_forces_every_service_onto_the_project_network() {
    for image in [APP_IMAGE, CACHE_IMAGE] {
        pull(image).await;
    }
    let mut bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let stable = format!("it{}-production", &DeploymentId::new().to_string()[..8]);
    bench.networks.push(docker::project_network(project));
    bench.compose_projects.push(stable.clone());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.test", web)])
        .await
        .unwrap();
    let deployment = DeploymentId::new();
    bench
        .compose_projects
        .push(format!("ciphera-d-{deployment}"));
    let bundle = json!({
        "name": stable,
        "services": {
            "web": {
                "image": APP_IMAGE,
                "depends_on": {"cache": {"condition": "service_started", "required": true}},
                "networks": {"default": null},
                "environment": {"GREETING": "from-file"}
            },
            "cache": {"image": CACHE_IMAGE, "networks": {"default": null}}
        },
        "networks": {"default": {"name": format!("{stable}_default")}}
    });
    bench
        .db
        .put_bundle(deployment, &bundle.to_string())
        .unwrap();
    let spec = DeploySpec {
        deployment_id: deployment,
        service_id: web,
        image: ImageRef(APP_IMAGE.into()),
        internal_port: Some(80),
        project_id: project,
        healthcheck: Healthcheck {
            path: "/".into(),
            tries: 30,
            interval_secs: 1,
        },
        compose: Some(ComposeDeploy {
            service: "web".into(),
            bundle: deployment,
        }),
        service_name: Some("web".into()),
        // Ciphera l'emporte sur le fichier ; valeur avec `$` et espaces.
        env: vec![
            EnvVar {
                key: "GREETING".into(),
                value: EnvValue::new("from-ciphera".into()),
            },
            EnvVar {
                key: "API_TOKEN".into(),
                value: EnvValue::new("t0k$n with spaces".into()),
            },
        ],
        environment_id: None,
        run_as: None,
        read_only_rootfs: false,
    };
    let (result, stages) = bench.deploy(&spec).await;
    result.unwrap_or_else(|e| panic!("compose : {e} ({stages:?})"));
    let addr = bench.routes.upstreams("shop.example.test")[0];
    TcpProbe::default().check(addr, "/").await.unwrap();

    // Chaque conteneur du projet : reseau du projet seul.
    let api = Docker::connect_with_socket_defaults().unwrap();
    let mut seen = Vec::new();
    for project_name in [stable.clone(), format!("ciphera-d-{deployment}")] {
        for c in bench
            .docker
            .list_managed(&[(COMPOSE_PROJECT_LABEL, project_name.as_str())])
            .await
            .unwrap()
        {
            let info = api.inspect_container(&c.id.0, None).await.unwrap();
            let env = info
                .config
                .as_ref()
                .and_then(|c| c.env.clone())
                .unwrap_or_default();
            for var in ["GREETING=from-ciphera", "API_TOKEN=t0k$n with spaces"] {
                assert!(env.iter().any(|e| e == var), "{}: {env:?}", c.name);
            }
            let networks = info.network_settings.unwrap().networks.unwrap();
            assert_eq!(
                networks.keys().cloned().collect::<Vec<_>>(),
                vec![docker::project_network(project)],
                "{}",
                c.name
            );
            let host = info.host_config.unwrap();
            assert!(
                host.security_opt
                    .clone()
                    .unwrap_or_default()
                    .iter()
                    .any(|o| o.starts_with("no-new-privileges")),
                "{}",
                c.name
            );
            // Memes limites que l'application simple, sans les avoir demandees.
            assert_eq!(host.pids_limit, Some(docker::APP_PIDS_LIMIT), "{}", c.name);
            let dropped = host.cap_drop.unwrap_or_default();
            for cap in ["NET_RAW", "MKNOD"] {
                assert!(
                    dropped.iter().any(|d| d.ends_with(cap)),
                    "{}: {dropped:?}",
                    c.name
                );
            }
            assert!(host.memory.unwrap_or(0) >= 256 << 20, "{}", c.name);
            assert!(host.nano_cpus.unwrap_or(0) >= 1_000_000_000, "{}", c.name);
            seen.push(c.name);
        }
    }
    assert_eq!(seen.len(), 2, "{seen:?}");

    // Retrait du deploiement servi : toute la pile Compose part.
    bench.deployer.remove(deployment).await;
    for project_name in [stable.clone(), format!("ciphera-d-{deployment}")] {
        assert!(
            bench
                .docker
                .list_managed(&[(COMPOSE_PROJECT_LABEL, project_name.as_str())])
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert!(bench.routes.upstreams("shop.example.test").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et docker compose : job agent-docker de la CI"]
async fn real_compose_restart_leaves_previous_deployments_stopped() {
    for image in [APP_IMAGE, CACHE_IMAGE] {
        pull(image).await;
    }
    let mut bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let stable = format!("it{}-production", &DeploymentId::new().to_string()[..8]);
    bench.networks.push(docker::project_network(project));
    bench.compose_projects.push(stable.clone());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.test", web)])
        .await
        .unwrap();
    let bundle = json!({
        "name": stable,
        "services": {
            "web": {
                "image": APP_IMAGE,
                "depends_on": {"cache": {"condition": "service_started", "required": true}},
                "networks": {"default": null}
            },
            "cache": {"image": CACHE_IMAGE, "networks": {"default": null}}
        },
        "networks": {"default": {"name": format!("{stable}_default")}}
    });
    let mut ids = Vec::new();
    for _ in 0..2 {
        let deployment = DeploymentId::new();
        bench
            .compose_projects
            .push(format!("ciphera-d-{deployment}"));
        bench
            .db
            .put_bundle(deployment, &bundle.to_string())
            .unwrap();
        let spec = DeploySpec {
            deployment_id: deployment,
            service_id: web,
            image: ImageRef(APP_IMAGE.into()),
            internal_port: Some(80),
            project_id: project,
            healthcheck: Healthcheck {
                path: "/".into(),
                tries: 30,
                interval_secs: 1,
            },
            compose: Some(ComposeDeploy {
                service: "web".into(),
                bundle: deployment,
            }),
            service_name: Some("web".into()),
            env: Vec::new(),
            environment_id: None,
            run_as: None,
            read_only_rootfs: false,
        };
        let (result, stages) = bench.deploy(&spec).await;
        result.unwrap_or_else(|e| panic!("compose : {e} ({stages:?})"));
        ids.push(deployment);
    }
    let (first, second) = (ids[0], ids[1]);
    let container = |id: DeploymentId| bench.db.record(id).unwrap().unwrap().container;
    assert_eq!(
        bench.docker.health(&container(first)).await.unwrap(),
        Health::Stopped,
        "l'ancien deploiement est arrete apres le drainage"
    );

    bench.deployer.restart(second).await;

    assert_eq!(
        bench.docker.health(&container(first)).await.unwrap(),
        Health::Stopped,
        "Redemarrer ne relance pas l'ancien deploiement"
    );
    assert_ne!(
        bench.docker.health(&container(second)).await.unwrap(),
        Health::Stopped
    );
}

fn cache_app(project: ProjectId, service: ServiceId) -> DeploySpec {
    DeploySpec {
        internal_port: None,
        compose: None,
        ..app(project, service, CACHE_IMAGE, 10)
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job agent-docker de la CI"]
async fn container_runs_as_requested_uid() {
    pull(CACHE_IMAGE).await;
    let mut bench = Bench::new();
    let (project, service) = (ProjectId::new(), ServiceId::new());
    bench.networks.push(docker::project_network(project));
    // Sans table de routes du plan de controle, `Switch` echoue.
    bench
        .deployer
        .apply_routes(vec![route("uid.example.test", service)])
        .await
        .unwrap();
    let spec = DeploySpec {
        run_as: Some("1000:1000".into()),
        read_only_rootfs: true,
        ..cache_app(project, service)
    };
    let name = format!("ciphera-d-{}", spec.deployment_id);
    bench.containers.push(name.clone());
    let (result, stages) = bench.deploy(&spec).await;
    result.unwrap_or_else(|e| panic!("deploiement : {e} ({stages:?})"));

    let api = Docker::connect_with_socket_defaults().unwrap();
    let info = api.inspect_container(&name, None).await.unwrap();
    assert_eq!(info.config.unwrap().user.as_deref(), Some("1000:1000"));
    let host = info.host_config.unwrap();
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(
        host.tmpfs.unwrap().get("/tmp").map(String::as_str),
        Some("rw,nosuid,nodev,size=64m")
    );
    let dropped = host.cap_drop.unwrap();
    for cap in ["NET_RAW", "MKNOD", "AUDIT_WRITE", "SETPCAP"] {
        assert!(
            dropped.iter().any(|c| c.ends_with(cap)),
            "{cap} : {dropped:?}"
        );
    }
    // Le processus tourne bien sous l'uid demande : colonne `UID` numerique
    // (`-o pid,uid` : Docker exige la colonne PID ; jamais un nom resolu dans
    // les comptes de l'hote).
    let options = bollard::query_parameters::TopOptionsBuilder::default()
        .ps_args("-o pid,uid")
        .build();
    let top = api.top_processes(&name, Some(options)).await.unwrap();
    let uid_column = top
        .titles
        .unwrap()
        .iter()
        .position(|t| t.eq_ignore_ascii_case("UID"))
        .expect("colonne UID");
    let processes = top.processes.unwrap();
    assert!(!processes.is_empty());
    for process in processes {
        assert_eq!(process[uid_column].trim(), "1000", "{process:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job agent-docker de la CI"]
async fn root_reported_in_state() {
    pull(CACHE_IMAGE).await;
    let mut bench = Bench::new();
    let (project, service) = (ProjectId::new(), ServiceId::new());
    bench.networks.push(docker::project_network(project));
    // Sans table de routes du plan de controle, `Switch` echoue.
    bench
        .deployer
        .apply_routes(vec![route("root.example.test", service)])
        .await
        .unwrap();
    // Sans reglage : l'utilisateur de l'image (root pour redis), signale.
    let plain = cache_app(project, service);
    bench
        .containers
        .push(format!("ciphera-d-{}", plain.deployment_id));
    let (result, stages) = bench.deploy(&plain).await;
    result.unwrap_or_else(|e| panic!("deploiement : {e} ({stages:?})"));
    let record = bench.db.record(plain.deployment_id).unwrap().unwrap();
    assert!(record.deployment.runs_as_root);
    let state = ciphera_agent::reconcile::snapshot(bench.docker.as_ref(), &bench.db).await;
    let reported = state
        .iter()
        .find(|d| d.deployment_id == plain.deployment_id)
        .unwrap();
    assert!(reported.runs_as_root);

    // Avec un uid numerique non nul : plus root.
    let user = DeploySpec {
        run_as: Some("1000".into()),
        ..cache_app(project, service)
    };
    bench
        .containers
        .push(format!("ciphera-d-{}", user.deployment_id));
    let (result, stages) = bench.deploy(&user).await;
    result.unwrap_or_else(|e| panic!("deploiement : {e} ({stages:?})"));
    let state = ciphera_agent::reconcile::snapshot(bench.docker.as_ref(), &bench.db).await;
    let reported = state
        .iter()
        .find(|d| d.deployment_id == user.deployment_id)
        .unwrap();
    assert!(!reported.runs_as_root);
}

/// Ensemble Compose du projet Compose `stable` : service expose (nginx) et
/// cache (redis), chacun avec un volume nomme.
fn volume_bundle(stable: &str) -> serde_json::Value {
    json!({
        "name": stable,
        "services": {
            "web": {
                "image": APP_IMAGE,
                "depends_on": {"cache": {"condition": "service_started", "required": true}},
                "networks": {"default": null},
                "volumes": [{"type": "volume", "source": "data", "target": "/srv"}]
            },
            "cache": {
                "image": CACHE_IMAGE,
                "networks": {"default": null},
                "volumes": [{"type": "volume", "source": "cache", "target": "/data"}]
            }
        },
        "networks": {"default": {"name": format!("{stable}_default")}},
        "volumes": {
            "data": {"name": format!("{stable}_data")},
            "cache": {"name": format!("{stable}_cache")}
        }
    })
}

/// Deploie l'ensemble `volume_bundle` du projet `project` (environnement
/// `env`) ; renvoie le deploiement et le projet Compose stable.
async fn deploy_volume_bundle(
    bench: &mut Bench,
    project: ProjectId,
    env: EnvironmentId,
    web: ServiceId,
) -> (DeploymentId, String) {
    let stable = docker::compose_project_name(project, env);
    let deployment = DeploymentId::new();
    bench.networks.push(docker::project_network(project));
    bench.compose_projects.push(stable.clone());
    bench
        .compose_projects
        .push(format!("ciphera-d-{deployment}"));
    for key in ["data", "cache"] {
        bench.volumes.push(format!("{stable}_{key}"));
    }
    bench
        .db
        .put_bundle(deployment, &volume_bundle(&stable).to_string())
        .unwrap();
    let spec = DeploySpec {
        deployment_id: deployment,
        service_id: web,
        image: ImageRef(APP_IMAGE.into()),
        internal_port: Some(80),
        project_id: project,
        healthcheck: Healthcheck {
            path: "/".into(),
            tries: 30,
            interval_secs: 1,
        },
        compose: Some(ComposeDeploy {
            service: "web".into(),
            bundle: deployment,
        }),
        service_name: Some("web".into()),
        env: Vec::new(),
        environment_id: Some(env),
        run_as: None,
        read_only_rootfs: false,
    };
    let (result, stages) = bench.deploy(&spec).await;
    result.unwrap_or_else(|e| panic!("compose : {e} ({stages:?})"));
    (deployment, stable)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et docker compose : job agent-docker de la CI"]
async fn remove_project_removes_services_network_and_volumes_when_purged() {
    for image in [APP_IMAGE, CACHE_IMAGE] {
        pull(image).await;
    }
    let mut bench = Bench::new();
    let (project, env, web) = (ProjectId::new(), EnvironmentId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("purge.example.test", web)])
        .await
        .unwrap();
    let (_, stable) = deploy_volume_bundle(&mut bench, project, env, web).await;
    let api = Docker::connect_with_socket_defaults().unwrap();
    for key in ["data", "cache"] {
        api.inspect_volume(&format!("{stable}_{key}"))
            .await
            .unwrap_or_else(|e| panic!("volume {key} : {e}"));
    }
    let label = project.to_string();
    assert_eq!(
        bench
            .docker
            .list_managed(&[(docker::PROJECT_LABEL, label.as_str())])
            .await
            .unwrap()
            .len(),
        2
    );

    bench.deployer.remove_project(project, true).await.unwrap();

    assert!(
        bench
            .docker
            .list_managed(&[(docker::PROJECT_LABEL, label.as_str())])
            .await
            .unwrap()
            .is_empty(),
        "services du projet supprimes"
    );
    assert!(
        api.inspect_network(&docker::project_network(project), None)
            .await
            .is_err(),
        "reseau du projet supprime"
    );
    for key in ["data", "cache"] {
        assert!(
            api.inspect_volume(&format!("{stable}_{key}"))
                .await
                .is_err(),
            "volume {key} supprime"
        );
    }
    assert!(bench.routes.upstreams("purge.example.test").is_empty());
    assert!(bench.db.deployments().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel et docker compose : job agent-docker de la CI"]
async fn new_project_same_name_gets_fresh_volumes() {
    for image in [APP_IMAGE, CACHE_IMAGE] {
        pull(image).await;
    }
    let mut bench = Bench::new();
    let (old_web, new_web) = (ServiceId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![
            route("old.example.test", old_web),
            route("new.example.test", new_web),
        ])
        .await
        .unwrap();
    // Projet « shop » supprime sans ses volumes : ils restent sur le noeud.
    let old = ProjectId::new();
    let (_, old_stable) =
        deploy_volume_bundle(&mut bench, old, EnvironmentId::new(), old_web).await;
    bench.deployer.remove_project(old, false).await.unwrap();
    let api = Docker::connect_with_socket_defaults().unwrap();
    api.inspect_volume(&format!("{old_stable}_data"))
        .await
        .expect("volume garde sans purge");

    // Nouveau projet « shop » (meme nom, autre UUID) : volumes neufs.
    let new = ProjectId::new();
    let (deployment, new_stable) =
        deploy_volume_bundle(&mut bench, new, EnvironmentId::new(), new_web).await;
    assert_ne!(old_stable, new_stable);
    let container = bench.db.record(deployment).unwrap().unwrap().container;
    let info = api.inspect_container(&container.0, None).await.unwrap();
    let volumes: Vec<String> = info
        .mounts
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| m.name)
        .collect();
    assert_eq!(volumes, vec![format!("{new_stable}_data")]);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job agent-docker de la CI"]
async fn empty_project_networks_swept() {
    pull(APP_IMAGE).await;
    let mut bench = Bench::new();
    // Reseau d'un projet supprime pendant une absence de l'agent : vide.
    let gone = docker::project_network(ProjectId::new());
    bench.networks.push(gone.clone());
    bench.docker.ensure_network(&gone).await.unwrap();
    // Projet deploye : son reseau reste.
    let (live, web) = (ProjectId::new(), ServiceId::new());
    bench.networks.push(docker::project_network(live));
    bench
        .deployer
        .apply_routes(vec![route("live.example.test", web)])
        .await
        .unwrap();
    let spec = app(live, web, APP_IMAGE, 30);
    bench
        .containers
        .push(format!("ciphera-d-{}", spec.deployment_id));
    let (result, stages) = bench.deploy(&spec).await;
    result.unwrap_or_else(|e| panic!("deploiement : {e} ({stages:?})"));

    assert!(bench.deployer.sweep_networks().await >= 1);

    let api = Docker::connect_with_socket_defaults().unwrap();
    assert!(api.inspect_network(&gone, None).await.is_err());
    api.inspect_network(&docker::project_network(live), None)
        .await
        .expect("reseau du projet deploye garde");
}

/// Conteneur etiquete d'un deploiement absent de l'etat local (suppression
/// perdue) : garde tant qu'il tourne, supprime par le balayage une fois
/// arrete ; le deploiement enregistre reste.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "Docker reel : job agent-docker de la CI"]
async fn orphan_labeled_container_swept() {
    pull(APP_IMAGE).await;
    let mut bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench.networks.push(docker::project_network(project));
    bench
        .deployer
        .apply_routes(vec![route("orphan.example.test", web)])
        .await
        .unwrap();
    let kept = app(project, web, APP_IMAGE, 30);
    let orphan = app(project, web, APP_IMAGE, 30);
    for spec in [&kept, &orphan] {
        let name = format!("ciphera-d-{}", spec.deployment_id);
        bench.containers.push(name);
        let (result, stages) = bench.deploy(spec).await;
        result.unwrap_or_else(|e| panic!("deploiement : {e} ({stages:?})"));
    }
    // Etat local perdu pour le second (ancien agent, suppression en echec).
    let record = bench.db.record(orphan.deployment_id).unwrap().unwrap();
    assert!(bench.db.remove_deployment(orphan.deployment_id).unwrap());

    // En marche : garde (seul le plan de controle peut le retirer).
    bench.deployer.sweep_orphans_of_project(project).await;
    let api = Docker::connect_with_socket_defaults().unwrap();
    let orphan_name = format!("ciphera-d-{}", orphan.deployment_id);
    assert!(
        api.inspect_container(&orphan_name, None).await.is_ok(),
        "orphelin en marche garde"
    );

    // Arrete : supprime.
    bench.docker.stop(&record.container).await.unwrap();
    bench.deployer.sweep_orphans_of_project(project).await;

    assert!(
        api.inspect_container(&orphan_name, None).await.is_err(),
        "conteneur orphelin supprime"
    );
    assert!(
        api.inspect_container(&format!("ciphera-d-{}", kept.deployment_id), None)
            .await
            .is_ok(),
        "deploiement enregistre intact"
    );
}
