//! Onglet Git et en-tete d'un projet : `GET|PATCH /api/projects/{id}/git`,
//! `POST /api/projects/{id}/restart` et `POST /api/projects/{id}/redeploy`.
//!
//! Lecture des reglages : viewer et plus ; modification : developer et plus
//! (`Projects` en ecriture). Redemarrage et redeploiement : developer et plus
//! (`Deployments` en ecriture). Ecritures sous CSRF et auditees.

use api_types::project_git::{RestartRequested, UpdateGitSettings};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use uuid::Uuid;

use super::deployments::{read, request_error};
use super::project_overview::action_error;
use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::domain::deployments::orchestrator::{self, Guard, Origin, Requested};
use crate::domain::projects::git::{self, GitError};
use crate::domain::projects::services::{self, Action, Target};
use crate::domain::projects::store::PRODUCTION;
use crate::state::AppState;

const MAX_BODY: usize = 16 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/git", get(settings).patch(update))
        .route("/api/projects/{id}/restart", post(restart))
        .route("/api/projects/{id}/redeploy", post(redeploy))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn not_found() -> ApiError {
    ApiError::new(404, "not_found", "Projet introuvable.")
}

fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| not_found())
}

fn git_error(context: &str, e: GitError) -> ApiError {
    match e {
        GitError::NotFound => not_found(),
        GitError::NotGit => ApiError::new(
            409,
            "not_git",
            "Ce projet déploie une image : il n'a pas de réglages Git.",
        ),
        GitError::Db(e) => internal(context, e),
    }
}

/// `GET /api/projects/{id}/git`
async fn settings(
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
    let settings = git::get(&mut conn, id, false)
        .await
        .map_err(|e| git_error("lecture des reglages Git", e))?;
    Ok(Json(settings).into_response())
}

/// `PATCH /api/projects/{id}/git`
async fn update(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Write>,
    Path(id): Path<String>,
    body: Result<Json<UpdateGitSettings>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    if input.branch.is_none()
        && input.root_directory.is_none()
        && input.watch_paths.is_none()
        && input.auto_deploy.is_none()
        && input.skip_ci.is_none()
        && input.ignore_bots.is_none()
    {
        return Err(invalid("Aucune modification demandée."));
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction des reglages Git", e))?;
    let current = git::get(&mut tx, id, true)
        .await
        .map_err(|e| git_error("lecture des reglages Git", e))?;
    let settings = git::apply(current, input).map_err(invalid)?;
    git::save(&mut tx, id, &settings)
        .await
        .map_err(|e| internal("ecriture des reglages Git", e))?;
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, $2, 'project.git.update', 'project', $3)",
    )
    .bind(Uuid::new_v4())
    .bind(auth.user.id)
    .bind(json!({
        "project_id": id,
        "branch": settings.branch,
        "auto_deploy": settings.auto_deploy,
        "watch_paths": settings.watch_paths.len(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("audit des reglages Git", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des reglages Git", e))?;
    Ok(Json(settings).into_response())
}

/// `POST /api/projects/{id}/restart` : conteneurs du deploiement servi de
/// la production redemarres par l'agent (sans rebuild) ; relance un service
/// arrete.
async fn restart(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let deployment = services::act(&state, auth.user.id, id, Target::Project, Action::Restart)
        .await
        .map_err(|e| {
            action_error(
                "redemarrage du projet",
                e,
                "Projet introuvable.",
                "Aucun déploiement en ligne à redémarrer.",
            )
        })?;
    Ok((
        StatusCode::ACCEPTED,
        Json(RestartRequested {
            deployment_id: deployment.to_string(),
        }),
    )
        .into_response())
}

/// `POST /api/projects/{id}/redeploy` : deploiement manuel de la
/// production, refuse si un deploiement est deja en file ou en cours.
async fn redeploy(
    State(state): State<AppState>,
    auth: Require<res::Deployments, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let requested = orchestrator::request(
        &state,
        id,
        PRODUCTION,
        Origin::Manual(auth.user.id),
        Guard::RefuseActive,
    )
    .await
    .map_err(|e| match e {
        orchestrator::RequestError::NotFound => not_found(),
        other => request_error("redeploiement", other),
    })?;
    let (Requested::Created(created) | Requested::AlreadyQueued(created)) = requested;
    Ok((StatusCode::CREATED, Json(read(&state, created).await?)).into_response())
}
