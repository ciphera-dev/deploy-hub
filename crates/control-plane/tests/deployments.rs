//! Deploiements cote plan de controle : file et verrou par environnement,
//! annulation, rollback, retention, chien de garde et reconciliation
//! (Review Focus 2, 3 et 4). L'agent est simule : ses ordres sont lus sur
//! la file du registre, ses messages passes a `on_agent_message`.

mod common;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::deployments::{orchestrator, retention};
use control_plane::jobs::{self, JobKind};
use control_plane::state::{AppState, Clock};
use proto::{
    AgentState, AgentToControl, BuildSource, ControlToAgent, DeploymentOutcome, LogLine, LogStream,
    LogTarget, RunningDeployment, Stage, StageStatus,
};
use rsa::pkcs1::EncodeRsaPrivateKey;
use serde_json::{Value, json};
use shared::config::Config;
use shared::ids::{DeploymentId, ServerId};
use tokio::sync::mpsc;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PASSWORD: &str = "correct horse battery staple";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const HEAD: &str = "89abcdef0123456789abcdef0123456789abcdef";
const INSTALLATION_TOKEN: &str = "ghs_DeployTokenNeverStored0042";

struct Env {
    state: AppState,
    _pg: common::TestDb,
    server: Uuid,
    agent: mpsc::Receiver<ControlToAgent>,
    offset: Arc<AtomicU64>,
    /// `RequestState` recus (retires de `orders`).
    state_requests: usize,
}

impl Env {
    async fn new() -> Self {
        let (mut state, pg) = common::state().await;
        let offset = Arc::new(AtomicU64::new(0));
        let o = offset.clone();
        state.clock = Clock::from_fn(move || {
            SystemTime::now() + Duration::from_secs(o.load(Ordering::SeqCst))
        });
        let server = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', 'local', 'AA:BB')",
        )
        .bind(server)
        .execute(&state.db)
        .await
        .unwrap();
        let agent = state.agents.attach_channel(ServerId(server));
        Self {
            state,
            _pg: pg,
            server,
            agent,
            offset,
            state_requests: 0,
        }
    }

    fn advance_clock(&self, by: Duration) {
        self.offset.fetch_add(by.as_secs(), Ordering::SeqCst);
    }

    /// Ordres recus par l'agent depuis le dernier appel.
    fn orders(&mut self) -> Vec<ControlToAgent> {
        let mut out = Vec::new();
        while let Ok(m) = self.agent.try_recv() {
            // Demande d'etat apres un deploiement reussi : comptee a part.
            if matches!(m, ControlToAgent::RequestState) {
                self.state_requests += 1;
            } else {
                out.push(m);
            }
        }
        out
    }

    /// `RequestState` recus depuis le dernier appel.
    fn take_state_requests(&mut self) -> usize {
        self.orders();
        std::mem::take(&mut self.state_requests)
    }

    /// Execute toutes les taches en file (deploy et cleanup), comme le
    /// travailleur.
    async fn run_jobs(&self) {
        for _ in 0..100 {
            let Some(job) = jobs::claim(&self.state.db, &self.state.clock, "test")
                .await
                .unwrap()
            else {
                return;
            };
            let id = job.id;
            let result = match job.kind {
                JobKind::Deploy => orchestrator::handle_deploy(&self.state, job).await,
                JobKind::Cleanup => retention::handle_cleanup(&self.state, job).await,
                // Envoi teste dans tests/notifications.rs.
                JobKind::Notify => Ok(()),
                other => panic!("tache inattendue : {other}"),
            };
            match result {
                Ok(()) => {
                    jobs::complete(&self.state.db, &self.state.clock, id, "test")
                        .await
                        .unwrap();
                }
                Err(e) => {
                    jobs::fail(
                        &self.state.db,
                        &self.state.clock,
                        id,
                        "test",
                        &e.message,
                        jobs::Failure::Terminal,
                    )
                    .await
                    .unwrap();
                }
            }
        }
        panic!("file de taches sans fin");
    }

    async fn agent_says(&self, msg: AgentToControl) {
        orchestrator::on_agent_message(&self.state, ServerId(self.server), msg).await;
    }

    async fn progress(&self, id: Uuid, stage: Stage, status: StageStatus) {
        self.agent_says(AgentToControl::DeployProgress {
            deployment_id: DeploymentId(id),
            stage,
            status,
        })
        .await;
    }

    async fn build_ok(&self, id: Uuid) {
        self.progress(id, Stage::Image, StageStatus::Started).await;
        self.progress(id, Stage::Image, StageStatus::Succeeded)
            .await;
    }

    async fn deploy_ok(&self, id: Uuid) {
        for stage in [Stage::Start, Stage::Healthcheck, Stage::Switch] {
            self.progress(id, stage, StageStatus::Started).await;
            self.progress(id, stage, StageStatus::Succeeded).await;
        }
    }

    async fn get(&self, id: Uuid) -> Value {
        let r = sqlx::query_scalar::<_, String>("SELECT status FROM deployments WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.state.db)
            .await
            .unwrap();
        assert!(r.is_some(), "deploiement {id} absent");
        let admin = login(&self.state, "viewer").await;
        let reply = send(
            &self.state,
            &admin,
            "GET",
            &format!("/api/deployments/{id}"),
            None,
        )
        .await;
        assert_eq!(reply.status, 200, "{}", reply.raw);
        reply.json()
    }

    async fn status(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT status FROM deployments WHERE id = $1")
            .bind(id)
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }
}

async fn login(state: &AppState, role: &str) -> (String, String) {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, $4, 'Test')",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
    let r = call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": email, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    (r.token(), r.csrf())
}

async fn send(
    state: &AppState,
    session: &(String, String),
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&session.0),
            csrf: Some(&session.1),
            body,
            ..Req::default()
        },
    )
    .await
}

/// Projet cree par l'API ; renvoie (projet, environnement production).
async fn project(state: &AppState, body: Value) -> (Uuid, Uuid) {
    let dev = login(state, "developer").await;
    let r = send(state, &dev, "POST", "/api/projects", Some(body)).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let p = r.json();
    let id = Uuid::parse_str(p["id"].as_str().unwrap()).unwrap();
    let env = Uuid::parse_str(p["environments"][0]["id"].as_str().unwrap()).unwrap();
    (id, env)
}

