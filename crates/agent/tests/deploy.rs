//! Deploiement sans interruption, annulation et retrait, avec Docker, le
//! healthcheck HTTP, `docker compose` et le proxy simules (Review Focus 2 et
//! 3, part de l'agent). Horloge de tokio en pause : les delais (essais du
//! healthcheck, drainage) sont mesures sans attendre.

mod common;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciphera_agent::build::argv::{AllowedBin, ValidatedArg};
use ciphera_agent::build::compose::{ComposeFile, bundle};
use ciphera_agent::build::sandbox::PrepareContainer;
use ciphera_agent::build::{BuildBackend, BuildConfig, BuildError, ProgressSink};
use ciphera_agent::deploy::compose::{
    COMPOSE_LABEL, COMPOSE_PROJECT_LABEL, COMPOSE_SERVICE_LABEL, ComposePlan, ComposeRunner,
    ENV_PREFIX, PROJECT_LABEL, up_command,
};
use ciphera_agent::deploy::probe::HttpProbe;
use ciphera_agent::deploy::{DRAIN, DeployConfig, Deployer};
use ciphera_agent::docker::{
    DEPLOYMENT_LABEL, MANAGED_LABEL, PREPARE_ROLE, ROLE_LABEL, compose_project_name,
    project_network,
};
use ciphera_agent::ops::OpKind;
use ciphera_agent::routes::RouteSink;
use ciphera_agent::state::StateDb;
use common::{FakeDocker, TempPath};
use proto::{
    AgentToControl, BuildMethod, BuildSource, BuildSpec, ComposeDeploy, DeploySpec,
    DeploymentOutcome, EnvValue, EnvVar, Healthcheck, ImageRef, Route, Stage, StageStatus, TlsMode,
};
use serde_json::{Value, json};
use shared::ids::{DeploymentId, EnvironmentId, ProjectId, ServiceId};
use tokio::sync::mpsc;
use tokio::time::Instant;

// --- doublures ---------------------------------------------------------------

/// Healthcheck simule : seules les adresses de `ok` repondent (apres
/// `fail_first` echecs) ; celles de `hang` ne repondent jamais.
#[derive(Default)]
struct FakeProbe {
    ok: Mutex<HashSet<IpAddr>>,
    hang: Mutex<HashSet<IpAddr>>,
    fail_first: Mutex<usize>,
    /// Essais : instant, adresse, chemin.
    tries: Mutex<Vec<(Instant, SocketAddr, String)>>,
}

impl FakeProbe {
    fn answer(&self, ip: IpAddr) {
        self.ok.lock().unwrap().insert(ip);
    }
}

/// Poignee du healthcheck simule donnee a l'agent (le test garde l'autre).
struct Probe(Arc<FakeProbe>);

impl HttpProbe for Probe {
    async fn check(&self, addr: SocketAddr, path: &str) -> Result<(), String> {
        self.0.check(addr, path).await
    }
}

impl FakeProbe {
    async fn check(&self, addr: SocketAddr, path: &str) -> Result<(), String> {
        self.tries
            .lock()
            .unwrap()
            .push((Instant::now(), addr, path.to_string()));
        if self.hang.lock().unwrap().contains(&addr.ip()) {
            std::future::pending::<()>().await;
        }
        {
            let mut fail = self.fail_first.lock().unwrap();
            if *fail > 0 {
                *fail -= 1;
                return Err("connexion refusee".into());
            }
        }
        if self.ok.lock().unwrap().contains(&addr.ip()) {
            Ok(())
        } else {
            Err("statut HTTP 503".into())
        }
    }
}

/// Tables envoyees au proxy.
#[derive(Clone, Default)]
struct Routes(Arc<Mutex<Vec<Vec<Route>>>>);

impl Routes {
    fn last(&self) -> Option<Vec<Route>> {
        self.0.lock().unwrap().last().cloned()
    }

    fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

impl RouteSink for Routes {
    fn apply(&self, routes: Vec<Route>) {
        self.0.lock().unwrap().push(routes);
    }
}

/// `docker compose up` simule : cree le conteneur du service dans le faux
/// Docker, avec les etiquettes du fichier et celles de Compose.
struct FakeCompose {
    docker: Arc<FakeDocker>,
    /// `(projet, service)` dans l'ordre des appels.
    ups: Mutex<Vec<(String, String)>>,
    /// Dernier fichier recu.
    file: Mutex<Option<Value>>,
    /// Texte brut de chaque fichier recu.
    raw: Mutex<Vec<String>>,
    /// Variables donnees au processus, par appel.
    envs: Mutex<Vec<Vec<(String, String)>>>,
    /// Vrai : comme Compose quand la configuration change, `up` d'un service
    /// non expose cree un nouveau conteneur (`cid-<nom>-r<n>`, identifiant
    /// toujours neuf) qui remplace l'ancien, image du fichier ; faux : meme
    /// identifiant.
    recreate: AtomicBool,
}

struct Compose(Arc<FakeCompose>);

impl ComposeRunner for Compose {
    async fn up(
        &self,
        project: &ValidatedArg,
        file: &ValidatedArg,
        service: &ValidatedArg,
        home: &ValidatedArg,
        env: &[EnvVar],
        sink: &ProgressSink,
    ) -> Result<(), String> {
        // Environnement reellement donne au processus `docker compose`.
        let cmd = up_command(project, file, service, home, env);
        let vars = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        self.0.envs.lock().unwrap().push(vars);
        self.0.up(project, file, service, home, sink).await
    }
}

impl FakeCompose {
    async fn up(
        &self,
        project: &ValidatedArg,
        file: &ValidatedArg,
        service: &ValidatedArg,
        _home: &ValidatedArg,
        _sink: &ProgressSink,
    ) -> Result<(), String> {
        let text = std::fs::read_to_string(file.as_str()).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        self.raw.lock().unwrap().push(text);
        let mut labels: Vec<(String, String)> = doc["services"][service.as_str()]["labels"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        labels.push((COMPOSE_PROJECT_LABEL.into(), project.as_str().into()));
        labels.push((COMPOSE_SERVICE_LABEL.into(), service.as_str().into()));
        let pairs: Vec<(&str, &str)> = labels
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let n = self.ups.lock().unwrap().len() + 100;
        let name = format!("{}-{}-1", project.as_str(), service.as_str());
        let mut id = format!("cid-{name}");
        let recreate =
            self.recreate.load(Ordering::SeqCst) && !project.as_str().starts_with("ciphera-d-");
        if recreate {
            // Identifiant toujours neuf, comme Docker.
            let mut containers = self.docker.containers.lock().unwrap();
            containers.retain(|_, c| c.name != name);
            id = format!("cid-{name}-r{n}");
        }
        self.docker
            .add_container(&id, &name, &pairs, Some(&format!("172.18.1.{n}")));
        if recreate {
            let image = doc["services"][service.as_str()]["image"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            self.docker
                .containers
                .lock()
                .unwrap()
                .get_mut(&id)
                .unwrap()
                .image = image;
        }
        self.ups
            .lock()
            .unwrap()
            .push((project.as_str().into(), service.as_str().into()));
        *self.file.lock().unwrap() = Some(doc);
        Ok(())
    }
}

type TestDeployer = Deployer<FakeDocker, Probe, Compose, Routes>;

/// Banc d'essai : agent simule et dossiers sous `target/` (supprimes a la
/// fin).
struct Bench {
    docker: Arc<FakeDocker>,
    probe: Arc<FakeProbe>,
    compose: Arc<FakeCompose>,
    routes: Routes,
    db: StateDb,
    deployer: Arc<TestDeployer>,
    dir: String,
    _db_path: TempPath,
}

impl Bench {
    fn new() -> Self {
        let db_path = TempPath::new("ciphera-agent-deploy");
        let db = StateDb::open(&db_path.0).unwrap();
        let docker = Arc::new(FakeDocker::default());
        let probe = Arc::new(FakeProbe::default());
        let compose = Arc::new(FakeCompose {
            docker: docker.clone(),
            ups: Mutex::new(Vec::new()),
            file: Mutex::new(None),
            raw: Mutex::new(Vec::new()),
            envs: Mutex::new(Vec::new()),
            recreate: AtomicBool::new(false),
        });
        let routes = Routes::default();
        let dir = format!("target/deploy-tests/{}", DeploymentId::new());
        let cfg = DeployConfig {
            builds_dir: format!("{dir}/builds"),
            deploys_dir: format!("{dir}/deploys"),
            drain: DRAIN,
            panel_upstream: None,
        };
        std::fs::create_dir_all(&dir).unwrap();
        let deployer = Arc::new(
            Deployer::new(
                docker.clone(),
                db.clone(),
                routes.clone(),
                Probe(probe.clone()),
                Compose(compose.clone()),
                cfg,
            )
            .with_routes_file(PathBuf::from(format!("{dir}/routes.json"))),
        );
        Self {
            docker,
            probe,
            compose,
            routes,
            db,
            deployer,
            dir,
            _db_path: db_path,
        }
    }

    /// Fichier de la table de routes gardee.
    fn routes_file(&self) -> PathBuf {
        PathBuf::from(format!("{}/routes.json", self.dir))
    }

    /// Agent redemarre (meme etat local, meme fichier de routes), sans
    /// table du plan de controle.
    fn restarted(&self) -> TestDeployer {
        Deployer::new(
            self.docker.clone(),
            self.db.clone(),
            self.routes.clone(),
            Probe(self.probe.clone()),
            Compose(self.compose.clone()),
            DeployConfig {
                builds_dir: format!("{}/builds", self.dir),
                deploys_dir: format!("{}/deploys", self.dir),
                drain: DRAIN,
                panel_upstream: None,
            },
        )
        .with_routes_file(self.routes_file())
    }

    /// Table du plan de controle recue (vide) : les bascules sont permises.
    async fn sync_routes(&self) {
        self.deployer.apply_routes(Vec::new()).await.unwrap();
    }

    fn builds_dir(&self) -> String {
        format!("{}/builds", self.dir)
    }

    /// Lance un deploiement jusqu'au bout ; resultat et messages emis.
    async fn deploy(&self, spec: &DeploySpec) -> (Result<(), String>, Vec<AgentToControl>) {
        let (tx, mut rx) = mpsc::channel(1024);
        let sink = ProgressSink::new(spec.deployment_id, tx);
        let result = self
            .deployer
            .run(spec, &sink)
            .await
            .map_err(|e| e.to_string());
        drop(sink);
        let mut out = Vec::new();
        while let Some(m) = rx.recv().await {
            out.push(m);
        }
        (result, out)
    }

    /// Adresse attribuee par le faux Docker au conteneur d'un deploiement.
    fn ip_of(&self, deployment: DeploymentId) -> IpAddr {
        self.docker
            .container(&format!("id-ciphera-d-{deployment}"))
            .and_then(|c| c.ip)
            .expect("conteneur du deploiement")
    }

    fn upstreams(&self, host: &str) -> Vec<SocketAddr> {
        self.routes
            .last()
            .expect("table envoyee")
            .into_iter()
            .find(|r| r.host == host)
            .expect("route")
            .upstreams
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn spec(project: ProjectId, service: ServiceId, image: &str) -> DeploySpec {
    DeploySpec {
        deployment_id: DeploymentId::new(),
        service_id: service,
        image: ImageRef(image.into()),
        internal_port: Some(8080),
        project_id: project,
        healthcheck: Healthcheck::default(),
        compose: None,
        service_name: Some("web".into()),
        env: Vec::new(),
        environment_id: None,
        run_as: None,
        read_only_rootfs: false,
    }
}

fn route(host: &str, service: Option<ServiceId>) -> Route {
    Route {
        host: host.into(),
        path_prefix: "/".into(),
        // Jamais repris : l'agent calcule les upstreams.
        upstreams: vec!["10.0.0.1:5432".parse().unwrap()],
        redirect_to: None,
        tls: TlsMode::Acme,
        service,
    }
}

fn stages(messages: &[AgentToControl]) -> Vec<(Stage, StageStatus)> {
    messages
        .iter()
        .filter_map(|m| match m {
            AgentToControl::DeployProgress { stage, status, .. } => Some((*stage, status.clone())),
            _ => None,
        })
        .collect()
}

fn ok(stage: Stage) -> [(Stage, StageStatus); 2] {
    [
        (stage, StageStatus::Started),
        (stage, StageStatus::Succeeded),
    ]
}

/// Deploie `spec` avec un healthcheck qui repond : il devient servi.
async fn deploy_ok(bench: &Bench, spec: &DeploySpec) {
    // Le conteneur n'existe qu'au demarrage : l'adresse est connue d'avance
    // (attribution sequentielle du faux Docker).
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    bench.probe.answer(format!("172.18.0.{n}").parse().unwrap());
    let (result, _) = bench.deploy(spec).await;
    result.unwrap();
}

// --- bascule sans interruption ------------------------------------------------

#[tokio::test(start_paused = true)]
async fn healthy_deploy_switches_routes_then_drains_and_stops_the_old_one() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    assert_eq!(bench.upstreams("app.example.com"), Vec::<SocketAddr>::new());

    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v1_addr = SocketAddr::new(bench.ip_of(v1.deployment_id), 8080);
    assert_eq!(bench.upstreams("app.example.com"), vec![v1_addr]);

    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    let v2_ip: IpAddr = format!("172.18.0.{n}").parse().unwrap();
    bench.probe.answer(v2_ip);
    let started = Instant::now();
    let (result, messages) = bench.deploy(&v2).await;
    result.unwrap();
    let mut expected = Vec::new();
    for s in [Stage::Start, Stage::Healthcheck, Stage::Switch] {
        expected.extend(ok(s));
    }
    assert_eq!(stages(&messages), expected);

    // Nouveau conteneur sur le reseau du projet, sans privilege.
    let run = bench.docker.runs.lock().unwrap().last().unwrap().clone();
    assert_eq!(run.network(), project_network(project));
    assert_eq!(run.image(), "ciphera.invalid/web-production:v2");
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("network:{}", project_network(project)))
    );
    // Upstream bascule sur v2, puis v1 arrete apres 10 s de drainage
    // (premier essai du healthcheck a 5 s).
    assert_eq!(
        bench.upstreams("app.example.com"),
        vec![SocketAddr::new(v2_ip, 8080)]
    );
    assert_eq!(started.elapsed(), Duration::from_secs(5) + DRAIN);
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("stop:{v1_container}"))
    );
    assert!(!bench.docker.container(&v1_container).unwrap().running);
    let serving: Vec<_> = bench
        .db
        .serving()
        .unwrap()
        .into_iter()
        .map(|r| r.deployment.deployment_id)
        .collect();
    assert_eq!(serving, vec![v2.deployment_id]);
    let probe = bench.probe.tries.lock().unwrap().last().unwrap().clone();
    assert_eq!(probe.1, SocketAddr::new(v2_ip, 8080));
    assert_eq!(probe.2, "/");
}

#[tokio::test(start_paused = true)]
async fn old_deployment_keeps_serving_during_the_drain() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    bench.probe.answer(format!("172.18.0.{n}").parse().unwrap());
    let deployer = bench.deployer.clone();
    let (tx, _rx) = mpsc::channel(1024);
    let task = tokio::spawn(async move {
        let sink = ProgressSink::new(v2.deployment_id, tx);
        deployer.run(&v2, &sink).await
    });
    // Bascule faite a 5 s ; a 14 s, l'ancien tourne encore.
    tokio::time::sleep(Duration::from_secs(14)).await;
    assert!(bench.docker.container(&v1_container).unwrap().running);
    assert!(
        !bench
            .docker
            .calls()
            .contains(&format!("stop:{v1_container}"))
    );
    task.await.unwrap().unwrap();
    assert!(!bench.docker.container(&v1_container).unwrap().running);
}

#[tokio::test(start_paused = true)]
async fn failed_healthcheck_keeps_old_serving_and_marks_failed() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v1_addr = SocketAddr::new(bench.ip_of(v1.deployment_id), 8080);
    let tables = bench.routes.count();

    // v2 ne repond jamais 2xx.
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    let (result, messages) = bench.deploy(&v2).await;
    let err = result.unwrap_err();
    assert!(err.contains("healthcheck"), "{err}");
    let st = stages(&messages);
    assert_eq!(
        st[..3],
        [
            (Stage::Start, StageStatus::Started),
            (Stage::Start, StageStatus::Succeeded),
            (Stage::Healthcheck, StageStatus::Started),
        ]
    );
    match st.last().unwrap() {
        (Stage::Healthcheck, StageStatus::Failed { message }) => {
            assert!(message.contains("5 essai(s)"), "{message}");
            assert!(message.contains("503"), "{message}");
        }
        other => panic!("echec du healthcheck attendu : {other:?}"),
    }
    assert!(!st.iter().any(|(s, _)| *s == Stage::Switch), "{st:?}");

    // Aucune route basculee : aucune nouvelle table, upstream toujours v1.
    assert_eq!(bench.routes.count(), tables);
    assert_eq!(bench.upstreams("app.example.com"), vec![v1_addr]);
    // v1 tourne et reste servi ; v2 supprime et oublie.
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    let v2_container = format!("id-ciphera-d-{}", v2.deployment_id);
    assert!(bench.docker.container(&v1_container).unwrap().running);
    assert!(
        !bench
            .docker
            .calls()
            .contains(&format!("stop:{v1_container}"))
    );
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("remove:{v2_container}"))
    );
    assert!(bench.docker.container(&v2_container).is_none());
    assert!(bench.db.record(v2.deployment_id).unwrap().is_none());
    let serving = bench.db.serving().unwrap();
    assert_eq!(serving.len(), 1);
    assert_eq!(serving[0].deployment.deployment_id, v1.deployment_id);
}

