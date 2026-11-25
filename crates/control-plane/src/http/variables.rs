//! Variables d'environnement :
//! - du projet, par environnement (`?env=`, `production` par defaut) :
//!   `GET` (Read : ops, developer, admin, owner), `PUT`, `PUT …/bulk`
//!   (format `.env`) et `DELETE …/{key}` (Write : developer, admin, owner) ;
//! - globales : `GET|POST /api/variables`, `PATCH|DELETE /api/variables/{id}`
//!   et `GET /api/variables/export` (Admin).
//!
//! Ecritures sous CSRF (couche du routeur) et auditees, sans valeur dans
//! l'audit. Un secret n'est jamais renvoye. Chaque ecriture verifie les
//! plafonds des projets concernes (`vars::check_caps`, 422 sinon), relit le
//! masquage de leurs journaux (`LogHub::invalidate`), et signale un secret
//! trop court pour etre masque (`warnings: ["too_short_to_redact"]`).

use api_types::variables::{
    BulkProjectVariables, GlobalVariable, NewGlobalVariable, PutProjectVariable,
    UpdateGlobalVariable, VariablesStatus,
};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::domain::logs::redact::TOO_SHORT_TO_REDACT;
use crate::domain::projects::store::PRODUCTION;
use crate::domain::variables::{self as vars, Scope, VarError, Wanted};
use crate::state::AppState;

/// Corps au plus (contenu `.env` d'un environnement).
const MAX_BODY: usize = 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/projects/{id}/variables",
            get(list_project).put(put_project),
        )
        .route("/api/projects/{id}/variables/bulk", put(bulk_project))
        .route("/api/projects/{id}/variables/status", get(status_project))
        .route("/api/projects/{id}/variables/{key}", delete(delete_project))
        .route("/api/variables", get(list_globals).post(create_global))
        .route("/api/variables/export", get(export_globals))
        .route(
            "/api/variables/{id}",
            patch(update_global).delete(delete_global),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

#[derive(Deserialize)]
struct EnvQuery {
    env: Option<String>,
}

fn project_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Projet introuvable.")
}

fn variable_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Variable introuvable.")
}

fn unprocessable(message: impl Into<String>) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn var_error(e: VarError) -> ApiError {
    match e {
        VarError::Invalid(m) => unprocessable(m),
        VarError::Overlap => ApiError::new(422, "variable_scope_overlap", vars::OVERLAP_MESSAGE),
        VarError::NotFound => variable_not_found(),
        VarError::Seal => internal("variables", "chiffrement impossible"),
        VarError::Db(e) => internal("variables", e),
    }
}

fn db(context: &'static str) -> impl Fn(sqlx::Error) -> ApiError {
    move |e| internal(context, e)
}

fn no_store(mut res: Response) -> Response {
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn env_name(query: Result<Query<EnvQuery>, QueryRejection>) -> Result<String, ApiError> {
    let name = query
        .ok()
        .and_then(|Query(q)| q.env)
        .unwrap_or_else(|| PRODUCTION.to_string());
    if !vars::valid_environment(&name) {
        return Err(unprocessable("Nom d'environnement invalide."));
    }
    Ok(name)
}

/// Projet et environnement (404 distincts).
async fn target(
    conn: &mut PgConnection,
    project: &str,
    env: &str,
) -> Result<(Uuid, Uuid), ApiError> {
    let project = Uuid::parse_str(project).map_err(|_| project_not_found())?;
    let exists: Option<bool> = sqlx::query_scalar("SELECT true FROM projects WHERE id = $1")
        .bind(project)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db("lecture du projet"))?;
    if exists.is_none() {
        return Err(project_not_found());
    }
    let environment = vars::environment(conn, project, env)
        .await
        .map_err(db("lecture de l'environnement"))?
        .ok_or_else(|| ApiError::new(404, "not_found", "Environnement introuvable."))?;
    Ok((project, environment))
}

async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    target: &str,
    detail: Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(target)
    .bind(detail)
    .execute(conn)
    .await
    .map_err(db("audit des variables"))?;
    Ok(())
}

async fn project_list(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    env: &str,
) -> Result<Response, ApiError> {
    project_list_warned(conn, project, environment, env, &[]).await
}