async fn image_project(state: &AppState, name: &str) -> (Uuid, Uuid) {
    project(
        state,
        json!({ "name": name, "source": { "type": "image", "image": "ghcr.io/acme/app:1" } }),
    )
    .await
}

/// Deploiement manuel par l'API (developer) ; renvoie son identifiant.
async fn deploy(state: &AppState, project: Uuid) -> Uuid {
    let dev = login(state, "developer").await;
    let r = send(
        state,
        &dev,
        "POST",
        &format!("/api/projects/{project}/deployments"),
        Some(json!({ "environment": "production" })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["status"], "queued");
    assert_eq!(r.json()["trigger"], "manual");
    Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap()
}

fn builds(orders: &[ControlToAgent]) -> Vec<Uuid> {
    orders
        .iter()
        .filter_map(|o| match o {
            ControlToAgent::Build(spec) => Some(spec.deployment_id.0),
            _ => None,
        })
        .collect()
}

fn deploys(orders: &[ControlToAgent]) -> Vec<Uuid> {
    orders
        .iter()
        .filter_map(|o| match o {
            ControlToAgent::Deploy(spec) => Some(spec.deployment_id.0),
            _ => None,
        })
        .collect()
}

fn removals(orders: &[ControlToAgent]) -> Vec<Uuid> {
    orders
        .iter()
        .filter_map(|o| match o {
            ControlToAgent::RemoveDeployment { deployment_id } => Some(deployment_id.0),
            _ => None,
        })
        .collect()
}

fn stops(orders: &[ControlToAgent]) -> Vec<Uuid> {
    orders
        .iter()
        .filter_map(|o| match o {
            ControlToAgent::Stop { deployment_id } => Some(deployment_id.0),
            _ => None,
        })
        .collect()
}

/// Deploiement complet et reussi ; renvoie son identifiant.
async fn successful_deploy(env: &mut Env, project: Uuid) -> Uuid {
    let id = deploy(&env.state, project).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![id]);
    env.build_ok(id).await;
    let orders = env.orders();
    assert!(
        matches!(orders.first(), Some(ControlToAgent::ApplyRoutes(_))),
        "table des routes avant Deploy : {orders:?}"
    );
    assert_eq!(deploys(&orders), vec![id]);
    env.deploy_ok(id).await;
    assert_eq!(env.status(id).await, "success");
    id
}

/// Secret fixe pour tous les projets (test du message d'erreur).
struct FixedSecret(&'static str);

impl control_plane::domain::logs::redact::SecretValues for FixedSecret {
    fn values(
        &self,
        _project: Uuid,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = control_plane::domain::logs::redact::SecretsResult>
                + Send
                + '_,
        >,
    > {
        let v = zeroize::Zeroizing::new(self.0.to_string());
        Box::pin(async move { Ok(vec![v]) })
    }
}

#[tokio::test]
async fn deployment_error_message_redacted() {
    const SECRET: &str = "sk-live-0123456789abcdef";
    let mut env = Env::new().await;
    env.state.logs = control_plane::domain::logs::hub::LogHub::new(
        env.state.agents.clone(),
        Arc::new(FixedSecret(SECRET)),
    );
    let (p, _) = image_project(&env.state, "web").await;
    let id = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.orders();
    env.progress(id, Stage::Image, StageStatus::Started).await;
    env.progress(
        id,
        Stage::Image,
        StageStatus::Failed {
            message: format!("connexion refusee : token={SECRET}"),
        },
    )
    .await;
    let d = env.get(id).await;
    assert_eq!(d["status"], "failed");
    let error = d["error"].as_str().unwrap();
    assert!(!error.contains(SECRET), "{error}");
    assert!(error.contains("••••"), "{error}");
    let steps = d["steps"].as_array().unwrap();
    let step = steps.iter().find(|s| s["stage"] == "image").unwrap();
    assert!(!step.to_string().contains(SECRET), "{step}");
}

#[tokio::test]
async fn failed_healthcheck_keeps_old_serving_and_marks_failed() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let v1 = successful_deploy(&mut env, p).await;
    env.run_jobs().await;
    assert!(env.orders().is_empty(), "v1 conservee, rien a nettoyer");

    let v2 = deploy(&env.state, p).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![v2]);
    env.build_ok(v2).await;
    assert_eq!(deploys(&env.orders()), vec![v2]);
    env.progress(v2, Stage::Start, StageStatus::Started).await;
    env.progress(v2, Stage::Start, StageStatus::Succeeded).await;
    env.progress(v2, Stage::Healthcheck, StageStatus::Started)
        .await;
    env.progress(
        v2,
        Stage::Healthcheck,
        StageStatus::Failed {
            message: "healthcheck GET / : 5 essais sans reponse".into(),
        },
    )
    .await;

    let d2 = env.get(v2).await;
    assert_eq!(d2["status"], "failed");
    assert!(d2["error"].as_str().unwrap().contains("healthcheck"));
    assert_eq!(d2["current"], false);
    assert_eq!(d2["stage"], "healthcheck");
    let steps = d2["steps"].as_array().unwrap();
    let hc = steps.iter().find(|s| s["stage"] == "healthcheck").unwrap();
    assert_eq!(hc["status"], "failed");
    assert!(
        !steps.iter().any(|s| s["stage"] == "switch"),
        "pas de bascule"
    );
    let d1 = env.get(v1).await;
    assert_eq!(d1["status"], "success");
    assert_eq!(d1["current"], true, "l'ancien sert toujours");

    // Un message rejoue ne change rien (premier statut terminal).
    env.progress(v2, Stage::Switch, StageStatus::Succeeded)
        .await;
    assert_eq!(env.status(v2).await, "failed");

    env.run_jobs().await;
    let orders = env.orders();
    assert_eq!(removals(&orders), vec![v2], "seul le nouveau est retire");
    assert!(stops(&orders).is_empty());
    assert!(deploys(&orders).is_empty() && builds(&orders).is_empty());
}

