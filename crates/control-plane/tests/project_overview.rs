//! Apercu et services d'un projet : `GET /api/projects/{id}/overview`,
//! `GET /api/projects/{id}/services`, `POST .../services/{sid}/{restart,stop}`.

mod common;

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::state::AppState;
use proto::ControlToAgent;
use serde_json::{Value, json};
use shared::ids::ServerId;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

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
    csrf: bool,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&session.0),
            csrf: csrf.then_some(session.1.as_str()),
            ..Req::default()
        },
    )
    .await
}

/// Projet Git `ciphera/web` ; renvoie (projet, environnement production, service).
async fn git_project(state: &AppState, name: &str) -> (Uuid, Uuid, Uuid) {
    let dev = login(state, "developer").await;
    let r = call(
        state,
        "POST",
        "/api/projects",
        Req {
            cookie: Some(&dev.0),
            csrf: Some(&dev.1),
            body: Some(json!({
                "name": name,
                "source": { "type": "git", "repository": "ciphera/web", "branch": "main" },
                "build": { "kind": "railpack" }
            })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let p = r.json();
    let id = |v: &Value| Uuid::parse_str(v.as_str().unwrap()).unwrap();
    (
        id(&p["id"]),
        id(&p["environments"][0]["id"]),
        id(&p["services"][0]["id"]),
    )
}

async fn server(state: &AppState) -> Uuid {
    let server = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', 'local', 'AA:BB')",
    )
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();
    server
}

/// Deploiement termine (`success`/`failed`) ou actif (`queued`/`running`).
async fn deployment(
    state: &AppState,
    project: Uuid,
    env: Uuid,
    server: Uuid,
    status: &str,
    age_secs: i64,
) -> Uuid {
    let id = Uuid::new_v4();
    let finished = matches!(status, "success" | "failed");
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, status, trigger, \
         commit_sha, branch, image, created_at, started_at, finished_at) \
         VALUES ($1, $2, $3, $4, $5, 'push', $6, 'main', 'ciphera/web-production:1', \
         now() - make_interval(secs => $7), now() - make_interval(secs => $7), \
         CASE WHEN $8 THEN now() - make_interval(secs => $7) + interval '30 seconds' END)",
    )
    .bind(id)
    .bind(project)
    .bind(env)
    .bind(server)
    .bind(status)
    .bind(COMMIT)
    .bind(age_secs as f64)
    .bind(finished)
    .execute(&state.db)
    .await
    .unwrap();
    id
}

