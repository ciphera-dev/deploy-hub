//! Metriques des agents (`AgentToControl::Metrics`, toutes les 30 s) : noeud
//! et conteneur servi de chaque service.
//!
//! - Mesures brutes gardees 24 h (par heures entieres : une heure presente
//!   dans les mesures brutes y est complete), agregats horaires 30 jours.
//! - L'agregation est la tache `metrics_rollup` (une seule en file, cle
//!   `metrics-rollup`, demandee toutes les heures par [`run`]) : une
//!   transaction courte qui recalcule les heures encore presentes dans les
//!   mesures brutes, puis purge. Rejouee, elle donne le meme resultat.
//! - L'etat du conteneur servi (`running`) est aussi garde sur son
//!   deploiement (`container_running`), comme celui du `State` envoye a
//!   chaque connexion : un conteneur arrete hors du panneau apparait arrete.

use std::collections::HashMap;
use std::time::Duration;

use proto::{NodeMetrics, RunningDeployment};
use serde_json::json;
use sqlx::{PgConnection, PgPool};
use tokio::sync::watch;
use uuid::Uuid;

use crate::jobs::worker::{HandlerError, Handlers};
use crate::jobs::{self, Job, JobKind};
use crate::state::AppState;

/// Duree de garde des mesures brutes.
pub const RAW_RETENTION: Duration = Duration::from_secs(24 * 3600);
/// Duree de garde des agregats horaires.
pub const HOURLY_RETENTION: Duration = Duration::from_secs(30 * 86_400);
/// Intervalle des demandes d'agregation.
pub const ROLLUP_EVERY: Duration = Duration::from_secs(3600);
/// Une mesure plus ancienne n'est plus « courante » (4 periodes de 30 s).
pub const FRESH: Duration = Duration::from_secs(120);
/// Cle de deduplication de la tache d'agregation.
const ROLLUP_KEY: &str = "metrics-rollup";
/// Ecart minimal entre deux mesures enregistrees d'un meme serveur (l'agent
/// mesure toutes les 30 s, plus une fois a chaque connexion) : un agent qui
/// en envoie davantage ne remplit pas la base.
pub const MIN_INTERVAL: Duration = Duration::from_secs(10);
/// Conteneurs enregistres au plus par mesure.
pub const MAX_CONTAINERS: usize = 512;