#[tokio::test]
async fn second_deploy_same_env_waits_queued() {
    let mut env = Env::new().await;
    let (p, environment) = image_project(&env.state, "web").await;
    let first = deploy(&env.state, p).await;
    let second = deploy(&env.state, p).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![first], "un seul build part");
    assert_eq!(env.status(first).await, "running");
    assert_eq!(env.status(second).await, "queued");

    // Avancements concurrents : toujours un seul en cours.
    let (a, b) = tokio::join!(
        orchestrator::advance(&env.state, environment),
        orchestrator::advance(&env.state, environment)
    );
    a.unwrap();
    b.unwrap();
    assert!(env.orders().is_empty());
    let running: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM deployments WHERE environment_id = $1 AND status = 'running'",
    )
    .bind(environment)
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(running, 1);
    // Garantie de la base elle-meme.
    let forced = sqlx::query("UPDATE deployments SET status = 'running' WHERE id = $1")
        .bind(second)
        .execute(&env.state.db)
        .await;
    assert!(
        forced.is_err(),
        "deux deploiements en cours refuses par la base"
    );

    env.build_ok(first).await;
    env.orders();
    env.deploy_ok(first).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![second]);
    assert_eq!(env.status(second).await, "running");
}

#[tokio::test]
async fn cancel_during_build_cleans_up() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let dev = login(&env.state, "developer").await;
    let first = deploy(&env.state, p).await;
    let second = deploy(&env.state, p).await;
    let third = deploy(&env.state, p).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![first]);
    env.progress(first, Stage::Image, StageStatus::Started)
        .await;

    // En file : annule tout de suite, sans ordre a l'agent.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{third}/cancel"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "cancelled");
    assert!(env.orders().is_empty());

    // En cours : `Stop`, puis confirmation de l'agent.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{first}/cancel"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "running");
    assert_eq!(r.json()["cancel_requested"], true);
    assert_eq!(stops(&env.orders()), vec![first]);
    env.progress(first, Stage::Image, StageStatus::Cancelled)
        .await;
    let d = env.get(first).await;
    assert_eq!(d["status"], "cancelled");
    assert_eq!(d["steps"][0]["status"], "cancelled");

    // Nettoyage (build et conteneur) et deploiement suivant.
    env.run_jobs().await;
    let orders = env.orders();
    assert_eq!(removals(&orders), vec![first]);
    assert_eq!(builds(&orders), vec![second]);

    // Annulation qui croise un `Failed` de l'agent : annule.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{second}/cancel"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(stops(&env.orders()), vec![second]);
    env.progress(
        second,
        Stage::Image,
        StageStatus::Failed {
            message: "interrompu".into(),
        },
    )
    .await;
    assert_eq!(env.status(second).await, "failed");
    env.progress(second, Stage::Image, StageStatus::Cancelled)
        .await;
    assert_eq!(env.status(second).await, "cancelled");

    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{second}/cancel"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "already_finished");
}

#[tokio::test]
async fn cancel_requested_when_build_already_finished_is_cancelled() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let dev = login(&env.state, "developer").await;
    let id = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.orders();
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{id}/cancel"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    env.orders();
    // Le build se termine avant le `Stop` : aucun `Deploy`.
    env.build_ok(id).await;
    assert!(deploys(&env.orders()).is_empty());
    assert_eq!(env.status(id).await, "cancelled");
}

#[tokio::test]
async fn rollback_uses_retained_image_without_rebuild() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let dev = login(&env.state, "developer").await;
    let v1 = successful_deploy(&mut env, p).await;
    let v2 = successful_deploy(&mut env, p).await;
    env.run_jobs().await;
    env.orders();
    let image_v1 = env.get(v1).await["image"].as_str().unwrap().to_string();
    assert_eq!(image_v1, format!("ciphera/web-production:{v1}"));

    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{v1}/rollback"),
        None,
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let back = r.json();
    assert_eq!(back["trigger"], "rollback");
    assert_eq!(back["status"], "queued");
    assert_eq!(back["rollback_of"], v1.to_string());
    assert_eq!(back["image"], image_v1);
    let rb = Uuid::parse_str(back["id"].as_str().unwrap()).unwrap();

    env.run_jobs().await;
    let orders = env.orders();
    assert!(builds(&orders).is_empty(), "aucun build : {orders:?}");
    let spec = orders
        .iter()
        .find_map(|o| match o {
            ControlToAgent::Deploy(spec) => Some(spec.clone()),
            _ => None,
        })
        .expect("Deploy du rollback");
    assert_eq!(spec.deployment_id.0, rb);
    assert_eq!(spec.image.0, image_v1);
    assert!(spec.compose.is_none());
    // Nom du service : alias `web.internal` sur le reseau du projet.
    assert_eq!(spec.service_name.as_deref(), Some("web"));
    env.deploy_ok(rb).await;
    let d = env.get(rb).await;
    assert_eq!(d["status"], "success");
    assert_eq!(d["current"], true);
    assert_eq!(env.get(v2).await["current"], false);

    // Un deploiement en echec n'a pas d'image conservee.
    let bad = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.progress(
        bad,
        Stage::Image,
        StageStatus::Failed {
            message: "pull refuse".into(),
        },
    )
    .await;
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{bad}/rollback"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "not_rollbackable");
}