#[tokio::test(start_paused = true)]
async fn healthcheck_tries_five_times_five_seconds_apart() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench.sync_routes().await;
    // Echec : 5 essais, a 5, 10, 15, 20 et 25 s.
    let failing = spec(project, web, "ciphera.invalid/web-production:v1");
    let started = Instant::now();
    let (result, _) = bench.deploy(&failing).await;
    assert!(result.is_err());
    let times: Vec<Duration> = bench
        .probe
        .tries
        .lock()
        .unwrap()
        .iter()
        .map(|(t, _, _)| *t - started)
        .collect();
    assert_eq!(
        times,
        (1..=5)
            .map(|i| Duration::from_secs(5 * i))
            .collect::<Vec<_>>()
    );
    assert_eq!(started.elapsed(), Duration::from_secs(25));

    // Reussite au troisieme essai (15 s), chemin et rythme configures.
    bench.probe.tries.lock().unwrap().clear();
    *bench.probe.fail_first.lock().unwrap() = 2;
    let mut custom = spec(project, web, "ciphera.invalid/web-production:v2");
    custom.healthcheck = Healthcheck {
        path: "/healthz".into(),
        tries: 3,
        interval_secs: 2,
    };
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    bench.probe.answer(format!("172.18.0.{n}").parse().unwrap());
    let started = Instant::now();
    let (result, _) = bench.deploy(&custom).await;
    result.unwrap();
    let tries = bench.probe.tries.lock().unwrap().clone();
    assert_eq!(tries.len(), 3);
    assert!(tries.iter().all(|(_, _, p)| p == "/healthz"));
    assert_eq!(tries[2].0 - started, Duration::from_secs(6));
}

#[tokio::test(start_paused = true)]
async fn invalid_healthcheck_is_refused_before_starting_anything() {
    let bench = Bench::new();
    let mut bad = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    bad.healthcheck.path = "/x\r\nHost: evil".into();
    let (result, messages) = bench.deploy(&bad).await;
    assert!(result.unwrap_err().contains("healthcheck"));
    assert!(bench.docker.runs.lock().unwrap().is_empty());
    assert!(matches!(
        stages(&messages).last(),
        Some((Stage::Start, StageStatus::Failed { .. }))
    ));
    let mut zero = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    zero.internal_port = Some(0);
    assert!(bench.deploy(&zero).await.0.unwrap_err().contains("port 0"));
    assert!(bench.docker.runs.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn service_without_port_must_be_running_and_gets_no_upstream() {
    let bench = Bench::new();
    let (project, worker) = (ProjectId::new(), ServiceId::new());
    bench.sync_routes().await;
    let mut w1 = spec(project, worker, "ciphera.invalid/worker-production:v1");
    w1.internal_port = None;
    let container = format!("id-ciphera-d-{}", w1.deployment_id);
    // Le conteneur redemarre en boucle : jamais en marche.
    let (result, _) = bench.deploy(&w1).await;
    assert!(result.is_err());
    assert!(
        bench.probe.tries.lock().unwrap().is_empty(),
        "pas de HTTP sans port"
    );
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("remove:{container}"))
    );

    let mut w2 = spec(project, worker, "ciphera.invalid/worker-production:v2");
    w2.internal_port = None;
    bench.docker.set_health(
        &format!("id-ciphera-d-{}", w2.deployment_id),
        ciphera_agent::docker::Health::Running,
    );
    let (result, _) = bench.deploy(&w2).await;
    result.unwrap();
    assert!(
        bench
            .db
            .record(w2.deployment_id)
            .unwrap()
            .unwrap()
            .deployment
            .serving
    );
}

#[tokio::test(start_paused = true)]
async fn replayed_deploy_is_idempotent() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    deploy_ok(&bench, &v1).await;
    let (result, messages) = bench.deploy(&v1).await;
    result.unwrap();
    assert_eq!(
        stages(&messages),
        vec![(Stage::Switch, StageStatus::Succeeded)]
    );
    assert_eq!(bench.docker.runs.lock().unwrap().len(), 1);
}

// --- routes ------------------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn upstreams_only_come_from_project_network_containers() {
    let bench = Bench::new();
    let (project, web, api) = (ProjectId::new(), ServiceId::new(), ServiceId::new());
    let mut redirect = route("example.com", None);
    redirect.redirect_to = Some("https://www.example.com".into());
    let routes = vec![
        route("app.example.com", Some(web)),
        route("api.example.com", Some(api)),
        redirect,
    ];
    bench.sync_routes().await;
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;

    // `api` sert depuis un conteneur sans adresse sur un reseau de projet.
    let mut a1 = spec(project, api, "ciphera.invalid/api-production:v1");
    a1.healthcheck.tries = 1;
    a1.internal_port = None;
    bench.docker.set_health(
        &format!("id-ciphera-d-{}", a1.deployment_id),
        ciphera_agent::docker::Health::Running,
    );
    bench.deploy(&a1).await.0.unwrap();
    bench
        .docker
        .containers
        .lock()
        .unwrap()
        .get_mut(&format!("id-ciphera-d-{}", a1.deployment_id))
        .unwrap()
        .ip = None;

    bench.deployer.apply_routes(routes).await.unwrap();
    let table = bench.routes.last().unwrap();
    let by_host: HashMap<&str, &Route> = table.iter().map(|r| (r.host.as_str(), r)).collect();
    // Upstreams du plan de controle (10.0.0.1:5432) jamais repris.
    assert_eq!(
        by_host["app.example.com"].upstreams,
        vec![SocketAddr::new(bench.ip_of(v1.deployment_id), 8080)]
    );
    assert!(by_host["api.example.com"].upstreams.is_empty());
    assert!(by_host["example.com"].upstreams.is_empty());
    assert_eq!(
        by_host["example.com"].redirect_to.as_deref(),
        Some("https://www.example.com")
    );
    assert!(
        table
            .iter()
            .flat_map(|r| r.upstreams.iter())
            .all(|u| u.ip().to_string().starts_with("172.18.0."))
    );
}

// --- annulation ----------------------------------------------------------------

/// Outils du build simules : `git clone` cree le dossier puis ne finit
/// jamais (build en cours).
#[derive(Default)]
struct HangingBuild {
    started: AtomicBool,
    interrupted: Arc<AtomicBool>,
}

/// Leve `interrupted` quand l'outil en cours est abandonne (processus tue).
struct KillFlag(Arc<AtomicBool>);

impl Drop for KillFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl BuildBackend for HangingBuild {
    fn buildkit_available(&self) -> bool {
        true
    }

    async fn exec(
        &self,
        _bin: AllowedBin,
        cmd: Command,
        _sink: &ProgressSink,
        _capture: bool,
    ) -> Result<Vec<u8>, BuildError> {
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        if args.iter().any(|a| a == "clone") {
            std::fs::create_dir_all(args.last().unwrap()).unwrap();
            let _flag = KillFlag(self.interrupted.clone());
            self.started.store(true, Ordering::SeqCst);
            std::future::pending::<()>().await;
        }
        Ok(Vec::new())
    }

    async fn build_and_load(
        &self,
        _cmd: Command,
        _staging: &ImageRef,
        _sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        Ok(())
    }

    async fn pull(&self, _image: &ImageRef, _sink: &ProgressSink) -> Result<(), BuildError> {
        Ok(())
    }

    async fn tag(&self, _source: &ImageRef, _target: &ImageRef) -> Result<(), BuildError> {
        Ok(())
    }

    async fn remove_image(&self, _image: &ImageRef) -> Result<(), BuildError> {
        Ok(())
    }

    async fn prepare(
        &self,
        _container: &PrepareContainer,
        _sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        Ok(())
    }
}

async fn wait_until(what: &str, cond: impl Fn() -> bool) {
    for _ in 0..500 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("attendu : {what}");
}

async fn drain(rx: &mut mpsc::Receiver<AgentToControl>) -> Vec<AgentToControl> {
    let mut out = Vec::new();
    while let Ok(m) = rx.try_recv() {
        out.push(m);
    }
    out
}

#[tokio::test]
async fn cancel_during_build_cleans_up() {
    let bench = Bench::new();
    let id = DeploymentId::new();
    let build_spec = BuildSpec {
        deployment_id: id,
        project: "web".into(),
        environment: "production".into(),
        source: BuildSource::Git {
            url: "https://github.com/acme/web".into(),
            branch: "main".into(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".into(),
        },
        build: BuildMethod::Railpack,
        git_token: None,
        root_directory: None,
        project_id: Some(ProjectId::new()),
        environment_id: Some(EnvironmentId::new()),
    };
    let cfg = BuildConfig {
        builds_dir: bench.builds_dir(),
        buildkit_socket: "/run/ciphera-agent/buildkit/buildkitd.sock".into(),
        askpass: PathBuf::from("/usr/local/bin/ciphera-agent"),
        prepare: None,
    };
    let backend = Arc::new(HangingBuild::default());
    let (tx, mut rx) = mpsc::channel(1024);
    bench
        .deployer
        .spawn_build(build_spec.clone(), cfg.clone(), backend.clone(), tx.clone());
    wait_until("build lance", || backend.started.load(Ordering::SeqCst)).await;
    let workdir = format!("{}/{id}", bench.builds_dir());
    assert!(Path::new(&workdir).is_dir());
    assert_eq!(bench.deployer.ops().in_progress(), vec![id]);
    assert_eq!(bench.deployer.state().await.in_progress, vec![id]);
    // Un second ordre du meme build est ignore.
    bench
        .deployer
        .spawn_build(build_spec, cfg, backend.clone(), tx.clone());
    // Bac a sable `railpack prepare` du build et image provisoire en cours.
    let label = id.to_string();
    bench.docker.add_container(
        "prep",
        &format!("ciphera-prepare-{id}"),
        &[(DEPLOYMENT_LABEL, label.as_str())],
        None,
    );

    bench.deployer.stop(id, &tx).await;

    assert!(
        backend.interrupted.load(Ordering::SeqCst),
        "outil interrompu"
    );
    assert!(!Path::new(&workdir).exists(), "dossier du build supprime");
    assert!(
        bench.docker.container("prep").is_none(),
        "bac a sable supprime"
    );
    let calls = bench.docker.calls();
    assert!(calls.contains(&"remove:prep".to_string()), "{calls:?}");
    assert!(calls.contains(&format!("rmi:{id}")), "{calls:?}");
    assert!(bench.deployer.ops().in_progress().is_empty());
    let st = stages(&drain(&mut rx).await);
    assert_eq!(
        st.last(),
        Some(&(Stage::Build, StageStatus::Cancelled)),
        "{st:?}"
    );
    assert_eq!(
        st.iter()
            .filter(|(_, s)| *s == StageStatus::Cancelled)
            .count(),
        1
    );
    assert_eq!(
        bench.db.outcome(id).unwrap().map(|o| o.status),
        Some(StageStatus::Cancelled)
    );
}

#[tokio::test(start_paused = true)]
async fn cancel_during_deploy_removes_the_new_container_and_keeps_routes() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let mut v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    v1.healthcheck.interval_secs = 1;
    deploy_ok(&bench, &v1).await;
    let before = bench.upstreams("app.example.com");

    let mut v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    v2.healthcheck.interval_secs = 1;
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    bench
        .probe
        .hang
        .lock()
        .unwrap()
        .insert(format!("172.18.0.{n}").parse().unwrap());
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(v2.clone(), tx.clone());
    let v2_container = format!("id-ciphera-d-{}", v2.deployment_id);
    wait_until("healthcheck en cours", || {
        bench.probe.tries.lock().unwrap().len() >= 2
    })
    .await;
    assert!(bench.docker.container(&v2_container).is_some());

    bench.deployer.stop(v2.deployment_id, &tx).await;

    assert!(bench.docker.container(&v2_container).is_none());
    assert_eq!(bench.upstreams("app.example.com"), before);
    assert!(bench.db.record(v2.deployment_id).unwrap().is_none());
    assert!(bench.deployer.ops().in_progress().is_empty());
    let st = stages(&drain(&mut rx).await);
    assert_eq!(
        st.last(),
        Some(&(Stage::Start, StageStatus::Cancelled)),
        "{st:?}"
    );
    assert!(
        bench
            .docker
            .container(&format!("id-ciphera-d-{}", v1.deployment_id))
            .unwrap()
            .running
    );
}

#[tokio::test(start_paused = true)]
async fn stop_of_the_serving_deployment_is_too_late_and_ignored() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let before = bench.upstreams("app.example.com");
    let (tx, mut rx) = mpsc::channel(16);
    bench.deployer.stop(v1.deployment_id, &tx).await;
    assert_eq!(
        stages(&drain(&mut rx).await),
        vec![(Stage::Switch, StageStatus::Succeeded)],
        "trop tard : la reussite est rapportee de nouveau"
    );
    assert!(
        bench
            .docker
            .container(&format!("id-ciphera-d-{}", v1.deployment_id))
            .unwrap()
            .running
    );
    assert_eq!(bench.upstreams("app.example.com"), before);
    assert_eq!(bench.db.serving().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn stop_after_switch_is_too_late_and_the_drain_continues() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    let v2_ip: IpAddr = format!("172.18.0.{n}").parse().unwrap();
    bench.probe.answer(v2_ip);
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(v2.clone(), tx.clone());
    // Bascule a 5 s ; Stop pendant le drainage (a 7 s).
    tokio::time::sleep(Duration::from_secs(7)).await;
    assert_eq!(
        bench.upstreams("app.example.com"),
        vec![SocketAddr::new(v2_ip, 8080)]
    );
    bench.deployer.stop(v2.deployment_id, &tx).await;
    let v2_container = format!("id-ciphera-d-{}", v2.deployment_id);
    assert!(bench.docker.container(&v2_container).unwrap().running);
    assert_eq!(
        bench.upstreams("app.example.com"),
        vec![SocketAddr::new(v2_ip, 8080)]
    );
    // Le drainage continue : v1 arrete a 15 s, v2 sert toujours.
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(!bench.docker.container(&v1_container).unwrap().running);
    assert!(bench.docker.container(&v2_container).unwrap().running);
    let st = stages(&drain(&mut rx).await);
    assert!(
        !st.iter()
            .any(|(_, s)| matches!(s, StageStatus::Cancelled | StageStatus::Failed { .. })),
        "{st:?}"
    );
    assert_eq!(
        st.last(),
        Some(&(Stage::Switch, StageStatus::Succeeded)),
        "{st:?}"
    );
    assert_eq!(
        bench
            .db
            .outcome(v2.deployment_id)
            .unwrap()
            .map(|o| o.status),
        Some(StageStatus::Succeeded)
    );
}

#[tokio::test(start_paused = true)]
async fn switch_without_proxy_table_fails_safely_and_keeps_the_old_one() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench.sync_routes().await;
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    // Agent redemarre : v1 servi d'apres l'etat local, aucune table recue.
    let restarted: TestDeployer = Deployer::new(
        bench.docker.clone(),
        bench.db.clone(),
        bench.routes.clone(),
        Probe(bench.probe.clone()),
        Compose(bench.compose.clone()),
        DeployConfig {
            builds_dir: format!("{}/builds", bench.dir),
            deploys_dir: format!("{}/deploys", bench.dir),
            drain: DRAIN,
            panel_upstream: None,
        },
    );
    let tables = bench.routes.count();
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    let n = bench.docker.containers.lock().unwrap().len() + 10;
    bench.probe.answer(format!("172.18.0.{n}").parse().unwrap());
    let (tx, mut rx) = mpsc::channel(1024);
    let sink = ProgressSink::new(v2.deployment_id, tx);
    let err = restarted.run(&v2, &sink).await.unwrap_err().to_string();
    drop(sink);
    assert!(err.contains("routes not synced"), "{err}");
    let st = stages(&drain(&mut rx).await);
    match st.last().unwrap() {
        (Stage::Switch, StageStatus::Failed { message }) => {
            assert!(message.contains("routes not synced"), "{message}");
        }
        other => panic!("echec de la bascule attendu : {other:?}"),
    }
    assert_eq!(bench.routes.count(), tables, "aucune table envoyee");
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    assert!(bench.docker.container(&v1_container).unwrap().running);
    assert!(
        !bench
            .docker
            .calls()
            .contains(&format!("stop:{v1_container}"))
    );
    assert!(
        bench
            .docker
            .container(&format!("id-ciphera-d-{}", v2.deployment_id))
            .is_none()
    );
    let serving = bench.db.serving().unwrap();
    assert_eq!(serving.len(), 1);
    assert_eq!(serving[0].deployment.deployment_id, v1.deployment_id);
}