async fn count(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn overview_shape_matches_screen() {
    let (state, _pg) = common::state().await;
    let (project, env, service) = git_project(&state, "web").await;
    let viewer = login(&state, "viewer").await;
    let path = format!("/api/projects/{project}/overview");

    // Jamais deploye : rien en production, statut `none`, series vides.
    let r = send(&state, &viewer, "GET", &path, false).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let o = r.json();
    assert_eq!(o["status"], "none");
    assert!(o["production"].is_null());
    assert!(o["domain"].is_null());
    for kpi in ["requests", "latency_p95_ms", "error_rate", "uptime"] {
        assert!(o["metrics"][kpi]["value"].is_null(), "{kpi}");
        assert_eq!(o["metrics"][kpi]["series"], json!([]), "{kpi}");
    }
    assert_eq!(o["activity"], json!([]));
    let s = &o["services"][0];
    assert_eq!(s["id"], service.to_string());
    assert_eq!(s["name"], "web");
    assert_eq!(s["environment"], "production");
    assert_eq!(s["state"], "none");
    assert!(s["deployment_id"].is_null());
    assert_eq!(s["replicas"], json!({ "running": 0, "desired": 1 }));
    assert!(s["cpu_percent"].is_null());
    assert!(s["memory_bytes"].is_null());

    // Deploiement en cours, rien de servi : `building` / `deploying`.
    let srv = server(&state).await;
    let first = deployment(&state, project, env, srv, "running", 600).await;
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "building");
    assert_eq!(o["services"][0]["state"], "deploying");
    sqlx::query("UPDATE deployments SET status = 'success', finished_at = now() - interval '500 seconds' WHERE id = $1")
        .bind(first)
        .execute(&state.db)
        .await
        .unwrap();

    // Servi, agent deconnecte : le service et le projet sont injoignables.
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "unreachable");
    assert_eq!(o["production"]["id"], first.to_string());
    assert_eq!(o["production"]["commit_sha"], COMMIT);
    assert_eq!(o["production"]["current"], true);
    assert_eq!(o["services"][0]["state"], "unreachable");
    assert_eq!(o["services"][0]["deployment_id"], first.to_string());

    // Agent connecte : en ligne, 1 / 1.
    let _agent = state.agents.attach_channel(ServerId(srv));
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "running");
    let s = &o["services"][0];
    assert_eq!(s["state"], "running");
    assert_eq!(s["replicas"], json!({ "running": 1, "desired": 1 }));
    assert_eq!(s["image"], "ciphera/web-production:1");
    assert!(s["deployment_number"].is_i64());

    // Un echec plus recent : l'ancien reste servi, le projet reste en ligne
    // (l'echec se voit dans l'activite).
    let failed = deployment(&state, project, env, srv, "failed", 100).await;
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "running");
    assert_eq!(o["activity"][0]["status"], "failed");
    assert_eq!(o["services"][0]["state"], "running");
    assert_eq!(o["production"]["id"], first.to_string());
    // Activite : les deploiements du projet, le plus recent d'abord (4 au plus).
    for age in [50, 40, 30] {
        deployment(&state, project, env, srv, "failed", age).await;
    }
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    let activity = o["activity"].as_array().unwrap();
    assert_eq!(activity.len(), 4);
    assert!(activity.iter().all(|d| d["id"] != first.to_string()));
    assert_eq!(activity[3]["id"], failed.to_string());

    // Liste des projets : meme statut derive.
    let list = send(&state, &viewer, "GET", "/api/projects", false)
        .await
        .json();
    assert_eq!(list[0]["status"], "running");

    // `GET /services` : memes lignes que l'apercu.
    let services = send(
        &state,
        &viewer,
        "GET",
        &format!("/api/projects/{project}/services"),
        false,
    )
    .await;
    assert_eq!(services.status, 200, "{}", services.raw);
    assert_eq!(services.json(), o["services"]);

    // Projet inconnu : 404.
    let r = send(
        &state,
        &viewer,
        "GET",
        &format!("/api/projects/{}/overview", Uuid::new_v4()),
        false,
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
}

