//! Deploy hooks : `GET|POST /api/projects/{id}/deploy-hooks`,
//! `PATCH|DELETE /api/projects/{id}/deploy-hooks/{hookId}` (admin, CSRF,
//! audit) et la route publique `POST /v1/hooks/deploy/{hookId}`.
//!
//! La creation exige le mot de passe du compte (re-authentification, meme
//! limite que les autres actions sensibles) et fixe une expiration (90 jours
//! par defaut, modifiable) : un hook expire est refuse comme un secret faux.
//!
//! Route publique (hors `/api/`, donc hors CSRF), dans cet ordre :
//! 0. limite en memoire par IP (`public_limit::HOOK_PER_IP` appels par
//!    minute, reussis ou non), avant toute ecriture en base : 429 ;
//! 1. limitation de debit (`rate_limit::admit`, espace `IpKey::hook`) : 5
//!    echecs par couple (IP, hook) et 50 par IP en 15 min, 429 avec
//!    `Retry-After` ; un appel accepte remet le compteur du couple a zero ;
//! 2. secret lu dans l'en-tete `X-Ciphera-Hook-Token` (voie conseillee,
//!    `X-Deploy-Hook-Token` accepte aussi) ou dans `?token=` (deconseille :
//!    une URL se retrouve dans des journaux tiers), jamais journalise (la
//!    requete n'est pas tracee ; le proxy du noeud ne journalise que le
//!    chemin, sans la chaine de requete) ;
//! 3. hook inconnu, identifiant mal forme, secret absent ou faux : le meme
//!    404, apres la meme comparaison a temps constant (`hooks::verify`) ;
//! 4. secret valide : au plus `public_limit::HOOK_PER_HOOK` declenchements
//!    par minute et par hook (429 au-dela) ;
//! 5. sinon un deploiement `hook` de la production, par le meme chemin que
//!    le redeploiement (`orchestrator::request`), sans doublon : un
//!    deploiement deja en file est reutilise (`already_queued`).

use api_types::project_git::{CreatedDeployHook, HookTriggered, NewDeployHook, UpdateDeployHook};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::client_ip::ClientIp;
use super::me::{Reauth, reauthenticate};
use super::mfa::rate_limited;
use crate::auth::{internal, rate_limit};
use crate::authz::{Require, lvl, res};
use crate::domain::deployments::orchestrator::{self, Guard, Origin, Requested};
use crate::domain::projects::hooks;
use crate::domain::projects::store::PRODUCTION;
use crate::state::AppState;

const MAX_BODY: usize = 4 * 1024;
/// En-tete du secret, tel qu'affiche (voie conseillee, au lieu de `?token=`).
pub const TOKEN_HEADER_NAME: &str = "X-Ciphera-Hook-Token";
/// Meme en-tete en minuscules, pour la lecture.
pub const TOKEN_HEADER: &str = "x-ciphera-hook-token";
/// Ancien nom de l'en-tete, toujours accepte.
const LEGACY_TOKEN_HEADER: &str = "x-deploy-hook-token";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects/{id}/deploy-hooks", get(list).post(create))
        .route(
            "/api/projects/{id}/deploy-hooks/{hook}",
            axum::routing::patch(update).delete(remove),
        )
        .route("/v1/hooks/deploy/{hook}", post(trigger))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn project_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Projet introuvable.")
}

fn hook_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Deploy hook introuvable.")
}

fn parse(id: &str, missing: fn() -> ApiError) -> Result<Uuid, ApiError> {
    Uuid::parse_str(id).map_err(|_| missing())
}

async fn project_exists(conn: &mut PgConnection, id: Uuid) -> Result<(), ApiError> {
    let found: Option<bool> =
        sqlx::query_scalar("SELECT true FROM projects WHERE id = $1 FOR SHARE")
            .bind(id)
            .fetch_optional(conn)
            .await
            .map_err(|e| internal("lecture du projet", e))?;
    found.map(|_| ()).ok_or_else(project_not_found)
}

async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    project: Uuid,
    hook: Uuid,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, $2, $3, 'deploy_hook', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(json!({ "project_id": project, "hook_id": hook }))
    .execute(conn)
    .await
    .map_err(|e| internal("audit des deploy hooks", e))?;
    Ok(())
}

/// `GET /api/projects/{id}/deploy-hooks`
async fn list(
    State(state): State<AppState>,
    _auth: Require<res::Projects, lvl::Admin>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse(&id, project_not_found)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| internal("connexion a la base", e))?;
    project_exists(&mut conn, id).await?;
    let list = hooks::list(&mut conn, id)
        .await
        .map_err(|e| internal("liste des deploy hooks", e))?;
    Ok(Json(list).into_response())
}

