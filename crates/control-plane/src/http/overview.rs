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
use crate::authz::{Reads, Require, lvl, res};
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

/// Retire ce que le role ne lit pas : serveurs (liste, comptes, alertes de
/// serveur injoignable) sans `Servers:Read`, deploiements (liste, compte du
/// jour, graphe) sans `Deployments:Read`. Les champs absents sont omis.
fn restrict(o: &mut Overview, reads: Reads) {
    if !reads.servers {
        o.servers = None;
        o.servers_online = 0;
        o.servers_total = 0;
        o.issues.retain(|i| i.kind != IssueKind::ServerUnreachable);
    }
    if !reads.deployments {
        o.recent_deployments = None;
        o.deployments_today = 0;
        o.chart.deployments = KpiSeries::default();
    }
}

/// `GET /api/overview`
async fn overview(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Read>,
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
        recent_deployments: Some(recent_deployments),
        servers: Some(servers),
    };
    let mut body = body;
    restrict(&mut body, Reads::of(auth.role));
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authz::{Resource, Role};

    #[test]
    fn change_needs_a_previous_period() {
        assert_eq!(change(3.0, 0.0), None);
        assert_eq!(change(3.0, 2.0), Some(50.0));
        assert_eq!(change(1.0, 3.0), Some(-66.7));
    }

    fn full() -> Overview {
        Overview {
            issues: vec![
                OverviewIssue {
                    kind: IssueKind::ProjectDown,
                    id: "p".into(),
                    name: "app".into(),
                },
                OverviewIssue {
                    kind: IssueKind::ServerUnreachable,
                    id: "s".into(),
                    name: "edge".into(),
                },
            ],
            projects_online: 1,
            projects_total: 1,
            servers_online: 0,
            servers_total: 1,
            deployments_today: 2,
            period: OverviewPeriod::Days7,
            chart: OverviewChart {
                deployments: KpiSeries {
                    total: Some(2.0),
                    change_percent: None,
                    points: vec![],
                },
                requests: KpiSeries::default(),
                latency_p95_ms: KpiSeries::default(),
            },
            recent_deployments: Some(vec![]),
            servers: Some(vec![ServerLoad {
                id: "s".into(),
                name: "edge".into(),
                online: false,
                kind: "remote".into(),
                cpu_percent: None,
                memory_percent: None,
                link_error: None,
            }]),
        }
    }

    #[test]
    fn overview_hides_servers_for_viewer_without_servers_read() {
        let reads = Reads::with(|r| r != Resource::Servers);
        let mut o = full();
        restrict(&mut o, reads);
        let json = serde_json::to_value(&o).unwrap();
        assert!(json.get("servers").is_none(), "{json}");
        assert_eq!(json["servers_total"], 0);
        assert_eq!(json["servers_online"], 0);
        let kinds: Vec<_> = o.issues.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, [IssueKind::ProjectDown]);
        // Les deploiements restent, faute d'avoir ete retires.
        assert!(json.get("recent_deployments").is_some());
        assert_eq!(json["deployments_today"], 2);
    }

    #[test]
    fn overview_hides_deployments_without_deployments_read() {
        let reads = Reads::with(|r| r != Resource::Deployments);
        let mut o = full();
        restrict(&mut o, reads);
        let json = serde_json::to_value(&o).unwrap();
        assert!(json.get("recent_deployments").is_none(), "{json}");
        assert_eq!(json["deployments_today"], 0);
        assert_eq!(
            json["chart"]["deployments"]["total"],
            serde_json::Value::Null
        );
        assert!(json.get("servers").is_some());
    }

    #[test]
    fn default_roles_keep_the_whole_overview() {
        for role in [
            Role::Owner,
            Role::Admin,
            Role::Ops,
            Role::Developer,
            Role::Viewer,
        ] {
            let mut o = full();
            restrict(&mut o, Reads::of(role));
            let json = serde_json::to_value(&o).unwrap();
            assert!(json.get("servers").is_some() && json.get("recent_deployments").is_some());
        }
    }
}