#[tokio::test]
async fn stop_requires_developer() {
    let (state, _pg) = common::state().await;
    let (project, env, service) = git_project(&state, "web").await;
    let stop = format!("/api/projects/{project}/services/{service}/stop");
    let restart = format!("/api/projects/{project}/services/{service}/restart");
    let dev = login(&state, "developer").await;
    let viewer = login(&state, "viewer").await;

    // Lecteur : 403 ; sans jeton CSRF : 403.
    for path in [&stop, &restart] {
        let r = send(&state, &viewer, "POST", path, true).await;
        assert_eq!(r.status, 403, "{}", r.raw);
        let r = send(&state, &dev, "POST", path, false).await;
        assert_eq!(r.status, 403, "{}", r.raw);
    }
    // Rien de servi : 409.
    for path in [&stop, &restart] {
        let r = send(&state, &dev, "POST", path, true).await;
        assert_eq!(r.status, 409, "{}", r.raw);
        assert_eq!(r.code(), "nothing_serving");
    }
    // Service d'un autre projet, ou inconnu : 404.
    let (_, _, other) = git_project(&state, "api").await;
    for sid in [other, Uuid::new_v4()] {
        let r = send(
            &state,
            &dev,
            "POST",
            &format!("/api/projects/{project}/services/{sid}/stop"),
            true,
        )
        .await;
        assert_eq!(r.status, 404, "{}", r.raw);
    }

    let srv = server(&state).await;
    let serving = deployment(&state, project, env, srv, "success", 600).await;
    // Agent deconnecte : 503, aucune marque d'arret.
    let r = send(&state, &dev, "POST", &stop, true).await;
    assert_eq!(r.status, 503, "{}", r.raw);
    assert_eq!(r.code(), "agent_unavailable");
    let services_path = format!("/api/projects/{project}/services");

    let mut agent = state.agents.attach_channel(ServerId(srv));
    let r = send(&state, &dev, "POST", &stop, true).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["deployment_id"], serving.to_string());
    match agent.try_recv() {
        Ok(ControlToAgent::StopContainers { deployment_id }) => {
            assert_eq!(deployment_id.0, serving)
        }
        other => panic!("ordre attendu : StopContainers, recu {other:?}"),
    }
    let s = send(&state, &viewer, "GET", &services_path, false)
        .await
        .json();
    assert_eq!(s[0]["state"], "stopped");
    assert_eq!(s[0]["replicas"], json!({ "running": 0, "desired": 1 }));

    // Redemarrer relance le service arrete.
    let r = send(&state, &dev, "POST", &restart, true).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    match agent.try_recv() {
        Ok(ControlToAgent::Restart { deployment_id }) => assert_eq!(deployment_id.0, serving),
        other => panic!("ordre attendu : Restart, recu {other:?}"),
    }
    let s = send(&state, &viewer, "GET", &services_path, false)
        .await
        .json();
    assert_eq!(s[0]["state"], "running");

    // Arrete puis redeploye : le nouveau deploiement tourne.
    let r = send(&state, &dev, "POST", &stop, true).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    deployment(&state, project, env, srv, "success", 10).await;
    let s = send(&state, &viewer, "GET", &services_path, false)
        .await
        .json();
    assert_eq!(s[0]["state"], "running");

    for (action, n) in [("service.stop", 2), ("service.restart", 1)] {
        assert_eq!(
            count(
                &state,
                &format!("SELECT count(*) FROM audit_events WHERE action = '{action}'")
            )
            .await,
            n,
            "{action}"
        );
    }
}

#[tokio::test]
async fn project_status_reflects_what_users_get() {
    let (state, _pg) = common::state().await;
    let (project, env, _) = git_project(&state, "web").await;
    let viewer = login(&state, "viewer").await;
    let overview = format!("/api/projects/{project}/overview");
    let detail = format!("/api/projects/{project}");
    macro_rules! status {
        () => {{
            let o = send(&state, &viewer, "GET", &overview, false).await.json();
            let d = send(&state, &viewer, "GET", &detail, false).await.json();
            assert_eq!(o["status"], d["status"], "apercu et detail");
            o["status"].as_str().unwrap().to_string()
        }};
    }
    // Jamais deploye.
    assert_eq!(status!(), "none");
    let srv = server(&state).await;
    let _agent = state.agents.attach_channel(ServerId(srv));
    // Rien de servi, en cours.
    let first = deployment(&state, project, env, srv, "queued", 900).await;
    assert_eq!(status!(), "building");
    // Rien de servi, dernier en echec.
    sqlx::query("UPDATE deployments SET status = 'failed', finished_at = now() - interval '800 seconds' WHERE id = $1")
        .bind(first)
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(status!(), "failed");
    // Servi : en ligne, meme avec un echec plus recent ou un deploiement en cours.
    deployment(&state, project, env, srv, "success", 700).await;
    assert_eq!(status!(), "running");
    deployment(&state, project, env, srv, "failed", 600).await;
    assert_eq!(status!(), "running");
    deployment(&state, project, env, srv, "running", 500).await;
    assert_eq!(status!(), "running");
}