fn int(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// Enregistre une mesure de l'agent `server` (horodatee `at`, secondes
/// Unix) et l'etat des conteneurs servis qu'elle porte. Mesure ignoree si
/// une autre du meme serveur est a moins de `MIN_INTERVAL` ; seuls les
/// conteneurs des deploiements de ce serveur (pour un service de leur
/// environnement) sont gardes, `MAX_CONTAINERS` au plus.
pub async fn record(db: &PgPool, server: Uuid, m: &NodeMetrics, at: f64) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    // Mesures d'un meme serveur serialisees (verrou de transaction).
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-metrics:' || $1::text, 0))",
    )
    .bind(server)
    .execute(&mut *tx)
    .await?;
    let recent: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM node_metrics_raw WHERE server_id = $1 \
         AND at > to_timestamp($2::float8) - make_interval(secs => $3::float8) \
         AND at < to_timestamp($2::float8) + make_interval(secs => $3::float8))",
    )
    .bind(server)
    .bind(at)
    .bind(MIN_INTERVAL.as_secs_f64())
    .fetch_one(&mut *tx)
    .await?;
    if recent {
        tracing::debug!(%server, "mesure trop rapprochee de la precedente : ignoree");
        return tx.commit().await;
    }
    sqlx::query(
        "INSERT INTO node_metrics_raw (server_id, at, cpu_percent, mem_used_bytes, \
         mem_total_bytes, disk_used_bytes, disk_total_bytes, net_rx_bytes, net_tx_bytes) \
         VALUES ($1, to_timestamp($2::float8), $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(server)
    .bind(at)
    .bind(m.cpu_percent)
    .bind(int(m.mem_used_bytes))
    .bind(int(m.mem_total_bytes))
    .bind(int(m.disk_used_bytes))
    .bind(int(m.disk_total_bytes))
    .bind(int(m.net_rx_bytes))
    .bind(int(m.net_tx_bytes))
    .execute(&mut *tx)
    .await?;
    if !m.containers.is_empty() {
        let containers = &m.containers[..m.containers.len().min(MAX_CONTAINERS)];
        let deployments: Vec<Uuid> = containers.iter().map(|c| c.deployment_id.0).collect();
        let services: Vec<Uuid> = containers.iter().map(|c| c.service_id.0).collect();
        let cpu: Vec<f32> = containers.iter().map(|c| c.cpu_percent).collect();
        let mem: Vec<i64> = containers.iter().map(|c| int(c.mem_used_bytes)).collect();
        let running: Vec<bool> = containers.iter().map(|c| c.running).collect();
        // Conteneurs d'un deploiement de ce serveur, pour un service de son
        // environnement : un agent ne rapporte que les siens.
        sqlx::query(
            "INSERT INTO container_metrics_raw (server_id, deployment_id, service_id, at, \
             cpu_percent, mem_used_bytes, running) \
             SELECT $1, x.d, x.s, to_timestamp($2::float8), x.c, x.m, x.r \
             FROM unnest($3::uuid[], $4::uuid[], $5::real[], $6::bigint[], $7::bool[]) AS x(d, s, c, m, r) \
             JOIN deployments dep ON dep.id = x.d AND dep.server_id = $1 \
             JOIN services svc ON svc.id = x.s AND svc.environment_id = dep.environment_id",
        )
        .bind(server)
        .bind(at)
        .bind(&deployments)
        .bind(&services)
        .bind(&cpu)
        .bind(&mem)
        .bind(&running)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE deployments d SET container_running = x.r, \
             container_reported_at = to_timestamp($2::float8) \
             FROM unnest($3::uuid[], $4::uuid[], $5::bool[]) AS x(id, s, r) \
             JOIN services svc ON svc.id = x.s \
             WHERE d.id = x.id AND d.server_id = $1 AND svc.environment_id = d.environment_id",
        )
        .bind(server)
        .bind(at)
        .bind(&deployments)
        .bind(&services)
        .bind(&running)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Etat complet de l'agent `server` (`State`, a chaque connexion) : chaque
/// deploiement reussi de ce serveur est en marche s'il y figure sain, arrete
/// sinon (conteneur disparu, en echec).
pub async fn record_state(
    db: &PgPool,
    server: Uuid,
    deployments: &[RunningDeployment],
    at: f64,
) -> sqlx::Result<()> {
    let ids: Vec<Uuid> = deployments.iter().map(|d| d.deployment_id.0).collect();
    let healthy: Vec<bool> = deployments.iter().map(|d| d.healthy).collect();
    let root: Vec<bool> = deployments.iter().map(|d| d.runs_as_root).collect();
    sqlx::query(
        "UPDATE deployments d SET container_running = COALESCE( \
           (SELECT x.h FROM unnest($2::uuid[], $3::bool[]) AS x(id, h) WHERE x.id = d.id), false), \
         container_reported_at = to_timestamp($4::float8), \
         runs_as_root = COALESCE( \
           (SELECT x.r FROM unnest($2::uuid[], $5::bool[]) AS x(id, r) WHERE x.id = d.id), false) \
         WHERE d.server_id = $1 AND d.status = 'success'",
    )
    .bind(server)
    .bind(&ids)
    .bind(&healthy)
    .bind(at)
    .bind(&root)
    .execute(db)
    .await
    .map(|_| ())
}

/// Agrege les mesures brutes par heure, puis purge les mesures brutes de
/// plus de 24 h (heures entieres) et les agregats de plus de 30 jours.
/// Idempotent.
pub async fn rollup(db: &PgPool, now: f64) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO node_metrics_hourly (server_id, hour, cpu_avg, mem_used_avg, \
         mem_total_bytes, disk_used_bytes, disk_total_bytes, samples) \
         SELECT server_id, date_trunc('hour', at), avg(cpu_percent), avg(mem_used_bytes)::bigint, \
                max(mem_total_bytes), max(disk_used_bytes), max(disk_total_bytes), count(*) \
         FROM node_metrics_raw GROUP BY 1, 2 \
         ON CONFLICT (server_id, hour) DO UPDATE SET cpu_avg = excluded.cpu_avg, \
           mem_used_avg = excluded.mem_used_avg, mem_total_bytes = excluded.mem_total_bytes, \
           disk_used_bytes = excluded.disk_used_bytes, \
           disk_total_bytes = excluded.disk_total_bytes, samples = excluded.samples",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO container_metrics_hourly (service_id, hour, cpu_avg, mem_avg, \
         running_samples, samples) \
         SELECT service_id, date_trunc('hour', at), avg(cpu_percent), avg(mem_used_bytes)::bigint, \
                count(*) FILTER (WHERE running), count(*) \
         FROM container_metrics_raw GROUP BY 1, 2 \
         ON CONFLICT (service_id, hour) DO UPDATE SET cpu_avg = excluded.cpu_avg, \
           mem_avg = excluded.mem_avg, running_samples = excluded.running_samples, \
           samples = excluded.samples",
    )
    .execute(&mut *tx)
    .await?;
    let raw_before = now - RAW_RETENTION.as_secs_f64();
    for table in ["node_metrics_raw", "container_metrics_raw"] {
        // Nom de table fixe (liste ci-dessus), aucune donnee.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE at < date_trunc('hour', to_timestamp($1::float8))"
        )))
        .bind(raw_before)
        .execute(&mut *tx)
        .await?;
    }
    let hourly_before = now - HOURLY_RETENTION.as_secs_f64();
    for table in ["node_metrics_hourly", "container_metrics_hourly"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE hour < to_timestamp($1::float8)"
        )))
        .bind(hourly_before)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Gestionnaire des taches `metrics_rollup`.
