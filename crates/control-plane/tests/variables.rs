//! Variables d'environnement : projet et globales, secrets chiffres (age)
//! en ecriture seule, mode `.env`, export sans secrets, priorite du projet,
//! injection dans `DeploySpec.env` a l'envoi et masquage des journaux.

mod common;

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::deployments::{orchestrator, retention};
use control_plane::domain::logs::hub::Source;
use control_plane::domain::logs::redact::MASK;
use control_plane::jobs::{self, JobKind};
use control_plane::state::AppState;
use proto::{
    AgentToControl, ControlToAgent, DeploySpec, LogLine, LogStream, LogTarget, Stage, StageStatus,
};
use serde_json::{Value, json};
use shared::ids::{DeploymentId, ServerId};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const SECRET: &str = "sk_test_NeverReturnedValue42";
const OTHER_SECRET: &str = "postgres://web:An0therS3cret@db:5432/app";

type Session = (String, String);

async fn login(state: &AppState, role: &str) -> Session {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, 'Test', now())",
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
    s: &Session,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            body,
            ..Req::default()
        },
    )
    .await
}

async fn image_project(state: &AppState, name: &str) -> Uuid {
    let dev = login(state, "developer").await;
    let r = send(
        state,
        &dev,
        "POST",
        "/api/projects",
        Some(json!({ "name": name, "source": { "type": "image", "image": "ghcr.io/acme/app:1" } })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap()
}

async fn put_var(state: &AppState, s: &Session, project: Uuid, body: Value) -> Reply {
    send(
        state,
        s,
        "PUT",
        &format!("/api/projects/{project}/variables?env=production"),
        Some(body),
    )
    .await
}

async fn list(state: &AppState, s: &Session, project: Uuid) -> Vec<Value> {
    let r = send(
        state,
        s,
        "GET",
        &format!("/api/projects/{project}/variables?env=production"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!r.raw.contains(SECRET) && !r.raw.contains(OTHER_SECRET));
    r.json().as_array().unwrap().clone()
}

fn row<'a>(rows: &'a [Value], key: &str) -> &'a Value {
    rows.iter()
        .find(|r| r["key"] == key)
        .unwrap_or_else(|| panic!("{key} absent de {rows:?}"))
}

async fn create_global(state: &AppState, s: &Session, body: Value) -> Value {
    let r = send(state, s, "POST", "/api/variables", Some(body)).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert!(!r.raw.contains(SECRET) && !r.raw.contains(OTHER_SECRET));
    r.json()
}

async fn sealed(state: &AppState, project: Uuid, key: &str) -> (Option<String>, Option<Vec<u8>>) {
    sqlx::query_as(
        "SELECT v.value, v.value_sealed FROM project_variables v \
         JOIN environments e ON e.id = v.environment_id \
         WHERE e.project_id = $1 AND v.key = $2",
    )
    .bind(project)
    .bind(key)
    .fetch_one(&state.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn secret_value_never_returned() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let admin = login(&state, "admin").await;

    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_KEY", "value": SECRET, "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!r.raw.contains(SECRET));
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "NODE_ENV", "value": "production", "secret": false }),
    )
    .await;
    let rows = list(&state, &dev, project).await;
    let secret = row(&rows, "STRIPE_KEY");
    assert_eq!(secret["value"], Value::Null);
    assert_eq!(secret["secret"], true);
    assert_eq!(secret["inherited"], false);
    assert_eq!(row(&rows, "NODE_ENV")["value"], "production");

    // Globale secrete : jamais renvoyee non plus (liste, creation, export).
    let g = create_global(
        &state,
        &admin,
        json!({ "key": "RESEND_API_KEY", "value": OTHER_SECRET, "secret": true,
                "environments": ["production"], "all_projects": true, "projects": [] }),
    )
    .await;
    assert_eq!(g["value"], Value::Null);
    let all = send(&state, &admin, "GET", "/api/variables", None).await;
    assert_eq!(all.status, 200, "{}", all.raw);
    assert!(!all.raw.contains(OTHER_SECRET));
    let rows = list(&state, &dev, project).await;
    let inherited = row(&rows, "RESEND_API_KEY");
    assert_eq!(inherited["value"], Value::Null);
    assert_eq!(inherited["inherited"], true);

    // Ni le journal d'audit.
    let audit: Vec<String> = sqlx::query_scalar("SELECT detail::text FROM audit_events")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert!(audit.iter().any(|d| d.contains("STRIPE_KEY")));
    assert!(
        audit
            .iter()
            .all(|d| !d.contains(SECRET) && !d.contains(OTHER_SECRET))
    );
}

/// Valeur claire d'un secret stocke (chiffre lie a sa ligne : etiquette
/// retiree, sans la verifier).
fn clear(state: &AppState, sealed: &[u8]) -> Vec<u8> {
    let framed = state.secrets.open(sealed).unwrap();
    let rest = framed
        .strip_prefix(b"ciphera-bound/1\n".as_slice())
        .expect("chiffre lie a sa ligne");
    let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
    rest[4 + len..].to_vec()
}

