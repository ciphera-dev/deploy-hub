//! Projets : `GET|POST /api/projects`, `GET|PATCH|DELETE /api/projects/{id}`.
//!
//! Lecture : viewer et plus ; creation et modification : developer et plus ;
//! suppression : owner et admin. Les ecritures passent par la couche CSRF.
//! Un projet cree recoit l'environnement `production` et son service. La
//! suppression envoie `RemoveDeployment` aux agents pour chacun de ses
//! deploiements (au mieux), puis efface les lignes de la base (cascade). Nom, depot, branche, chemins et
//! image sont valides en liste blanche avant toute ecriture.

use api_types::projects::{Build, NewProject, Source, UpdateProject};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::domain::deployments::orchestrator;
use crate::domain::projects::store::{self, Draft, StoreError};
use crate::domain::projects::validate;
use crate::state::AppState;

const MAX_BODY: usize = 16 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list).post(create))
        .route(
            "/api/projects/{id}",
            get(detail).patch(update).delete(remove),
        )
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

fn store_error(context: &str, e: StoreError) -> ApiError {
    match e {
        StoreError::NameTaken => {
            ApiError::new(409, "name_taken", "Ce nom de projet est déjà utilisé.")
        }
        StoreError::NotFound => not_found(),
        StoreError::Db(e) => internal(context, e),
    }
}

async fn begin(state: &AppState) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction des projets", e))
}

async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    project: Uuid,
    name: &str,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, 'project', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(json!({ "project_id": project, "name": name }))
    .execute(conn)
    .await
    .map_err(|e| internal("audit des projets", e))?;
    Ok(())
}

fn valid_draft(name: String, source: Source, build: Option<Build>) -> Result<Draft, ApiError> {
    validate::project_name(&name).map_err(invalid)?;
    let source = validate::source(source).map_err(invalid)?;
    match (&source, &build) {
        (Source::Git(_), None) => return Err(invalid("Choisissez une méthode de build.")),
        (Source::Image(_), Some(_)) => {
            return Err(invalid("Une image ne se construit pas : retirez le build."));
        }
        (Source::Git(_), Some(b)) => validate::build(b).map_err(invalid)?,
        (Source::Image(_), None) => {}
    }
    Ok(Draft {
        name,
        source,
        build,
    })
}

/// `GET /api/projects`
async fn list(
    State(state): State<AppState>,
    _auth: Require<res::Projects, lvl::Read>,
) -> Result<Response, ApiError> {
    let projects = store::list(&state.db, &state.agents)
        .await
        .map_err(|e| internal("liste des projets", e))?;
    Ok(Json(projects).into_response())
}

/// `POST /api/projects`
async fn create(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Write>,
    body: Result<Json<NewProject>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let draft = valid_draft(input.name, input.source, input.build)?;
    let mut tx = begin(&state).await?;
    let id = store::create(&mut tx, &draft, auth.user.id)
        .await
        .map_err(|e| store_error("creation du projet", e))?;
    audit(&mut tx, auth.user.id, "project.create", id, &draft.name).await?;
    // Domaine genere `<projet>.<ip>.sslip.io` (desactivable).
    crate::domain::domains::ensure_generated(&mut tx, &state.cfg, id)
        .await
        .map_err(|e| internal("domaine genere", e))?;
    let project = store::get(&mut tx, &state.agents, id)
        .await
        .map_err(|e| internal("lecture du projet", e))?
        .ok_or_else(not_found)?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des projets", e))?;
    Ok((StatusCode::CREATED, Json(project)).into_response())
}

/// `GET /api/projects/{id}`
async fn detail(
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
    Ok(Json(project).into_response())
}

