//! Domaines d'un projet :
//! - `GET /api/domains?project=` (Read : viewer et plus) ;
//! - `POST|PATCH /api/domains?project=`, `PATCH|DELETE /api/domains/{id}`,
//!   `POST /api/domains/{id}/{verify,renew,primary,redirect}` (Write :
//!   developer, ops, admin, owner).
//!
//! Ecritures sous CSRF (couche du routeur) et auditees. Chaque modification
//! qui change ce qui est en ligne renvoie la table des routes aux agents.

use api_types::domains::{
    DomainRedirect, NewDomain, ProjectDomains, UpdateDomain, UpdateProjectDomains,
};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Level, Require, Resource, Role, allowed, lvl, res};
use crate::domain::domains::{self, DomainError, acme, dns};
use crate::http::panel_host;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/domains", get(list).post(create).patch(update_project))
        .route(
            "/api/domains/{id}",
            axum::routing::patch(update).delete(remove),
        )
        .route("/api/domains/{id}/verify", post(verify))
        .route("/api/domains/{id}/renew", post(renew))
        .route("/api/domains/{id}/primary", post(primary))
        .route("/api/domains/{id}/redirect", post(redirect))
}

#[derive(Deserialize)]
struct ProjectQuery {
    project: Option<String>,
}

fn project_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Projet introuvable.")
}

fn domain_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Domaine introuvable.")
}

fn unprocessable(message: impl Into<String>) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn db(context: &'static str) -> impl Fn(sqlx::Error) -> ApiError {
    move |e| internal(context, e)
}

fn domain_error(e: DomainError) -> ApiError {
    match e {
        DomainError::Invalid(m) => unprocessable(m),
        DomainError::PortRequired => {
            ApiError::new(422, "internal_port_required", domains::PORT_REQUIRED)
        }
        DomainError::NoNodeIp => ApiError::new(
            422,
            "node_ip_unknown",
            "IP publique du nœud inconnue : définissez CIPHERA_PUBLIC_IP.",
        ),
        DomainError::HostTaken => ApiError::new(
            409,
            "host_taken",
            "Ce domaine est déjà utilisé sur cette instance.",
        ),
        DomainError::Reserved => ApiError::new(
            409,
            "reserved_host",
            "Ce domaine est réservé au panneau Ciphera.",
        ),
        DomainError::Forbidden => crate::authz::forbidden(),
        DomainError::Conflict(m) => ApiError::new(409, "conflict", m),
        DomainError::NotFound(m) => ApiError::new(404, "not_found", m),
        DomainError::Db(e) => internal("domaines", e),
    }
}

/// Port interne et healthcheck sont des reglages du service : les changer par
/// la route des domaines exige aussi `Projects:Write`. Renvoyer la valeur
/// actuelle n'est pas un changement.
fn may_edit_service(role: Role) -> bool {
    allowed(role, Resource::Projects, Level::Write)
}

fn project_id(query: Result<Query<ProjectQuery>, QueryRejection>) -> Result<Uuid, ApiError> {
    query
        .ok()
        .and_then(|Query(q)| q.project)
        .and_then(|p| Uuid::parse_str(&p).ok())
        .ok_or_else(project_not_found)
}

fn domain_id(id: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| domain_not_found())
}

async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    detail: Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, 'domain', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(conn)
    .await
    .map_err(db("audit des domaines"))?;
    Ok(())
}

async fn project_domains(
    state: &AppState,
    conn: &mut PgConnection,
    project: Uuid,
) -> Result<ProjectDomains, ApiError> {
    domains::list(conn, &state.cfg, project)
        .await
        .map_err(db("liste des domaines"))?
        .ok_or_else(project_not_found)
}

async fn one(conn: &mut PgConnection, id: Uuid) -> Result<domains::Found, ApiError> {
    domains::get(conn, id)
        .await
        .map_err(db("lecture du domaine"))?
        .ok_or_else(domain_not_found)
}

async fn begin(state: &AppState) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(db("transaction des domaines"))
}

/// `GET /api/domains?project=`
async fn list(
    State(state): State<AppState>,
    _auth: Require<res::Domains, lvl::Read>,
    query: Result<Query<ProjectQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let project = project_id(query)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    Ok(Json(project_domains(&state, &mut conn, project).await?).into_response())
}