#[tokio::test]
async fn secret_stored_encrypted() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_KEY", "value": SECRET, "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let (value, bytes) = sealed(&state, project, "STRIPE_KEY").await;
    assert_eq!(value, None);
    let bytes = bytes.expect("valeur chiffree");
    assert!(!bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()));
    assert_eq!(clear(&state, &bytes).as_slice(), SECRET.as_bytes());

    let admin = login(&state, "admin").await;
    create_global(
        &state,
        &admin,
        json!({ "key": "RESEND_API_KEY", "value": OTHER_SECRET, "secret": true,
                "environments": ["production"], "all_projects": true, "projects": [] }),
    )
    .await;
    let (value, bytes): (Option<String>, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT value, value_sealed FROM global_variables WHERE key = 'RESEND_API_KEY'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(value, None);
    let bytes = bytes.unwrap();
    assert!(
        !bytes
            .windows(OTHER_SECRET.len())
            .any(|w| w == OTHER_SECRET.as_bytes())
    );
    assert_eq!(clear(&state, &bytes).as_slice(), OTHER_SECRET.as_bytes());
}

#[tokio::test]
async fn empty_secret_update_keeps_previous() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_KEY", "value": SECRET, "secret": true }),
    )
    .await;
    let open = |b: Option<Vec<u8>>| clear(&state, &b.unwrap());

    // « Laisser vide pour conserver » : valeur vide ou absente.
    for body in [
        json!({ "key": "STRIPE_KEY", "value": "", "secret": true }),
        json!({ "key": "STRIPE_KEY", "secret": true }),
    ] {
        let r = put_var(&state, &dev, project, body).await;
        assert_eq!(r.status, 200, "{}", r.raw);
        assert_eq!(
            open(sealed(&state, project, "STRIPE_KEY").await.1),
            SECRET.as_bytes()
        );
    }
    // Renommage : la valeur suit.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_SECRET_KEY", "value": "", "secret": true, "previous_key": "STRIPE_KEY" }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        open(sealed(&state, project, "STRIPE_SECRET_KEY").await.1),
        SECRET.as_bytes()
    );
    // Mode .env : `CLE=` garde le secret, les cles absentes sont retirees.
    let r = send(
        &state,
        &dev,
        "PUT",
        &format!("/api/projects/{project}/variables/bulk?env=production"),
        Some(json!({ "content": "STRIPE_SECRET_KEY=  # secret\nPORT=3000\nQUOTED=\"a b\"\n" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!r.raw.contains(SECRET));
    assert_eq!(
        open(sealed(&state, project, "STRIPE_SECRET_KEY").await.1),
        SECRET.as_bytes()
    );
    let rows = list(&state, &dev, project).await;
    assert_eq!(row(&rows, "PORT")["value"], "3000");
    assert_eq!(row(&rows, "QUOTED")["value"], "a b");
    assert_eq!(row(&rows, "STRIPE_SECRET_KEY")["secret"], true);
    // Un secret ne devient jamais public sans nouvelle valeur.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_SECRET_KEY", "value": "", "secret": false }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Globale : PATCH sans valeur garde le secret.
    let admin = login(&state, "admin").await;
    let g = create_global(
        &state,
        &admin,
        json!({ "key": "RESEND_API_KEY", "value": OTHER_SECRET, "secret": true,
                "environments": ["production"], "all_projects": true, "projects": [] }),
    )
    .await;
    let r = send(
        &state,
        &admin,
        "PATCH",
        &format!("/api/variables/{}", g["id"].as_str().unwrap()),
        Some(json!({ "value": "", "environments": ["production", "staging"] })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT value_sealed FROM global_variables WHERE key = 'RESEND_API_KEY'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(clear(&state, &bytes).as_slice(), OTHER_SECRET.as_bytes());
    assert_eq!(r.json()["environments"], json!(["production", "staging"]));
}

#[tokio::test]
async fn export_excludes_secrets() {
    let (state, _pg) = common::state().await;
    let admin = login(&state, "admin").await;
    for body in [
        json!({ "key": "SENTRY_DSN", "value": "https://k@o1.ingest.sentry.io/7", "secret": false,
                "environments": ["production", "staging"], "all_projects": true, "projects": [] }),
        json!({ "key": "RESEND_API_KEY", "value": OTHER_SECRET, "secret": true,
                "environments": ["production"], "all_projects": true, "projects": [] }),
        json!({ "key": "LOG_LEVEL", "value": "debug", "secret": false,
                "environments": ["staging"], "all_projects": true, "projects": [] }),
    ] {
        create_global(&state, &admin, body).await;
    }
    let r = send(&state, &admin, "GET", "/api/variables/export", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(
        r.raw
            .contains("SENTRY_DSN=https://k@o1.ingest.sentry.io/7\n"),
        "{}",
        r.raw
    );
    assert!(r.raw.contains("LOG_LEVEL=debug\n"));
    assert!(
        !r.raw.contains("RESEND_API_KEY") && !r.raw.contains(OTHER_SECRET),
        "{}",
        r.raw
    );
    assert!(
        r.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("ciphera-globales.env")
    );
    let prod = send(
        &state,
        &admin,
        "GET",
        "/api/variables/export?env=production",
        None,
    )
    .await;
    assert_eq!(prod.status, 200);
    assert!(prod.raw.contains("SENTRY_DSN=") && !prod.raw.contains("LOG_LEVEL"));
    assert!(!prod.raw.contains("RESEND_API_KEY"));
    // Export reserve aux administrateurs.
    let dev = login(&state, "developer").await;
    assert_eq!(
        send(&state, &dev, "GET", "/api/variables/export", None)
            .await
            .status,
        403
    );
}

#[tokio::test]
async fn invalid_key_422() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let long = format!("A{}", "B".repeat(128));
    for key in ["lower", "1ABC", "A-B", "", long.as_str()] {
        let r = put_var(
            &state,
            &dev,
            project,
            json!({ "key": key, "value": "x", "secret": false }),
        )
        .await;
        assert_eq!(r.status, 422, "{key}: {}", r.raw);
    }
    let ok = format!("_{}", "A".repeat(127));
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": ok, "value": "x", "secret": false }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Valeur au plus 32 Kio.
    let big = "x".repeat(32 * 1024 + 1);
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "BIG", "value": big, "secret": true }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "BIG", "value": "x".repeat(32 * 1024), "secret": false }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Mode .env : la ligne fautive est citee, rien n'est enregistre.
    let r = send(
        &state,
        &dev,
        "PUT",
        &format!("/api/projects/{project}/variables/bulk?env=production"),
        Some(json!({ "content": "GOOD=1\nbad-key=2\n" })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert!(r.raw.contains("2"), "{}", r.raw);
    assert!(
        list(&state, &dev, project)
            .await
            .iter()
            .all(|r| r["key"] != "GOOD")
    );
    // Globale.
    let admin = login(&state, "admin").await;
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/variables",
        Some(json!({ "key": "no", "value": "x", "secret": false,
                     "environments": ["production"], "all_projects": true, "projects": [] })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Environnement inconnu du projet.
    let r = send(
        &state,
        &dev,
        "GET",
        &format!("/api/projects/{project}/variables?env=staging"),
        None,
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
}

#[tokio::test]
async fn variables_follow_the_role_matrix_and_csrf() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let path = format!("/api/projects/{project}/variables?env=production");
    let body = json!({ "key": "PORT", "value": "3000", "secret": false });
    let viewer = login(&state, "viewer").await;
    assert_eq!(send(&state, &viewer, "GET", &path, None).await.status, 403);
    let ops = login(&state, "ops").await;
    assert_eq!(send(&state, &ops, "GET", &path, None).await.status, 200);
    assert_eq!(
        send(&state, &ops, "PUT", &path, Some(body.clone()))
            .await
            .status,
        403
    );
    let dev = login(&state, "developer").await;
    // Sans jeton CSRF : refuse.
    let r = call(
        &state,
        "PUT",
        &path,
        Req {
            cookie: Some(&dev.0),
            body: Some(body.clone()),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(
        send(&state, &dev, "PUT", &path, Some(body)).await.status,
        200
    );
    let r = send(
        &state,
        &dev,
        "DELETE",
        &format!("/api/projects/{project}/variables/PORT?env=production"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    // Globales : administrateurs seulement.
    assert_eq!(
        send(&state, &dev, "GET", "/api/variables", None)
            .await
            .status,
        403
    );
    let r = send(
        &state,
        &dev,
        "POST",
        "/api/variables",
        Some(json!({ "key": "A", "value": "1", "secret": false,
                     "environments": ["production"], "all_projects": true, "projects": [] })),
    )
    .await;
    assert_eq!(r.status, 403);
    let admin = login(&state, "admin").await;
    assert_eq!(
        send(&state, &admin, "GET", "/api/variables", None)
            .await
            .status,
        200
    );
}

// --- deploiement --------------------------------------------------------------

struct Agent {
    server: Uuid,
    rx: mpsc::Receiver<ControlToAgent>,
}

async fn attach_agent(state: &AppState) -> Agent {
    let server = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', 'local', 'AA:BB')",
    )
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();
    let rx = state.agents.attach_channel(ServerId(server));
    Agent { server, rx }
}

impl Agent {
    fn orders(&mut self) -> Vec<ControlToAgent> {
        let mut out = Vec::new();
        while let Ok(m) = self.rx.try_recv() {
            out.push(m);
        }
        out
    }

    async fn progress(&self, state: &AppState, id: Uuid, stage: Stage, status: StageStatus) {
        orchestrator::on_agent_message(
            state,
            ServerId(self.server),
            AgentToControl::DeployProgress {
                deployment_id: DeploymentId(id),
                stage,
                status,
            },
        )
        .await;
    }
}

async fn run_jobs(state: &AppState) {
    while let Some(job) = jobs::claim(&state.db, &state.clock, "test").await.unwrap() {
        let id = job.id;
        match job.kind {
            JobKind::Deploy => orchestrator::handle_deploy(state, job).await.unwrap(),
            JobKind::Cleanup => retention::handle_cleanup(state, job).await.unwrap(),
            other => panic!("tache inattendue : {other}"),
        }
        jobs::complete(&state.db, &state.clock, id, "test")
            .await
            .unwrap();
    }
}

/// Deploiement complet (build puis deploiement simules) ; renvoie l'ordre
/// `Deploy` recu par l'agent.
async fn deploy(state: &AppState, agent: &mut Agent, project: Uuid) -> DeploySpec {
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
    let id = Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap();
    run_jobs(state).await;
    agent.orders();
    agent
        .progress(state, id, Stage::Image, StageStatus::Started)
        .await;
    agent
        .progress(state, id, Stage::Image, StageStatus::Succeeded)
        .await;
    let spec = agent
        .orders()
        .into_iter()
        .find_map(|o| match o {
            ControlToAgent::Deploy(spec) => Some(spec),
            _ => None,
        })
        .expect("ordre Deploy");
    for stage in [Stage::Start, Stage::Healthcheck, Stage::Switch] {
        agent.progress(state, id, stage, StageStatus::Started).await;
        agent
            .progress(state, id, stage, StageStatus::Succeeded)
            .await;
    }
    spec
}

fn env_of(spec: &DeploySpec) -> Vec<(String, String)> {
    spec.env
        .iter()
        .map(|v| (v.key.clone(), v.value.expose().to_string()))
        .collect()
}

#[tokio::test]
async fn project_overrides_global() {
    let (state, _pg) = common::state().await;
    let mut agent = attach_agent(&state).await;
    let project = image_project(&state, "web").await;
    let other = image_project(&state, "billing").await;
    let admin = login(&state, "admin").await;
    let dev = login(&state, "developer").await;
    for body in [
        json!({ "key": "LOG_LEVEL", "value": "info", "secret": false,
                "environments": ["production", "staging"], "all_projects": true, "projects": [] }),
        json!({ "key": "SENTRY_DSN", "value": "https://k@sentry/1", "secret": false,
                "environments": ["production"], "all_projects": true, "projects": [] }),
        json!({ "key": "RESEND_API_KEY", "value": OTHER_SECRET, "secret": true,
                "environments": ["production"], "all_projects": false, "projects": [project.to_string()] }),
        json!({ "key": "BILLING_ONLY", "value": "1", "secret": false,
                "environments": ["production"], "all_projects": false, "projects": [other.to_string()] }),
        json!({ "key": "STAGING_ONLY", "value": "1", "secret": false,
                "environments": ["staging"], "all_projects": true, "projects": [] }),
    ] {
        create_global(&state, &admin, body).await;
    }
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "LOG_LEVEL", "value": "debug", "secret": false }),
    )
    .await;
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_KEY", "value": SECRET, "secret": true }),
    )
    .await;

    let rows = list(&state, &dev, project).await;
    let keys: Vec<&str> = rows.iter().map(|r| r["key"].as_str().unwrap()).collect();
    assert_eq!(
        keys.iter().filter(|k| **k == "LOG_LEVEL").count(),
        1,
        "{keys:?}"
    );
    assert!(
        !keys.contains(&"BILLING_ONLY") && !keys.contains(&"STAGING_ONLY"),
        "{keys:?}"
    );
    let log = row(&rows, "LOG_LEVEL");
    assert_eq!(
        (log["value"].as_str(), log["inherited"].as_bool()),
        (Some("debug"), Some(false))
    );
    assert_eq!(row(&rows, "SENTRY_DSN")["inherited"], true);
    assert_eq!(row(&rows, "SENTRY_DSN")["value"], "https://k@sentry/1");
    assert_eq!(row(&rows, "RESEND_API_KEY")["inherited"], true);
    // Rien n'est encore deploye : tout est a appliquer.
    assert!(rows.iter().all(|r| r["pending_apply"] == true), "{rows:?}");

    // Injection a l'envoi, secrets dechiffres, le projet l'emporte.
    let spec = deploy(&state, &mut agent, project).await;
    let mut env = env_of(&spec);
    env.sort();
    assert_eq!(
        env,
        vec![
            ("LOG_LEVEL".to_string(), "debug".to_string()),
            ("RESEND_API_KEY".to_string(), OTHER_SECRET.to_string()),
            ("SENTRY_DSN".to_string(), "https://k@sentry/1".to_string()),
            ("STRIPE_KEY".to_string(), SECRET.to_string()),
        ]
    );
    // Les valeurs ne passent pas par la base des deploiements.
    let stored: Vec<String> = sqlx::query_scalar("SELECT row_to_json(d)::text FROM deployments d")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert!(
        stored
            .iter()
            .all(|d| !d.contains(SECRET) && !d.contains(OTHER_SECRET))
    );

    // Deploye : plus rien en attente ; une modification l'est de nouveau.
    let rows = list(&state, &dev, project).await;
    assert!(rows.iter().all(|r| r["pending_apply"] == false), "{rows:?}");
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "PORT", "value": "3000", "secret": false }),
    )
    .await;
    let rows = list(&state, &dev, project).await;
    assert_eq!(row(&rows, "PORT")["pending_apply"], true);
    assert_eq!(row(&rows, "LOG_LEVEL")["pending_apply"], false);

    // L'autre projet herite de sa propre selection.
    let spec = deploy(&state, &mut agent, other).await;
    let keys: Vec<String> = env_of(&spec).into_iter().map(|(k, _)| k).collect();
    assert!(
        keys.contains(&"BILLING_ONLY".to_string()) && !keys.contains(&"RESEND_API_KEY".to_string())
    );
}

#[tokio::test]
async fn secret_added_mid_stream_is_masked() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let admin = login(&state, "admin").await;
    let target = LogTarget::Deployment(DeploymentId::new());
    let mut sub = state.logs.subscribe(
        Source {
            target,
            project,
            container: "build".into(),
            follow: None,
            origin: None,
        },
        0,
    );
    let line = |text: &str| {
        AgentToControl::LogLine(LogLine {
            target,
            stream: LogStream::Stdout,
            timestamp_ms: 1,
            line: text.to_string(),
        })
    };
    let server = ServerId(Uuid::new_v4());
    state.logs.assign_build(target, server);
    state
        .logs
        .dispatch(server, &line(&format!("avant {SECRET}")))
        .await;
    assert_eq!(sub.next().await.unwrap().message, format!("avant {SECRET}"));

    // Secret du projet ajoute pendant le flux : masque des la ligne suivante.
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "STRIPE_KEY", "value": SECRET, "secret": true }),
    )
    .await;
    state
        .logs
        .dispatch(server, &line(&format!("apres {SECRET}")))
        .await;
    assert_eq!(sub.next().await.unwrap().message, format!("apres {MASK}"));

    // Globale secrete qui s'applique au projet : de meme.
    create_global(
        &state,
        &admin,
        json!({ "key": "RESEND_API_KEY", "value": OTHER_SECRET, "secret": true,
                "environments": ["production"], "all_projects": true, "projects": [] }),
    )
    .await;
    state
        .logs
        .dispatch(server, &line(&format!("cle {OTHER_SECRET}")))
        .await;
    assert_eq!(sub.next().await.unwrap().message, format!("cle {MASK}"));
}