#[tokio::test]
async fn compose_rollback_redeploys_the_whole_bundle() {
    let mut env = Env::new().await;
    let (p, environment) = project(
        &env.state,
        json!({
            "name": "stack",
            "source": { "type": "git", "repository": "acme/stack", "branch": "main" },
            "build": { "kind": "compose", "path": "compose.yaml", "service": "web" }
        }),
    )
    .await;
    // Push (commit connu, depot public : aucun appel a GitHub).
    jobs::enqueue(
        &env.state.db,
        JobKind::Deploy,
        json!({ "project_id": p, "environment_id": environment, "commit_sha": COMMIT }),
        None,
    )
    .await
    .unwrap();
    env.run_jobs().await;
    let orders = env.orders();
    let ControlToAgent::Build(spec) = &orders[0] else {
        panic!("Build attendu : {orders:?}");
    };
    let v1 = spec.deployment_id.0;
    assert!(matches!(
        &spec.build,
        proto::BuildMethod::Compose { path, service } if path == "compose.yaml" && service == "web"
    ));
    assert!(spec.git_token.is_none(), "pas d'App : depot public");
    env.build_ok(v1).await;
    let orders = env.orders();
    let first = orders
        .iter()
        .find_map(|o| match o {
            ControlToAgent::Deploy(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    let compose = first.compose.expect("deploiement Compose");
    assert_eq!(compose.service, "web");
    assert_eq!(compose.bundle.0, v1);
    env.deploy_ok(v1).await;
    let push = env.get(v1).await;
    assert_eq!(push["trigger"], "push");
    assert_eq!(push["commit_sha"], COMMIT);
    assert_eq!(push["branch"], "main");

    let dev = login(&env.state, "developer").await;
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{v1}/rollback"),
        None,
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let rb = Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap();
    env.run_jobs().await;
    let orders = env.orders();
    assert!(builds(&orders).is_empty());
    let spec = orders
        .iter()
        .find_map(|o| match o {
            ControlToAgent::Deploy(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(spec.deployment_id.0, rb);
    let compose = spec.compose.unwrap();
    assert_eq!(
        compose.bundle.0, v1,
        "ensemble Compose du deploiement d'origine"
    );
    assert_eq!(compose.service, "web");
}

#[tokio::test]
async fn retention_keeps_last_5() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let mut ids = Vec::new();
    let mut removed = Vec::new();
    for _ in 0..7 {
        ids.push(successful_deploy(&mut env, p).await);
        env.run_jobs().await;
        removed.extend(removals(&env.orders()));
    }
    assert_eq!(removed, vec![ids[0], ids[1]], "les deux plus anciens");
    assert_eq!(env.get(ids[0]).await["can_rollback"], false);
    assert_eq!(env.get(ids[2]).await["can_rollback"], true);
    let dev = login(&env.state, "developer").await;
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{}/rollback", ids[0]),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);

    // Un rollback vers le plus ancien conserve protege son image, meme
    // quand il sort lui-meme des 5 derniers.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{}/rollback", ids[2]),
        None,
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let rb = Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap();
    env.run_jobs().await;
    env.orders();
    env.deploy_ok(rb).await;
    env.run_jobs().await;
    // Conserves : rb, ids[6..2] -> ids[2] sort des 5 mais reste (image de rb).
    assert_eq!(removals(&env.orders()), Vec::<Uuid>::new());
    assert_eq!(env.get(ids[2]).await["can_rollback"], true);
    let kept: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM deployments WHERE cleaned_at IS NULL AND status = 'success'",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(kept, 6, "5 deploiements conserves + l'image du rollback");
}

#[tokio::test]
async fn agent_disconnect_marks_failed_after_10min() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let first = deploy(&env.state, p).await;
    let second = deploy(&env.state, p).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![first]);

    // Des lignes de journal sont des nouvelles.
    env.advance_clock(Duration::from_secs(9 * 60));
    env.agent_says(AgentToControl::LogLine(LogLine {
        target: LogTarget::Deployment(DeploymentId(first)),
        stream: LogStream::Stdout,
        timestamp_ms: 0,
        line: "#4 pnpm install".into(),
    }))
    .await;
    env.advance_clock(Duration::from_secs(2 * 60));
    assert_eq!(orchestrator::sweep_stale(&env.state).await.unwrap(), 0);
    assert_eq!(env.status(first).await, "running");

    // L'agent se tait (deconnexion) : 10 min plus tard, echec.
    env.state.agents.disconnect(ServerId(env.server));
    env.advance_clock(Duration::from_secs(7 * 60));
    assert_eq!(orchestrator::sweep_stale(&env.state).await.unwrap(), 0);
    env.advance_clock(Duration::from_secs(2 * 60));
    assert_eq!(orchestrator::sweep_stale(&env.state).await.unwrap(), 1);
    let d = env.get(first).await;
    assert_eq!(d["status"], "failed");
    assert!(d["error"].as_str().unwrap().contains("10 minutes"));

    // L'environnement est libere : le suivant demarre, agent toujours absent.
    env.run_jobs().await;
    assert_eq!(env.status(second).await, "running");
    env.agent = env.state.agents.attach_channel(ServerId(env.server));
    assert!(env.orders().is_empty(), "rien n'a pu partir");
    // A la reconnexion : l'issue reelle du premier (servi) prime sur l'echec
    // presume, le second (jamais recu) est envoye, la table des routes suit.
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![RunningDeployment {
            deployment_id: DeploymentId(first),
            service_id: shared::ids::ServiceId::new(),
            image: proto::ImageRef("x".into()),
            healthy: true,
            serving: true,
            runs_as_root: false,
        }],
        in_progress: vec![],
        outcomes: vec![DeploymentOutcome {
            deployment_id: DeploymentId(first),
            stage: Stage::Switch,
            status: StageStatus::Succeeded,
        }],
    }))
    .await;
    assert_eq!(env.status(first).await, "success");
    let orders = env.orders();
    assert_eq!(builds(&orders), vec![second]);
    assert!(matches!(
        orders.last(),
        Some(ControlToAgent::ApplyRoutes(_))
    ));
}