pub fn register(handlers: Handlers, state: AppState) -> Handlers {
    handlers.register(JobKind::MetricsRollup, move |job| {
        let state = state.clone();
        async move { run_rollup(&state, job).await }
    })
}

/// Execute une tache d'agregation.
/// Tache horaire d'entretien : elle balaie aussi les notifications
/// ([`crate::domain::notifications::sweep`]), purge les envois anciens
/// ([`crate::domain::notifications::purge`]) et les taches terminees depuis
/// plus de 7 jours ([`jobs::purge_finished`]), et verifie l'expiration de
/// la CA de l'instance ([`crate::domain::notifications::check_ca_expiry`]).
pub async fn run_rollup(state: &AppState, _job: Job) -> Result<(), HandlerError> {
    let now = state.clock.unix_secs();
    // Purge au mieux, avant l'agregation : un echec de celle-ci ne l'arrete
    // pas.
    if let Err(e) = jobs::purge_finished(&state.db, &state.clock).await {
        tracing::warn!(error = %e, "purge des taches terminees impossible");
    }
    rollup(&state.db, now).await.map_err(|e| {
        tracing::error!(error = %e, "agregation des metriques impossible");
        HandlerError::retry("agregation des metriques : base de donnees indisponible")
    })?;
    let notifications = async {
        crate::domain::notifications::sweep(&state.db, now).await?;
        crate::domain::notifications::purge(&state.db, now).await
    };
    if let Err(e) = crate::http::email_verification::purge_pending(&state.db).await {
        tracing::warn!(error = %e, "purge des comptes en attente impossible");
    }
    if let Err(e) = crate::domain::notifications::check_ca_expiry(state).await {
        tracing::warn!(error = %e, "expiration de la CA de l'instance non verifiee");
    }
    notifications.await.map_err(|e| {
        tracing::error!(error = %e, "entretien des notifications impossible");
        HandlerError::retry("entretien des notifications : base de donnees indisponible")
    })
}

/// Demande une agregation (une seule tache en file a la fois).
pub async fn schedule(db: &PgPool) -> sqlx::Result<()> {
    jobs::enqueue(
        db,
        JobKind::MetricsRollup,
        json!({}),
        Some(ROLLUP_KEY.into()),
    )
    .await
    .map(|_| ())
}

