//! Metriques des agents : mesures brutes gardees 24 h, agregats horaires
//! gardes 30 jours (tache `metrics_rollup`, idempotente).

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use control_plane::domain::metrics;
use control_plane::jobs::{self, JobKind};
use control_plane::state::{AppState, Clock};
use proto::{ContainerMetrics, NodeMetrics};
use shared::ids::{DeploymentId, ServiceId};
use uuid::Uuid;

const HOUR: f64 = 3600.0;
const DAY: f64 = 86_400.0;

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

async fn server(state: &AppState) -> Uuid {
    let id = Uuid::new_v4();
    // Le premier est le serveur local, les suivants distants.
    sqlx::query(
        "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', \
         CASE WHEN EXISTS (SELECT 1 FROM servers) THEN 'remote' ELSE 'local' END, $1::text)",
    )
    .bind(id)
    .execute(&state.db)
    .await
    .unwrap();
    id
}

/// Deploiement servi d'un service sur `server` (seul un conteneur d'un
/// deploiement de l'agent qui mesure est enregistre).
async fn serving(state: &AppState, server: Uuid) -> (DeploymentId, ServiceId) {
    let (project, env, service, deployment) = (
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
    );
    for (sql, a, b) in [
        (
            "INSERT INTO projects (id, name, source_kind, image_ref) \
             VALUES ($1, 'p-' || left(replace($2::text, '-', ''), 8), 'image', 'ghcr.io/acme/app:1')",
            project,
            project,
        ),
        (
            "INSERT INTO environments (id, project_id, name) VALUES ($1, $2, 'production')",
            env,
            project,
        ),
        (
            "INSERT INTO services (id, environment_id, name) VALUES ($1, $2, 'web')",
            service,
            env,
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
            .bind(a)
            .bind(b)
            .execute(&state.db)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, status, trigger, \
         image, created_at, started_at, finished_at) \
         VALUES ($1, $2, $3, $4, 'success', 'manual', 'ghcr.io/acme/app:1', now(), now(), now())",
    )
    .bind(deployment)
    .bind(project)
    .bind(env)
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();
    (DeploymentId(deployment), ServiceId(service))
}

fn sample(cpu: f32, (deployment, service): (DeploymentId, ServiceId)) -> NodeMetrics {
    NodeMetrics {
        cpu_percent: cpu,
        mem_used_bytes: 1 << 30,
        mem_total_bytes: 4 << 30,
        disk_used_bytes: 10 << 30,
        disk_total_bytes: 40 << 30,
        net_rx_bytes: 1,
        net_tx_bytes: 2,
        containers: vec![ContainerMetrics {
            deployment_id: deployment,
            service_id: service,
            cpu_percent: cpu / 2.0,
            mem_used_bytes: 64 << 20,
            running: true,
        }],
    }
}

