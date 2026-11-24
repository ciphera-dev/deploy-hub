//! Vue d'ensemble : `GET /api/overview?period=7d|30d` (bloc « A traiter /
//! Rien a signaler », indicateurs, graphe, deploiements recents, charge des
//! serveurs). Sans donnees : etats vides, jamais de valeurs inventees.

mod common;

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::deployments::orchestrator;
use control_plane::state::AppState;
use proto::{AgentToControl, NodeMetrics};
use serde_json::{Value, json};
use shared::ids::ServerId;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

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

async fn get(state: &AppState, session: &(String, String), path: &str) -> Reply {
    call(
        state,
        "GET",
        path,
        Req {
            cookie: Some(&session.0),
            ..Req::default()
        },
    )
    .await
}

fn points(series: &Value) -> Vec<f64> {
    series["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["value"].as_f64().unwrap())
        .collect()
}

#[tokio::test]
async fn overview_empty_state_rien_a_signaler() {
    let (state, _pg) = common::state().await;
    let viewer = login(&state, "viewer").await;

    // Sans session : 401.
    let r = call(&state, "GET", "/api/overview", Req::default()).await;
    assert_eq!(r.status, 401, "{}", r.raw);

    let r = get(&state, &viewer, "/api/overview").await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let o = r.json();
    // Rien a traiter.
    assert_eq!(o["issues"], json!([]));
    assert_eq!(o["projects_online"], 0);
    assert_eq!(o["projects_total"], 0);
    assert_eq!(o["servers_online"], 0);
    assert_eq!(o["servers_total"], 0);
    assert_eq!(o["deployments_today"], 0);
    // 30 jours par defaut : un point par jour, tous a zero.
    assert_eq!(o["period"], "30d");
    let deployments = points(&o["chart"]["deployments"]);
    assert_eq!(deployments.len(), 30);
    assert!(deployments.iter().all(|v| *v == 0.0));
    assert_eq!(o["chart"]["deployments"]["total"], 0.0);
    assert!(o["chart"]["deployments"]["change_percent"].is_null());
    // Requetes et latence : aucune source en v0.1, series vides.
    for kpi in ["requests", "latency_p95_ms"] {
        assert!(o["chart"][kpi]["total"].is_null(), "{kpi}");
        assert_eq!(o["chart"][kpi]["points"], json!([]), "{kpi}");
    }
    assert_eq!(o["recent_deployments"], json!([]));
    assert_eq!(o["servers"], json!([]));

    // 7 jours : 7 points ; periode inconnue : 422.
    let o = get(&state, &viewer, "/api/overview?period=7d").await.json();
    assert_eq!(o["period"], "7d");
    assert_eq!(points(&o["chart"]["deployments"]).len(), 7);
    let r = get(&state, &viewer, "/api/overview?period=1y").await;
    assert_eq!(r.status, 422, "{}", r.raw);
}

#[tokio::test]
async fn overview_lists_issues_deployments_and_server_load() {
    let (state, _pg) = common::state().await;
    let viewer = login(&state, "viewer").await;
    let dev = login(&state, "developer").await;
    let r = call(
        &state,
        "POST",
        "/api/projects",
        Req {
            cookie: Some(&dev.0),
            csrf: Some(&dev.1),
            body: Some(json!({ "name": "billing", "source": { "type": "image", "image": "ghcr.io/acme/app:1" } })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let project = Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap();
    let env = Uuid::parse_str(r.json()["environments"][0]["id"].as_str().unwrap()).unwrap();
    let (online, offline) = (Uuid::new_v4(), Uuid::new_v4());
    for (id, name, kind) in [(online, "local", "local"), (offline, "edge-2", "remote")] {
        sqlx::query(
            "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(name)
        .bind(kind)
        .bind(format!("FP:{id}"))
        .execute(&state.db)
        .await
        .unwrap();
    }
    // Dernier deploiement en echec, aujourd'hui.
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, status, trigger, \
         image, created_at, started_at, finished_at) \
         VALUES ($1, $2, $3, $4, 'failed', 'manual', 'ghcr.io/acme/app:1', now(), now(), now())",
    )
    .bind(Uuid::new_v4())
    .bind(project)
    .bind(env)
    .bind(online)
    .execute(&state.db)
    .await
    .unwrap();
    let _agent = state.agents.attach_channel(ServerId(online));
    orchestrator::on_agent_message(
        &state,
        ServerId(online),
        AgentToControl::Metrics(NodeMetrics {
            cpu_percent: 37.5,
            mem_used_bytes: 1,
            mem_total_bytes: 2,
            disk_used_bytes: 3,
            disk_total_bytes: 4,
            net_rx_bytes: 5,
            net_tx_bytes: 6,
            containers: Vec::new(),
        }),
    )
    .await;

    let o = get(&state, &viewer, "/api/overview").await.json();
    let issues = o["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2, "{issues:?}");
    assert!(issues.iter().any(|i| i["kind"] == "project_failed"
        && i["name"] == "billing"
        && i["id"] == project.to_string()));
    assert!(
        issues
            .iter()
            .any(|i| i["kind"] == "server_unreachable" && i["name"] == "edge-2")
    );
    assert_eq!(
        (o["projects_online"].clone(), o["projects_total"].clone()),
        (json!(0), json!(1))
    );
    assert_eq!(
        (o["servers_online"].clone(), o["servers_total"].clone()),
        (json!(1), json!(2))
    );
    assert_eq!(o["deployments_today"], 1);
    assert_eq!(*points(&o["chart"]["deployments"]).last().unwrap(), 1.0);
    assert_eq!(o["recent_deployments"][0]["project_name"], "billing");
    let servers = o["servers"].as_array().unwrap();
    let local = servers.iter().find(|s| s["name"] == "local").unwrap();
    assert_eq!(local["online"], true);
    assert_eq!(local["cpu_percent"], 37.5);
    let edge = servers.iter().find(|s| s["name"] == "edge-2").unwrap();
    assert_eq!(edge["online"], false);
    assert!(edge["cpu_percent"].is_null());
}