/// Etat remonte par l'agent : un conteneur arrete hors du panneau rend le
/// service et le projet « down » ; CPU, memoire et disponibilite viennent
/// des metriques.
#[tokio::test]
async fn agent_reported_state_and_metrics_feed_the_overview() {
    use control_plane::domain::deployments::orchestrator;
    use proto::{
        AgentState, AgentToControl, ContainerMetrics, ImageRef, NodeMetrics, RunningDeployment,
    };
    use shared::ids::{DeploymentId, ServiceId};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime};

    let (mut state, _pg) = common::state().await;
    // Une mesure par serveur toutes les 10 s au plus : l'horloge avance de
    // 30 s (periode de l'agent) entre deux mesures.
    let offset = Arc::new(AtomicU64::new(0));
    let o = offset.clone();
    state.clock = control_plane::state::Clock::from_fn(move || {
        SystemTime::now() + Duration::from_secs(o.load(Ordering::SeqCst))
    });
    let next_period = || offset.fetch_add(30, Ordering::SeqCst);
    let (project, env, service) = git_project(&state, "web").await;
    let viewer = login(&state, "viewer").await;
    let path = format!("/api/projects/{project}/overview");
    let srv = server(&state).await;
    let _agent = state.agents.attach_channel(ServerId(srv));
    let serving = deployment(&state, project, env, srv, "success", 600).await;
    let metrics = |running: bool| {
        AgentToControl::Metrics(NodeMetrics {
            cpu_percent: 50.0,
            mem_used_bytes: 1,
            mem_total_bytes: 2,
            disk_used_bytes: 3,
            disk_total_bytes: 4,
            net_rx_bytes: 5,
            net_tx_bytes: 6,
            containers: vec![ContainerMetrics {
                deployment_id: DeploymentId(serving),
                service_id: ServiceId(service),
                cpu_percent: 12.5,
                mem_used_bytes: 256 << 20,
                running,
            }],
        })
    };

    orchestrator::on_agent_message(&state, ServerId(srv), metrics(true)).await;
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "running");
    let s = &o["services"][0];
    assert_eq!(s["state"], "running");
    assert_eq!(s["cpu_percent"], 12.5);
    assert_eq!(s["memory_bytes"], 256u64 << 20);
    // Disponibilite : une serie journaliere, 100 % aujourd'hui.
    assert_eq!(o["metrics"]["uptime"]["value"], 100.0);
    assert_eq!(o["metrics"]["uptime"]["series"], json!([100.0]));
    // Requetes, latence, erreurs : aucune source en v0.1.
    for kpi in ["requests", "latency_p95_ms", "error_rate"] {
        assert!(o["metrics"][kpi]["value"].is_null(), "{kpi}");
    }

    // Conteneur arrete hors du panneau : down.
    next_period();
    orchestrator::on_agent_message(&state, ServerId(srv), metrics(false)).await;
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "down");
    assert_eq!(o["services"][0]["state"], "down");
    assert_eq!(
        o["services"][0]["replicas"],
        json!({ "running": 0, "desired": 1 })
    );
    assert_eq!(o["metrics"]["uptime"]["value"], 50.0);
    let list = send(&state, &viewer, "GET", "/api/projects", false)
        .await
        .json();
    assert_eq!(list[0]["status"], "down");

    // L'etat complet envoye a la reconnexion fait foi aussi.
    orchestrator::on_agent_message(
        &state,
        ServerId(srv),
        AgentToControl::State(AgentState {
            deployments: vec![RunningDeployment {
                deployment_id: DeploymentId(serving),
                service_id: ServiceId(service),
                image: ImageRef("ciphera/web-production:1".into()),
                healthy: true,
                serving: true,
                runs_as_root: false,
            }],
            ..AgentState::default()
        }),
    )
    .await;
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "running");

    // Arrete depuis le panneau : « stopped », pas « down ».
    let dev = login(&state, "developer").await;
    let r = send(
        &state,
        &dev,
        "POST",
        &format!("/api/projects/{project}/services/{service}/stop"),
        true,
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    next_period();
    orchestrator::on_agent_message(&state, ServerId(srv), metrics(false)).await;
    let o = send(&state, &viewer, "GET", &path, false).await.json();
    assert_eq!(o["status"], "stopped");
    assert_eq!(o["services"][0]["state"], "stopped");
}