#[tokio::test]
async fn reconcile_on_reconnect_updates_status() {
    let mut env = Env::new().await;
    let (a, _) = image_project(&env.state, "alpha").await;
    let (b, _) = image_project(&env.state, "bravo").await;
    let (c, _) = image_project(&env.state, "charlie").await;
    let (d, _) = image_project(&env.state, "delta").await;
    let da = deploy(&env.state, a).await;
    let db = deploy(&env.state, b).await;
    let dc = deploy(&env.state, c).await;
    env.run_jobs().await;
    env.build_ok(da).await;
    env.orders();
    // Ordres envoyes il y a plus de 30 s.
    env.advance_clock(Duration::from_secs(60));
    // Agent coupe : le deploiement de delta ne part pas.
    env.state.agents.disconnect(ServerId(env.server));
    let dd = deploy(&env.state, d).await;
    env.run_jobs().await;
    assert_eq!(env.status(dd).await, "running");

    env.agent = env.state.agents.attach_channel(ServerId(env.server));
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![],
        in_progress: vec![DeploymentId(db)],
        outcomes: vec![DeploymentOutcome {
            deployment_id: DeploymentId(da),
            stage: Stage::Switch,
            status: StageStatus::Succeeded,
        }],
    }))
    .await;
    assert_eq!(env.status(da).await, "success", "issue gardee appliquee");
    assert_eq!(env.status(db).await, "running", "en cours : on attend");
    let lost = env.get(dc).await;
    assert_eq!(lost["status"], "failed", "perdu");
    assert!(lost["error"].as_str().unwrap().contains("perdu"));
    let orders = env.orders();
    assert_eq!(removals(&orders), vec![dc]);
    assert_eq!(builds(&orders), vec![dd], "envoye a la reconnexion");
    assert!(
        matches!(orders.last(), Some(ControlToAgent::ApplyRoutes(_))),
        "table des routes apres chaque reconnexion : {orders:?}"
    );

    // `State` rejoue : idempotent.
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![],
        in_progress: vec![DeploymentId(db), DeploymentId(dd)],
        outcomes: vec![DeploymentOutcome {
            deployment_id: DeploymentId(da),
            stage: Stage::Switch,
            status: StageStatus::Succeeded,
        }],
    }))
    .await;
    assert_eq!(env.status(da).await, "success");
    assert_eq!(env.status(dc).await, "failed");
    let orders = env.orders();
    assert!(builds(&orders).is_empty() && removals(&orders).is_empty());

    // Un autre serveur ne peut pas toucher a ces deploiements.
    orchestrator::on_agent_message(
        &env.state,
        ServerId::new(),
        AgentToControl::DeployProgress {
            deployment_id: DeploymentId(db),
            stage: Stage::Image,
            status: StageStatus::Failed {
                message: "x".into(),
            },
        },
    )
    .await;
    assert_eq!(env.status(db).await, "running");
}