/// `PATCH /api/projects/{id}`
async fn update(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Write>,
    Path(id): Path<String>,
    body: Result<Json<UpdateProject>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    if input.name.is_none() && input.branch.is_none() && input.build.is_none() {
        return Err(invalid("Aucune modification demandée."));
    }
    let mut tx = begin(&state).await?;
    let (old_name, mut source, mut build) = store::lock(&mut tx, id)
        .await
        .map_err(|e| internal("lecture du projet", e))?
        .ok_or_else(not_found)?;
    if let Some(new_branch) = input.branch {
        match &mut source {
            Source::Git(git) => git.branch = new_branch,
            Source::Image(_) => return Err(invalid("Une image n'a pas de branche.")),
        }
    }
    if let Some(new_build) = input.build {
        if matches!(source, Source::Image(_)) {
            return Err(invalid("Une image ne se construit pas."));
        }
        build = Some(new_build);
    }
    let name = input.name.unwrap_or_else(|| old_name.clone());
    let draft = valid_draft(name, source, build)?;
    store::update(&mut tx, id, &draft)
        .await
        .map_err(|e| store_error("modification du projet", e))?;
    audit(&mut tx, auth.user.id, "project.update", id, &draft.name).await?;
    // Renomme : le domaine genere suit le nom.
    let renamed = draft.name != old_name;
    if renamed {
        crate::domain::domains::regenerate(&mut tx, &state.cfg, id)
            .await
            .map_err(|e| internal("domaine genere", e))?;
    }
    let project = store::get(&mut tx, &state.agents, id)
        .await
        .map_err(|e| internal("lecture du projet", e))?
        .ok_or_else(not_found)?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des projets", e))?;
    if renamed {
        crate::domain::domains::push_routes(&state).await;
    }
    Ok(Json(project).into_response())
}

/// `DELETE /api/projects/{id}` : conteneurs retires des agents, puis cascade.
async fn remove(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Admin>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    match delete_project(&state, auth.user.id, id, false).await? {
        Removal::NotFound => Err(not_found()),
        // Jamais renvoye sans `require_agents`.
        Removal::AgentUnreachable(_) => Err(internal("suppression du projet", "agent")),
        Removal::Deleted(hosts) => {
            // Domaines du projet supprimes en cascade : routes et certificats
            // retires.
            crate::domain::domains::push_routes(&state).await;
            crate::domain::domains::remove_certificates(&state, &hosts);
            Ok(StatusCode::NO_CONTENT.into_response())
        }
    }
}

/// Issue de [`delete_project`].
pub(crate) enum Removal {
    /// Supprime ; hotes de ses domaines (certificats a effacer, table des
    /// routes a renvoyer par l'appelant).
    Deleted(Vec<String>),
    NotFound,
    /// `require_agents` : un agent qui porte ses conteneurs est injoignable ;
    /// rien n'est supprime (serveurs en cause).
    AgentUnreachable(Vec<Uuid>),
}

/// Supprime un projet : ordres `RemoveDeployment` aux agents, puis cascade en
/// base et audit. Sans `require_agents`, un agent absent est seulement
/// journalise (ses conteneurs restent) ; avec, le projet est garde tant
/// qu'un ordre ne peut pas partir.
pub(crate) async fn delete_project(
    state: &AppState,
    actor: Uuid,
    id: Uuid,
    require_agents: bool,
) -> Result<Removal, ApiError> {
    let mut tx = begin(state).await?;
    let Some((name, _, _)) = store::lock(&mut tx, id)
        .await
        .map_err(|e| internal("lecture du projet", e))?
    else {
        return Ok(Removal::NotFound);
    };
    if require_agents {
        let offline = orchestrator::unreachable_servers(state, id)
            .await
            .map_err(|e| internal("serveurs du projet", e))?;
        if !offline.is_empty() {
            return Ok(Removal::AgentUnreachable(offline));
        }
    }
    // Conteneurs et images du projet retires des agents avant la cascade
    // (au mieux : un agent absent est journalise).
    let sent = orchestrator::remove_project_resources(state, id).await;
    if require_agents && !sent {
        return Ok(Removal::AgentUnreachable(Vec::new()));
    }
    // Domaines du projet (supprimes en cascade) : certificats a effacer.
    let hosts: Vec<String> =
        sqlx::query_scalar("SELECT host FROM domains WHERE project_id = $1 AND NOT generated")
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| internal("domaines du projet", e))?;
    store::delete(&mut tx, id)
        .await
        .map_err(|e| store_error("suppression du projet", e))?;
    audit(&mut tx, actor, "project.delete", id, &name).await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des projets", e))?;
    Ok(Removal::Deleted(hosts))
}