// --- builds suivis d'un deploiement, issues gardees ------------------------------

/// Source `Image` simulee : le telechargement prend `delay`.
struct SlowPull {
    delay: Duration,
    pulls: Mutex<usize>,
}

impl BuildBackend for SlowPull {
    fn buildkit_available(&self) -> bool {
        true
    }

    async fn exec(
        &self,
        _bin: AllowedBin,
        _cmd: Command,
        _sink: &ProgressSink,
        _capture: bool,
    ) -> Result<Vec<u8>, BuildError> {
        Ok(Vec::new())
    }

    async fn build_and_load(
        &self,
        _cmd: Command,
        _staging: &ImageRef,
        _sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        Ok(())
    }

    async fn pull(&self, _image: &ImageRef, _sink: &ProgressSink) -> Result<(), BuildError> {
        *self.pulls.lock().unwrap() += 1;
        tokio::time::sleep(self.delay).await;
        Ok(())
    }

    async fn tag(&self, _source: &ImageRef, _target: &ImageRef) -> Result<(), BuildError> {
        Ok(())
    }

    async fn remove_image(&self, _image: &ImageRef) -> Result<(), BuildError> {
        Ok(())
    }

    async fn prepare(
        &self,
        _container: &PrepareContainer,
        _sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        Ok(())
    }
}

fn image_build(id: DeploymentId) -> BuildSpec {
    BuildSpec {
        deployment_id: id,
        project: "web".into(),
        environment: "production".into(),
        source: BuildSource::Image {
            reference: ImageRef("docker.io/library/nginx:1.27".into()),
        },
        build: BuildMethod::Railpack,
        git_token: None,
        root_directory: None,
        project_id: None,
        environment_id: None,
    }
}

fn build_cfg(bench: &Bench) -> BuildConfig {
    BuildConfig {
        builds_dir: bench.builds_dir(),
        buildkit_socket: "/run/ciphera-agent/buildkit/buildkitd.sock".into(),
        askpass: PathBuf::from("/usr/local/bin/ciphera-agent"),
        prepare: None,
    }
}

fn slow_pull(delay: Duration) -> Arc<SlowPull> {
    Arc::new(SlowPull {
        delay,
        pulls: Mutex::new(0),
    })
}

#[tokio::test(start_paused = true)]
async fn build_success_is_reported_only_once_finalized() {
    let bench = Bench::new();
    let id = DeploymentId::new();
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_build(
        image_build(id),
        build_cfg(&bench),
        slow_pull(Duration::from_secs(1)),
        tx,
    );
    loop {
        match rx.recv().await.expect("reussite du build") {
            AgentToControl::DeployProgress {
                stage: Stage::Image,
                status: StageStatus::Succeeded,
                ..
            } => break,
            AgentToControl::DeployProgress {
                status: StageStatus::Failed { message },
                ..
            } => panic!("{message}"),
            _ => {}
        }
    }
    // Au moment ou la reussite part : operation rendue, issue gardee.
    assert!(bench.deployer.ops().in_progress().is_empty());
    assert_eq!(
        bench.db.outcome(id).unwrap(),
        Some(DeploymentOutcome {
            deployment_id: id,
            stage: Stage::Image,
            status: StageStatus::Succeeded,
        })
    );
}

#[tokio::test(start_paused = true)]
async fn deploy_sent_while_its_build_finishes_waits_for_it() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let mut v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_build(
        image_build(v1.deployment_id),
        build_cfg(&bench),
        slow_pull(Duration::from_secs(3)),
        tx.clone(),
    );
    v1.healthcheck.interval_secs = 1;
    bench.probe.answer("172.18.0.10".parse().unwrap());
    // Le plan de controle envoie le deploiement avant la fin du build.
    bench.deployer.spawn_deploy(v1.clone(), tx.clone());
    tokio::time::sleep(Duration::from_secs(10)).await;
    let st = stages(&drain(&mut rx).await);
    assert!(
        !st.iter()
            .any(|(_, s)| matches!(s, StageStatus::Failed { .. })),
        "{st:?}"
    );
    let image_done = st
        .iter()
        .position(|x| *x == (Stage::Image, StageStatus::Succeeded))
        .expect("build conclu");
    let start = st
        .iter()
        .position(|x| *x == (Stage::Start, StageStatus::Started))
        .expect("deploiement lance");
    assert!(image_done < start, "{st:?}");
    assert_eq!(
        st.last(),
        Some(&(Stage::Switch, StageStatus::Succeeded)),
        "{st:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn deploy_refused_while_its_build_never_ends_reports_failed() {
    let bench = Bench::new();
    let id = DeploymentId::new();
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_build(
        image_build(id),
        build_cfg(&bench),
        slow_pull(Duration::from_secs(3600)),
        tx.clone(),
    );
    let mut v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    v1.deployment_id = id;
    bench.deployer.spawn_deploy(v1, tx.clone());
    tokio::time::sleep(Duration::from_secs(61)).await;
    let st = stages(&drain(&mut rx).await);
    match st.last() {
        Some((Stage::Start, StageStatus::Failed { message })) => {
            assert!(message.contains("build"), "{message}");
        }
        other => panic!("refus rapporte attendu : {other:?} ({st:?})"),
    }
    assert!(bench.docker.runs.lock().unwrap().is_empty());
    // Le build, lui, continue.
    assert_eq!(bench.deployer.ops().in_progress(), vec![id]);
}

#[tokio::test(start_paused = true)]
async fn duplicate_deploy_while_deploying_is_logged_not_silent() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    bench.probe.answer("172.18.0.10".parse().unwrap());
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(v1.clone(), tx.clone());
    tokio::time::sleep(Duration::from_secs(1)).await;
    bench.deployer.spawn_deploy(v1.clone(), tx.clone());
    tokio::time::sleep(Duration::from_secs(10)).await;
    let messages = drain(&mut rx).await;
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, AgentToControl::LogLine(l) if l.line.contains("deja en cours"))),
        "{messages:?}"
    );
    let st = stages(&messages);
    assert!(
        !st.iter()
            .any(|(_, s)| matches!(s, StageStatus::Failed { .. })),
        "{st:?}"
    );
    assert_eq!(bench.docker.runs.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn replayed_deploy_after_failure_reports_the_stored_outcome_only() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    let (result, first) = bench.deploy(&v1).await;
    assert!(result.is_err());
    let failed = stages(&first).last().unwrap().clone();
    assert!(matches!(
        failed,
        (Stage::Healthcheck, StageStatus::Failed { .. })
    ));
    // Rejeu (livraison au moins une fois), alors que l'app repondrait.
    bench.probe.answer("172.18.0.11".parse().unwrap());
    let (_, replay) = bench.deploy(&v1).await;
    assert_eq!(stages(&replay), vec![failed]);
    assert_eq!(bench.docker.runs.lock().unwrap().len(), 1, "rien relance");
    assert!(bench.db.serving().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn replayed_build_after_success_reports_the_stored_outcome_only() {
    let bench = Bench::new();
    let id = DeploymentId::new();
    let backend = slow_pull(Duration::from_millis(10));
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_build(
        image_build(id),
        build_cfg(&bench),
        backend.clone(),
        tx.clone(),
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    drain(&mut rx).await;
    bench.deployer.spawn_build(
        image_build(id),
        build_cfg(&bench),
        backend.clone(),
        tx.clone(),
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        stages(&drain(&mut rx).await),
        vec![(Stage::Image, StageStatus::Succeeded)]
    );
    assert_eq!(*backend.pulls.lock().unwrap(), 1, "rien retelecharge");
}

#[tokio::test(start_paused = true)]
async fn state_carries_recent_outcomes_for_reconciliation() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let failed = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    assert!(bench.deploy(&failed).await.0.is_err());
    let ok = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/api-production:v1",
    );
    deploy_ok(&bench, &ok).await;
    let state = bench.deployer.state().await;
    let by_id: HashMap<DeploymentId, &DeploymentOutcome> = state
        .outcomes
        .iter()
        .map(|o| (o.deployment_id, o))
        .collect();
    assert_eq!(by_id[&ok.deployment_id].stage, Stage::Switch);
    assert_eq!(by_id[&ok.deployment_id].status, StageStatus::Succeeded);
    assert!(matches!(
        by_id[&failed.deployment_id].status,
        StageStatus::Failed { .. }
    ));
    assert_eq!(
        state.outcomes[0].deployment_id, ok.deployment_id,
        "recente d'abord"
    );
}

#[tokio::test(start_paused = true)]
async fn stop_while_a_deploy_waits_on_its_build_cancels_both() {
    let bench = Bench::new();
    // Nettoyage lent : la fenetre entre liberation et issue gardee s'ouvre.
    *bench.docker.rmi_delay.lock().unwrap() = Duration::from_secs(5);
    bench.sync_routes().await;
    let mut v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    v1.healthcheck.interval_secs = 1;
    bench.probe.answer("172.18.0.10".parse().unwrap());
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_build(
        image_build(v1.deployment_id),
        build_cfg(&bench),
        slow_pull(Duration::from_secs(3600)),
        tx.clone(),
    );
    bench.deployer.spawn_deploy(v1.clone(), tx.clone());
    tokio::time::sleep(Duration::from_secs(1)).await;
    bench.deployer.stop(v1.deployment_id, &tx).await;
    tokio::time::sleep(Duration::from_secs(70)).await;
    let st = stages(&drain(&mut rx).await);
    assert!(bench.docker.runs.lock().unwrap().is_empty(), "{st:?}");
    assert!(
        !st.contains(&(Stage::Start, StageStatus::Started)),
        "{st:?}"
    );
    assert!(!st.iter().any(|(s, _)| *s == Stage::Switch), "{st:?}");
    assert_eq!(
        bench
            .db
            .outcome(v1.deployment_id)
            .unwrap()
            .map(|o| o.status),
        Some(StageStatus::Cancelled)
    );
    assert!(bench.db.serving().unwrap().is_empty());
    assert!(bench.deployer.ops().in_progress().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cancellation_is_durable_before_the_slot_is_released() {
    let bench = Bench::new();
    // Nettoyage lent : la fenetre entre liberation et issue gardee s'ouvre.
    *bench.docker.rmi_delay.lock().unwrap() = Duration::from_secs(5);
    bench.sync_routes().await;
    let mut v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    v1.healthcheck.interval_secs = 1;
    let ip: IpAddr = "172.18.0.10".parse().unwrap();
    bench.probe.hang.lock().unwrap().insert(ip);
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(v1.clone(), tx.clone());
    wait_until("healthcheck en cours", || {
        !bench.probe.tries.lock().unwrap().is_empty()
    })
    .await;
    // Un ordre en attente du creneau (comme un Deploy qui attend son
    // build) : au reveil, l'annulation doit deja etre gardee.
    let busy = bench
        .deployer
        .ops()
        .spawn(v1.deployment_id, OpKind::Deploy, |_| async {})
        .expect_err("operation en cours");
    let db = bench.db.clone();
    let id = v1.deployment_id;
    let seen = tokio::spawn(async move {
        assert!(busy.wait(Duration::from_secs(60)).await);
        db.outcome(id).unwrap().map(|o| o.status)
    });
    tokio::task::yield_now().await;
    bench.deployer.stop(id, &tx).await;
    assert_eq!(seen.await.unwrap(), Some(StageStatus::Cancelled));
    drain(&mut rx).await;
    // Rejeu apres l'annulation, l'app repondrait : rien n'est relance.
    bench.probe.hang.lock().unwrap().clear();
    bench.probe.answer("172.18.0.11".parse().unwrap());
    let (_, replay) = bench.deploy(&v1).await;
    assert_eq!(
        stages(&replay),
        vec![(Stage::Start, StageStatus::Cancelled)]
    );
    assert_eq!(bench.docker.runs.lock().unwrap().len(), 1);
    assert!(bench.db.serving().unwrap().is_empty());
}

// --- retrait et nettoyage ------------------------------------------------------

#[tokio::test(start_paused = true)]
async fn restart_restarts_only_the_serving_deployment_and_refreshes_routes() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    deploy_ok(&bench, &v2).await;
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    let v2_container = format!("id-ciphera-d-{}", v2.deployment_id);

    // Deploiement non servi : rien ne redemarre.
    bench.deployer.restart(v1.deployment_id).await;
    assert!(
        !bench
            .docker
            .calls()
            .iter()
            .any(|c| c.starts_with("restart:"))
    );
    assert!(!bench.docker.container(&v1_container).unwrap().running);

    // Deploiement servi : ses conteneurs redemarrent, et la table du proxy
    // suit leur nouvelle adresse.
    let tables = bench.routes.count();
    bench.deployer.restart(v2.deployment_id).await;
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("restart:{v2_container}"))
    );
    assert!(bench.docker.container(&v2_container).unwrap().running);
    assert!(bench.routes.count() > tables, "routes recalculees");
    assert_eq!(
        bench.upstreams("app.example.com"),
        vec![SocketAddr::new(bench.ip_of(v2.deployment_id), 8080)]
    );
    assert_eq!(bench.db.serving().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn app_container_is_reachable_as_service_internal_on_the_project_network() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let runs = bench.docker.runs.lock().unwrap().clone();
    let app = runs
        .iter()
        .find(|s| s.name() == format!("ciphera-d-{}", v1.deployment_id))
        .unwrap();
    let body = ciphera_agent::docker::create_body(app);
    let endpoints = body.networking_config.unwrap().endpoints_config.unwrap();
    assert_eq!(
        endpoints[&ciphera_agent::docker::project_network(project)].aliases,
        Some(vec!["web.internal".to_string()])
    );
}

#[tokio::test(start_paused = true)]
async fn stop_containers_stops_only_the_serving_deployment_and_restart_brings_it_back() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    deploy_ok(&bench, &v2).await;
    let v2_container = format!("id-ciphera-d-{}", v2.deployment_id);

    // Deploiement non servi : rien n'est arrete.
    let stops = bench
        .docker
        .calls()
        .iter()
        .filter(|c| c.starts_with("stop:"))
        .count();
    bench.deployer.stop_containers(v1.deployment_id).await;
    assert_eq!(
        bench
            .docker
            .calls()
            .iter()
            .filter(|c| c.starts_with("stop:"))
            .count(),
        stops
    );

    // Deploiement servi : arrete, toujours servi, plus d'upstream.
    let tables = bench.routes.count();
    bench.deployer.stop_containers(v2.deployment_id).await;
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("stop:{v2_container}"))
    );
    assert!(!bench.docker.container(&v2_container).unwrap().running);
    assert!(bench.routes.count() > tables, "routes recalculees");
    assert!(bench.upstreams("app.example.com").is_empty());
    let serving = bench.db.serving().unwrap();
    assert_eq!(serving.len(), 1);
    assert_eq!(serving[0].deployment.deployment_id, v2.deployment_id);
    assert!(!serving[0].deployment.healthy);

    // `Restart` le relance et la route revient.
    bench.deployer.restart(v2.deployment_id).await;
    assert!(bench.docker.container(&v2_container).unwrap().running);
    assert_eq!(bench.upstreams("app.example.com").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn remove_deployment_deletes_containers_images_and_bundle() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    deploy_ok(&bench, &v2).await;
    bench.db.put_bundle(v1.deployment_id, "{}").unwrap();

    // Retention : l'ancien (arrete) disparait, les routes ne bougent pas.
    let tables = bench.routes.count();
    bench.deployer.remove(v1.deployment_id).await;
    let v1_container = format!("id-ciphera-d-{}", v1.deployment_id);
    assert!(bench.docker.container(&v1_container).is_none());
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("rmi:{}", v1.deployment_id))
    );
    assert!(bench.db.record(v1.deployment_id).unwrap().is_none());
    assert_eq!(bench.db.bundle(v1.deployment_id).unwrap(), None);
    assert_eq!(bench.routes.count(), tables);

    // Suppression du projet : le deploiement servi aussi, upstream retire.
    bench.deployer.remove(v2.deployment_id).await;
    assert!(bench.upstreams("app.example.com").is_empty());
    assert!(bench.db.deployments().unwrap().is_empty());
}

