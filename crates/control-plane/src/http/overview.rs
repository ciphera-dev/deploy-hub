//! `GET /api/overview?period=7d|30d` (viewer et plus) : vue d'ensemble de
//! l'instance. Elements « A traiter » (production en echec ou arretee hors
//! du panneau, serveur injoignable), projets et serveurs en ligne,
//! deploiements du jour et par jour, derniers deploiements, charge des
//! serveurs. Requetes et latence n'ont aucune source en v0.1 : series vides,
//! jamais de valeurs inventees.

use api_types::overview::{
    ChartPoint, IssueKind, KpiSeries, Overview, OverviewChart, OverviewIssue, OverviewPeriod,
    ServerLoad,
};
use api_types::projects::ProjectStatus;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use shared::error::ApiError;
use shared::ids::ServerId;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::domain::deployments::{self, Filter};
use crate::domain::metrics;
use crate::domain::projects::store;
use crate::state::AppState;

/// Derniers deploiements affiches.
const RECENT: i64 = 5;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/overview", get(overview))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverviewQuery {
    #[serde(default)]
    period: OverviewPeriod,
}

fn db(context: &'static str) -> impl Fn(sqlx::Error) -> ApiError {
    move |e| internal(context, e)
}

/// Variation en pourcentage (une decimale) ; sans base : absente.
fn change(current: f64, previous: f64) -> Option<f64> {
    (previous > 0.0).then(|| ((current - previous) / previous * 1000.0).round() / 10.0)
}

/// `GET /api/overview`
async fn overview(
    State(state): State<AppState>,
    _auth: Require<res::Projects, lvl::Read>,
    query: Result<Query<OverviewQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(OverviewQuery { period }) = query
        .map_err(|_| ApiError::new(422, "validation_failed", "Période inconnue : 7d ou 30d."))?;
    let now = state.clock.unix_secs();
    let days = period.days();

    let projects = store::list(&state.db, &state.agents)
        .await
        .map_err(db("liste des projets"))?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;

    // Un point par jour (UTC), le plus ancien d'abord, plus le total de la
    // periode precedente.
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT to_char(g.day, 'YYYY-MM-DD'), count(d.id) \
         FROM generate_series( \
           date_trunc('day', to_timestamp($1::float8) AT TIME ZONE 'UTC') - make_interval(days => $2::int - 1), \
           date_trunc('day', to_timestamp($1::float8) AT TIME ZONE 'UTC'), interval '1 day') AS g(day) \
         LEFT JOIN deployments d ON date_trunc('day', d.created_at AT TIME ZONE 'UTC') = g.day \
         GROUP BY g.day ORDER BY g.day",
    )
    .bind(now)
    .bind(days as i32)
    .fetch_all(&mut *conn)
    .await
    .map_err(db("deploiements par jour"))?;
    let previous: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM deployments WHERE \
         created_at AT TIME ZONE 'UTC' >= date_trunc('day', to_timestamp($1::float8) AT TIME ZONE 'UTC') - make_interval(days => 2 * $2::int - 1) \
         AND created_at AT TIME ZONE 'UTC' < date_trunc('day', to_timestamp($1::float8) AT TIME ZONE 'UTC') - make_interval(days => $2::int - 1)",
    )
    .bind(now)
    .bind(days as i32)
    .fetch_one(&mut *conn)
    .await
    .map_err(db("deploiements de la periode precedente"))?;
    let deployments_today = rows.last().map_or(0, |r| r.1);
    let total: i64 = rows.iter().map(|r| r.1).sum();
    let points = rows
        .into_iter()
        .map(|(date, n)| ChartPoint {
            date,
            value: n as f64,
        })
        .collect();

    let servers: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, name, kind FROM servers WHERE revoked_at IS NULL \
         ORDER BY (kind = 'local') DESC, created_at, name",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(db("liste des serveurs"))?;
    let load = metrics::current_load(&mut conn, now)
        .await
        .map_err(db("charge des serveurs"))?;
    drop(conn);

    let mut issues: Vec<OverviewIssue> = projects
        .iter()
        .filter_map(|p| {
            let kind = match p.status {
                ProjectStatus::Failed => IssueKind::ProjectFailed,
                ProjectStatus::Down => IssueKind::ProjectDown,
                _ => return None,
            };
            Some(OverviewIssue {
                kind,
                id: p.id.clone(),
                name: p.name.clone(),
            })
        })
        .collect();
    let servers: Vec<ServerLoad> = servers
        .into_iter()
        .map(|(id, name, kind)| {
            let online = state.agents.is_connected(ServerId(id));
            let usage = load.get(&id).filter(|_| online);
            ServerLoad {
                id: id.to_string(),
                name,
                online,
                kind,
                cpu_percent: usage.map(|u| u.0),
                memory_percent: usage.and_then(|u| u.1),
                link_error: state.agents.link_error(ServerId(id)).filter(|_| !online),
            }
        })
        .collect();
    issues.extend(servers.iter().filter(|s| !s.online).map(|s| OverviewIssue {
        kind: IssueKind::ServerUnreachable,
        id: s.id.clone(),
        name: s.name.clone(),
    }));

    let recent_deployments = deployments::list(
        &state.db,
        &Filter {
            project: None,
            status: None,
            limit: RECENT,
        },
    )
    .await
    .map_err(db("derniers deploiements"))?;

    let body = Overview {
        issues,
        projects_online: projects
            .iter()
            .filter(|p| p.status == ProjectStatus::Running)
            .count() as i64,
        projects_total: projects.len() as i64,
        servers_online: servers.iter().filter(|s| s.online).count() as i64,
        servers_total: servers.len() as i64,
        deployments_today,
        period,
        chart: OverviewChart {
            deployments: KpiSeries {
                total: Some(total as f64),
                change_percent: change(total as f64, previous as f64),
                points,
            },
            requests: KpiSeries::default(),
            latency_p95_ms: KpiSeries::default(),
        },
        recent_deployments,
        servers,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_needs_a_previous_period() {
        assert_eq!(change(3.0, 0.0), None);
        assert_eq!(change(3.0, 2.0), Some(50.0));
        assert_eq!(change(1.0, 3.0), Some(-66.7));
    }
}