/// Liste de l'environnement ; les variables de `short` portent
/// l'avertissement `too_short_to_redact`.
async fn project_list_warned(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    env: &str,
    short: &[String],
) -> Result<Response, ApiError> {
    let mut list = vars::list_project(conn, project, environment, env)
        .await
        .map_err(db("liste des variables"))?;
    for v in &mut list {
        if !v.inherited && short.contains(&v.key) {
            v.warnings.push(TOO_SHORT_TO_REDACT.to_string());
        }
    }
    Ok(no_store(Json(list).into_response()))
}

/// `GET /api/projects/{id}/variables?env=`
async fn list_project(
    State(state): State<AppState>,
    _auth: Require<res::Variables, lvl::Read>,
    Path(id): Path<String>,
    query: Result<Query<EnvQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let env = env_name(query)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    let (project, environment) = target(&mut conn, &id, &env).await?;
    project_list(&mut conn, project, environment, &env).await
}

/// `GET /api/projects/{id}/variables/status?env=` : une modification
/// (suppressions comprises) attend-elle un deploiement ?
async fn status_project(
    State(state): State<AppState>,
    _auth: Require<res::Variables, lvl::Read>,
    Path(id): Path<String>,
    query: Result<Query<EnvQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let env = env_name(query)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    let (_, environment) = target(&mut conn, &id, &env).await?;
    let pending_apply = vars::environment_pending(&mut conn, environment)
        .await
        .map_err(db("etat des variables"))?;
    Ok(no_store(
        Json(VariablesStatus { pending_apply }).into_response(),
    ))
}