#[tokio::test]
async fn startup_sweep_removes_leftover_sandboxes_and_dirs() {
    let bench = Bench::new();
    let stale = DeploymentId::new();
    let label = stale.to_string();
    bench.docker.add_container(
        "prep",
        &format!("ciphera-prepare-{stale}"),
        &[
            (DEPLOYMENT_LABEL, label.as_str()),
            (ROLE_LABEL, PREPARE_ROLE),
        ],
        None,
    );
    bench
        .docker
        .add_container("app", "ciphera-d-x", &[(DEPLOYMENT_LABEL, "x")], None);
    // Projet Compose d'un utilisateur dont le nom ressemble a un bac a
    // sable : jamais touche (pas d'etiquette de role).
    bench.docker.add_container(
        "user-db",
        "ciphera-prepare-production-db-1",
        &[
            (COMPOSE_LABEL, "ciphera-prepare-production"),
            (COMPOSE_PROJECT_LABEL, "ciphera-prepare-production"),
        ],
        None,
    );
    let build_dir = format!("{}/{stale}/src", bench.builds_dir());
    let deploy_dir = format!("{}/deploys/{stale}", bench.dir);
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::create_dir_all(&deploy_dir).unwrap();

    bench.deployer.sweep().await;

    assert!(bench.docker.container("prep").is_none());
    assert!(
        bench.docker.container("app").is_some(),
        "applications intactes"
    );
    assert!(
        bench.docker.container("user-db").is_some(),
        "reconnu par etiquettes, jamais par nom"
    );
    assert!(!Path::new(&format!("{}/{stale}", bench.builds_dir())).exists());
    assert!(!Path::new(&deploy_dir).exists());
    assert!(Path::new(&bench.builds_dir()).is_dir());
}

/// Accuses `DeploymentRemoved` recus sur `rx` jusqu'ici.
fn removal_acks(rx: &mut mpsc::Receiver<AgentToControl>) -> Vec<DeploymentId> {
    let mut out = Vec::new();
    while let Ok(m) = rx.try_recv() {
        if let AgentToControl::DeploymentRemoved { deployment_id } = m {
            out.push(deployment_id);
        }
    }
    out
}

/// Suppression Docker en echec : l'etat local est garde, sans accuse ; le
/// prochain ordre recommence et n'accuse qu'une fois tout supprime.
#[tokio::test(start_paused = true)]
async fn agent_keeps_state_when_docker_remove_fails() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let v2 = spec(project, web, "ciphera.invalid/web-production:v2");
    deploy_ok(&bench, &v2).await;
    let id = v1.deployment_id;
    let tag = format!("ciphera.invalid/web-production:{id}");
    bench.docker.images.lock().unwrap().push(tag.clone());
    bench.db.put_bundle(id, "{}").unwrap();
    let container = format!("id-ciphera-d-{id}");
    let (tx, mut rx) = mpsc::channel(16);

    // Conteneur (et image) non supprimes : etat garde, aucun accuse.
    bench.docker.fail_remove.store(true, Ordering::SeqCst);
    bench.docker.fail_rmi.store(true, Ordering::SeqCst);
    bench.deployer.remove_acked(id, &tx).await;
    assert!(removal_acks(&mut rx).is_empty());
    assert!(bench.db.record(id).unwrap().is_some(), "etat local garde");
    assert!(bench.db.bundle(id).unwrap().is_some());
    assert!(bench.docker.container(&container).is_some());

    // Conteneur supprime mais image en echec : toujours garde.
    bench.docker.fail_remove.store(false, Ordering::SeqCst);
    bench.deployer.remove_acked(id, &tx).await;
    assert!(removal_acks(&mut rx).is_empty());
    assert!(bench.db.record(id).unwrap().is_some());
    assert!(bench.docker.container(&container).is_none());
    assert_eq!(*bench.docker.images.lock().unwrap(), vec![tag]);

    // Ordre suivant, Docker revenu : tout part, accuse.
    bench.docker.fail_rmi.store(false, Ordering::SeqCst);
    bench.deployer.remove_acked(id, &tx).await;
    assert_eq!(removal_acks(&mut rx), vec![id]);
    assert!(bench.db.record(id).unwrap().is_none());
    assert!(bench.db.bundle(id).unwrap().is_none());
    assert!(bench.docker.images.lock().unwrap().is_empty());
    assert!(
        bench.db.record(v2.deployment_id).unwrap().is_some(),
        "le servi reste"
    );

    // Ordre rejoue (deja absent) : accuse de nouveau.
    bench.deployer.remove_acked(id, &tx).await;
    assert_eq!(removal_acks(&mut rx), vec![id]);
}

/// Rend le conteneur `id` du faux Docker arrete, avec l'image `image`.
fn stopped_with_image(bench: &Bench, id: &str, image: &str) {
    let mut containers = bench.docker.containers.lock().unwrap();
    let c = containers.get_mut(id).unwrap();
    c.running = false;
    c.image = image.to_string();
}

/// Balayage des restes : conteneurs arretes etiquetes d'un deploiement absent
/// de l'etat local et images dont l'etat local ne garde rien et qu'aucun
/// conteneur restant n'utilise sont supprimes ; le reste (deploiements
/// enregistres, orphelin en marche, build conclu en attente de son `Deploy`,
/// services Compose sans deploiement, autres conteneurs) reste.
#[tokio::test(start_paused = true)]
async fn orphan_labeled_containers_and_images_are_swept() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench.sync_routes().await;
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let kept = v1.deployment_id;
    let orphan = DeploymentId::new();
    let live = DeploymentId::new();
    let built = DeploymentId::new();
    let stray_image = DeploymentId::new();
    let shared = DeploymentId::new();
    // Anciens noms (`ciphera/`, `ciphera-staging/`) d'un deploiement
    // anterieur : memes regles.
    let legacy_stray = DeploymentId::new();
    let legacy_used = DeploymentId::new();
    let legacy = |id: DeploymentId| format!("ciphera/web-production:{id}");
    bench
        .db
        .put_outcome(built, Stage::Image, &StageStatus::Succeeded)
        .unwrap();
    let tag = |id: DeploymentId| format!("ciphera.invalid/web-production:{id}");
    let project_label = project.to_string();
    for (cid, id) in [("orphan", orphan), ("live", live)] {
        let label = id.to_string();
        bench.docker.add_container(
            cid,
            &format!("ciphera-d-{id}"),
            &[
                (DEPLOYMENT_LABEL, label.as_str()),
                (PROJECT_LABEL, project_label.as_str()),
            ],
            None,
        );
    }
    stopped_with_image(&bench, "orphan", &tag(orphan));
    bench
        .docker
        .containers
        .lock()
        .unwrap()
        .get_mut("live")
        .unwrap()
        .image = tag(live);
    bench.docker.add_container(
        "compose-db",
        "web-production-db-1",
        &[
            (DEPLOYMENT_LABEL, ""),
            (COMPOSE_LABEL, "web-production"),
            (PROJECT_LABEL, project_label.as_str()),
        ],
        None,
    );
    // Conteneur sans deploiement qui utilise l'image d'un deploiement inconnu
    // (arrete compris) : l'image reste.
    bench
        .docker
        .add_container("odd", "odd", &[(DEPLOYMENT_LABEL, "pas-un-uuid")], None);
    stopped_with_image(&bench, "odd", &tag(shared));
    bench.docker.add_container(
        "odd-legacy",
        "odd-legacy",
        &[(DEPLOYMENT_LABEL, "pas-un-uuid")],
        None,
    );
    stopped_with_image(&bench, "odd-legacy", &legacy(legacy_used));
    bench.docker.images.lock().unwrap().extend([
        legacy(legacy_stray),
        format!("ciphera-staging/{legacy_stray}-web:build"),
        legacy(legacy_used),
        tag(kept),
        tag(orphan),
        tag(live),
        tag(built),
        tag(stray_image),
        tag(shared),
        format!("ciphera.invalid/staging/{stray_image}-web:build"),
        "nginx:alpine".to_string(),
    ]);

    bench.deployer.sweep_orphans().await;

    let kept_container = format!("id-ciphera-d-{kept}");
    assert!(bench.docker.container(&kept_container).is_some());
    assert!(
        bench.docker.container("orphan").is_none(),
        "orphelin arrete supprime"
    );
    assert!(
        bench.docker.container("live").is_some(),
        "orphelin en marche garde : le plan de controle decide"
    );
    assert!(bench.docker.container("compose-db").is_some());
    assert!(bench.docker.container("odd").is_some());
    let mut images = bench.docker.images.lock().unwrap().clone();
    images.sort();
    let mut expected = vec![
        tag(kept),
        tag(live),
        tag(built),
        tag(shared),
        legacy(legacy_used),
        "nginx:alpine".to_string(),
    ];
    expected.sort();
    assert_eq!(images, expected);
    assert!(bench.db.record(kept).unwrap().is_some());
}

/// Projet dont le retrait (`RemoveProject`) est a reprendre : ses conteneurs
/// relevent de `remove_project`, jamais du balayage des restes.
#[tokio::test(start_paused = true)]
async fn orphan_sweep_leaves_a_retiring_project_alone() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let v1 = spec(
        ProjectId::new(),
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    deploy_ok(&bench, &v1).await;
    let retiring = ProjectId::new();
    let orphan = DeploymentId::new();
    let (orphan_label, project_label) = (orphan.to_string(), retiring.to_string());
    bench.docker.add_container(
        "retiring",
        &format!("ciphera-d-{orphan}"),
        &[
            (DEPLOYMENT_LABEL, orphan_label.as_str()),
            (PROJECT_LABEL, project_label.as_str()),
        ],
        None,
    );
    stopped_with_image(&bench, "retiring", "");
    bench.docker.fail_remove.store(true, Ordering::SeqCst);
    assert!(bench.deployer.remove_project(retiring, true).await.is_err());
    bench.docker.fail_remove.store(false, Ordering::SeqCst);

    bench.deployer.sweep_orphans().await;
    assert!(bench.docker.container("retiring").is_some());
}

/// Etat local neuf (aucun deploiement) alors que des conteneurs de
/// deploiement existent : rien n'est supprime, ni conteneur (meme arrete), ni
/// image.
#[tokio::test(start_paused = true)]
async fn fresh_state_skips_the_orphan_sweep() {
    let bench = Bench::new();
    let (running, stopped) = (DeploymentId::new(), DeploymentId::new());
    let tag = |id: DeploymentId| format!("ciphera.invalid/web-production:{id}");
    for (cid, id) in [("running", running), ("stopped", stopped)] {
        let label = id.to_string();
        bench.docker.add_container(
            cid,
            &format!("ciphera-d-{id}"),
            &[(DEPLOYMENT_LABEL, label.as_str())],
            None,
        );
    }
    stopped_with_image(&bench, "stopped", &tag(stopped));
    let unused = DeploymentId::new();
    bench
        .docker
        .images
        .lock()
        .unwrap()
        .extend([tag(running), tag(stopped), tag(unused)]);
    assert!(bench.db.deployments().unwrap().is_empty());

    bench.deployer.sweep_orphans().await;

    assert!(bench.docker.container("running").is_some());
    assert!(bench.docker.container("stopped").is_some());
    assert_eq!(bench.docker.images.lock().unwrap().len(), 3);
    assert!(
        !bench
            .docker
            .calls()
            .iter()
            .any(|c| c.starts_with("remove:")),
        "{:?}",
        bench.docker.calls()
    );
}

// --- Compose -------------------------------------------------------------------

/// Deploiement dont le build a produit `compose_bundle()` : ses images
/// construites portent cette etiquette.
fn bundle_id() -> DeploymentId {
    "0b7c3e2a-5d41-4f6e-9a8b-1c2d3e4f5a6b".parse().unwrap()
}

/// `bundle` tel que le build du deploiement `id` l'enregistre : images
/// construites etiquetees `id`.
fn built_as(bundle: &Value, id: DeploymentId) -> Value {
    let mut out = bundle.clone();
    for svc in out["services"].as_object_mut().unwrap().values_mut() {
        let Some(image) = svc["image"].as_str() else {
            continue;
        };
        if shared::images::is_internal_ref(image) {
            let (repo, _) = image.rsplit_once(':').unwrap();
            svc["image"] = json!(format!("{repo}:{id}"));
        }
    }
    out
}

/// Ensemble Compose tel que produit par le build `bundle_id()` (forme
/// normalisee).
fn compose_bundle() -> Value {
    let web = format!("ciphera.invalid/shop-production/web:{}", bundle_id());
    let api = format!("ciphera.invalid/shop-production/api:{}", bundle_id());
    json!({
        "name": "shop-production",
        "services": {
            "web": {
                "image": web,
                "depends_on": {"api": {"condition": "service_started", "required": true}},
                "networks": {"default": null},
                "environment": {"API_HOST": "api"},
                "volumes": [{"type": "volume", "source": "uploads", "target": "/srv"}]
            },
            "api": {
                "image": api,
                "depends_on": {"cache": {"condition": "service_started", "required": true}},
                "networks": {"default": null},
                "labels": {"app.tier": "back"}
            },
            "cache": {
                "image": "docker.io/library/redis:7-alpine",
                "networks": {"default": null}
            }
        },
        "networks": {"default": {"name": "shop-production_default"}},
        "volumes": {"uploads": {"name": "shop-production_uploads"}}
    })
}

/// Rotation des journaux imposee a chaque service Compose (la politique
/// refuse deja la section `logging` de l'utilisateur).
#[test]
fn compose_services_get_rotated_logs() {
    let plan = ComposePlan::new(
        &compose_bundle(),
        "web",
        ProjectId::new(),
        DeploymentId::new(),
        bundle_id(),
    )
    .unwrap();
    for (name, svc) in plan.file["services"].as_object().unwrap() {
        assert_eq!(
            svc["logging"],
            json!({
                "driver": ciphera_agent::docker::LOG_DRIVER,
                "options": {
                    "max-size": ciphera_agent::docker::LOG_MAX_SIZE,
                    "max-file": ciphera_agent::docker::LOG_MAX_FILE,
                }
            }),
            "{name}"
        );
    }
}
#[test]
fn compose_services_are_forced_onto_the_project_network() {
    let (project, deployment) = (ProjectId::new(), DeploymentId::new());
    let plan =
        ComposePlan::new(&compose_bundle(), "web", project, deployment, bundle_id()).unwrap();
    let file = &plan.file;
    // Reseau `default` = reseau du projet, externe ; aucun autre reseau.
    assert_eq!(
        file["networks"],
        json!({"default": {"name": project_network(project), "external": true}})
    );
    for (name, svc) in file["services"].as_object().unwrap() {
        // Seul reseau : `default`, avec l'alias `<service>.internal`.
        assert_eq!(
            svc["networks"],
            json!({"default": {"aliases": [format!("{name}.internal")]}}),
            "{name} : {svc}"
        );
        let labels = svc["labels"].as_object().unwrap();
        assert_eq!(labels[MANAGED_LABEL], "true");
        assert_eq!(labels[COMPOSE_LABEL], "shop-production");
        assert_eq!(labels[PROJECT_LABEL], project.to_string());
        assert_eq!(labels[ROLE_LABEL], "", "{name} : role");
        assert_eq!(labels[ROLE_LABEL], "");
        // Toujours posee, vide hors du service expose : jamais celle de l'image.
        assert_eq!(
            labels[DEPLOYMENT_LABEL],
            if name == "web" {
                deployment.to_string()
            } else {
                String::new()
            },
            "{name} : etiquette de deploiement sur le service expose seul"
        );
        assert!(
            svc["security_opt"]
                .as_array()
                .unwrap()
                .contains(&json!("no-new-privileges:true")),
            "{name}"
        );
    }
    assert_eq!(file["services"]["api"]["labels"]["app.tier"], "back");
    assert!(file.get("name").is_none());
    assert_eq!(
        file["volumes"]["uploads"]["name"],
        "shop-production_uploads"
    );
    // Services non exposes dans l'ordre de depends_on, expose a part.
    let order: Vec<&str> = plan.order.iter().map(ValidatedArg::as_str).collect();
    assert_eq!(order, ["cache", "api"]);
    assert_eq!(plan.stable.as_str(), "shop-production");
    assert_eq!(plan.exposed.as_str(), "web");
    assert_eq!(
        plan.exposed_project.as_str(),
        format!("ciphera-d-{deployment}")
    );
}

#[test]
fn compose_bundle_with_declared_network_or_privilege_is_refused() {
    let (project, deployment) = (ProjectId::new(), DeploymentId::new());
    let mut declared = compose_bundle();
    declared["networks"]["backend"] = json!({});
    declared["services"]["api"]["networks"] = json!({"backend": null});
    assert!(ComposePlan::new(&declared, "web", project, deployment, bundle_id()).is_err());
    let mut host = compose_bundle();
    host["services"]["api"]["network_mode"] = json!("host");
    assert!(ComposePlan::new(&host, "web", project, deployment, bundle_id()).is_err());
    let mut forged = compose_bundle();
    forged["services"]["api"]["labels"] = json!({"dev.ciphera.deployment": "x"});
    assert!(ComposePlan::new(&forged, "web", project, deployment, bundle_id()).is_err());
    let mut cycle = compose_bundle();
    cycle["services"]["cache"]["depends_on"] = json!(["web"]);
    let err = ComposePlan::new(&cycle, "web", project, deployment, bundle_id()).unwrap_err();
    assert!(err.contains("cycle"), "{err}");
    let mut unknown = compose_bundle();
    unknown["services"]["cache"]["depends_on"] = json!(["db"]);
    assert!(ComposePlan::new(&unknown, "web", project, deployment, bundle_id()).is_err());
    assert!(
        ComposePlan::new(
            &compose_bundle(),
            "absent",
            project,
            deployment,
            bundle_id()
        )
        .is_err()
    );
    let mut unbuilt = compose_bundle();
    unbuilt["services"]["api"]
        .as_object_mut()
        .unwrap()
        .remove("image");
    assert!(ComposePlan::new(&unbuilt, "web", project, deployment, bundle_id()).is_err());
}

