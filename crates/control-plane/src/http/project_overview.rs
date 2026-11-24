//! Apercu et services d'un projet : `GET /api/projects/{id}/overview`,
//! `GET /api/projects/{id}/services` et
//! `POST /api/projects/{id}/services/{sid}/{restart,stop}`.
//!
//! Lecture : viewer et plus (`Projects`). Redemarrage et arret : developer
//! et plus (`Deployments` en ecriture, comme le redemarrage du projet),
//! sous CSRF et audites. La disponibilite vient de l'etat du conteneur servi
//! remonte par l'agent ; requetes, latence et erreurs restent vides (aucune
//! source en v0.1) ; le domaine, tant qu'aucun n'est configure.

use api_types::project_overview::{
    OverviewMetrics, ProjectOverview, ServiceActionAccepted, Sparkline,
};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use shared::error::ApiError;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::domain::deployments::{self, Filter};
use crate::domain::metrics;
use crate::domain::projects::services::{self, Action, ActionError, Target};
use crate::domain::projects::store::{self, PRODUCTION};
use crate::state::AppState;

/// Deploiements de l'activite recente.
const ACTIVITY: i64 = 4;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/overview", get(overview))
        .route("/api/projects/{id}/services", get(list))
        .route("/api/projects/{id}/services/{sid}/restart", post(restart))
        .route("/api/projects/{id}/services/{sid}/stop", post(stop))
}

fn not_found() -> ApiError {
    ApiError::new(404, "not_found", "Projet introuvable.")
}

fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| not_found())
}

/// Erreurs d'un ordre envoye a l'agent (redemarrage, arret), communes au
/// projet et a ses services.
pub(super) fn action_error(
    context: &str,
    e: ActionError,
    missing: &'static str,
    nothing: &'static str,
) -> ApiError {
    match e {
        ActionError::NotFound => ApiError::new(404, "not_found", missing),
        ActionError::NothingServing => ApiError::new(409, "nothing_serving", nothing),
        ActionError::AgentUnavailable => ApiError::new(
            503,
            "agent_unavailable",
            "Le serveur du projet est injoignable. Réessayez plus tard.",
        ),
        ActionError::Db(e) => internal(context, e),
    }
}

/// `GET /api/projects/{id}/overview`
async fn overview(
    State(state): State<AppState>,
    _auth: Require<res::Projects, lvl::Read>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| internal("connexion a la base", e))?;
    let project = store::get(&mut conn, &state.agents, id)
        .await
        .map_err(|e| internal("lecture du projet", e))?
        .ok_or_else(not_found)?;
    let services = services::list(&state, &mut conn, id)
        .await
        .map_err(|e| internal("lecture des services", e))?
        .ok_or_else(not_found)?;
    let production = match project
        .environments
        .iter()
        .find(|e| e.name == PRODUCTION)
        .and_then(|e| Uuid::parse_str(&e.id).ok())
    {
        Some(env) => services::serving(&mut conn, env)
            .await
            .map_err(|e| internal("lecture du deploiement servi", e))?,
        None => None,
    };
    let production = match production {
        Some((deployment, _)) => deployments::get(&mut conn, deployment)
            .await
            .map_err(|e| internal("lecture du deploiement servi", e))?,
        None => None,
    };
    let activity = deployments::list(
        &state.db,
        &Filter {
            project: Some(id),
            status: None,
            limit: ACTIVITY,
        },
    )
    .await
    .map_err(|e| internal("activite du projet", e))?;
    let domain = crate::domain::domains::primary_host(&mut conn, id)
        .await
        .map_err(|e| internal("domaine du projet", e))?;
    let production_services: Vec<Uuid> = services
        .iter()
        .filter(|s| s.environment == PRODUCTION)
        .filter_map(|s| Uuid::parse_str(&s.id).ok())
        .collect();
    let (uptime, uptime_series) =
        metrics::uptime(&mut conn, &production_services, state.clock.unix_secs())
            .await
            .map_err(|e| internal("disponibilite du projet", e))?;
    Ok(Json(ProjectOverview {
        status: project.status,
        production,
        domain,
        metrics: OverviewMetrics {
            uptime: Sparkline {
                value: uptime,
                series: uptime_series,
            },
            ..OverviewMetrics::default()
        },
        services,
        activity,
    })
    .into_response())
}

/// `GET /api/projects/{id}/services`
async fn list(
    State(state): State<AppState>,
    _auth: Require<res::Projects, lvl::Read>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| internal("connexion a la base", e))?;
    let services = services::list(&state, &mut conn, id)
        .await
        .map_err(|e| internal("lecture des services", e))?
        .ok_or_else(not_found)?;
    Ok(Json(services).into_response())
}

async fn service_action(
    state: &AppState,
    actor: Uuid,
    project: &str,
    service: &str,
    action: Action,
) -> Result<Response, ApiError> {
    let missing = "Service introuvable.";
    let not_found = || ApiError::new(404, "not_found", missing);
    let project = Uuid::parse_str(project).map_err(|_| not_found())?;
    let service = Uuid::parse_str(service).map_err(|_| not_found())?;
    let deployment = services::act(state, actor, project, Target::Service(service), action)
        .await
        .map_err(|e| {
            action_error(
                "ordre au service",
                e,
                missing,
                "Aucun déploiement en ligne pour ce service.",
            )
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(ServiceActionAccepted {
            deployment_id: deployment.to_string(),
        }),
    )
        .into_response())
}

/// `POST /api/projects/{id}/services/{sid}/restart`
async fn restart(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    service_action(&state, auth.user.id, &id, &sid, Action::Restart).await
}

/// `POST /api/projects/{id}/services/{sid}/stop`
async fn stop(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path((id, sid)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    service_action(&state, auth.user.id, &id, &sid, Action::Stop).await
}