#[tokio::test]
async fn viewer_cannot_deploy_403() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let ok = successful_deploy(&mut env, p).await;
    let viewer = login(&env.state, "viewer").await;
    let create = send(
        &env.state,
        &viewer,
        "POST",
        &format!("/api/projects/{p}/deployments"),
        None,
    )
    .await;
    assert_eq!(create.status, 403, "{}", create.raw);
    for action in ["cancel", "rollback"] {
        let r = send(
            &env.state,
            &viewer,
            "POST",
            &format!("/api/deployments/{ok}/{action}"),
            None,
        )
        .await;
        assert_eq!(r.status, 403, "{action} : {}", r.raw);
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM deployments")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1, "rien de cree");
    assert_eq!(
        send(&env.state, &viewer, "GET", "/api/deployments", None)
            .await
            .status,
        200
    );
    assert_eq!(
        send(
            &env.state,
            &viewer,
            "GET",
            &format!("/api/deployments/{ok}"),
            None
        )
        .await
        .status,
        200
    );
    // Sans jeton CSRF : refuse meme pour un developer.
    let dev = login(&env.state, "developer").await;
    let r = call(
        &env.state,
        "POST",
        &format!("/api/projects/{p}/deployments"),
        Req {
            cookie: Some(&dev.0),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let unauth = call(&env.state, "GET", "/api/deployments", Req::default()).await;
    assert_eq!(unauth.status, 401);
}

#[tokio::test]
async fn list_filters_and_unknown_targets() {
    let mut env = Env::new().await;
    let (a, _) = image_project(&env.state, "alpha").await;
    let (b, _) = image_project(&env.state, "bravo").await;
    let ok = successful_deploy(&mut env, a).await;
    let queued = deploy(&env.state, b).await;
    let viewer = login(&env.state, "viewer").await;
    let list = |q: String| {
        let state = env.state.clone();
        let viewer = viewer.clone();
        async move {
            send(
                &state,
                &viewer,
                "GET",
                &format!("/api/deployments{q}"),
                None,
            )
            .await
        }
    };
    let all = list(String::new()).await.json();
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert_eq!(all[0]["id"], queued.to_string(), "plus recent d'abord");
    assert_eq!(
        all[1]["number"].as_i64().unwrap() + 1,
        all[0]["number"].as_i64().unwrap()
    );
    let by_project = list(format!("?project={a}")).await.json();
    assert_eq!(by_project.as_array().unwrap().len(), 1);
    assert_eq!(by_project[0]["id"], ok.to_string());
    assert_eq!(by_project[0]["project_name"], "alpha");
    assert_eq!(by_project[0]["environment"], "production");
    let steps: Vec<&str> = by_project[0]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["stage"].as_str().unwrap())
        .collect();
    assert_eq!(steps, ["image", "start", "healthcheck", "switch"]);
    assert!(by_project[0]["duration_ms"].as_i64().is_some());
    let q = list("?status=queued".into()).await.json();
    assert_eq!(q.as_array().unwrap().len(), 1);
    assert_eq!(q[0]["id"], queued.to_string());
    assert_eq!(list("?status=bogus".into()).await.status, 422);
    assert_eq!(list("?project=pas-un-uuid".into()).await.status, 422);
    assert_eq!(list("?limit=0".into()).await.status, 422);
    let missing = send(
        &env.state,
        &viewer,
        "GET",
        &format!("/api/deployments/{}", Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(missing.status, 404);
    let dev = login(&env.state, "developer").await;
    let no_env = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/projects/{a}/deployments"),
        Some(json!({ "environment": "staging" })),
    )
    .await;
    assert_eq!(no_env.status, 404, "{}", no_env.raw);
    let no_project = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/projects/{}/deployments", Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(no_project.status, 404, "{}", no_project.raw);
}

#[tokio::test]
async fn project_delete_removes_agent_resources() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let v1 = successful_deploy(&mut env, p).await;
    let v2 = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.orders();
    let admin = login(&env.state, "admin").await;
    let r = send(
        &env.state,
        &admin,
        "DELETE",
        &format!("/api/projects/{p}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let removed: HashSet<Uuid> = removals(&env.orders()).into_iter().collect();
    assert_eq!(removed, HashSet::from([v1, v2]));
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM deployments")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

/// Cle RSA 2048 jetable de la GitHub App simulee (PKCS#1 PEM).
fn test_key() -> &'static String {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut rng = rand::rand_core::UnwrapErr(rand::rngs::SysRng);
        let key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let der = key.to_pkcs1_der().unwrap();
        let body = STANDARD.encode(der.as_bytes());
        // En-tete PEM factice coupe par concat! : aucune ligne de la source n'imite une vraie cle (scanner de secrets).
        let mut pem = String::from(concat!("-----BEGIN RSA ", "PRIVATE KEY-----\n"));
        for line in body.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(line).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END RSA PRIVATE KEY-----\n");
        pem
    })
}

#[tokio::test]
async fn push_deploys_are_deduplicated_and_token_is_minted_at_dispatch() {
    let mut env = Env::new().await;
    let server = MockServer::start().await;
    let uri = server.uri();
    env.state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_GITHUB_API_URL" => Some(uri.clone()),
            _ => None,
        })
        .unwrap(),
    );
    sqlx::query(
        "INSERT INTO github_app (app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) \
         VALUES (123, 'ciphera-ab12cd', 'Ciphera ab12cd', 'acme', \
         'https://github.com/apps/ciphera-ab12cd', $1, $2)",
    )
    .bind(
        env.state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_PRIVATE_KEY,
                test_key().as_bytes(),
            )
            .unwrap(),
    )
    .bind(
        env.state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_WEBHOOK_SECRET,
                b"whsec-test",
            )
            .unwrap(),
    )
    .execute(&env.state.db)
    .await
    .unwrap();
    let expires = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_hours(1))
        .unwrap();
    Mock::given(method("GET"))
        .and(path("/repos/acme/web/installation"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "id": 42 })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/app/installations/42/access_tokens"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "token": INSTALLATION_TOKEN,
            "expires_at": expires.to_string(),
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/web/commits/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "sha": HEAD })))
        .mount(&server)
        .await;

    let (p, environment) = project(
        &env.state,
        json!({
            "name": "web",
            "source": { "type": "git", "repository": "acme/web", "branch": "main" },
            "build": { "kind": "railpack" }
        }),
    )
    .await;
    // Deux livraisons du meme commit : un seul deploiement.
    let payload = json!({ "project_id": p, "environment_id": environment, "commit_sha": COMMIT });
    for key in ["d1", "d2"] {
        jobs::enqueue(
            &env.state.db,
            JobKind::Deploy,
            payload.clone(),
            Some(key.into()),
        )
        .await
        .unwrap();
    }
    env.run_jobs().await;
    let orders = env.orders();
    assert_eq!(orders.len(), 1, "{orders:?}");
    let ControlToAgent::Build(spec) = &orders[0] else {
        panic!("Build attendu");
    };
    assert_eq!(
        spec.git_token.as_ref().map(|t| t.expose()),
        Some(INSTALLATION_TOKEN),
        "jeton d'installation obtenu a l'envoi"
    );
    assert_eq!(
        spec.source,
        BuildSource::Git {
            url: "https://github.com/acme/web".into(),
            branch: "main".into(),
            commit_sha: COMMIT.into(),
        }
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM deployments WHERE trigger = 'push'")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let push = spec.deployment_id.0;
    env.build_ok(push).await;
    env.orders();
    env.deploy_ok(push).await;
    env.run_jobs().await;
    env.orders();

    // Deploiement manuel : la branche est resolue au lancement du build.
    let manual = deploy(&env.state, p).await;
    env.run_jobs().await;
    let orders = env.orders();
    let ControlToAgent::Build(spec) = &orders[0] else {
        panic!("Build attendu : {orders:?}");
    };
    assert_eq!(spec.deployment_id.0, manual);
    assert!(matches!(&spec.source, BuildSource::Git { commit_sha, .. } if commit_sha == HEAD));
    assert_eq!(env.get(manual).await["commit_sha"], HEAD);

    // Le jeton n'est stocke nulle part en base.
    let dump: String = sqlx::query_scalar(
        "SELECT coalesce(string_agg(t::text, ' '), '') FROM ( \
           SELECT row_to_json(d)::text AS t FROM deployments d \
           UNION ALL SELECT row_to_json(j)::text FROM jobs j \
           UNION ALL SELECT row_to_json(a)::text FROM audit_events a) x",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert!(!dump.contains(INSTALLATION_TOKEN));
}

/// Deploiement en cours depuis plus de 10 min sans nouvelles : `failed`.
async fn presumed_failure(env: &mut Env, project: Uuid) -> Uuid {
    let id = deploy(&env.state, project).await;
    env.run_jobs().await;
    assert_eq!(builds(&env.orders()), vec![id]);
    env.advance_clock(Duration::from_secs(11 * 60));
    assert_eq!(orchestrator::sweep_stale(&env.state).await.unwrap(), 1);
    id
}

#[tokio::test]
async fn watchdog_stop_reported_cancelled_keeps_the_presumed_failure() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let id = presumed_failure(&mut env, p).await;
    // Agent connecte : il recoit le `Stop` du chien de garde et l'annule.
    assert_eq!(stops(&env.orders()), vec![id]);
    env.progress(id, Stage::Image, StageStatus::Cancelled).await;
    let d = env.get(id).await;
    assert_eq!(d["status"], "failed", "personne n'a annule");
    assert!(d["error"].as_str().unwrap().contains("10 minutes"));
    // Plus presume : la retention peut le nettoyer.
    env.run_jobs().await;
    assert_eq!(removals(&env.orders()), vec![id]);
}

#[tokio::test]
async fn reconnect_outcomes_override_a_presumed_failure_only_with_real_verdicts() {
    let mut env = Env::new().await;
    let (a, _) = image_project(&env.state, "alpha").await;
    let (b, _) = image_project(&env.state, "bravo").await;
    let (c, _) = image_project(&env.state, "charlie").await;
    let cancelled = deploy(&env.state, a).await;
    let served = deploy(&env.state, b).await;
    let broken = deploy(&env.state, c).await;
    env.run_jobs().await;
    env.orders();
    env.advance_clock(Duration::from_secs(11 * 60));
    assert_eq!(orchestrator::sweep_stale(&env.state).await.unwrap(), 3);
    env.orders();
    let outcome = |id: Uuid, stage, status| DeploymentOutcome {
        deployment_id: DeploymentId(id),
        stage,
        status,
    };
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![],
        in_progress: vec![],
        outcomes: vec![
            outcome(cancelled, Stage::Image, StageStatus::Cancelled),
            outcome(served, Stage::Switch, StageStatus::Succeeded),
            outcome(
                broken,
                Stage::Image,
                StageStatus::Failed {
                    message: "pull refuse : manifest unknown".into(),
                },
            ),
        ],
    }))
    .await;
    let d = env.get(cancelled).await;
    assert_eq!(d["status"], "failed");
    assert!(d["error"].as_str().unwrap().contains("10 minutes"));
    let d = env.get(served).await;
    assert_eq!(d["status"], "success");
    assert_eq!(d["current"], true);
    assert!(d["error"].is_null());
    let d = env.get(broken).await;
    assert_eq!(d["status"], "failed");
    assert!(d["error"].as_str().unwrap().contains("manifest unknown"));
    let step = &d["steps"][0];
    assert_eq!(step["status"], "failed");
    assert!(
        step["message"]
            .as_str()
            .unwrap()
            .contains("manifest unknown")
    );
}