/// Ensemble a deployer : une image interne n'est acceptee que si c'est
/// l'image construite du service lui-meme, et n'est jamais telechargee.
#[test]
fn compose_bundle_internal_images_only_their_own() {
    let (project, deployment) = (ProjectId::new(), DeploymentId::new());
    let b = bundle_id();
    let plan = ComposePlan::new(&compose_bundle(), "web", project, deployment, b).unwrap();
    let services = &plan.file["services"];
    assert_eq!(services["web"]["pull_policy"], "never");
    assert_eq!(services["api"]["pull_policy"], "never");
    assert!(services["cache"].get("pull_policy").is_none());
    // Ancien nom enregistre (deploiement anterieur) : accepte, jamais
    // telecharge non plus.
    let mut legacy = compose_bundle();
    legacy["services"]["web"]["image"] = json!(format!("ciphera/shop-production/web:{b}"));
    let plan = ComposePlan::new(&legacy, "web", project, deployment, b).unwrap();
    assert_eq!(plan.file["services"]["web"]["pull_policy"], "never");
    for image in [
        format!("ciphera.invalid/shop-production/api:{b}"),
        format!("ciphera.invalid/other-production:{b}"),
        format!("ciphera.invalid/staging/{b}-web:build"),
        format!("ciphera.invalid/a/b/web:{b}"),
        format!("ciphera.invalid//web:{b}"),
        "ciphera.invalid/shop-production/web".to_string(),
        format!("docker.io/ciphera/shop-production/web:{b}"),
        format!("ciphera-staging/{b}-web:build"),
        format!("CIPHERA.INVALID/shop-production/web:{b}"),
    ] {
        let mut other = compose_bundle();
        other["services"]["web"]["image"] = json!(image);
        assert!(
            ComposePlan::new(&other, "web", project, deployment, b).is_err(),
            "{image}"
        );
    }
}

/// Image construite acceptee seulement sous l'etiquette du build qui a
/// enregistre l'ensemble : jamais celle d'un autre deploiement (meme
/// projet, meme service), ni une etiquette quelconque.
#[test]
fn compose_bundle_image_tag_must_be_its_build() {
    let (project, deployment) = (ProjectId::new(), DeploymentId::new());
    let b = bundle_id();
    // Rollback : deploiement nouveau, ensemble (et images) du build `b`.
    ComposePlan::new(&compose_bundle(), "web", project, deployment, b).unwrap();
    // Meme ensemble annonce comme celui d'un autre build : refuse.
    assert!(ComposePlan::new(&compose_bundle(), "web", project, deployment, deployment).is_err());
    let other = DeploymentId::new();
    for (service, image) in [
        (
            "web",
            format!("ciphera.invalid/shop-production/web:{other}"),
        ),
        (
            "web",
            "ciphera.invalid/shop-production/web:latest".to_string(),
        ),
        (
            "api",
            format!("ciphera.invalid/shop-production/api:{other}"),
        ),
        ("web", format!("ciphera/shop-production/web:{other}")),
    ] {
        let mut bundle = compose_bundle();
        bundle["services"][service]["image"] = json!(image);
        assert!(
            ComposePlan::new(&bundle, "web", project, deployment, b).is_err(),
            "{service} : {image}"
        );
    }
}

const GIB: u64 = 1 << 30;

fn limited_plan(bundle: &Value) -> Result<ComposePlan, String> {
    let limits = ciphera_agent::docker::AppLimits::compute(8 * GIB, 4);
    ComposePlan::with_limits(
        bundle,
        "web",
        ProjectId::new(),
        DeploymentId::new(),
        bundle_id(),
        limits,
    )
}

/// Volumes sans nom dans l'ensemble : nommes d'apres le projet stable, pour
/// que le service expose (projet `ciphera-d-<id>`) garde ses donnees d'un
/// deploiement a l'autre et que le retrait du projet les retrouve.
#[test]
fn compose_volumes_are_named_after_the_stable_project() {
    let mut bundle = compose_bundle();
    bundle["volumes"] = json!({"uploads": null, "logs": {}});
    bundle["services"]["api"]["volumes"] =
        json!([{"type": "volume", "source": "logs", "target": "/logs"}]);
    let plan = limited_plan(&bundle).unwrap();
    assert_eq!(
        plan.file["volumes"],
        json!({
            "uploads": {"name": "shop-production_uploads"},
            "logs": {"name": "shop-production_logs"}
        })
    );
    // Nom deja present : garde.
    let plan = limited_plan(&compose_bundle()).unwrap();
    assert_eq!(
        plan.file["volumes"]["uploads"]["name"],
        json!("shop-production_uploads")
    );
}

#[test]
fn compose_exposed_service_gets_user_and_read_only_root() {
    let mut plan = limited_plan(&compose_bundle()).unwrap();
    // Sans reglage : rien ne change.
    let before = plan.file.clone();
    plan.secure_exposed(None, false).unwrap();
    assert_eq!(plan.file, before);
    assert!(plan.secure_exposed(Some("root"), false).is_err());
    plan.secure_exposed(Some("1000:1000"), true).unwrap();
    let services = plan.file["services"].as_object().unwrap();
    assert_eq!(services["web"]["user"], json!("1000:1000"));
    assert_eq!(services["web"]["read_only"], json!(true));
    assert_eq!(
        services["web"]["tmpfs"],
        json!(["/tmp:rw,nosuid,nodev,size=64m"])
    );
    // Les autres services gardent leur fichier Compose.
    for other in ["api", "cache"] {
        for key in ["user", "read_only", "tmpfs"] {
            assert!(services[other].get(key).is_none(), "{other}.{key}");
        }
    }
    for (name, svc) in services {
        let drop = svc["cap_drop"].as_array().unwrap();
        for cap in ["AUDIT_WRITE", "SETPCAP"] {
            assert!(drop.contains(&json!(cap)), "{name} {cap}");
        }
    }
}

#[test]
fn compose_read_only_keeps_a_declared_tmp() {
    let mut bundle = compose_bundle();
    bundle["services"]["web"]["tmpfs"] = json!("/tmp:size=1m");
    let mut plan = limited_plan(&bundle).unwrap();
    plan.secure_exposed(None, true).unwrap();
    let tmpfs = plan.file["services"]["web"]["tmpfs"].as_array().unwrap();
    assert_eq!(tmpfs.len(), 1);
    assert!(tmpfs[0].as_str().unwrap().starts_with("/tmp:"));
}

#[test]
fn compose_plan_forces_pids_and_caps() {
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["cap_drop"] = json!(["SYS_CHROOT"]);
    bundle["services"]["cache"]["pids_limit"] = json!(50);
    bundle["services"]["web"]["pids_limit"] = json!(-1);
    let plan = limited_plan(&bundle).unwrap();
    let services = plan.file["services"].as_object().unwrap();
    for (name, svc) in services {
        let drop = svc["cap_drop"].as_array().unwrap();
        assert!(drop.contains(&json!("NET_RAW")), "{name}");
        assert!(drop.contains(&json!("MKNOD")), "{name}");
        // Moitie de 8 Gio, 3 CPU (4 - 1), sans swap en plus.
        assert_eq!(svc["mem_limit"], json!(4 * GIB), "{name}");
        assert_eq!(svc["memswap_limit"], json!(4 * GIB), "{name}");
        assert_eq!(svc["cpus"], json!(3.0), "{name}");
    }
    assert!(
        services["api"]["cap_drop"]
            .as_array()
            .unwrap()
            .contains(&json!("SYS_CHROOT"))
    );
    assert_eq!(services["api"]["pids_limit"], json!(1024));
    assert_eq!(
        services["cache"]["pids_limit"],
        json!(50),
        "plus bas : gardee"
    );
    assert_eq!(
        services["web"]["pids_limit"],
        json!(1024),
        "illimite : plafonne"
    );
}

#[test]
fn compose_pids_conflict_single_value() {
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["pids_limit"] = json!(5000);
    bundle["services"]["api"]["deploy"] = json!({
        "resources": {"limits": {"pids": 300, "memory": "512m", "cpus": "0.5"}}
    });
    let plan = limited_plan(&bundle).unwrap();
    let api = &plan.file["services"]["api"];
    assert_eq!(api["pids_limit"], json!(300));
    assert_eq!(api["mem_limit"], json!(512u64 << 20));
    assert_eq!(api["cpus"], json!(0.5));
    // Plus aucune limite en double sous `deploy`.
    assert!(api.get("deploy").is_none(), "{api}");
}

#[test]
fn replicas_over_cap_refused() {
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["scale"] = json!(5);
    let err = limited_plan(&bundle).unwrap_err();
    assert!(err.contains("repliques"), "{err}");
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["deploy"] = json!({"replicas": 100});
    assert!(limited_plan(&bundle).is_err());
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["scale"] = json!(4);
    bundle["services"]["cache"]["deploy"] = json!({"replicas": 2});
    assert!(limited_plan(&bundle).is_ok());
}

#[test]
fn exposed_service_replicas_must_be_1() {
    let mut bundle = compose_bundle();
    bundle["services"]["web"]["scale"] = json!(2);
    let err = limited_plan(&bundle).unwrap_err();
    assert!(err.contains("une seule replique"), "{err}");
    let mut bundle = compose_bundle();
    bundle["services"]["web"]["deploy"] = json!({"replicas": 3});
    assert!(limited_plan(&bundle).is_err());
    let mut bundle = compose_bundle();
    bundle["services"]["web"]["scale"] = json!(1);
    assert!(limited_plan(&bundle).is_ok());
}

#[test]
fn tmpfs_shm_bounded() {
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["tmpfs"] = json!([
        "/run",
        "/tmp:size=10g,mode=1777",
        "/var:size=1m",
        "/a:size=0",
        "/b:size=zz",
    ]);
    bundle["services"]["api"]["shm_size"] = json!("4g");
    bundle["services"]["cache"]["shm_size"] = json!(1_048_576);
    bundle["services"]["web"]["shm_size"] = json!(0);
    bundle["services"]["web"]["volumes"] = json!([
        {"type": "tmpfs", "target": "/scratch"},
        {"type": "tmpfs", "target": "/big", "tmpfs": {"size": 10_000_000_000u64}},
        {"type": "tmpfs", "target": "/zero", "tmpfs": {"size": 0}},
    ]);
    let plan = limited_plan(&bundle).unwrap();
    let services = &plan.file["services"];
    let max = 256u64 << 20;
    assert_eq!(
        services["api"]["tmpfs"],
        json!([
            format!("/run:size={max}"),
            format!("/tmp:mode=1777,size={max}"),
            format!("/var:size={}", 1u64 << 20),
            format!("/a:size={max}"),
            format!("/b:size={max}"),
        ])
    );
    assert_eq!(services["api"]["shm_size"], json!(max));
    assert_eq!(services["cache"]["shm_size"], json!(1_048_576));
    assert_eq!(services["web"]["volumes"][0]["tmpfs"]["size"], json!(max));
    assert_eq!(services["web"]["volumes"][1]["tmpfs"]["size"], json!(max));
    assert_eq!(services["web"]["volumes"][2]["tmpfs"]["size"], json!(max));
    assert_eq!(services["web"]["shm_size"], json!(max));
}

#[test]
fn memory_reservation_clamped_to_limit() {
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["mem_reservation"] = json!("64g");
    bundle["services"]["cache"]["mem_reservation"] = json!("1m");
    bundle["services"]["web"]["deploy"] =
        json!({"resources": {"reservations": {"memory": "100g", "cpus": "0.5"}}});
    let plan = limited_plan(&bundle).unwrap();
    let services = &plan.file["services"];
    assert_eq!(services["api"]["mem_reservation"], json!(4 * GIB));
    assert_eq!(services["cache"]["mem_reservation"], json!("1m"));
    assert_eq!(
        services["web"]["deploy"]["resources"]["reservations"]["memory"],
        json!(4 * GIB)
    );
}

#[test]
fn build_bundle_replaces_build_sections_with_built_images() {
    let normalized = json!({
        "name": "shop-production",
        "services": {
            "web": {"build": {"context": "/x/web"}, "env_file": ["/x/.env"], "environment": {"A": "1"}},
            "cache": {"image": "docker.io/library/redis:7-alpine"}
        }
    });
    let file = ComposeFile::from_normalized_json(
        normalized.to_string().as_bytes(),
        Path::new("/x"),
        Path::new("/x"),
    )
    .unwrap();
    let out: Value = serde_json::from_str(&bundle(
        &file,
        &[(
            "web".into(),
            ImageRef("ciphera.invalid/shop-production/web:d".into()),
        )],
    ))
    .unwrap();
    assert_eq!(
        out["services"]["web"]["image"],
        "ciphera.invalid/shop-production/web:d"
    );
    assert!(out["services"]["web"].get("build").is_none());
    assert!(out["services"]["web"].get("env_file").is_none());
    assert_eq!(out["services"]["web"]["environment"]["A"], "1");
    assert_eq!(
        out["services"]["cache"]["image"],
        "docker.io/library/redis:7-alpine"
    );
}

#[test]
fn compose_up_command_is_fixed_and_validated() {
    let p = ValidatedArg::name("shop-production").unwrap();
    let f = ValidatedArg::path("/var/lib/ciphera-agent/deploys/d/compose.json").unwrap();
    let s = ValidatedArg::compose_service("web").unwrap();
    let h = ValidatedArg::path("/var/lib/ciphera-agent/deploys/d").unwrap();
    let cmd = up_command(&p, &f, &s, &h, &[]);
    assert_eq!(cmd.get_program(), "/usr/bin/docker");
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "compose",
            "-p",
            "shop-production",
            "-f",
            "/var/lib/ciphera-agent/deploys/d/compose.json",
            "up",
            "-d",
            "--no-deps",
            "--no-build",
            "--quiet-pull",
            "web"
        ]
    );
    assert!(ValidatedArg::compose_service("-web").is_err());
    assert!(ValidatedArg::compose_service("web;rm").is_err());
}

#[tokio::test(start_paused = true)]
async fn compose_deploy_restarts_others_in_order_and_switches_the_exposed_service() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    let build = DeploymentId::new();
    bench
        .db
        .put_bundle(build, &built_as(&compose_bundle(), build).to_string())
        .unwrap();
    let deploy = |id: DeploymentId| DeploySpec {
        deployment_id: id,
        service_id: web,
        image: ImageRef("ciphera.invalid/shop-production/web:x".into()),
        internal_port: Some(3000),
        project_id: project,
        healthcheck: Healthcheck::default(),
        compose: Some(ComposeDeploy {
            service: "web".into(),
            bundle: build,
        }),
        service_name: Some("web".into()),
        env: Vec::new(),
        environment_id: None,
        run_as: None,
        read_only_rootfs: false,
    };
    let first = deploy(build);
    // Adresses attribuees par le faux Compose : 172.18.1.<100 + rang>.
    for n in 100..110 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let (result, messages) = bench.deploy(&first).await;
    result.unwrap();
    assert_eq!(
        stages(&messages).last(),
        Some(&(Stage::Switch, StageStatus::Succeeded))
    );
    let exposed_project = format!("ciphera-d-{build}");
    assert_eq!(
        *bench.compose.ups.lock().unwrap(),
        vec![
            ("shop-production".to_string(), "cache".to_string()),
            ("shop-production".to_string(), "api".to_string()),
            (exposed_project.clone(), "web".to_string()),
        ]
    );
    let file = bench.compose.file.lock().unwrap().clone().unwrap();
    assert_eq!(
        file["networks"]["default"]["name"],
        project_network(project)
    );
    assert!(
        bench
            .docker
            .calls()
            .contains(&format!("network:{}", project_network(project)))
    );
    let web_ip = bench
        .docker
        .container(&format!("cid-{exposed_project}-web-1"))
        .unwrap()
        .ip
        .unwrap();
    assert_eq!(
        bench.upstreams("shop.example.com"),
        vec![SocketAddr::new(web_ip, 3000)]
    );
    // Dossier transitoire du fichier Compose supprime.
    assert!(!Path::new(&format!("{}/deploys/{build}", bench.dir)).exists());

    // Rollback / redeploiement depuis le meme ensemble : nouveau service
    // expose a cote, l'ancien arrete apres le drainage.
    let second = deploy(DeploymentId::new());
    bench.deploy(&second).await.0.unwrap();
    let old = format!("cid-{exposed_project}-web-1");
    assert!(!bench.docker.container(&old).unwrap().running);
    assert!(
        bench
            .docker
            .container("cid-shop-production-cache-1")
            .unwrap()
            .running,
        "services non exposes jamais arretes par le drainage"
    );
}