/// `POST /api/domains?project=`
async fn create(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    headers: HeaderMap,
    uri: Uri,
    query: Result<Query<ProjectQuery>, QueryRejection>,
    body: Result<Json<NewDomain>, JsonRejection>,
) -> Result<Response, ApiError> {
    let project = project_id(query)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    // Nom par lequel le panneau est joint : jamais un domaine de projet.
    let current = panel_host::request_host_name(&headers, &uri);
    let mut tx = begin(&state).await?;
    let id = domains::create(
        &mut tx,
        &state.cfg,
        current.as_deref(),
        project,
        &input,
        auth.user.id,
        may_edit_service(auth.role),
    )
    .await
    .map_err(domain_error)?;
    let found = one(&mut tx, id).await?;
    audit(
        &mut tx,
        auth.user.id,
        "domain.create",
        json!({ "project_id": project, "host": found.domain.host, "redirect": input.redirect, "www": input.www }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    Ok((StatusCode::CREATED, Json(found.domain)).into_response())
}

/// `PATCH /api/domains?project=` : domaine genere active ou non.
async fn update_project(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    query: Result<Query<ProjectQuery>, QueryRejection>,
    body: Result<Json<UpdateProjectDomains>, JsonRejection>,
) -> Result<Response, ApiError> {
    let project = project_id(query)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    let mut tx = begin(&state).await?;
    let removed = domains::set_generated(&mut tx, &state.cfg, project, input.generated_domain)
        .await
        .map_err(domain_error)?;
    audit(
        &mut tx,
        auth.user.id,
        "domain.generated",
        json!({ "project_id": project, "enabled": input.generated_domain }),
    )
    .await?;
    let list = project_domains(&state, &mut tx, project).await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    domains::remove_certificates(&state, &removed);
    domains::push_routes(&state).await;
    Ok(Json(list).into_response())
}

/// `PATCH /api/domains/{id}`
async fn update(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    Path(id): Path<String>,
    body: Result<Json<UpdateDomain>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = domain_id(&id)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    let mut tx = begin(&state).await?;
    domains::update(&mut tx, id, &input, may_edit_service(auth.role))
        .await
        .map_err(domain_error)?;
    let found = one(&mut tx, id).await?;
    audit(
        &mut tx,
        auth.user.id,
        "domain.update",
        json!({ "project_id": found.project_id, "host": found.domain.host,
                "path_prefix": input.path_prefix, "internal_port": input.internal_port }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    domains::push_routes(&state).await;
    Ok(Json(found.domain).into_response())
}

/// `DELETE /api/domains/{id}`
async fn remove(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = domain_id(&id)?;
    let mut tx = begin(&state).await?;
    let found = domains::delete(&mut tx, id).await.map_err(domain_error)?;
    audit(
        &mut tx,
        auth.user.id,
        "domain.delete",
        json!({ "project_id": found.project_id, "host": found.domain.host }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    domains::push_routes(&state).await;
    // Domaine supprime : seul cas ou certificat et cle sont effaces.
    if !found.domain.generated {
        domains::remove_certificates(&state, &[found.domain.host]);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/domains/{id}/verify` : A/AAAA vers l'IP du noeud. Correct :
/// en ligne et certificat demande. Refuse (409 `reserved_host`) un hote
/// reserve au panneau ou par lequel la requete joint le panneau.
async fn verify(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    headers: HeaderMap,
    uri: Uri,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = domain_id(&id)?;
    let current = panel_host::request_host_name(&headers, &uri);
    let (v4, v6) = state.cfg.node_ips();
    if v4.is_none() && v6.is_none() {
        return Err(domain_error(DomainError::NoNodeIp));
    }
    let found = {
        let mut conn = state
            .db
            .acquire()
            .await
            .map_err(db("connexion a la base"))?;
        let found = one(&mut conn, id).await?;
        if domains::refused(&state.cfg, current.as_deref(), &found.domain.host) {
            return Err(domain_error(DomainError::Reserved));
        }
        if !domains::ready(&mut conn, &found)
            .await
            .map_err(db("port du service"))?
        {
            return Err(domain_error(DomainError::PortRequired));
        }
        found
    };
    // Resolution hors transaction (jusqu'a 5 s).
    let host = found.domain.host.clone();
    let error = match state.dns.resolve(&host).await {
        Ok(ips) => dns::verdict(&host, v4, v6, &ips),
        Err(e) => {
            tracing::warn!(%host, "resolution DNS impossible : {e}");
            Some(format!(
                "Résolution DNS de {host} impossible pour le moment : réessayez."
            ))
        }
    };
    let mut tx = begin(&state).await?;
    // Domaine supprime entre-temps : 404.
    one(&mut tx, id).await?;
    domains::record_dns(&mut tx, id, error.as_deref())
        .await
        .map_err(db("verification du domaine"))?;
    // Certificat demande seulement s'il n'y en a pas de valide a plus de
    // 30 jours (une verification repetee ne re-emet rien) ; sinon le proxy
    // est interroge et le re-emet s'il ne le detient plus.
    if error.is_none() && !found.domain.generated {
        let requested = acme::request(&mut tx, id, state.clock.unix_secs() as i64)
            .await
            .map_err(db("demande de certificat"))?;
        if !requested {
            acme::check(&mut tx, id)
                .await
                .map_err(db("verification du certificat"))?;
        }
    }
    audit(
        &mut tx,
        auth.user.id,
        "domain.verify",
        json!({ "project_id": found.project_id, "host": host, "ok": error.is_none() }),
    )
    .await?;
    let updated = one(&mut tx, id).await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    domains::push_routes(&state).await;
    Ok(Json(updated.domain).into_response())
}

/// `POST /api/domains/{id}/renew` : nouvelle emission du certificat.
async fn renew(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = domain_id(&id)?;
    let mut tx = begin(&state).await?;
    let found = one(&mut tx, id).await?;
    if found.domain.generated {
        return Err(ApiError::new(
            409,
            "conflict",
            "Le certificat du domaine généré est géré automatiquement : une tentative par jour au plus.",
        ));
    }
    if found.domain.dns_status != api_types::domains::DnsStatus::Ok {
        return Err(ApiError::new(
            409,
            "dns_not_verified",
            "Vérifiez d'abord le DNS du domaine.",
        ));
    }
    // Renouvellement manuel : seulement a moins de 30 jours de l'expiration
    // (le renouvellement automatique a lieu a J-30).
    if !acme::request(&mut tx, id, state.clock.unix_secs() as i64)
        .await
        .map_err(db("demande de certificat"))?
    {
        return Err(ApiError::new(
            409,
            "certificate_still_valid",
            "Certificat encore valide : il sera renouvelé automatiquement 30 jours avant son expiration.",
        ));
    }
    audit(
        &mut tx,
        auth.user.id,
        "domain.renew",
        json!({ "project_id": found.project_id, "host": found.domain.host }),
    )
    .await?;
    let updated = one(&mut tx, id).await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    Ok((StatusCode::ACCEPTED, Json(updated.domain)).into_response())
}

/// `POST /api/domains/{id}/primary`
async fn primary(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = domain_id(&id)?;
    let mut tx = begin(&state).await?;
    let found = domains::set_primary(&mut tx, id)
        .await
        .map_err(domain_error)?;
    audit(
        &mut tx,
        auth.user.id,
        "domain.primary",
        json!({ "project_id": found.project_id, "host": found.domain.host }),
    )
    .await?;
    let list = project_domains(&state, &mut tx, found.project_id).await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    domains::push_routes(&state).await;
    Ok(Json(list).into_response())
}

/// `POST /api/domains/{id}/redirect`
async fn redirect(
    State(state): State<AppState>,
    auth: Require<res::Domains, lvl::Write>,
    Path(id): Path<String>,
    body: Result<Json<DomainRedirect>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = domain_id(&id)?;
    let Json(input) = body.map_err(|_| unprocessable("Corps de requête invalide."))?;
    let mut tx = begin(&state).await?;
    domains::set_redirect(&mut tx, id, input.enabled)
        .await
        .map_err(domain_error)?;
    let found = one(&mut tx, id).await?;
    audit(
        &mut tx,
        auth.user.id,
        "domain.redirect",
        json!({ "project_id": found.project_id, "host": found.domain.host, "enabled": input.enabled }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction des domaines"))?;
    domains::push_routes(&state).await;
    Ok(Json(found.domain).into_response())
}
