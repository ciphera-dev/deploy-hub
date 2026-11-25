//! Deploiements : `POST /api/projects/{id}/deployments`,
//! `GET /api/deployments[?project=&status=&limit=]`, `GET /api/deployments/{id}`,
//! `POST /api/deployments/{id}/cancel` et `POST /api/deployments/{id}/rollback`.
//!
//! Lecture : viewer et plus ; creation, annulation et rollback : developer
//! et plus (`Require<res::Deployments, _>`). Les ecritures passent par la
//! couche CSRF et sont auditees.

use api_types::deployments::{DeploymentStatus, NewDeployment};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use shared::error::ApiError;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::domain::deployments::orchestrator::{self, RequestError};
use crate::domain::deployments::{self, Filter};
use crate::domain::projects::store::PRODUCTION;
use crate::state::AppState;

const MAX_BODY: usize = 4 * 1024;
const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/deployments", post(create))
        .route("/api/deployments", get(list))
        .route("/api/deployments/{id}", get(detail))
        .route("/api/deployments/{id}/cancel", post(cancel))
        .route("/api/deployments/{id}/rollback", post(rollback))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn not_found() -> ApiError {
    ApiError::new(404, "not_found", "Déploiement introuvable.")
}

fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| not_found())
}

pub(super) fn request_error(context: &str, e: RequestError) -> ApiError {
    match e {
        RequestError::NotFound => not_found(),
        RequestError::AlreadyFinished => {
            ApiError::new(409, "already_finished", "Ce déploiement est déjà terminé.")
        }
        RequestError::NotRollbackable => ApiError::new(
            409,
            "not_rollbackable",
            "Ce déploiement n'a pas d'image conservée : rollback impossible.",
        ),
        RequestError::InProgress => ApiError::new(
            409,
            "deployment_in_progress",
            "Un déploiement est déjà en cours",
        ),
        RequestError::TooManyQueued => ApiError::new(
            429,
            "too_many_queued",
            "Trop de déploiements en file pour cet environnement (10 au plus) : attendez la fin des précédents.",
        ),
        RequestError::Db(e) => internal(context, e),
    }
}

pub(super) async fn read(
    state: &AppState,
    id: Uuid,
) -> Result<api_types::deployments::Deployment, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| internal("connexion a la base", e))?;
    deployments::get(&mut conn, id)
        .await
        .map_err(|e| internal("lecture du deploiement", e))?
        .ok_or_else(not_found)
}

/// `POST /api/projects/{id}/deployments`
async fn create(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path(project): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let project_not_found = || ApiError::new(404, "not_found", "Projet introuvable.");
    let project = Uuid::parse_str(&project).map_err(|_| project_not_found())?;
    // Corps facultatif : vide = environnement `production`.
    let input: NewDeployment = if body.is_empty() {
        NewDeployment::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| invalid("Requête invalide."))?
    };
    let environment = input.environment.unwrap_or_else(|| PRODUCTION.to_string());
    shared::validate::name(&environment).map_err(|_| invalid("Environnement invalide."))?;
    let id = orchestrator::create_manual(&state, project, &environment, auth.user.id)
        .await
        .map_err(|e| match e {
            RequestError::NotFound => {
                ApiError::new(404, "not_found", "Projet ou environnement introuvable.")
            }
            other => request_error("creation du deploiement", other),
        })?;
    Ok((StatusCode::CREATED, Json(read(&state, id).await?)).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    project: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
}

/// `GET /api/deployments`
async fn list(
    State(state): State<AppState>,
    _auth: Require<res::Deployments, lvl::Read>,
    query: Result<Query<ListQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = query.map_err(|_| invalid("Filtre invalide."))?;
    let project = q
        .project
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| invalid("Projet invalide."))?;
    let status = q
        .status
        .as_deref()
        .map(|s| DeploymentStatus::parse(s).ok_or_else(|| invalid("Statut invalide.")))
        .transpose()?;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(invalid("Limite invalide (1 à 200)."));
    }
    let rows = deployments::list(
        &state.db,
        &Filter {
            project,
            status,
            limit,
        },
    )
    .await
    .map_err(|e| internal("liste des deploiements", e))?;
    Ok(Json(rows).into_response())
}

/// `GET /api/deployments/{id}`
async fn detail(
    State(state): State<AppState>,
    _auth: Require<res::Deployments, lvl::Read>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    Ok(Json(read(&state, id).await?).into_response())
}

/// `POST /api/deployments/{id}/cancel`
async fn cancel(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    orchestrator::cancel(&state, id, auth.user.id)
        .await
        .map_err(|e| request_error("annulation du deploiement", e))?;
    Ok(Json(read(&state, id).await?).into_response())
}

/// `POST /api/deployments/{id}/rollback`
async fn rollback(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let created = orchestrator::rollback(&state, id, auth.user.id)
        .await
        .map_err(|e| request_error("rollback du deploiement", e))?;
    Ok((StatusCode::CREATED, Json(read(&state, created).await?)).into_response())
}