fn compose_deploy_spec(project: ProjectId, web: ServiceId, id: DeploymentId) -> DeploySpec {
    DeploySpec {
        deployment_id: id,
        service_id: web,
        image: ImageRef("ciphera.invalid/shop-production/web:x".into()),
        internal_port: Some(3000),
        project_id: project,
        healthcheck: Healthcheck::default(),
        compose: Some(ComposeDeploy {
            service: "web".into(),
            bundle: id,
        }),
        service_name: Some("web".into()),
        env: Vec::new(),
        environment_id: None,
        run_as: None,
        read_only_rootfs: false,
    }
}

#[tokio::test(start_paused = true)]
async fn restart_does_not_start_previous_compose_deployment() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..120 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let (first, second) = (DeploymentId::new(), DeploymentId::new());
    for id in [first, second] {
        bench
            .db
            .put_bundle(id, &built_as(&compose_bundle(), id).to_string())
            .unwrap();
        bench
            .deploy(&compose_deploy_spec(project, web, id))
            .await
            .0
            .unwrap();
    }
    let old = format!("cid-ciphera-d-{first}-web-1");
    let current = format!("cid-ciphera-d-{second}-web-1");
    assert!(!bench.docker.container(&old).unwrap().running);
    assert!(bench.docker.container(&current).unwrap().running);

    bench.deployer.restart(second).await;

    let calls = bench.docker.calls();
    assert!(calls.contains(&format!("restart:{current}")));
    assert!(
        calls.contains(&"restart:cid-shop-production-cache-1".to_string()),
        "les services non exposes redemarrent"
    );
    assert!(
        !calls.contains(&format!("restart:{old}")),
        "l'ancien deploiement n'est pas relance"
    );
    assert!(!bench.docker.container(&old).unwrap().running);
}

#[tokio::test(start_paused = true)]
async fn image_labels_cannot_join_other_project() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let other = ProjectId::new();
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    // Ensemble Compose d'un autre projet : service non expose en marche.
    let other_project = other.to_string();
    bench.docker.add_container(
        "cid-other-db",
        "other-db",
        &[
            (COMPOSE_LABEL, "other-production"),
            (PROJECT_LABEL, other_project.as_str()),
        ],
        Some("172.18.9.9"),
    );
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let container = format!("id-ciphera-d-{}", v1.deployment_id);

    // Les etiquettes reservees sont posees par l'agent, vides si sans objet.
    let run = bench.docker.runs.lock().unwrap().last().unwrap().clone();
    let body = ciphera_agent::docker::create_body(&run);
    let labels = body.labels.unwrap();
    assert_eq!(labels[COMPOSE_LABEL], "");
    assert_eq!(labels[PROJECT_LABEL], project.to_string());
    assert_eq!(labels[DEPLOYMENT_LABEL], v1.deployment_id.to_string());

    // Meme si le conteneur portait des etiquettes etrangeres (image
    // hostile), l'etat local fait foi : aucun ordre ne vise l'autre projet.
    bench
        .docker
        .containers
        .lock()
        .unwrap()
        .get_mut(&container)
        .unwrap()
        .labels
        .extend([
            (COMPOSE_LABEL.to_string(), "other-production".to_string()),
            (PROJECT_LABEL.to_string(), other_project.clone()),
        ]);
    bench.deployer.restart(v1.deployment_id).await;
    bench.deployer.stop_containers(v1.deployment_id).await;
    bench.deployer.remove(v1.deployment_id).await;
    let calls = bench.docker.calls();
    assert!(
        !calls.iter().any(|c| c.ends_with("cid-other-db")),
        "le conteneur de l'autre projet n'est pas touche : {calls:?}"
    );
    assert!(bench.docker.container("cid-other-db").unwrap().running);
}

// --- variables d'environnement (Task 29) ----------------------------------------

/// Fichiers de la base locale (`state.db`, et son journal WAL).
fn state_files(db: &Path) -> Vec<u8> {
    let dir = db.parent().unwrap();
    let name = db.file_name().unwrap().to_string_lossy().into_owned();
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with(&name) {
            all.extend(std::fs::read(entry.path()).unwrap_or_default());
        }
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

fn with_env(mut spec: DeploySpec, vars: &[(&str, &str)]) -> DeploySpec {
    spec.env = vars
        .iter()
        .map(|(k, v)| EnvVar {
            key: (*k).into(),
            value: EnvValue::new((*v).into()),
        })
        .collect();
    spec
}

#[tokio::test(start_paused = true)]
async fn agent_state_db_has_no_secret_values() {
    const SECRETS: [&str; 3] = [
        "postgres://web:S3cretDbPassw0rd@db:5432/app",
        "sk_test_AgentNeverStoresThis",
        "hunter2-second-deploy-value",
    ];
    let bench = Bench::new();
    bench.sync_routes().await;
    let (project, web) = (ProjectId::new(), ServiceId::new());

    // Deploiement reussi : l'environnement arrive au conteneur.
    let v1 = with_env(
        spec(project, web, "ciphera.invalid/web-production:v1"),
        &[("DATABASE_URL", SECRETS[0]), ("STRIPE_KEY", SECRETS[1])],
    );
    deploy_ok(&bench, &v1).await;
    let run = bench.docker.runs.lock().unwrap().last().unwrap().clone();
    assert_eq!(
        run.env(),
        &[
            ("DATABASE_URL".to_string(), SECRETS[0].to_string()),
            ("STRIPE_KEY".to_string(), SECRETS[1].to_string()),
        ]
    );
    assert!(!format!("{run:?}").contains("S3cret"));

    // Deploiement en echec (healthcheck) : issue gardee, sans valeur.
    let v2 = with_env(
        spec(project, web, "ciphera.invalid/web-production:v2"),
        &[("DATABASE_URL", SECRETS[2])],
    );
    let (result, messages) = bench.deploy(&v2).await;
    assert!(result.is_err());
    // Cle invalide : refusee avant tout demarrage, sans la valeur.
    let v3 = with_env(
        spec(project, web, "ciphera.invalid/web-production:v3"),
        &[("BAD-KEY", SECRETS[2])],
    );
    let (refused, refused_messages) = bench.deploy(&v3).await;
    let refused = refused.unwrap_err();
    assert!(refused.contains("BAD-KEY"), "{refused}");
    assert!(!refused.contains(SECRETS[2]));
    assert!(
        !bench
            .docker
            .calls()
            .contains(&format!("run:ciphera-d-{}", v3.deployment_id))
    );

    let sent = format!("{messages:?}{refused_messages:?}");
    let stored = state_files(&bench._db_path.0);
    assert!(!stored.is_empty());
    for secret in SECRETS {
        assert!(!contains(&stored, secret), "valeur en clair dans state.db");
        assert!(!sent.contains(secret), "valeur dans un message de l'agent");
    }
    // L'etat local garde bien les deploiements (base relue).
    assert!(bench.db.record(v1.deployment_id).unwrap().is_some());
    assert!(bench.db.outcome(v2.deployment_id).unwrap().is_some());
}

#[tokio::test(start_paused = true)]
async fn compose_env_keys_on_disk_values_only_in_child_env() {
    const SECRET: &str = "sk_test_ComposeNeverOnDisk";
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    let build = DeploymentId::new();
    let mut bundle = compose_bundle();
    bundle["services"]["api"]["environment"] = json!(["API_HOST=from-file", "KEEP=1"]);
    bench
        .db
        .put_bundle(build, &built_as(&bundle, build).to_string())
        .unwrap();
    let spec = with_env(
        DeploySpec {
            deployment_id: build,
            service_id: web,
            image: ImageRef("ciphera.invalid/shop-production/web:x".into()),
            internal_port: Some(3000),
            project_id: project,
            healthcheck: Healthcheck::default(),
            compose: Some(ComposeDeploy {
                service: "web".into(),
                bundle: build,
            }),
            service_name: Some("web".into()),
            env: Vec::new(),
            environment_id: None,
            run_as: None,
            read_only_rootfs: false,
        },
        &[
            ("API_HOST", "from-ciphera"),
            ("STRIPE_KEY", SECRET),
            ("LD_PRELOAD", "/tmp/evil.so"),
        ],
    );
    for n in 100..110 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let (result, messages) = bench.deploy(&spec).await;
    result.unwrap();

    // Fichier : chaque service recoit les cles, en reference, et Ciphera
    // l'emporte sur le fichier ; aucune valeur sur le disque.
    let file = bench.compose.file.lock().unwrap().clone().unwrap();
    for svc in ["web", "api", "cache"] {
        let env = &file["services"][svc]["environment"];
        for key in ["API_HOST", "STRIPE_KEY", "LD_PRELOAD"] {
            assert_eq!(env[key], format!("${{{ENV_PREFIX}{key}}}"), "{svc}: {env}");
        }
    }
    assert_eq!(file["services"]["api"]["environment"]["KEEP"], "1");
    let raw = bench.compose.raw.lock().unwrap().clone();
    assert_eq!(raw.len(), 3);
    for text in &raw {
        for value in [SECRET, "from-ciphera", "/tmp/evil.so"] {
            assert!(!text.contains(value), "valeur sur le disque : {value}");
        }
    }

    // Processus `docker compose` : valeurs sous le prefixe seulement.
    let envs = bench.compose.envs.lock().unwrap().clone();
    assert_eq!(envs.len(), 3);
    for vars in &envs {
        let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("CIPHERA_VAR_STRIPE_KEY"), Some(SECRET));
        assert_eq!(get("CIPHERA_VAR_API_HOST"), Some("from-ciphera"));
        assert_eq!(get("CIPHERA_VAR_LD_PRELOAD"), Some("/tmp/evil.so"));
        assert_eq!(get("LD_PRELOAD"), None);
        assert_eq!(get("STRIPE_KEY"), None);
    }

    // Ni la base locale ni les messages emis ne portent les valeurs.
    let state = state_files(&bench._db_path.0);
    let sent = format!("{messages:?}");
    for value in [SECRET, "from-ciphera"] {
        assert!(!contains(&state, value));
        assert!(!sent.contains(value));
    }
}

/// Machine redemarree sans plan de controle joignable : la derniere table
/// recue est reprise du disque et renvoyee au proxy, amonts recalcules ;
/// une table gardee illisible est ignoree ; une table du plan de controle
/// deja recue n'est jamais remplacee par celle du disque.
#[tokio::test(start_paused = true)]
async fn routes_table_survives_an_agent_restart_without_the_control_plane() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let served = bench.upstreams("app.example.com");
    assert_eq!(
        served,
        vec![SocketAddr::new(bench.ip_of(v1.deployment_id), 8080)]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(bench.routes_file())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let restarted = bench.restarted();
    let sent = bench.routes.count();
    assert!(restarted.restore_routes().await, "table gardee reprise");
    assert_eq!(bench.routes.count(), sent + 1, "renvoyee au proxy");
    assert_eq!(bench.upstreams("app.example.com"), served);

    // Table du plan de controle recue ensuite : gardee a son tour, et une
    // reprise ne la remplace pas.
    restarted
        .apply_routes(vec![route("new.example.com", Some(web))])
        .await
        .unwrap();
    std::fs::write(bench.routes_file(), b"[]").unwrap();
    assert!(restarted.restore_routes().await);
    assert!(
        bench
            .routes
            .last()
            .unwrap()
            .iter()
            .any(|r| r.host == "new.example.com")
    );

    // Fichier abime : ignore, rien n'est envoye.
    std::fs::write(bench.routes_file(), b"{pas du json").unwrap();
    let broken = bench.restarted();
    let sent = bench.routes.count();
    assert!(!broken.restore_routes().await);
    assert_eq!(bench.routes.count(), sent);
}

// --- retrait d'un projet -------------------------------------------------------

/// `RemoveProject` : build en cours du projet interrompu et nettoye,
/// conteneurs du projet (services Compose compris), etat local, reseau et,
/// avec `purge_volumes`, volumes du projet supprimes ; rien d'un autre
/// projet n'est touche. Les ordres arrives ensuite pour ce projet sont
/// refuses, et le retrait se rejoue sans erreur.
#[tokio::test(start_paused = true)]
async fn remove_project_cancels_its_build_and_removes_everything_of_the_project() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let (other, other_web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![
            route("app.example.com", Some(web)),
            route("other.example.com", Some(other_web)),
        ])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let keep = spec(other, other_web, "ciphera.invalid/other-production:v1");
    deploy_ok(&bench, &keep).await;
    let label = project.to_string();
    bench.docker.add_container(
        "cid-cache",
        "cp-cache-1",
        &[(PROJECT_LABEL, label.as_str()), (DEPLOYMENT_LABEL, "")],
        None,
    );
    let env = EnvironmentId::new();
    bench
        .docker
        .add_volume(&compose_project_name(project, env), "data");
    bench
        .docker
        .add_volume(&compose_project_name(other, env), "data");
    let build = DeploymentId::new();
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_build(
        BuildSpec {
            project_id: Some(project),
            environment_id: Some(env),
            ..image_build(build)
        },
        build_cfg(&bench),
        slow_pull(Duration::from_secs(3600)),
        tx.clone(),
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(bench.deployer.ops().in_progress(), vec![build]);

    bench.deployer.remove_project(project, true).await.unwrap();

    assert!(
        bench.deployer.ops().in_progress().is_empty(),
        "build annule"
    );
    assert!(bench.docker.calls().contains(&format!("rmi:{build}")));
    assert!(
        bench
            .docker
            .container(&format!("id-ciphera-d-{}", v1.deployment_id))
            .is_none()
    );
    assert!(bench.docker.container("cid-cache").is_none());
    assert!(
        bench
            .docker
            .container(&format!("id-ciphera-d-{}", keep.deployment_id))
            .is_some()
    );
    assert!(!bench.docker.has_network(&project_network(project)));
    assert!(bench.docker.has_network(&project_network(other)));
    assert_eq!(
        bench.docker.volume_names(),
        vec![format!("{}_data", compose_project_name(other, env))]
    );
    assert!(bench.db.record(v1.deployment_id).unwrap().is_none());
    assert!(bench.db.record(keep.deployment_id).unwrap().is_some());
    assert!(bench.upstreams("app.example.com").is_empty());
    assert_eq!(bench.upstreams("other.example.com").len(), 1);

    // Ordres arrives apres le retrait : refuses, rien n'est lance.
    drain(&mut rx).await;
    let late_build = DeploymentId::new();
    bench.deployer.spawn_build(
        BuildSpec {
            project_id: Some(project),
            environment_id: Some(env),
            ..image_build(late_build)
        },
        build_cfg(&bench),
        slow_pull(Duration::from_secs(1)),
        tx.clone(),
    );
    let late_deploy = spec(project, web, "ciphera.invalid/web-production:v2");
    let runs = bench.docker.runs.lock().unwrap().len();
    bench.deployer.spawn_deploy(late_deploy.clone(), tx.clone());
    tokio::time::sleep(Duration::from_secs(1)).await;
    let refused: Vec<_> = drain(&mut rx)
        .await
        .into_iter()
        .filter_map(|m| match m {
            AgentToControl::DeployProgress {
                deployment_id,
                status: StageStatus::Failed { message },
                ..
            } => Some((deployment_id, message)),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 2, "{refused:?}");
    for id in [late_build, late_deploy.deployment_id] {
        let (_, message) = refused.iter().find(|(d, _)| *d == id).unwrap();
        assert!(message.contains("projet supprime"), "{message}");
    }
    assert!(bench.deployer.ops().in_progress().is_empty());
    assert_eq!(bench.docker.runs.lock().unwrap().len(), runs);

    // Rejoue (accuse perdu) : rien a faire, sans erreur.
    bench.deployer.remove_project(project, true).await.unwrap();
}

/// Sans `purge_volumes`, les volumes du projet restent.
#[tokio::test(start_paused = true)]
async fn remove_project_keeps_volumes_unless_purged() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let project = ProjectId::new();
    let env = EnvironmentId::new();
    deploy_ok(
        &bench,
        &spec(
            project,
            ServiceId::new(),
            "ciphera.invalid/web-production:v1",
        ),
    )
    .await;
    bench
        .docker
        .add_volume(&compose_project_name(project, env), "data");
    bench.deployer.remove_project(project, false).await.unwrap();
    assert_eq!(bench.docker.volume_names().len(), 1);
    assert!(!bench.docker.has_network(&project_network(project)));
    assert!(bench.db.deployments().unwrap().is_empty());
}