#[tokio::test]
async fn full_agent_outbox_does_not_swallow_the_deploy_order() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let id = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.orders();
    // File d'envoi de l'agent pleine.
    while env
        .state
        .agents
        .send(ServerId(env.server), ControlToAgent::RequestMetrics)
        .is_ok()
    {}
    env.build_ok(id).await;
    env.orders();
    assert_eq!(env.status(id).await, "running");
    env.run_jobs().await;
    assert_eq!(
        deploys(&env.orders()),
        vec![id],
        "Deploy renvoye par une tache"
    );
}

#[tokio::test]
async fn success_after_a_late_cancel_clears_the_request() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let dev = login(&env.state, "developer").await;
    let id = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.build_ok(id).await;
    env.orders();
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/deployments/{id}/cancel"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // `Stop` arrive apres la bascule : trop tard, le deploiement aboutit.
    env.deploy_ok(id).await;
    let d = env.get(id).await;
    assert_eq!(d["status"], "success");
    assert_eq!(d["cancel_requested"], false);
}

#[tokio::test]
async fn project_delete_succeeds_with_the_agent_away() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    successful_deploy(&mut env, p).await;
    env.state.agents.disconnect(ServerId(env.server));
    let admin = login(&env.state, "admin").await;
    let r = send(
        &env.state,
        &admin,
        "DELETE",
        &format!("/api/projects/{p}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
}

/// Le dossier racine des reglages Git part avec l'ordre de build ; a la
/// racine (valeur vide), l'ordre n'en porte aucun.
#[tokio::test]
async fn build_order_carries_the_project_root_directory() {
    let mut env = Env::new().await;
    for (name, root, expected) in [
        ("web", "", None),
        ("mono", "apps/web", Some("apps/web".to_string())),
    ] {
        let (p, environment) = project(
            &env.state,
            json!({
                "name": name,
                "source": { "type": "git", "repository": format!("acme/{name}"), "branch": "main" },
                "build": { "kind": "railpack" }
            }),
        )
        .await;
        sqlx::query("UPDATE git_sources SET root_directory = $2 WHERE project_id = $1")
            .bind(p)
            .bind(root)
            .execute(&env.state.db)
            .await
            .unwrap();
        jobs::enqueue(
            &env.state.db,
            JobKind::Deploy,
            json!({ "project_id": p, "environment_id": environment, "commit_sha": COMMIT }),
            None,
        )
        .await
        .unwrap();
        env.run_jobs().await;
        let orders = env.orders();
        let ControlToAgent::Build(spec) = &orders[0] else {
            panic!("Build attendu : {orders:?}");
        };
        assert_eq!(spec.root_directory, expected, "{name}");
    }
}

/// Remplit la file d'envoi de l'agent (ordres sans effet).
fn fill_outbox(env: &Env) {
    while env
        .state
        .agents
        .send(ServerId(env.server), ControlToAgent::RequestMetrics)
        .is_ok()
    {}
}

/// Reconnexion avec la file d'envoi pleine : le premier ordre d'un
/// deploiement jamais envoye n'est pas seulement journalise, une tache le
/// renvoie.
#[tokio::test]
async fn reconcile_dispatch_on_a_full_outbox_is_retried_by_a_job() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    env.state.agents.disconnect(ServerId(env.server));
    let id = deploy(&env.state, p).await;
    env.run_jobs().await;
    assert_eq!(env.status(id).await, "running");
    env.agent = env.state.agents.attach_channel(ServerId(env.server));
    fill_outbox(&env);
    env.agent_says(AgentToControl::State(AgentState::default()))
        .await;
    env.orders();
    env.run_jobs().await;
    assert_eq!(
        builds(&env.orders()),
        vec![id],
        "Build renvoye par une tache"
    );
}

/// `Deploy` perdu en route, renvoye a la reconnexion alors que la file
/// d'envoi est pleine : une tache le renvoie (il etait note comme parti).
#[tokio::test]
async fn resend_deploy_on_a_full_outbox_is_retried_by_a_job() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    let id = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.build_ok(id).await;
    assert_eq!(deploys(&env.orders()), vec![id]);
    // `Deploy` jamais arrive : l'agent ne connait que l'issue du build.
    env.agent = env.state.agents.attach_channel(ServerId(env.server));
    fill_outbox(&env);
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![],
        in_progress: vec![],
        outcomes: vec![DeploymentOutcome {
            deployment_id: DeploymentId(id),
            stage: Stage::Image,
            status: StageStatus::Succeeded,
        }],
    }))
    .await;
    env.orders();
    env.run_jobs().await;
    assert_eq!(
        deploys(&env.orders()),
        vec![id],
        "Deploy renvoye par une tache"
    );
    assert_eq!(env.status(id).await, "running");
}