/// `PUT /api/projects/{id}/variables?env=`
async fn put_project(
    State(state): State<AppState>,
    auth: Require<res::Variables, lvl::Write>,
    Path(id): Path<String>,
    query: Result<Query<EnvQuery>, QueryRejection>,
    body: Result<Json<PutProjectVariable>, JsonRejection>,
) -> Result<Response, ApiError> {
    let env = env_name(query)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    if !vars::valid_key(&input.key) {
        return Err(var_error(vars::invalid_key(&input.key)));
    }
    if let Some(p) = &input.previous_key
        && !vars::valid_key(p)
    {
        return Err(variable_not_found());
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(db("transaction des variables"))?;
    let (project, environment) = target(&mut tx, &id, &env).await?;
    vars::lock_environment(&mut tx, environment)
        .await
        .map_err(db("verrou de l'environnement"))?;
    let written = vars::put(
        &mut tx,
        &state.secrets,
        environment,
        &input.key,
        input.previous_key.as_deref(),
        Wanted {
            value: input.value.as_ref().map(|v| v.as_str()),
            secret: input.secret,
        },
        auth.user.id,
        state.clock.unix_secs(),
    )
    .await
    .map_err(var_error)?;
    let changed = written.changed;
    if changed {
        vars::check_caps(
            &mut tx,
            &state.secrets,
            project,
            &[(environment, env.clone())],
        )
        .await
        .map_err(var_error)?;
        vars::touch(&mut tx, environment, state.clock.unix_secs())
            .await
            .map_err(db("environnement des variables"))?;
        audit(
            &mut tx,
            auth.user.id,
            "variable.put",
            "project_variable",
            json!({ "project_id": project, "environment": env, "key": input.key,
                    "previous_key": input.previous_key, "secret": input.secret }),
        )
        .await?;
    }
    let short: &[String] = if written.short_secret {
        std::slice::from_ref(&input.key)
    } else {
        &[]
    };
    let res = project_list_warned(&mut tx, project, environment, &env, short).await?;
    tx.commit().await.map_err(db("transaction des variables"))?;
    if changed {
        state.logs.invalidate(project);
    }
    Ok(res)
}

/// `PUT /api/projects/{id}/variables/bulk?env=` : contenu `.env`.
async fn bulk_project(
    State(state): State<AppState>,
    auth: Require<res::Variables, lvl::Write>,
    Path(id): Path<String>,
    query: Result<Query<EnvQuery>, QueryRejection>,
    body: Result<Json<BulkProjectVariables>, JsonRejection>,
) -> Result<Response, ApiError> {
    let env = env_name(query)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    let lines = vars::parse_dotenv(&input.content)
        .map_err(|(line, why)| unprocessable(format!("Ligne {line} : {why}.")))?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(db("transaction des variables"))?;
    let (project, environment) = target(&mut tx, &id, &env).await?;
    vars::lock_environment(&mut tx, environment)
        .await
        .map_err(db("verrou de l'environnement"))?;
    let vars::Replaced {
        changed,
        removed,
        short_secrets,
    } = vars::replace_all(
        &mut tx,
        &state.secrets,
        environment,
        &lines,
        auth.user.id,
        state.clock.unix_secs(),
    )
    .await
    .map_err(var_error)?;
    let touched = !changed.is_empty() || !removed.is_empty();
    if touched {
        vars::check_caps(
            &mut tx,
            &state.secrets,
            project,
            &[(environment, env.clone())],
        )
        .await
        .map_err(var_error)?;
        vars::touch(&mut tx, environment, state.clock.unix_secs())
            .await
            .map_err(db("environnement des variables"))?;
        audit(
            &mut tx,
            auth.user.id,
            "variable.bulk",
            "project_variable",
            json!({ "project_id": project, "environment": env, "changed": changed, "removed": removed }),
        )
        .await?;
    }
    let res = project_list_warned(&mut tx, project, environment, &env, &short_secrets).await?;
    tx.commit().await.map_err(db("transaction des variables"))?;
    if touched {
        state.logs.invalidate(project);
    }
    Ok(res)
}

/// `DELETE /api/projects/{id}/variables/{key}?env=`
async fn delete_project(
    State(state): State<AppState>,
    auth: Require<res::Variables, lvl::Write>,
    Path((id, key)): Path<(String, String)>,
    query: Result<Query<EnvQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let env = env_name(query)?;
    if !vars::valid_key(&key) {
        return Err(variable_not_found());
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(db("transaction des variables"))?;
    let (project, environment) = target(&mut tx, &id, &env).await?;
    vars::lock_environment(&mut tx, environment)
        .await
        .map_err(db("verrou de l'environnement"))?;
    if !vars::delete(&mut tx, environment, &key)
        .await
        .map_err(db("suppression de la variable"))?
    {
        return Err(variable_not_found());
    }
    // Suppression : le conteneur garde la variable jusqu'au prochain
    // deploiement.
    vars::touch(&mut tx, environment, state.clock.unix_secs())
        .await
        .map_err(db("environnement des variables"))?;
    audit(
        &mut tx,
        auth.user.id,
        "variable.delete",
        "project_variable",
        json!({ "project_id": project, "environment": env, "key": key }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction des variables"))?;
    state.logs.invalidate(project);
    Ok(StatusCode::NO_CONTENT.into_response())
}

// --- globales -------------------------------------------------------------------

/// Masquage des journaux relu pour chaque projet de `scope`.
async fn invalidate(state: &AppState, scope: Scope) {
    let projects = match state.db.acquire().await {
        Ok(mut conn) => scope.projects(&mut conn).await,
        Err(e) => Err(e),
    };
    match projects {
        Ok(projects) => {
            for p in projects {
                state.logs.invalidate(p);
            }
        }
        // Filet : le masquage garde expire de lui-meme (`REDACTOR_TTL`).
        Err(e) => tracing::warn!(error = %e, "projets d'une variable globale illisibles"),
    }
}

fn parse_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| variable_not_found())
}

async fn reply_global(conn: &mut PgConnection, id: Uuid) -> Result<GlobalVariable, ApiError> {
    vars::get_global(conn, id)
        .await
        .map_err(db("lecture de la variable globale"))?
        .ok_or_else(variable_not_found)
}

/// `GET /api/variables`
async fn list_globals(
    State(state): State<AppState>,
    _auth: Require<res::Variables, lvl::Admin>,
) -> Result<Response, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    let list = vars::list_globals(&mut conn)
        .await
        .map_err(db("liste des variables globales"))?;
    Ok(no_store(Json(list).into_response()))
}

/// `POST /api/variables`
async fn create_global(
    State(state): State<AppState>,
    auth: Require<res::Variables, lvl::Admin>,
    body: Result<Json<NewGlobalVariable>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    if !vars::valid_key(&input.key) {
        return Err(var_error(vars::invalid_key(&input.key)));
    }
    let environments = vars::clean_environments(&input.environments).map_err(var_error)?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(db("transaction des variables"))?;
    // Creations en serie : plafond exact.
    sqlx::query("LOCK TABLE global_variables IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .map_err(db("verrou des variables globales"))?;
    if vars::count_globals(&mut tx)
        .await
        .map_err(db("compte des variables globales"))?
        >= vars::MAX_VARIABLES as i64
    {
        return Err(unprocessable("500 variables globales au plus."));
    }
    let scope = vars::clean_scope(&mut tx, input.all_projects, &input.projects)
        .await
        .map_err(var_error)?;
    let id = Uuid::new_v4();
    vars::check_global_overlap(&mut tx, id, &input.key, &environments, &scope)
        .await
        .map_err(var_error)?;
    let given = input.value.as_ref().map(|v| v.as_str());
    let short = vars::short_secret(input.secret, given, None);
    let value = vars::next_value(
        &state.secrets,
        vars::Owner::Global { id },
        None,
        Wanted {
            value: given,
            secret: input.secret,
        },
    )
    .map_err(var_error)?
    .unwrap_or(vars::Stored::Plain(String::new()));
    // Environnements verrouilles avant l'horodatage (voir `lock_scope`).
    vars::lock_scope(&mut tx, &scope, &environments)
        .await
        .map_err(db("verrou des environnements"))?;
    let now = state.clock.unix_secs();
    vars::save_global(
        &mut tx,
        id,
        &input.key,
        &value,
        &environments,
        &scope,
        auth.user.id,
        now,
    )
    .await
    .map_err(db("creation de la variable globale"))?;
    vars::check_scope_caps(&mut tx, &state.secrets, scope.clone(), &environments)
        .await
        .map_err(var_error)?;
    vars::touch_scope(&mut tx, &scope, &environments, now)
        .await
        .map_err(db("environnements de la variable globale"))?;
    audit(
        &mut tx,
        auth.user.id,
        "global_variable.create",
        "global_variable",
        json!({ "variable_id": id, "key": input.key, "secret": input.secret }),
    )
    .await?;
    let mut created = reply_global(&mut tx, id).await?;
    if short {
        created.warnings.push(TOO_SHORT_TO_REDACT.to_string());
    }
    tx.commit().await.map_err(db("transaction des variables"))?;
    invalidate(&state, scope).await;
    Ok(no_store(
        (StatusCode::CREATED, Json(created)).into_response(),
    ))
}

/// `PATCH /api/variables/{id}`
async fn update_global(
    State(state): State<AppState>,
    auth: Require<res::Variables, lvl::Admin>,
    Path(id): Path<String>,
    body: Result<Json<UpdateGlobalVariable>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    if let Some(key) = &input.key
        && !vars::valid_key(key)
    {
        return Err(var_error(vars::invalid_key(key)));
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(db("transaction des variables"))?;
    // Ecritures de globales en serie : le controle de chevauchement est exact.
    sqlx::query("LOCK TABLE global_variables IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await
        .map_err(db("verrou des variables globales"))?;
    let current = vars::lock_global(&mut tx, id)
        .await
        .map_err(db("lecture de la variable globale"))?
        .ok_or_else(variable_not_found)?;
    let key = input.key.clone().unwrap_or_else(|| current.key.clone());
    let environments = match &input.environments {
        Some(e) => vars::clean_environments(e).map_err(var_error)?,
        None => current.environments.clone(),
    };
    let all = input.all_projects.unwrap_or(current.scope == Scope::All);
    let scope = match (&input.projects, all, &current.scope) {
        (_, true, _) => Scope::All,
        (Some(p), false, _) => vars::clean_scope(&mut tx, false, p)
            .await
            .map_err(var_error)?,
        (None, false, Scope::Projects(p)) => Scope::Projects(p.clone()),
        (None, false, Scope::All) => {
            return Err(unprocessable("Choisissez au moins un projet."));
        }
    };
    vars::check_global_overlap(&mut tx, id, &key, &environments, &scope)
        .await
        .map_err(var_error)?;
    let secret = input.secret.unwrap_or(current.value.is_secret());
    let given = input.value.as_ref().map(|v| v.as_str());
    let changed = vars::next_value(
        &state.secrets,
        vars::Owner::Global { id },
        Some(&current.value),
        Wanted {
            value: given,
            secret,
        },
    )
    .map_err(var_error)?;
    let short = changed.is_some() && vars::short_secret(secret, given, Some(&current.value));
    let value = changed.unwrap_or_else(|| current.value.clone());
    // Ancienne et nouvelle portee : un projet retire perd la variable au
    // prochain deploiement. Verrouillees avant l'horodatage (`lock_scope`).
    let touched_scope = current.scope.clone().union(scope.clone());
    let mut touched_envs = current.environments.clone();
    touched_envs.extend(environments.iter().cloned());
    touched_envs.sort();
    touched_envs.dedup();
    vars::lock_scope(&mut tx, &touched_scope, &touched_envs)
        .await
        .map_err(db("verrou des environnements"))?;
    let now = state.clock.unix_secs();
    vars::save_global(
        &mut tx,
        id,
        &key,
        &value,
        &environments,
        &scope,
        auth.user.id,
        now,
    )
    .await
    .map_err(db("modification de la variable globale"))?;
    vars::check_scope_caps(&mut tx, &state.secrets, scope.clone(), &environments)
        .await
        .map_err(var_error)?;
    vars::touch_scope(&mut tx, &touched_scope, &touched_envs, now)
        .await
        .map_err(db("environnements de la variable globale"))?;
    audit(
        &mut tx,
        auth.user.id,
        "global_variable.update",
        "global_variable",
        json!({ "variable_id": id, "key": key, "secret": secret }),
    )
    .await?;
    let mut updated = reply_global(&mut tx, id).await?;
    if short {
        updated.warnings.push(TOO_SHORT_TO_REDACT.to_string());
    }
    tx.commit().await.map_err(db("transaction des variables"))?;
    invalidate(&state, current.scope.union(scope)).await;
    Ok(no_store(Json(updated).into_response()))
}

/// `DELETE /api/variables/{id}`
async fn delete_global(
    State(state): State<AppState>,
    auth: Require<res::Variables, lvl::Admin>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id(&id)?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(db("transaction des variables"))?;
    let current = vars::lock_global(&mut tx, id)
        .await
        .map_err(db("lecture de la variable globale"))?
        .ok_or_else(variable_not_found)?;
    vars::lock_scope(&mut tx, &current.scope, &current.environments)
        .await
        .map_err(db("verrou des environnements"))?;
    vars::delete_global(&mut tx, id)
        .await
        .map_err(db("suppression de la variable globale"))?;
    vars::touch_scope(
        &mut tx,
        &current.scope,
        &current.environments,
        state.clock.unix_secs(),
    )
    .await
    .map_err(db("environnements de la variable globale"))?;
    audit(
        &mut tx,
        auth.user.id,
        "global_variable.delete",
        "global_variable",
        json!({ "variable_id": id, "key": current.key }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction des variables"))?;
    invalidate(&state, current.scope).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `GET /api/variables/export?env=` : `.env` des globales publiques.
async fn export_globals(
    State(state): State<AppState>,
    _auth: Require<res::Variables, lvl::Admin>,
    query: Result<Query<EnvQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let env = query.ok().and_then(|Query(q)| q.env);
    if let Some(e) = &env
        && !vars::valid_environment(e)
    {
        return Err(unprocessable("Nom d'environnement invalide."));
    }
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    let text = vars::export_globals(&mut conn, env.as_deref())
        .await
        .map_err(db("export des variables globales"))?;
    let file = match &env {
        Some(e) => format!("ciphera-globales-{e}.env"),
        None => "ciphera-globales.env".to_string(),
    };
    let mut res = text.into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    // Nom deja restreint par `valid_environment`.
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{file}\"")) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(no_store(res))
}