/// Purge : les volumes du projet sont reconnus a leur nom
/// (`cp-<projet>-<env>_<cle>`), y compris ceux que Compose a etiquetes avec
/// le projet du service expose (`ciphera-d-<deploiement>`) ; les volumes
/// anonymes partent avec les conteneurs. Rien d'un autre projet, ni un
/// volume hors Compose, n'est touche.
#[tokio::test(start_paused = true)]
async fn remove_project_purge_removes_volumes_of_the_exposed_service_and_anonymous_ones() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let (project, other) = (ProjectId::new(), ProjectId::new());
    let env = EnvironmentId::new();
    let v1 = spec(
        project,
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    deploy_ok(&bench, &v1).await;
    let stable = compose_project_name(project, env);
    let other_stable = compose_project_name(other, env);
    let exposed = format!("ciphera-d-{}", v1.deployment_id);
    // Volume monte par le seul service expose : etiquete `ciphera-d-<id>`.
    bench
        .docker
        .add_volume_created_by(&format!("{stable}_uploads"), &exposed);
    bench.docker.add_volume(&stable, "data");
    // Autre projet, meme si cree sous un projet `ciphera-d-` : garde.
    bench.docker.add_volume_created_by(
        &format!("{other_stable}_uploads"),
        &format!("ciphera-d-{}", DeploymentId::new()),
    );
    // Nom approchant sans la forme `<env 8 hexa>_<cle>` : garde.
    bench
        .docker
        .add_volume_created_by(&format!("{stable}x_data"), &stable);

    bench.deployer.remove_project(project, true).await.unwrap();

    let mut kept = vec![format!("{other_stable}_uploads"), format!("{stable}x_data")];
    kept.sort();
    assert_eq!(bench.docker.volume_names(), kept);
    let calls = bench.docker.calls();
    let container = format!("id-ciphera-d-{}", v1.deployment_id);
    assert!(
        calls.contains(&format!("remove-v:{container}")),
        "{calls:?}"
    );
    assert!(!calls.contains(&format!("remove:{container}")));
}

/// Echec de Docker pendant le retrait (conteneur, volumes, reseau) : erreur
/// (pas d'accuse, l'ordre revient a la reconnexion) et l'etat local du
/// projet reste ; l'ordre suivant termine le retrait.
#[tokio::test(start_paused = true)]
async fn remove_project_keeps_local_state_when_docker_fails() {
    use std::sync::atomic::Ordering;
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("app.example.com", Some(web))])
        .await
        .unwrap();
    let v1 = spec(project, web, "ciphera.invalid/web-production:v1");
    deploy_ok(&bench, &v1).await;
    let env = EnvironmentId::new();
    bench
        .docker
        .add_volume(&compose_project_name(project, env), "data");

    for flag in [
        &bench.docker.fail_remove,
        &bench.docker.fail_volumes,
        &bench.docker.fail_network,
    ] {
        flag.store(true, Ordering::SeqCst);
        assert!(bench.deployer.remove_project(project, true).await.is_err());
        flag.store(false, Ordering::SeqCst);
        assert!(
            bench.db.record(v1.deployment_id).unwrap().is_some(),
            "etat local garde"
        );
        assert!(
            !bench
                .docker
                .calls()
                .contains(&format!("rmi:{}", v1.deployment_id))
        );
    }
    bench.deployer.remove_project(project, true).await.unwrap();
    assert!(bench.db.record(v1.deployment_id).unwrap().is_none());
    assert!(bench.upstreams("app.example.com").is_empty());
    assert!(bench.docker.volume_names().is_empty());
    assert!(!bench.docker.has_network(&project_network(project)));
}

/// Sans purge, les conteneurs partent sans leurs volumes anonymes.
#[tokio::test(start_paused = true)]
async fn remove_project_without_purge_keeps_anonymous_volumes() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let project = ProjectId::new();
    let v1 = spec(
        project,
        ServiceId::new(),
        "ciphera.invalid/web-production:v1",
    );
    deploy_ok(&bench, &v1).await;
    bench.deployer.remove_project(project, false).await.unwrap();
    let calls = bench.docker.calls();
    let container = format!("id-ciphera-d-{}", v1.deployment_id);
    assert!(calls.contains(&format!("remove:{container}")), "{calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("remove-v:")));
}

/// Premier deploiement Compose d'un service en echec : ses services non
/// exposes sont retires avec lui. Une fois un deploiement servi, un echec
/// ne touche plus a ses services.
#[tokio::test(start_paused = true)]
async fn failed_first_compose_deploy_removes_its_unexposed_services() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    let label = project.to_string();
    let of_project = |bench: &Bench| -> Vec<String> {
        let mut names: Vec<String> = bench
            .docker
            .containers
            .lock()
            .unwrap()
            .values()
            .filter(|c| c.labels.get(PROJECT_LABEL) == Some(&label))
            .map(|c| c.name.clone())
            .collect();
        names.sort();
        names
    };

    // Premier deploiement : le service expose ne repond jamais.
    let first = DeploymentId::new();
    bench
        .db
        .put_bundle(first, &built_as(&compose_bundle(), first).to_string())
        .unwrap();
    let (result, _) = bench
        .deploy(&compose_deploy_spec(project, web, first))
        .await;
    assert!(result.is_err());
    assert_eq!(bench.compose.ups.lock().unwrap().len(), 3, "tout a demarre");
    assert!(of_project(&bench).is_empty(), "{:?}", of_project(&bench));

    // Deploiement servi, puis un echec : les services restent.
    for n in 100..120 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let second = DeploymentId::new();
    bench
        .db
        .put_bundle(second, &built_as(&compose_bundle(), second).to_string())
        .unwrap();
    bench
        .deploy(&compose_deploy_spec(project, web, second))
        .await
        .0
        .unwrap();
    *bench.probe.fail_first.lock().unwrap() = 1000;
    let third = DeploymentId::new();
    bench
        .db
        .put_bundle(third, &built_as(&compose_bundle(), third).to_string())
        .unwrap();
    assert!(
        bench
            .deploy(&compose_deploy_spec(project, web, third))
            .await
            .0
            .is_err()
    );
    let left = of_project(&bench);
    for service in ["shop-production-api-1", "shop-production-cache-1"] {
        assert!(left.iter().any(|n| n == service), "{left:?}");
    }
    assert!(
        bench
            .docker
            .container("cid-shop-production-cache-1")
            .unwrap()
            .running
    );
}

/// Ensemble Compose a quatre services : `web` (expose) depend de `api`, qui
/// depend de `cache` ; `worker` (sans lien avec `web`) depend de `cache`.
/// Avec `mailer`, un service de plus, sans dependance.
fn worker_bundle(mailer: bool) -> Value {
    let mut bundle = compose_bundle();
    let services = bundle["services"].as_object_mut().unwrap();
    services.insert(
        "worker".into(),
        json!({
            "image": format!("ciphera.invalid/shop-production/worker:{}", bundle_id()),
            "depends_on": {"cache": {"condition": "service_started", "required": true}},
            "networks": {"default": null}
        }),
    );
    if mailer {
        services.insert(
            "mailer".into(),
            json!({
                "image": "docker.io/library/busybox:1.37",
                "networks": {"default": null}
            }),
        );
    }
    bundle
}

/// Deploiement Compose de `bundle` sous un nouvel identifiant ; renvoie
/// sa spec.
fn compose_spec_with(
    bench: &Bench,
    project: ProjectId,
    web: ServiceId,
    bundle: &Value,
) -> DeploySpec {
    let id = DeploymentId::new();
    bench
        .db
        .put_bundle(id, &built_as(bundle, id).to_string())
        .unwrap();
    compose_deploy_spec(project, web, id)
}

fn log_lines(messages: &[AgentToControl]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|m| match m {
            AgentToControl::LogLine(l) => Some(l.line.clone()),
            _ => None,
        })
        .collect()
}

/// Appels `up` depuis le rang `from` (services du deploiement suivant).
fn ups_since(bench: &Bench, from: usize) -> Vec<(String, String)> {
    bench.compose.ups.lock().unwrap()[from..].to_vec()
}

/// Projet Compose deja servi (premier deploiement sain), puis un
/// deploiement dont la sonde echoue : seuls les services nouveaux et les
/// dependances du service expose sont lances avant la sonde ; `worker`
/// n'est jamais touche, `mailer` (nouveau) est retire, et le journal nomme
/// les services deja en place qui ont ete mis a jour.
#[tokio::test(start_paused = true)]
async fn compose_failed_probe_leaves_unrelated_services_unchanged() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..110 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    let worker = "cid-shop-production-worker-1";
    assert!(bench.docker.container(worker).is_some());
    let served = bench.upstreams("shop.example.com");

    *bench.probe.fail_first.lock().unwrap() = 1000;
    let from = bench.compose.ups.lock().unwrap().len();
    let second = compose_spec_with(&bench, project, web, &worker_bundle(true));
    let (result, messages) = bench.deploy(&second).await;
    assert!(result.is_err());
    let stable = "shop-production".to_string();
    assert_eq!(
        ups_since(&bench, from),
        vec![
            (stable.clone(), "cache".to_string()),
            (stable.clone(), "mailer".to_string()),
            (stable.clone(), "api".to_string()),
            (
                format!("ciphera-d-{}", second.deployment_id),
                "web".to_string()
            ),
        ]
    );
    assert!(bench.docker.container(worker).is_some());
    assert!(
        bench
            .docker
            .container("cid-shop-production-mailer-1")
            .is_none(),
        "service nouveau retire"
    );
    assert!(
        bench
            .docker
            .container("cid-shop-production-api-1")
            .is_some()
    );
    assert_eq!(bench.upstreams("shop.example.com"), served);
    let logs = log_lines(&messages);
    assert!(
        logs.iter()
            .any(|l| l.contains(RESTORED) && l.contains("api") && l.contains("cache")),
        "{logs:?}"
    );
}

/// Sonde reussie : les autres services sont mis a jour apres la sonde,
/// avant la bascule.
#[tokio::test(start_paused = true)]
async fn compose_other_services_are_updated_after_the_probe() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..120 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    let from = bench.compose.ups.lock().unwrap().len();
    let second = compose_spec_with(&bench, project, web, &worker_bundle(false));
    let (result, messages) = bench.deploy(&second).await;
    result.unwrap();
    let stable = "shop-production".to_string();
    assert_eq!(
        ups_since(&bench, from),
        vec![
            (stable.clone(), "cache".to_string()),
            (stable.clone(), "api".to_string()),
            (
                format!("ciphera-d-{}", second.deployment_id),
                "web".to_string()
            ),
            (stable.clone(), "worker".to_string()),
        ]
    );
    // Mise a jour de `worker` entre la sonde et la bascule.
    let st = stages(&messages);
    assert_eq!(st.last(), Some(&(Stage::Switch, StageStatus::Succeeded)));
    assert!(
        bench.db.compose_updated().unwrap().is_empty(),
        "notes oubliees par la bascule"
    );
}

/// Annulation pendant la sonde d'un deploiement Compose : les services
/// nouveaux sont retires, ceux deja en place restent.
#[tokio::test(start_paused = true)]
async fn compose_cancel_during_probe_removes_new_services() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..105 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    for n in 105..120 {
        bench
            .probe
            .hang
            .lock()
            .unwrap()
            .insert(format!("172.18.1.{n}").parse().unwrap());
    }
    let tries = bench.probe.tries.lock().unwrap().len();
    let mut second = compose_spec_with(&bench, project, web, &worker_bundle(true));
    second.healthcheck.interval_secs = 1;
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(second.clone(), tx.clone());
    wait_until("sonde en cours", || {
        bench.probe.tries.lock().unwrap().len() > tries
    })
    .await;
    assert!(
        bench
            .docker
            .container("cid-shop-production-mailer-1")
            .is_some()
    );

    bench.deployer.stop(second.deployment_id, &tx).await;

    assert!(
        bench
            .docker
            .container("cid-shop-production-mailer-1")
            .is_none(),
        "service nouveau retire"
    );
    for kept in ["worker", "api", "cache"] {
        assert!(
            bench
                .docker
                .container(&format!("cid-shop-production-{kept}-1"))
                .is_some(),
            "{kept}"
        );
    }
    let messages = drain(&mut rx).await;
    assert_eq!(
        stages(&messages).last(),
        Some(&(Stage::Start, StageStatus::Cancelled))
    );
    let logs = log_lines(&messages);
    assert!(
        logs.iter()
            .any(|l| l.contains(RESTORED) && l.contains("api")),
        "{logs:?}"
    );
}

/// Projet Compose servi (`worker_bundle(false)`), adresses des sondes
/// toutes valides.
async fn served_worker_stack(bench: &Bench, project: ProjectId, web: ServiceId) {
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..160 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
}

const MAILER: &str = "cid-shop-production-mailer-1";

/// Echec de Docker au retrait d'un service nouveau (`mailer`) : la note
/// reste ; un deploiement suivant qui sert `mailer` l'adopte. Ni le
/// demarrage suivant ni le retrait du deploiement en echec ne le retirent.
#[tokio::test(start_paused = true)]
async fn compose_service_adopted_after_a_failed_removal_is_kept() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    served_worker_stack(&bench, project, web).await;

    // D1 : sonde en echec, retrait de `mailer` refuse par Docker.
    *bench.probe.fail_first.lock().unwrap() = 5;
    bench.docker.fail_remove.store(true, Ordering::SeqCst);
    let d1 = compose_spec_with(&bench, project, web, &worker_bundle(true));
    assert!(bench.deploy(&d1).await.0.is_err());
    bench.docker.fail_remove.store(false, Ordering::SeqCst);
    assert!(bench.docker.container(MAILER).is_some());
    assert_eq!(bench.db.compose_created().unwrap().len(), 1, "note gardee");

    // D2 : `mailer` deja en place, servi par D2.
    let d2 = compose_spec_with(&bench, project, web, &worker_bundle(true));
    bench.deploy(&d2).await.0.unwrap();
    assert!(bench.db.compose_created().unwrap().is_empty());

    bench.restarted().sweep().await;
    assert!(bench.docker.container(MAILER).is_some(), "demarrage");
    assert!(bench.deployer.remove(d1.deployment_id).await);
    assert!(bench.docker.container(MAILER).is_some(), "RemoveDeployment");
}

/// Note restee d'un deploiement servi puis remplace (effacement perdu),
/// et note d'un deploiement inconnu sur un service servi : la bascule
/// suivante oublie les notes de son ensemble, et la garde finale ne retire
/// jamais un service de l'ensemble servi.
#[tokio::test(start_paused = true)]
async fn stale_note_never_removes_a_serving_service() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    served_worker_stack(&bench, project, web).await;
    let d1 = compose_spec_with(&bench, project, web, &worker_bundle(true));
    bench.deploy(&d1).await.0.unwrap();
    // Effacement perdu apres la bascule de D1.
    bench
        .db
        .put_compose_created(d1.deployment_id, project, "shop-production", "mailer")
        .unwrap();
    let d2 = compose_spec_with(&bench, project, web, &worker_bundle(true));
    bench.deploy(&d2).await.0.unwrap();
    assert!(bench.db.compose_created().unwrap().is_empty());

    // Note sans bascule pour l'oublier : la garde finale tient.
    let stray = DeploymentId::new();
    bench
        .db
        .put_compose_created(stray, project, "shop-production", "mailer")
        .unwrap();
    bench.restarted().sweep().await;
    assert!(bench.docker.container(MAILER).is_some());
    assert!(bench.db.compose_created().unwrap().is_empty());
}

/// Note restee dans l'etat local seulement (apres un redemarrage) :
/// `RemoveDeployment` la traite ; tant que Docker refuse le retrait, il ne
/// s'accuse pas.
#[tokio::test(start_paused = true)]
async fn remove_deployment_handles_persisted_notes_before_acking() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    served_worker_stack(&bench, project, web).await;
    let d1 = DeploymentId::new();
    let label = project.to_string();
    bench.docker.add_container(
        "ghost",
        "shop-production-ghost-1",
        &[
            (COMPOSE_LABEL, "shop-production"),
            (COMPOSE_PROJECT_LABEL, "shop-production"),
            (COMPOSE_SERVICE_LABEL, "ghost"),
            (PROJECT_LABEL, label.as_str()),
        ],
        None,
    );
    bench
        .db
        .put_compose_created(d1, project, "shop-production", "ghost")
        .unwrap();

    bench.docker.fail_remove.store(true, Ordering::SeqCst);
    assert!(!bench.deployer.remove(d1).await, "pas d'accuse");
    assert_eq!(bench.db.compose_created().unwrap().len(), 1);
    bench.docker.fail_remove.store(false, Ordering::SeqCst);
    assert!(bench.deployer.remove(d1).await);
    assert!(bench.docker.container("ghost").is_none());
    assert!(bench.db.compose_created().unwrap().is_empty());
}

/// Agent arrete pendant la sonde d'un deploiement Compose : au demarrage
/// suivant, le service nouveau qu'il avait lance (en marche) est retire ;
/// les services du deploiement servi restent.
#[tokio::test(start_paused = true)]
async fn compose_new_services_of_an_interrupted_deploy_removed_at_startup() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..105 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    for n in 105..120 {
        bench
            .probe
            .hang
            .lock()
            .unwrap()
            .insert(format!("172.18.1.{n}").parse().unwrap());
    }
    let tries = bench.probe.tries.lock().unwrap().len();
    let mut second = compose_spec_with(&bench, project, web, &worker_bundle(true));
    second.healthcheck.interval_secs = 1;
    let (tx, _rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(second.clone(), tx);
    wait_until("sonde en cours", || {
        bench.probe.tries.lock().unwrap().len() > tries
    })
    .await;
    let mailer = "cid-shop-production-mailer-1";
    assert!(bench.docker.container(mailer).unwrap().running);

    // Redemarrage : nouvel agent, meme etat local, meme Docker.
    bench.restarted().sweep().await;

    assert!(
        bench.docker.container(mailer).is_none(),
        "service nouveau retire"
    );
    for kept in ["worker", "api", "cache"] {
        assert!(
            bench
                .docker
                .container(&format!("cid-shop-production-{kept}-1"))
                .is_some(),
            "{kept}"
        );
    }
    assert!(bench.db.compose_created().unwrap().is_empty());
}