/// `POST /api/projects/{id}/deploy-hooks` : le secret n'est renvoye qu'ici.
async fn create(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Admin>,
    ClientIp(ip): ClientIp,
    Path(id): Path<String>,
    body: Result<Json<NewDeployHook>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = parse(&id, project_not_found)?;
    let invalid = || {
        ApiError::new(
            422,
            "validation_failed",
            "Nom invalide (1 à 64 caractères) ou mot de passe absent.",
        )
    };
    let Json(input) = body.map_err(|_| invalid())?;
    let name = hooks::clean_name(&input.name).ok_or_else(invalid)?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction des deploy hooks", e))?;
    project_exists(&mut tx, id).await?;
    if let Some(at) = input.expires_at.as_deref() {
        check_expiry(&mut tx, at).await?;
    }
    let existing = hooks::count(&mut tx, id)
        .await
        .map_err(|e| internal("compte des deploy hooks", e))?;
    if existing >= hooks::MAX_PER_PROJECT {
        return Err(ApiError::new(
            409,
            "too_many_hooks",
            "20 deploy hooks au plus par projet.",
        ));
    }
    // Re-authentification : un hook declenche des deploiements sans session.
    // Apres les controles ci-dessus : une demande refusee ne coute aucun essai.
    let attempt = match reauthenticate(
        &state,
        &auth.user,
        &ip.to_string(),
        input.password,
        "deploy_hook.create.failure",
    )
    .await?
    {
        Reauth::Verified(attempt, _) => attempt,
        Reauth::Limited(response) => return Ok(response),
    };
    let token = hooks::generate().map_err(|e| internal("generateur aleatoire", e))?;
    let hook = hooks::insert(
        &mut tx,
        id,
        &name,
        &token,
        auth.user.id,
        input.expires_at.as_deref(),
    )
    .await
    .map_err(|e| internal("creation du deploy hook", e))?;
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("limitation de debit", e))?;
    audit(&mut tx, auth.user.id, "deploy_hook.create", id, hook).await?;
    let created = hooks::get(&mut tx, hook)
        .await
        .map_err(|e| internal("lecture du deploy hook", e))?
        .ok_or_else(hook_not_found)?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des deploy hooks", e))?;
    let mut res = (
        StatusCode::CREATED,
        Json(CreatedDeployHook {
            hook: created,
            token: token.to_string(),
            url_path: hooks::path(hook),
            token_header: TOKEN_HEADER_NAME.to_string(),
        }),
    )
        .into_response();
    // Le secret ne doit rester dans aucun cache.
    res.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(res)
}

/// Date d'expiration refusee : 422 ; erreur de base : 500.
async fn check_expiry(conn: &mut PgConnection, value: &str) -> Result<(), ApiError> {
    hooks::check_expiry(conn, value).await.map_err(|e| match e {
        hooks::ExpiryError::Invalid => ApiError::new(
            422,
            "validation_failed",
            "Date d'expiration invalide : RFC 3339, dans le futur et dans les 365 jours.",
        ),
        hooks::ExpiryError::Db(e) => internal("date d'expiration du deploy hook", e),
    })
}

/// `PATCH /api/projects/{id}/deploy-hooks/{hook}` : nouvelle expiration.
async fn update(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Admin>,
    ClientIp(ip): ClientIp,
    Path((id, hook)): Path<(String, String)>,
    body: Result<Json<UpdateDeployHook>, JsonRejection>,
) -> Result<Response, ApiError> {
    let id = parse(&id, project_not_found)?;
    let hook = parse(&hook, hook_not_found)?;
    let Json(input) =
        body.map_err(|_| ApiError::new(422, "validation_failed", "Date d'expiration requise."))?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction des deploy hooks", e))?;
    project_exists(&mut tx, id).await?;
    check_expiry(&mut tx, &input.expires_at).await?;
    let extends = hooks::lock_for_expiry(&mut tx, id, hook, &input.expires_at)
        .await
        .map_err(|e| internal("expiration du deploy hook", e))?
        .ok_or_else(hook_not_found)?;
    // Rallonger ou reactiver un hook expire redonne un secret valide : mot de
    // passe exige. Raccourcir reste libre.
    let mut attempt = None;
    if extends {
        let Some(password) = input.password.filter(|p| !p.is_empty()) else {
            return Err(ApiError::new(
                401,
                "invalid_credentials",
                "Saisissez votre mot de passe pour rallonger l'expiration.",
            ));
        };
        match reauthenticate(
            &state,
            &auth.user,
            &ip.to_string(),
            password,
            "deploy_hook.update.failure",
        )
        .await?
        {
            Reauth::Verified(id, _) => attempt = Some(id),
            Reauth::Limited(response) => return Ok(response),
        }
    }
    if let Some(attempt) = attempt {
        rate_limit::mark_mfa_success(&mut tx, attempt)
            .await
            .map_err(|e| internal("limitation de debit", e))?;
    }
    if !hooks::set_expiry(&mut tx, id, hook, &input.expires_at)
        .await
        .map_err(|e| internal("expiration du deploy hook", e))?
    {
        return Err(hook_not_found());
    }
    audit(&mut tx, auth.user.id, "deploy_hook.update", id, hook).await?;
    let updated = hooks::get(&mut tx, hook)
        .await
        .map_err(|e| internal("lecture du deploy hook", e))?
        .ok_or_else(hook_not_found)?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des deploy hooks", e))?;
    Ok(Json(updated).into_response())
}