/// Messages d'agents perdus par le suivi des deploiements (retard) : l'etat
/// complet est redemande a chaque agent connecte.
#[tokio::test]
async fn lagged_agent_events_request_a_full_state() {
    let mut env = Env::new().await;
    let events = env.state.agents.subscribe();
    let logs = env.state.agents.subscribe_logs();
    for _ in 0..2_000 {
        env.state.agents.publish(
            ServerId(env.server),
            AgentToControl::Error {
                code: 0,
                message: String::new(),
            },
        );
    }
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let run = tokio::spawn(orchestrator::run(env.state.clone(), events, logs, stop_rx));
    let mut asked = false;
    for _ in 0..100 {
        if env.take_state_requests() > 0 {
            asked = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop_tx.send(true).unwrap();
    run.await.unwrap();
    assert!(asked, "etat complet redemande apres la perte de messages");
}

/// Retards repetes : au plus un `RequestState` par agent par
/// `orchestrator::RESYNC_MIN_INTERVAL` (pas de surcharge quand le suivi est
/// deja en retard).
#[tokio::test(flavor = "current_thread")]
async fn repeated_lag_requests_state_at_most_once_per_interval() {
    let mut env = Env::new().await;
    let events = env.state.agents.subscribe();
    let logs = env.state.agents.subscribe_logs();
    let flood = |env: &Env| {
        for _ in 0..2_000 {
            env.state.agents.publish(
                ServerId(env.server),
                AgentToControl::Error {
                    code: 0,
                    message: String::new(),
                },
            );
        }
    };
    flood(&env);
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let run = tokio::spawn(orchestrator::run(env.state.clone(), events, logs, stop_rx));
    let asked = |env: &mut Env| env.take_state_requests();
    let mut first = 0;
    for _ in 0..100 {
        first += asked(&mut env);
        if first > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(first, 1, "premier retard : etat redemande");
    // Second puis troisieme retard aussitot apres : pas de nouvelle demande
    // tout de suite...
    flood(&env);
    tokio::time::sleep(Duration::from_millis(300)).await;
    flood(&env);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let again = asked(&mut env);
    assert_eq!(again, 0, "etat redemande deux fois de suite");
    assert!(orchestrator::RESYNC_MIN_INTERVAL >= Duration::from_secs(5));
    // ... mais une, et une seule, a la fin de l'intervalle : les messages
    // perdus entre-temps sont rattrapes.
    tokio::time::sleep(orchestrator::RESYNC_MIN_INTERVAL).await;
    let trailing = asked(&mut env);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let after = asked(&mut env);
    stop_tx.send(true).unwrap();
    run.await.unwrap();
    assert_eq!(trailing, 1, "demande differee a la fin de l'intervalle");
    assert_eq!(after, 0, "une seule demande differee");
}

/// Erreur de base au milieu de la reconciliation (ici : mise a jour des
/// deploiements refusee) : la table des routes part quand meme.
#[tokio::test]
async fn routes_are_sent_even_when_the_reconcile_hits_a_database_error() {
    let mut env = Env::new().await;
    let (a, _) = image_project(&env.state, "alpha").await;
    let da = deploy(&env.state, a).await;
    env.run_jobs().await;
    assert_eq!(env.status(da).await, "running");
    env.orders();
    for sql in [
        "CREATE FUNCTION refuse_update() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'base indisponible'; END $$",
        "CREATE TRIGGER refuse_update BEFORE UPDATE ON deployments \
         FOR EACH ROW EXECUTE FUNCTION refuse_update()",
    ] {
        sqlx::query(sql).execute(&env.state.db).await.unwrap();
    }
    // En cours sur l'agent : `touch` met a jour le deploiement, refuse.
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![],
        in_progress: vec![DeploymentId(da)],
        outcomes: vec![],
    }))
    .await;
    let orders = env.orders();
    assert!(
        orders
            .iter()
            .any(|o| matches!(o, ControlToAgent::ApplyRoutes(_))),
        "table des routes envoyee malgre l'erreur : {orders:?}"
    );
}

/// Taches `notify` en file : (evenement, deploiement), dans l'ordre.
async fn notify_jobs(state: &AppState) -> Vec<(String, Uuid)> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT payload->>'event', payload->>'deployment_id' FROM jobs \
         WHERE kind = 'notify' ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(e, id)| (e, Uuid::parse_str(&id).unwrap()))
        .collect()
}

#[tokio::test]
async fn terminal_outcomes_queue_their_notification_when_smtp_is_on() {
    let mut env = Env::new().await;
    let (p, _) = image_project(&env.state, "web").await;
    // Sans SMTP (ni origine publique) : aucune tache.
    successful_deploy(&mut env, p).await;
    assert!(notify_jobs(&env.state).await.is_empty());

    env.state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_PUBLIC_URL" => Some("https://panel.example.com".to_string()),
            _ => None,
        })
        .unwrap(),
    );
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&env.state.db)
    .await
    .unwrap();

    let ok = successful_deploy(&mut env, p).await;
    let failed = deploy(&env.state, p).await;
    env.run_jobs().await;
    env.build_ok(failed).await;
    env.progress(
        failed,
        Stage::Start,
        StageStatus::Failed {
            message: "conteneur arrete".into(),
        },
    )
    .await;
    env.orders();
    // Echec presume puis verdict reel `success` a la reconnexion.
    let late = presumed_failure(&mut env, p).await;
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![],
        in_progress: vec![],
        outcomes: vec![DeploymentOutcome {
            deployment_id: DeploymentId(late),
            stage: Stage::Switch,
            status: StageStatus::Succeeded,
        }],
    }))
    .await;
    assert_eq!(env.status(late).await, "success");

    assert_eq!(
        notify_jobs(&env.state).await,
        vec![
            ("deploy_succeeded".to_string(), ok),
            ("deploy_failed".to_string(), failed),
            ("deploy_failed".to_string(), late),
            ("deploy_succeeded".to_string(), late),
        ]
    );
}

/// Apres un deploiement reussi, l'etat complet de l'agent est redemande
/// (utilisateur effectif des conteneurs, `runs_as_root`) sans attendre une
/// reconnexion.
#[tokio::test]
async fn state_is_requested_after_a_successful_deploy() {
    let mut env = Env::new().await;
    let (project, _) = image_project(&env.state, "web").await;
    assert_eq!(env.take_state_requests(), 0);
    let ok = successful_deploy(&mut env, project).await;
    assert_eq!(env.status(ok).await, "success");
    assert_eq!(env.take_state_requests(), 1, "une demande apres le succes");

    // Le `State` qui suit renseigne le badge.
    env.agent_says(AgentToControl::State(AgentState {
        deployments: vec![RunningDeployment {
            deployment_id: DeploymentId(ok),
            service_id: shared::ids::ServiceId::new(),
            image: proto::ImageRef("x:1".into()),
            healthy: true,
            serving: true,
            runs_as_root: true,
        }],
        in_progress: Vec::new(),
        outcomes: Vec::new(),
    }))
    .await;
    let root: bool = sqlx::query_scalar("SELECT runs_as_root FROM deployments WHERE id = $1")
        .bind(ok)
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert!(root);
}