/// Ligne de journal des services dependants restaures.
const RESTORED: &str = "services dépendants restaurés";
/// Issue d'un deploiement dont les services dependants n'ont pas pu etre
/// restaures (agent redemarre en cours de deploiement).
const NOT_RESTORED: &str = "services dépendants non restaurés : redéployez la version servie";

/// Conteneurs nommes `name` : identifiant et conteneur.
fn named(bench: &Bench, name: &str) -> Vec<(String, common::FakeContainer)> {
    bench
        .docker
        .containers
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, c)| c.name == name)
        .map(|(id, c)| (id.clone(), c.clone()))
        .collect()
}

/// Ensemble `worker_bundle(false)` du deploiement suivant : la dependance
/// `api` change d'image (chaque build etiquette ses images de son propre
/// identifiant, voir `built_as`).
fn bundle_with_new_api() -> Value {
    worker_bundle(false)
}

/// Projet Compose servi, Compose recreant chaque service relance ; renvoie
/// l'ensemble suivant (`api` change d'image).
async fn recreating_stack(bench: &Bench, project: ProjectId, web: ServiceId) -> Value {
    bench.compose.recreate.store(true, Ordering::SeqCst);
    served_worker_stack(bench, project, web).await;
    bundle_with_new_api()
}

/// Conteneurs servis des dependances du service expose (`api`, `cache`).
fn dependencies_before(bench: &Bench) -> Vec<(String, common::FakeContainer)> {
    let mut out = named(bench, "shop-production-api-1");
    out.extend(named(bench, "shop-production-cache-1"));
    assert_eq!(out.len(), 2);
    assert!(
        out[0]
            .1
            .image
            .starts_with("ciphera.invalid/shop-production/api:"),
        "{}",
        out[0].1.image
    );
    out
}

/// Chaque service de `before` a de nouveau un seul conteneur, recree
/// (jamais celui du deploiement en echec), avec la meme image, les memes
/// etiquettes, en marche.
fn assert_restored(bench: &Bench, before: &[(String, common::FakeContainer)]) {
    for (old_id, old) in before {
        let now = named(bench, &old.name);
        assert_eq!(now.len(), 1, "{}: {now:?}", old.name);
        let (id, c) = &now[0];
        assert!(id.starts_with("restored-"), "{} : {id}", old.name);
        assert_ne!(id, old_id);
        assert_eq!(c.image, old.image, "{}", old.name);
        assert_eq!(c.labels, old.labels, "{}", old.name);
        assert!(c.running, "{}", old.name);
    }
}

/// Sonde en echec : les dependances du service expose, deja en place et
/// mises a jour avant la sonde, sont recreees telles qu'elles etaient ;
/// `worker` n'est pas touche ; le journal le dit.
#[tokio::test(start_paused = true)]
async fn compose_failed_probe_restores_updated_dependency_services() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let bundle = recreating_stack(&bench, project, web).await;
    let before = dependencies_before(&bench);
    let worker = named(&bench, "shop-production-worker-1");

    *bench.probe.fail_first.lock().unwrap() = 1000;
    let second = compose_spec_with(&bench, project, web, &bundle);
    let (result, messages) = bench.deploy(&second).await;
    assert!(result.is_err());

    assert_restored(&bench, &before);
    assert_eq!(named(&bench, "shop-production-worker-1")[0].0, worker[0].0);
    let logs = log_lines(&messages);
    assert!(
        logs.iter()
            .any(|l| l.contains(RESTORED) && l.contains("api") && l.contains("cache")),
        "{logs:?}"
    );
    assert!(bench.db.compose_updated().unwrap().is_empty());
}

/// Annulation pendant la sonde : memes services restaures.
#[tokio::test(start_paused = true)]
async fn compose_cancel_restores_updated_dependency_services() {
    let bench = Bench::new();
    bench.compose.recreate.store(true, Ordering::SeqCst);
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..105 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    for n in 105..120 {
        bench
            .probe
            .hang
            .lock()
            .unwrap()
            .insert(format!("172.18.1.{n}").parse().unwrap());
    }
    let before = dependencies_before(&bench);
    let tries = bench.probe.tries.lock().unwrap().len();
    let mut second = compose_spec_with(&bench, project, web, &bundle_with_new_api());
    second.healthcheck.interval_secs = 1;
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(second.clone(), tx.clone());
    wait_until("sonde en cours", || {
        bench.probe.tries.lock().unwrap().len() > tries
    })
    .await;
    assert_eq!(
        named(&bench, "shop-production-api-1")[0].1.image,
        format!(
            "ciphera.invalid/shop-production/api:{}",
            second.deployment_id
        )
    );

    bench.deployer.stop(second.deployment_id, &tx).await;

    assert_restored(&bench, &before);
    let messages = drain(&mut rx).await;
    assert_eq!(
        stages(&messages).last(),
        Some(&(Stage::Start, StageStatus::Cancelled))
    );
    let logs = log_lines(&messages);
    assert!(logs.iter().any(|l| l.contains(RESTORED)), "{logs:?}");
    assert!(bench.db.compose_updated().unwrap().is_empty());
}

/// Docker refuse la recreation : la restauration reste a faire (note
/// gardee) ; `RemoveDeployment` la reprend et ne s'accuse qu'une fois faite.
#[tokio::test(start_paused = true)]
async fn compose_restore_refused_by_docker_is_retried_before_acking() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let bundle = recreating_stack(&bench, project, web).await;
    let before = dependencies_before(&bench);

    *bench.probe.fail_first.lock().unwrap() = 1000;
    bench.docker.fail_recreate.store(true, Ordering::SeqCst);
    let second = compose_spec_with(&bench, project, web, &bundle);
    let (result, messages) = bench.deploy(&second).await;
    assert!(result.is_err());
    assert_eq!(
        bench.db.compose_updated().unwrap().len(),
        2,
        "notes gardees"
    );
    // Restauration a mi-chemin : l'issue le dit.
    match stages(&messages).last() {
        Some((Stage::Healthcheck, StageStatus::Failed { message })) => {
            assert!(message.contains(NOT_RESTORED), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let stored = bench.db.outcome(second.deployment_id).unwrap().unwrap();
    assert!(
        matches!(&stored.status, StageStatus::Failed { message } if message.contains(NOT_RESTORED)),
        "{stored:?}"
    );
    assert!(
        !bench.deployer.remove(second.deployment_id).await,
        "pas d'accuse"
    );

    bench.docker.fail_recreate.store(false, Ordering::SeqCst);
    assert!(bench.deployer.remove(second.deployment_id).await);
    assert_restored(&bench, &before);
    assert!(bench.db.compose_updated().unwrap().is_empty());
}

/// Restauration refusee par Docker, puis redemarrage de l'agent avant la
/// reprise : l'issue deja gardee (echec de la sonde) est remplacee par
/// `NOT_RESTORED` avant que les notes soient oubliees.
#[tokio::test(start_paused = true)]
async fn compose_restart_after_a_failed_restore_still_reports_it() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let bundle = recreating_stack(&bench, project, web).await;

    *bench.probe.fail_first.lock().unwrap() = 1000;
    bench.docker.fail_recreate.store(true, Ordering::SeqCst);
    let second = compose_spec_with(&bench, project, web, &bundle);
    assert!(bench.deploy(&second).await.0.is_err());
    // Une issue anterieure differente : annulation gardee.
    bench
        .db
        .put_outcome_forced(second.deployment_id, Stage::Start, &StageStatus::Cancelled)
        .unwrap();

    bench.restarted().sweep().await;

    let outcome = bench.db.outcome(second.deployment_id).unwrap().unwrap();
    assert_eq!(
        outcome.status,
        StageStatus::Failed {
            message: NOT_RESTORED.into()
        }
    );
    assert!(bench.db.compose_updated().unwrap().is_empty());
}

/// Annulation dont la restauration est refusee par Docker : l'issue est un
/// echec qui le dit, pas une simple annulation.
#[tokio::test(start_paused = true)]
async fn compose_cancel_with_a_failed_restore_reports_it() {
    let bench = Bench::new();
    bench.compose.recreate.store(true, Ordering::SeqCst);
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..105 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    for n in 105..120 {
        bench
            .probe
            .hang
            .lock()
            .unwrap()
            .insert(format!("172.18.1.{n}").parse().unwrap());
    }
    let tries = bench.probe.tries.lock().unwrap().len();
    let mut second = compose_spec_with(&bench, project, web, &bundle_with_new_api());
    second.healthcheck.interval_secs = 1;
    let (tx, mut rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(second.clone(), tx.clone());
    wait_until("sonde en cours", || {
        bench.probe.tries.lock().unwrap().len() > tries
    })
    .await;
    bench.docker.fail_recreate.store(true, Ordering::SeqCst);

    bench.deployer.stop(second.deployment_id, &tx).await;

    let messages = drain(&mut rx).await;
    match stages(&messages).last() {
        Some((Stage::Start, StageStatus::Failed { message })) => {
            assert!(message.contains(NOT_RESTORED), "{message}");
        }
        other => panic!("{other:?}"),
    }
    let stored = bench.db.outcome(second.deployment_id).unwrap().unwrap();
    assert!(
        matches!(&stored.status, StageStatus::Failed { message } if message.contains(NOT_RESTORED)),
        "{stored:?}"
    );
}

/// A en echec, restauration refusee par Docker ; puis `RemoveDeployment(A)`
/// pendant le deploiement B du meme projet Compose : la reprise de A ne
/// touche jamais les services de B (verrou du projet, garde de l'ensemble
/// servi). B bascule avec ses propres conteneurs.
#[tokio::test(start_paused = true)]
async fn compose_restore_resumed_during_a_later_deploy_never_touches_it() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let bundle = recreating_stack(&bench, project, web).await;

    *bench.probe.fail_first.lock().unwrap() = 1000;
    bench.docker.fail_recreate.store(true, Ordering::SeqCst);
    let a = compose_spec_with(&bench, project, web, &bundle);
    assert!(bench.deploy(&a).await.0.is_err());
    *bench.probe.fail_first.lock().unwrap() = 0;
    bench.docker.fail_recreate.store(false, Ordering::SeqCst);
    *bench.docker.recreate_delay.lock().unwrap() = Duration::from_secs(5);

    // Reprise de A en cours (recreation lente de `cache`)...
    let deployer = bench.deployer.clone();
    let a_id = a.deployment_id;
    let resumed = tokio::spawn(async move { deployer.remove(a_id).await });
    wait_until("restauration de A en cours", || {
        bench
            .docker
            .calls()
            .iter()
            .any(|c| c == "recreate:shop-production-cache-1")
    })
    .await;
    // ... pendant que B se deploie.
    let b = compose_spec_with(&bench, project, web, &worker_bundle(false));
    let (result, messages) = bench.deploy(&b).await;
    result.unwrap_or_else(|e| panic!("{e} {:?}", stages(&messages)));
    assert!(resumed.await.unwrap(), "A retire");

    assert!(
        bench
            .db
            .record(b.deployment_id)
            .unwrap()
            .unwrap()
            .deployment
            .serving
    );
    for service in ["shop-production-cache-1", "shop-production-api-1"] {
        let now = named(&bench, service);
        assert_eq!(now.len(), 1, "{service}: {now:?}");
        assert!(
            now[0].0.starts_with("cid-"),
            "{service} : conteneur de B attendu, {}",
            now[0].0
        );
        assert!(now[0].1.running, "{service}");
    }
    assert!(bench.db.compose_updated().unwrap().is_empty());
}

/// A en echec, restauration refusee par Docker ; B bascule ensuite ;
/// `RemoveDeployment(A)` arrive apres : les services de B restent.
#[tokio::test(start_paused = true)]
async fn compose_restore_resumed_after_a_later_switch_never_touches_it() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    let bundle = recreating_stack(&bench, project, web).await;

    *bench.probe.fail_first.lock().unwrap() = 1000;
    bench.docker.fail_recreate.store(true, Ordering::SeqCst);
    let a = compose_spec_with(&bench, project, web, &bundle);
    assert!(bench.deploy(&a).await.0.is_err());
    *bench.probe.fail_first.lock().unwrap() = 0;
    bench.docker.fail_recreate.store(false, Ordering::SeqCst);

    let b = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&b).await.0.unwrap();
    let kept: Vec<(String, common::FakeContainer)> =
        ["shop-production-cache-1", "shop-production-api-1"]
            .iter()
            .flat_map(|n| named(&bench, n))
            .collect();
    assert_eq!(kept.len(), 2);

    assert!(bench.deployer.remove(a.deployment_id).await);
    for (id, c) in &kept {
        let now = named(&bench, &c.name);
        assert_eq!(now.len(), 1);
        assert_eq!(&now[0].0, id, "{} : conteneur de B garde", c.name);
    }
    assert!(bench.db.compose_updated().unwrap().is_empty());
}

/// Agent arrete pendant la sonde, dependances deja mises a jour : leur
/// configuration d'avant est perdue ; au demarrage suivant, le deploiement
/// est mis en echec avec le message prevu (rejoue tel quel), sans jamais
/// etre relance.
#[tokio::test(start_paused = true)]
async fn compose_interrupted_deploy_with_updated_services_fails_at_startup() {
    let bench = Bench::new();
    let (project, web) = (ProjectId::new(), ServiceId::new());
    bench
        .deployer
        .apply_routes(vec![route("shop.example.com", Some(web))])
        .await
        .unwrap();
    for n in 100..105 {
        bench.probe.answer(format!("172.18.1.{n}").parse().unwrap());
    }
    let first = compose_spec_with(&bench, project, web, &worker_bundle(false));
    bench.deploy(&first).await.0.unwrap();
    for n in 105..120 {
        bench
            .probe
            .hang
            .lock()
            .unwrap()
            .insert(format!("172.18.1.{n}").parse().unwrap());
    }
    let tries = bench.probe.tries.lock().unwrap().len();
    let mut second = compose_spec_with(&bench, project, web, &worker_bundle(false));
    second.healthcheck.interval_secs = 1;
    let (tx, _rx) = mpsc::channel(1024);
    bench.deployer.spawn_deploy(second.clone(), tx);
    wait_until("sonde en cours", || {
        bench.probe.tries.lock().unwrap().len() > tries
    })
    .await;
    assert_eq!(bench.db.compose_updated().unwrap().len(), 2);

    let restarted = bench.restarted();
    restarted.sweep().await;

    let outcome = bench.db.outcome(second.deployment_id).unwrap().unwrap();
    assert_eq!(outcome.stage, Stage::Start);
    assert_eq!(
        outcome.status,
        StageStatus::Failed {
            message: NOT_RESTORED.into()
        }
    );
    assert!(bench.db.compose_updated().unwrap().is_empty());

    let ups = bench.compose.ups.lock().unwrap().len();
    let (tx, mut rx) = mpsc::channel(1024);
    let sink = ProgressSink::new(second.deployment_id, tx);
    restarted.run(&second, &sink).await.unwrap();
    drop(sink);
    let messages = drain(&mut rx).await;
    assert_eq!(
        stages(&messages),
        vec![(
            Stage::Start,
            StageStatus::Failed {
                message: NOT_RESTORED.into()
            }
        )]
    );
    assert_eq!(bench.compose.ups.lock().unwrap().len(), ups, "rien relance");
}

/// Balayage : reseau de projet vide d'un projet inconnu supprime ; ceux d'un
/// projet deploye, d'un projet qui a encore un conteneur, et les autres
/// reseaux restent.
#[tokio::test(start_paused = true)]
async fn empty_project_networks_of_unknown_projects_are_swept() {
    let bench = Bench::new();
    bench.sync_routes().await;
    let live = ProjectId::new();
    deploy_ok(
        &bench,
        &spec(live, ServiceId::new(), "ciphera.invalid/web-production:v1"),
    )
    .await;
    let gone = ProjectId::new();
    bench.docker.add_network(&project_network(gone));
    let busy = ProjectId::new();
    bench.docker.add_network(&project_network(busy));
    let busy_label = busy.to_string();
    bench.docker.add_container(
        "cid-busy",
        "busy-1",
        &[(PROJECT_LABEL, busy_label.as_str())],
        None,
    );
    bench.docker.add_network("ciphera-internal");

    assert_eq!(bench.deployer.sweep_networks().await, 1);
    assert!(!bench.docker.has_network(&project_network(gone)));
    for kept in [
        project_network(live),
        project_network(busy),
        "ciphera-internal".to_string(),
    ] {
        assert!(bench.docker.has_network(&kept), "{kept}");
    }
}