/// Demande une agregation au demarrage puis toutes les heures, jusqu'a
/// l'arret.
pub async fn run(db: PgPool, mut stop: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(ROLLUP_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = stop.wait_for(|s| *s) => return,
        }
        if let Err(e) = schedule(&db).await {
            tracing::warn!(error = %e, "agregation des metriques non demandee");
        }
    }
}

/// Charge courante (mesure de moins de 2 minutes) de chaque serveur : CPU
/// et memoire utilisee, en pourcentages (memoire absente si sa taille est
/// inconnue).
pub async fn current_load(
    conn: &mut PgConnection,
    now: f64,
) -> sqlx::Result<HashMap<Uuid, (f64, Option<f64>)>> {
    let rows: Vec<(Uuid, f32, i64, i64)> = sqlx::query_as(
        "SELECT DISTINCT ON (server_id) server_id, cpu_percent, mem_used_bytes, mem_total_bytes \
         FROM node_metrics_raw WHERE at > to_timestamp($1::float8) ORDER BY server_id, at DESC",
    )
    .bind(now - FRESH.as_secs_f64())
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(s, cpu, used, total)| {
            let mem = (total > 0).then(|| (used as f64 / total as f64 * 1000.0).round() / 10.0);
            (s, (f64::from(cpu), mem))
        })
        .collect())
}

/// CPU et memoire courants (mesure de moins de 2 minutes, conteneur en
/// marche) du conteneur servi de chaque service demande.
pub async fn current_usage(
    conn: &mut PgConnection,
    services: &[Uuid],
    now: f64,
) -> sqlx::Result<HashMap<Uuid, (f64, u64)>> {
    let rows: Vec<(Uuid, f32, i64, bool)> = sqlx::query_as(
        "SELECT DISTINCT ON (service_id) service_id, cpu_percent, mem_used_bytes, running \
         FROM container_metrics_raw WHERE service_id = ANY($1) AND at > to_timestamp($2::float8) \
         ORDER BY service_id, at DESC",
    )
    .bind(services)
    .bind(now - FRESH.as_secs_f64())
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|r| r.3)
        .map(|(s, cpu, mem, _)| (s, (f64::from(cpu), u64::try_from(mem).unwrap_or(0))))
        .collect())
}

/// Disponibilite des services sur 30 jours : pourcentage global et serie
/// journaliere (jours mesures seulement, le plus ancien d'abord). Aucune
/// mesure : `None` et serie vide.
pub async fn uptime(
    conn: &mut PgConnection,
    services: &[Uuid],
    now: f64,
) -> sqlx::Result<(Option<f64>, Vec<f64>)> {
    // Agregats des heures absentes des mesures brutes, plus les mesures
    // brutes : aucune heure comptee deux fois.
    let days: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT sum(up)::bigint, sum(n)::bigint FROM ( \
           SELECT h.hour AS at, h.running_samples AS up, h.samples AS n \
           FROM container_metrics_hourly h \
           WHERE h.service_id = ANY($1) AND h.hour >= to_timestamp($2::float8) \
             AND NOT EXISTS (SELECT 1 FROM container_metrics_raw r \
                             WHERE r.service_id = h.service_id AND date_trunc('hour', r.at) = h.hour) \
           UNION ALL \
           SELECT at, running::int, 1 FROM container_metrics_raw \
           WHERE service_id = ANY($1) AND at >= to_timestamp($2::float8) \
         ) s GROUP BY date_trunc('day', at) ORDER BY date_trunc('day', at)",
    )
    .bind(services)
    .bind(now - HOURLY_RETENTION.as_secs_f64())
    .fetch_all(conn)
    .await?;
    let pct = |up: i64, n: i64| {
        if n > 0 {
            (up as f64 / n as f64 * 1000.0).round() / 10.0
        } else {
            0.0
        }
    };
    let series: Vec<f64> = days.iter().map(|(up, n)| pct(*up, *n)).collect();
    let (up, n) = days
        .iter()
        .fold((0i64, 0i64), |(a, b), (up, n)| (a + up, b + n));
    Ok(((n > 0).then(|| pct(up, n)), series))
}