/// `DELETE /api/projects/{id}/deploy-hooks/{hook}`
async fn remove(
    State(state): State<AppState>,
    auth: Require<res::Projects, lvl::Admin>,
    Path((id, hook)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let id = parse(&id, project_not_found)?;
    let hook = parse(&hook, hook_not_found)?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction des deploy hooks", e))?;
    project_exists(&mut tx, id).await?;
    if !hooks::delete(&mut tx, id, hook)
        .await
        .map_err(|e| internal("suppression du deploy hook", e))?
    {
        return Err(hook_not_found());
    }
    audit(&mut tx, auth.user.id, "deploy_hook.delete", id, hook).await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des deploy hooks", e))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// `POST /v1/hooks/deploy/{hook}`
async fn trigger(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(hook): Path<String>,
    query: Result<Query<TokenQuery>, QueryRejection>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let from_header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let token = Zeroizing::new(
        from_header(TOKEN_HEADER)
            .or_else(|| from_header(LEGACY_TOKEN_HEADER))
            .or_else(|| query.ok().and_then(|Query(q)| q.token))
            .unwrap_or_default(),
    );

    // Limite en memoire d'abord : une rafale n'ecrit rien en base.
    if let Err(retry_after) = state.public_limits.hook_ip(ip) {
        tracing::warn!(%ip, "deploy hook refuse : trop d'appels");
        return Ok(rate_limited(retry_after));
    }
    if let Err(e) = rate_limit::purge(&state.db).await {
        tracing::warn!(error = %e, "purge des tentatives");
    }
    // Cle du couple : l'identifiant tel que recu (meme mal forme), borne.
    let hook_key: String = hook.chars().take(64).collect();
    let attempt = match rate_limit::admit(
        &state.db,
        &rate_limit::IpKey::hook(ip),
        &rate_limit::email_key(&format!("deploy-hook:{hook_key}")),
    )
    .await
    .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => {
            tracing::warn!(%ip, "deploy hook refuse : trop de tentatives");
            return Ok(rate_limited(retry_after));
        }
    };

    let id = Uuid::parse_str(&hook).ok();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| internal("connexion a la base", e))?;
    let found = match id {
        Some(id) => hooks::lookup(&mut conn, id)
            .await
            .map_err(|e| internal("lecture du deploy hook", e))?,
        None => None,
    };
    // Toujours la meme comparaison, hook connu ou non.
    let valid = hooks::verify(found.as_ref().map(|(_, h)| h.as_slice()), &token);
    let (Some(id), Some((project, _)), true) = (id, found, valid) else {
        return Err(hook_not_found());
    };
    rate_limit::mark_success(&mut conn, attempt)
        .await
        .map_err(|e| internal("limitation de debit", e))?;
    // Secret valide : borne aussi les deploiements demandes par ce hook.
    if let Err(retry_after) = state.public_limits.hook(id) {
        tracing::warn!(hook_id = %id, "deploy hook refuse : trop de declenchements");
        return Ok(rate_limited(retry_after));
    }
    hooks::touch(&mut conn, id, state.clock.unix_secs())
        .await
        .map_err(|e| internal("date du deploy hook", e))?;
    drop(conn);

    let requested = orchestrator::request(
        &state,
        project,
        PRODUCTION,
        Origin::Hook(id),
        Guard::ReuseQueued,
    )
    .await
    .map_err(|e| match e {
        // Projet sans environnement production : rien a deployer.
        orchestrator::RequestError::NotFound => hook_not_found(),
        other => super::deployments::request_error("deploy hook", other),
    })?;
    let (status, deployment) = match requested {
        Requested::Created(d) => ("queued", d),
        Requested::AlreadyQueued(d) => ("already_queued", d),
    };
    tracing::info!(hook_id = %id, %project, deployment_id = %deployment, "deploy hook : {status}");
    Ok((
        StatusCode::ACCEPTED,
        Json(HookTriggered {
            status: status.to_string(),
            deployment_id: deployment.to_string(),
        }),
    )
        .into_response())
}