async fn pending(state: &AppState, s: &Session, project: Uuid) -> bool {
    let r = send(
        state,
        s,
        "GET",
        &format!("/api/projects/{project}/variables/status?env=production"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    r.json()["pending_apply"].as_bool().unwrap()
}

#[tokio::test]
async fn deletions_and_narrowed_scope_mark_pending() {
    let (state, _pg) = common::state().await;
    let mut agent = attach_agent(&state).await;
    let project = image_project(&state, "web").await;
    let other = image_project(&state, "billing").await;
    let admin = login(&state, "admin").await;
    let dev = login(&state, "developer").await;
    let ops = login(&state, "ops").await;
    for key in ["A_KEY", "B_KEY"] {
        put_var(
            &state,
            &dev,
            project,
            json!({ "key": key, "value": "1", "secret": false }),
        )
        .await;
    }
    let all = create_global(
        &state,
        &admin,
        json!({ "key": "G_ALL", "value": "1", "secret": false,
                "environments": ["production"], "all_projects": true, "projects": [] }),
    )
    .await;
    let scoped = create_global(
        &state,
        &admin,
        json!({ "key": "G_SCOPED", "value": "1", "secret": false,
                "environments": ["production"], "all_projects": false,
                "projects": [project.to_string()] }),
    )
    .await;
    assert!(pending(&state, &ops, project).await);
    deploy(&state, &mut agent, project).await;
    assert!(!pending(&state, &ops, project).await);

    // Suppression d'une variable du projet.
    let r = send(
        &state,
        &dev,
        "DELETE",
        &format!("/api/projects/{project}/variables/A_KEY?env=production"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let rows = list(&state, &dev, project).await;
    assert!(rows.iter().all(|r| r["pending_apply"] == false), "{rows:?}");
    assert!(pending(&state, &ops, project).await);
    deploy(&state, &mut agent, project).await;
    assert!(!pending(&state, &ops, project).await);

    // Retrait par le mode .env ; un contenu identique ne change rien.
    let bulk = |content: &str| json!({ "content": content });
    let r = send(
        &state,
        &dev,
        "PUT",
        &format!("/api/projects/{project}/variables/bulk?env=production"),
        Some(bulk("B_KEY=1\n")),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!pending(&state, &ops, project).await);
    let r = send(
        &state,
        &dev,
        "PUT",
        &format!("/api/projects/{project}/variables/bulk?env=production"),
        Some(bulk("")),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(pending(&state, &ops, project).await);
    deploy(&state, &mut agent, project).await;
    assert!(!pending(&state, &ops, project).await);

    // Globale supprimee.
    let r = send(
        &state,
        &admin,
        "DELETE",
        &format!("/api/variables/{}", all["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert!(pending(&state, &ops, project).await);
    assert!(pending(&state, &ops, other).await);
    deploy(&state, &mut agent, project).await;
    assert!(!pending(&state, &ops, project).await);

    // Portee d'une globale reduite : le projet retire est concerne.
    let r = send(
        &state,
        &admin,
        "PATCH",
        &format!("/api/variables/{}", scoped["id"].as_str().unwrap()),
        Some(json!({ "all_projects": false, "projects": [other.to_string()] })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let rows = list(&state, &dev, project).await;
    assert!(rows.iter().all(|r| r["key"] != "G_SCOPED"), "{rows:?}");
    assert!(pending(&state, &ops, project).await);
    let spec = deploy(&state, &mut agent, project).await;
    assert!(env_of(&spec).iter().all(|(k, _)| k != "G_SCOPED"));
    assert!(!pending(&state, &ops, project).await);
}

#[tokio::test]
async fn change_between_resolve_and_send_stays_pending() {
    let (state, _pg) = common::state().await;
    let mut agent = attach_agent(&state).await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "PORT", "value": "3000", "secret": false }),
    )
    .await;
    deploy(&state, &mut agent, project).await;
    // L'heure de lecture des variables est notee a l'envoi.
    let (resolved, sent): (Option<f64>, Option<f64>) = sqlx::query_as(
        "SELECT extract(epoch FROM variables_resolved_at)::float8, \
                extract(epoch FROM deploy_sent_at)::float8 \
         FROM deployments WHERE project_id = $1",
    )
    .bind(project)
    .fetch_one(&state.db)
    .await
    .unwrap();
    let (resolved, sent) = (resolved.unwrap(), sent.unwrap());
    assert!(resolved <= sent);
    // Modification arrivee entre la lecture et l'envoi : pas appliquee.
    sqlx::query(
        "UPDATE deployments SET variables_resolved_at = variables_resolved_at - interval '10 seconds' \
         WHERE project_id = $1",
    )
    .bind(project)
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE project_variables SET updated_at = d.variables_resolved_at + interval '5 seconds' \
         FROM deployments d WHERE d.project_id = $1",
    )
    .bind(project)
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE environments SET variables_changed_at = d.variables_resolved_at + interval '5 seconds' \
         FROM deployments d WHERE d.project_id = $1 AND environments.id = d.environment_id",
    )
    .bind(project)
    .execute(&state.db)
    .await
    .unwrap();
    let rows = list(&state, &dev, project).await;
    assert_eq!(row(&rows, "PORT")["pending_apply"], true);
    assert!(pending(&state, &dev, project).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_committed_during_resolution_is_sent_or_pending() {
    let (state, _pg) = common::state().await;
    let mut agent = attach_agent(&state).await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    put_var(
        &state,
        &dev,
        project,
        json!({ "key": "PORT", "value": "3000", "secret": false }),
    )
    .await;
    let r = send(
        &state,
        &dev,
        "POST",
        &format!("/api/projects/{project}/deployments"),
        Some(json!({ "environment": "production" })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let id = Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap();
    run_jobs(&state).await;
    agent
        .progress(&state, id, Stage::Image, StageStatus::Started)
        .await;
    agent.orders();

    // Ecriture en cours : environnement verrouille, horodatee (`now()` du
    // debut de sa transaction) AVANT la lecture des variables par l'envoi.
    // `FOR NO KEY UPDATE` : le verrou le plus faible qui exclut `FOR SHARE`
    // (le `FOR UPDATE` des ecritures reelles bloque aussi, par ailleurs, la
    // mise a jour du deploiement sur ce chemin ; le test vise le verrou de
    // `send_deploy` seul).
    let mut writer = state.db.begin().await.unwrap();
    let environment: Uuid = sqlx::query_scalar(
        "SELECT id FROM environments WHERE project_id = $1 AND name = 'production' FOR NO KEY UPDATE",
    )
    .bind(project)
    .fetch_one(&mut *writer)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE project_variables SET value = '4000', updated_at = now() WHERE environment_id = $1",
    )
    .bind(environment)
    .execute(&mut *writer)
    .await
    .unwrap();
    sqlx::query("UPDATE environments SET variables_changed_at = now() WHERE id = $1")
        .bind(environment)
        .execute(&mut *writer)
        .await
        .unwrap();

    // Image prete : l'envoi attend la fin de l'ecriture.
    let sender = {
        let state = state.clone();
        let server = ServerId(agent.server);
        tokio::spawn(async move {
            orchestrator::on_agent_message(
                &state,
                server,
                AgentToControl::DeployProgress {
                    deployment_id: DeploymentId(id),
                    stage: Stage::Image,
                    status: StageStatus::Succeeded,
                },
            )
            .await;
        })
    };
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        !agent
            .orders()
            .iter()
            .any(|o| matches!(o, ControlToAgent::Deploy(_))),
        "Deploy envoye pendant une ecriture de variables"
    );
    writer.commit().await.unwrap();
    sender.await.unwrap();
    let spec = agent
        .orders()
        .into_iter()
        .find_map(|o| match o {
            ControlToAgent::Deploy(spec) => Some(spec),
            _ => None,
        })
        .expect("ordre Deploy");
    // La valeur ecrite est celle envoyee : « appliquee » a raison.
    assert_eq!(
        env_of(&spec),
        vec![("PORT".to_string(), "4000".to_string())]
    );
    for stage in [Stage::Start, Stage::Healthcheck, Stage::Switch] {
        agent
            .progress(&state, id, stage, StageStatus::Started)
            .await;
        agent
            .progress(&state, id, stage, StageStatus::Succeeded)
            .await;
    }
    assert!(!pending(&state, &dev, project).await);
}

/// Environnement `production` d'un projet : (identifiant, nom).
async fn production(state: &AppState, project: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1 AND name = 'production'")
        .bind(project)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

/// Valeurs resolues pour un deploiement de la production du projet, ou la
/// cle du secret illisible.
async fn resolved(state: &AppState, project: Uuid) -> Result<Vec<(String, String)>, String> {
    use control_plane::domain::variables::{self, ResolveError};
    let env = production(state, project).await;
    let mut conn = state.db.acquire().await.unwrap();
    match variables::resolve_env(&mut conn, &state.secrets, project, env, "production").await {
        Ok(vars) => Ok(vars
            .iter()
            .map(|v| (v.key.clone(), v.value.expose().to_string()))
            .collect()),
        Err(ResolveError::Unreadable(key)) => Err(key),
        Err(e) => panic!("{e}"),
    }
}

/// Un secret chiffre est lie a sa ligne (environnement et cle, ou globale) :
/// copie dans une autre ligne, il ne s'ouvre plus. Renomme par l'API, il
/// est rechiffre pour sa nouvelle cle.
#[tokio::test]
async fn sealed_secret_is_bound_to_its_row() {
    let (state, _pg) = common::state().await;
    let web = image_project(&state, "web").await;
    let billing = image_project(&state, "billing").await;
    let dev = login(&state, "developer").await;
    for (project, value) in [(web, SECRET), (billing, OTHER_SECRET)] {
        let r = put_var(
            &state,
            &dev,
            project,
            json!({ "key": "STRIPE_KEY", "value": value, "secret": true }),
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.raw);
    }
    // Rename sans nouvelle valeur : valeur gardee, rechiffree.
    let r = put_var(
        &state,
        &dev,
        web,
        json!({ "key": "STRIPE_SECRET", "previous_key": "STRIPE_KEY", "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        resolved(&state, web).await.unwrap(),
        vec![("STRIPE_SECRET".to_string(), SECRET.to_string())]
    );
    // Chiffre de web copie dans la ligne de billing : refuse.
    let (_, stolen) = sealed(&state, web, "STRIPE_SECRET").await;
    sqlx::query(
        "UPDATE project_variables SET value_sealed = $2 \
         WHERE environment_id = $1 AND key = 'STRIPE_KEY'",
    )
    .bind(production(&state, billing).await)
    .bind(stolen.unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    assert_eq!(
        resolved(&state, billing).await,
        Err("STRIPE_KEY".to_string())
    );
    // Meme cle, meme environnement, autre nom : refuse aussi.
    let (_, own) = sealed(&state, web, "STRIPE_SECRET").await;
    let r = put_var(
        &state,
        &dev,
        web,
        json!({ "key": "OTHER_KEY", "value": "x", "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    sqlx::query(
        "UPDATE project_variables SET value_sealed = $2 \
         WHERE environment_id = $1 AND key = 'OTHER_KEY'",
    )
    .bind(production(&state, web).await)
    .bind(own.unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    assert_eq!(resolved(&state, web).await, Err("OTHER_KEY".to_string()));

    // Globale : liee a son identifiant.
    let admin = login(&state, "admin").await;
    let mut ids = Vec::new();
    for (key, value) in [("GLOBAL_A", SECRET), ("GLOBAL_B", OTHER_SECRET)] {
        let g = create_global(
            &state,
            &admin,
            json!({ "key": key, "value": value, "secret": true,
                    "environments": ["production"], "all_projects": false,
                    "projects": [billing.to_string()] }),
        )
        .await;
        ids.push(Uuid::parse_str(g["id"].as_str().unwrap()).unwrap());
    }
    sqlx::query("DELETE FROM project_variables")
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE global_variables SET value_sealed = \
         (SELECT value_sealed FROM global_variables WHERE id = $1) WHERE id = $2",
    )
    .bind(ids[0])
    .bind(ids[1])
    .execute(&state.db)
    .await
    .unwrap();
    assert_eq!(resolved(&state, billing).await, Err("GLOBAL_B".to_string()));
}

/// Chiffres d'avant la liaison (age seul) : rechiffres lies a leur ligne une
/// seule fois au demarrage ; un chiffre non lie introduit ensuite est refuse.
#[tokio::test]
async fn legacy_ciphertexts_are_rebound_once() {
    use control_plane::domain::variables;
    let (state, _pg) = common::state().await;
    let web = image_project(&state, "web").await;
    let env = production(&state, web).await;
    let global = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO project_variables (id, environment_id, key, secret, value_sealed, updated_at) \
         VALUES ($1, $2, 'OLD_SECRET', true, $3, now())",
    )
    .bind(Uuid::new_v4())
    .bind(env)
    .bind(state.secrets.seal(SECRET.as_bytes()).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO global_variables (id, key, secret, value_sealed, environments, all_projects, updated_at) \
         VALUES ($1, 'OLD_GLOBAL', true, $2, '{production}', true, now())",
    )
    .bind(global)
    .bind(state.secrets.seal(OTHER_SECRET.as_bytes()).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    assert_eq!(
        variables::rebind_all(&state.db, &state.secrets)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        resolved(&state, web).await.unwrap(),
        vec![
            ("OLD_GLOBAL".to_string(), OTHER_SECRET.to_string()),
            ("OLD_SECRET".to_string(), SECRET.to_string()),
        ]
    );
    // Deja fait : un chiffre non lie introduit ensuite n'est pas rechiffre.
    sqlx::query("UPDATE project_variables SET value_sealed = $1 WHERE key = 'OLD_SECRET'")
        .bind(state.secrets.seal(SECRET.as_bytes()).unwrap())
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(
        variables::rebind_all(&state.db, &state.secrets)
            .await
            .unwrap(),
        0
    );
    assert_eq!(resolved(&state, web).await, Err("OLD_SECRET".to_string()));
}

async fn bulk(state: &AppState, s: &Session, project: Uuid, content: &str) -> Reply {
    send(
        state,
        s,
        "PUT",
        &format!("/api/projects/{project}/variables/bulk?env=production"),
        Some(json!({ "content": content })),
    )
    .await
}

#[tokio::test]
async fn import_secret_flag_on_existing_public_key_seals() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "API_KEY", "value": "public-value", "secret": false }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // `# secret` sur une cle deja publique : scellee, jamais renvoyee.
    let r = bulk(
        &state,
        &dev,
        project,
        &format!("API_KEY={SECRET} # secret\n"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!r.raw.contains(SECRET));
    let rows = list(&state, &dev, project).await;
    assert_eq!(row(&rows, "API_KEY")["secret"], true);
    assert_eq!(row(&rows, "API_KEY")["value"], Value::Null);
    let (value, bytes) = sealed(&state, project, "API_KEY").await;
    assert_eq!(value, None);
    assert_eq!(clear(&state, &bytes.unwrap()).as_slice(), SECRET.as_bytes());
    // Un secret existant reste secret sans le commentaire.
    let r = bulk(&state, &dev, project, "API_KEY=autre-valeur-secrete\n").await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!r.raw.contains("autre-valeur-secrete"));
    assert_eq!(sealed(&state, project, "API_KEY").await.0, None);
}

/// Secret de plusieurs lignes : la valeur entiere et chacune de ses lignes.
fn multi_line_secret(lines: usize, tag: &str) -> String {
    (0..lines)
        .map(|i| format!("{tag}-{i:05}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn fragment_cap_refused() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let admin = login(&state, "admin").await;
    // 999 lignes + la valeur entiere : 1 000 fragments, accepte.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "BIG_KEY", "value": multi_line_secret(999, "ligne"), "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Un de plus : refuse, rien n'est ecrit.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "EXTRA", "value": "encore-un-secret", "secret": true }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert!(r.raw.contains("1000"), "{}", r.raw);
    assert!(
        list(&state, &dev, project)
            .await
            .iter()
            .all(|v| v["key"] != "EXTRA")
    );
    // Import `.env` : meme plafond.
    let r = bulk(
        &state,
        &dev,
        project,
        "BIG_KEY=\nEXTRA=encore-un-secret # secret\n",
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Une globale secrete qui s'applique au projet compte aussi.
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/variables",
        Some(
            json!({ "key": "GLOBAL_SECRET", "value": "globale-secrete", "secret": true,
                     "environments": ["production"], "all_projects": true, "projects": [] }),
        ),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM global_variables")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    // Une valeur publique ne compte pas.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "PUBLIC", "value": "valeur-publique", "secret": false }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn env_bytes_cap_refused_at_write() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let big = "a".repeat(vars_max_value());
    for i in 0..31 {
        let r = put_var(
            &state,
            &dev,
            project,
            json!({ "key": format!("K{i:02}"), "value": big, "secret": false }),
        )
        .await;
        assert_eq!(r.status, 200, "{}", &r.raw[..r.raw.len().min(200)]);
    }
    // Au-dela de 1 Mio pour l'environnement : refuse des l'ecriture.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "K31", "value": big, "secret": true }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", &r.raw[..r.raw.len().min(200)]);
    assert!(r.raw.contains("1 Mio"), "{}", r.raw);
    let exists: Option<bool> =
        sqlx::query_scalar("SELECT true FROM project_variables WHERE key = 'K31'")
            .fetch_optional(&state.db)
            .await
            .unwrap();
    assert_eq!(exists, None);
}

fn vars_max_value() -> usize {
    control_plane::domain::variables::MAX_VALUE_BYTES
}

#[tokio::test]
async fn short_secret_warning() {
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let admin = login(&state, "admin").await;
    let warned =
        |rows: &[Value], key: &str| row(rows, key)["warnings"] == json!(["too_short_to_redact"]);

    // Secret de moins de 4 caracteres : accepte, avec avertissement.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "PIN", "value": "123", "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let rows = r.json().as_array().unwrap().clone();
    assert!(warned(&rows, "PIN"), "{rows:?}");
    assert_eq!(row(&rows, "PIN")["value"], Value::Null);
    // Ni pour une valeur publique, ni pour un secret assez long, ni a la lecture.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "SHORT", "value": "ab", "secret": false }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(
        row(r.json().as_array().unwrap(), "SHORT")
            .get("warnings")
            .is_none()
    );
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "LONG", "value": "abcd", "secret": true }),
    )
    .await;
    assert!(
        row(r.json().as_array().unwrap(), "LONG")
            .get("warnings")
            .is_none()
    );
    let rows = list(&state, &dev, project).await;
    assert!(rows.iter().all(|v| v.get("warnings").is_none()));
    // Import `.env`.
    let r = bulk(
        &state,
        &dev,
        project,
        "PIN=\nSHORT=ab\nLONG=\nCODE=xy # secret\n",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let rows = r.json().as_array().unwrap().clone();
    assert!(warned(&rows, "CODE"), "{rows:?}");
    assert!(row(&rows, "PIN").get("warnings").is_none());
    // Globale.
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/variables",
        Some(json!({ "key": "G_PIN", "value": "42", "secret": true,
                     "environments": ["production"], "all_projects": true, "projects": [] })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["warnings"], json!(["too_short_to_redact"]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_writes_cannot_exceed_caps() {
    use control_plane::domain::variables::{self as vars, Wanted};
    let (state, _pg) = common::state().await;
    let project = image_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    // 998 lignes + la valeur entiere : 999 fragments.
    let r = put_var(
        &state,
        &dev,
        project,
        json!({ "key": "BIG_KEY", "value": multi_line_secret(998, "ligne"), "secret": true }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let staging = Uuid::new_v4();
    sqlx::query("INSERT INTO environments (id, project_id, name) VALUES ($1, $2, 'staging')")
        .bind(staging)
        .bind(project)
        .execute(&state.db)
        .await
        .unwrap();
    let actor: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE role = 'developer' LIMIT 1")
        .fetch_one(&state.db)
        .await
        .unwrap();

    // Ecriture A (staging) : le 1 000e fragment, verifiee, pas encore validee.
    let mut tx = state.db.begin().await.unwrap();
    vars::lock_environment(&mut tx, staging).await.unwrap();
    let written = vars::put(
        &mut tx,
        &state.secrets,
        staging,
        "EXTRA_A",
        None,
        Wanted {
            value: Some("secret-de-staging"),
            secret: true,
        },
        actor,
        state.clock.unix_secs(),
    )
    .await
    .unwrap();
    assert!(written.changed);
    vars::check_caps(
        &mut tx,
        &state.secrets,
        project,
        &[(staging, "staging".into())],
    )
    .await
    .unwrap();

    // Ecriture B (production, autre environnement) : attend la fin de A.
    let (state_b, dev_b) = (state.clone(), dev.clone());
    let b = tokio::spawn(async move {
        put_var(
            &state_b,
            &dev_b,
            project,
            json!({ "key": "EXTRA_B", "value": "secret-de-production", "secret": true }),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!b.is_finished(), "la verification de B n'a pas attendu A");
    tx.commit().await.unwrap();
    // B voit A validee : 1 001 fragments, refusee.
    let r = b.await.unwrap();
    assert_eq!(r.status, 422, "{}", r.raw);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project_variables v JOIN environments e ON e.id = v.environment_id \
         WHERE e.project_id = $1 AND v.secret",
    )
    .bind(project)
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(count, 2);
}