async fn count(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn raw_metrics_pruned_after_24h() {
    let (state, _pg) = common::state().await;
    let srv = server(&state).await;
    let service = serving(&state, srv).await;
    let t = now();
    for age in [26.0 * HOUR, 2.0 * HOUR, 60.0] {
        metrics::record(&state.db, srv, &sample(10.0, service), t - age)
            .await
            .unwrap();
    }
    assert_eq!(
        count(&state, "SELECT count(*) FROM node_metrics_raw").await,
        3
    );
    metrics::rollup(&state.db, t).await.unwrap();
    // La mesure d'il y a 26 h est partie ; les deux recentes restent.
    assert_eq!(
        count(&state, "SELECT count(*) FROM node_metrics_raw").await,
        2
    );
    assert_eq!(
        count(&state, "SELECT count(*) FROM container_metrics_raw").await,
        2
    );
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM node_metrics_raw WHERE at < now() - interval '25 hours'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn hourly_rollup_kept_30d() {
    let (state, _pg) = common::state().await;
    let srv = server(&state).await;
    let service = serving(&state, srv).await;
    // Milieu d'une heure : les deux mesures a 60 s d'ecart ne tombent jamais
    // de part et d'autre d'un changement d'heure.
    let t = (now() / HOUR).floor() * HOUR - HOUR / 2.0;
    // Deux mesures dans la meme heure il y a 29 jours, une il y a 31 jours,
    // une il y a 26 h.
    for (age, cpu) in [
        (31.0 * DAY, 90.0),
        (29.0 * DAY, 10.0),
        (29.0 * DAY + 60.0, 30.0),
        (26.0 * HOUR, 50.0),
    ] {
        metrics::record(&state.db, srv, &sample(cpu, service), t - age)
            .await
            .unwrap();
    }
    metrics::rollup(&state.db, t).await.unwrap();
    // Rejouee (au moins une fois) : meme resultat.
    metrics::rollup(&state.db, t).await.unwrap();

    let rows: Vec<(f64, f64, i32)> = sqlx::query_as(
        "SELECT extract(epoch FROM hour)::float8, cpu_avg, samples FROM node_metrics_hourly \
         WHERE server_id = $1 ORDER BY hour",
    )
    .bind(srv)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    // L'heure d'il y a 29 jours : moyenne des deux mesures.
    assert!((rows[0].0 - (t - 29.0 * DAY)).abs() < 2.0 * HOUR);
    assert!((rows[0].1 - 20.0).abs() < 1e-6, "{rows:?}");
    assert_eq!(rows[0].2, 2);
    assert!((rows[1].1 - 50.0).abs() < 1e-6);
    // Conteneurs : meme regle, par service.
    let container: Vec<(f64, i32, i32)> = sqlx::query_as(
        "SELECT cpu_avg, running_samples, samples FROM container_metrics_hourly \
         WHERE service_id = $1 ORDER BY hour",
    )
    .bind(service.1.0)
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(container.len(), 2, "{container:?}");
    assert!((container[0].0 - 10.0).abs() < 1e-6);
    assert_eq!((container[0].1, container[0].2), (2, 2));
    // Plus rien de brut au-dela de 24 h.
    assert_eq!(
        count(&state, "SELECT count(*) FROM node_metrics_raw").await,
        0
    );
}

#[tokio::test]
async fn rollup_job_is_scheduled_once_and_runs_on_the_queue() {
    let (state, _pg) = common::state().await;
    let srv = server(&state).await;
    metrics::record(
        &state.db,
        srv,
        &sample(10.0, serving(&state, srv).await),
        now() - 26.0 * HOUR,
    )
    .await
    .unwrap();
    // Plusieurs demandes : une seule tache en file.
    metrics::schedule(&state.db).await.unwrap();
    metrics::schedule(&state.db).await.unwrap();
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM jobs WHERE kind = 'metrics_rollup' AND status = 'queued'"
        )
        .await,
        1
    );
    let job = jobs::claim(&state.db, &Clock::system(), "w")
        .await
        .unwrap()
        .expect("tache due");
    assert_eq!(job.kind, JobKind::MetricsRollup);
    metrics::run_rollup(&state, job).await.expect("agregation");
    assert_eq!(
        count(&state, "SELECT count(*) FROM node_metrics_raw").await,
        0
    );
}

/// Une mesure n'enregistre que les conteneurs des deploiements de l'agent
/// qui la rapporte (au plus `MAX_CONTAINERS`), et au plus une mesure par
/// `MIN_INTERVAL` et par serveur.
#[tokio::test]
async fn metrics_are_bound_to_the_reporting_server_and_throttled() {
    let (state, _pg) = common::state().await;
    let a = server(&state).await;
    let b = server(&state).await;
    let on_a = serving(&state, a).await;
    let t = now();
    // L'agent B rapporte un conteneur d'un deploiement de A.
    let mut m = sample(10.0, on_a);
    m.containers[0].running = false;
    metrics::record(&state.db, b, &m, t).await.unwrap();
    assert_eq!(
        count(&state, "SELECT count(*) FROM container_metrics_raw").await,
        0,
        "conteneur d'un autre serveur ignore"
    );
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM deployments WHERE container_running IS NOT NULL"
        )
        .await,
        0
    );
    // Service d'un autre environnement que celui du deploiement : ignore.
    let other = serving(&state, a).await;
    let mut m = sample(10.0, on_a);
    m.containers[0].service_id = other.1;
    metrics::record(&state.db, a, &m, t).await.unwrap();
    assert_eq!(
        count(&state, "SELECT count(*) FROM container_metrics_raw").await,
        0
    );
    // Mesure suivante trop proche (moins de 10 s) : ignoree.
    metrics::record(&state.db, a, &sample(20.0, on_a), t + 5.0)
        .await
        .unwrap();
    assert_eq!(
        count(&state, "SELECT count(*) FROM node_metrics_raw").await,
        2,
        "une mesure par serveur"
    );
    // Liste demesuree : bornee.
    let mut m = sample(30.0, on_a);
    m.containers = vec![m.containers[0].clone(); metrics::MAX_CONTAINERS + 50];
    metrics::record(&state.db, a, &m, t + 60.0).await.unwrap();
    assert_eq!(
        count(&state, "SELECT count(*) FROM container_metrics_raw").await,
        metrics::MAX_CONTAINERS as i64
    );
}
