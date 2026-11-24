//! Parametres v0.1 (Parametres : Admin requis ; proprietaire seulement pour
//! la zone de danger) :
//! - `GET|PATCH /api/settings/general` : nom, identifiant, fuseau, langue ;
//! - `GET|PATCH /api/settings/deployments` : deploiement automatique des
//!   nouveaux projets, retention (fixe : 5) ;
//! - `GET|PATCH /api/settings/security` : 2FA obligatoire (appliquee a la
//!   prochaine connexion, sessions ouvertes intactes) et liste d'IP
//!   autorisees (voir [`super::ip_allowlist`]). Une liste qui exclut
//!   l'adresse de la requete est refusee ;
//! - `GET /api/settings/danger` : ce que la suppression effacerait.
//!
//! Notifications : [`super::notifications`] ; domaine du panneau :
//! [`super::panel_domain`]. Ecritures sous CSRF (couche du routeur), auditees.

use api_types::settings::{
    DEPLOY_RETENTION, DangerZone, DeploySettings, GeneralSettings, OrgSecurity,
    UpdateDeploySettings, UpdateGeneralSettings, UpdateOrgSecurity,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, Role, forbidden, lvl, res};
use crate::http::client_ip::ClientIp;
use crate::http::ip_allowlist::{self, IpNet};
use crate::http::me::valid_timezone;
use crate::http::setup::validate_slug;
use crate::state::AppState;

const MAX_BODY: usize = 16 * 1024;
const MAX_NAME_CHARS: usize = 100;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/general", get(general).patch(update_general))
        .route(
            "/api/settings/deployments",
            get(deployments).patch(update_deployments),
        )
        .route(
            "/api/settings/security",
            get(security).patch(update_security),
        )
        .route("/api/settings/danger", get(danger))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

pub(crate) type Admin = Require<res::Settings, lvl::Admin>;

/// Zone de danger et domaine du panneau : proprietaire seulement
/// (l'administrateur a aussi `Settings/Admin`).
pub(crate) fn owner_only(auth: &Admin) -> Result<Uuid, ApiError> {
    if auth.role == Role::Owner {
        Ok(auth.user.id)
    } else {
        Err(forbidden())
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn db(context: &'static str) -> impl Fn(sqlx::Error) -> ApiError {
    move |e| internal(context, e)
}

fn not_configured() -> ApiError {
    ApiError::new(404, "not_found", "Instance non configurée.")
}

pub(crate) async fn audit(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    action: &str,
    detail: Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, 'settings', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(conn)
    .await
    .map(|_| ())
    .map_err(db("audit des parametres"))
}

fn body<T>(input: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    input
        .map(|Json(v)| v)
        .map_err(|_| invalid("Requête invalide."))
}

async fn load_general(conn: &mut PgConnection) -> Result<GeneralSettings, ApiError> {
    type Row = (Uuid, String, Option<String>, String, String, bool, i64);
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, name, slug, timezone, language, mfa_required, \
                (SELECT count(*) FROM users) \
         FROM instance LIMIT 1",
    )
    .fetch_optional(conn)
    .await
    .map_err(db("lecture des parametres"))?;
    let (id, name, slug, timezone, language, mfa_required, members) =
        row.ok_or_else(not_configured)?;
    Ok(GeneralSettings {
        id: id.to_string(),
        name,
        slug,
        timezone,
        language,
        members,
        mfa_required,
    })
}

/// `GET /api/settings/general`
async fn general(State(state): State<AppState>, _auth: Admin) -> Result<Response, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    Ok(Json(load_general(&mut conn).await?).into_response())
}

/// `PATCH /api/settings/general`
async fn update_general(
    State(state): State<AppState>,
    auth: Admin,
    input: Result<Json<UpdateGeneralSettings>, JsonRejection>,
) -> Result<Response, ApiError> {
    let input = body(input)?;
    let name = match input.name.as_deref().map(str::trim) {
        Some(name)
            if name.is_empty()
                || name.chars().count() > MAX_NAME_CHARS
                || name.chars().any(char::is_control) =>
        {
            return Err(invalid("Nom de l'organisation : 1 à 100 caractères."));
        }
        other => other.map(str::to_string),
    };
    if input.slug.as_deref().is_some_and(|s| !validate_slug(s)) {
        return Err(invalid(
            "Identifiant : lettres minuscules, chiffres et tirets (50 caractères au plus).",
        ));
    }
    if input
        .timezone
        .as_deref()
        .is_some_and(|tz| !valid_timezone(tz))
    {
        return Err(invalid("Fuseau horaire inconnu."));
    }
    if input
        .language
        .as_deref()
        .is_some_and(|l| !matches!(l, "fr" | "en"))
    {
        return Err(invalid("Langue : fr ou en."));
    }
    let mut changed = Vec::new();
    for (field, set) in [
        ("name", name.is_some()),
        ("slug", input.slug.is_some()),
        ("timezone", input.timezone.is_some()),
        ("language", input.language.is_some()),
    ] {
        if set {
            changed.push(field);
        }
    }
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    let updated = sqlx::query(
        "UPDATE instance SET name = coalesce($1, name), slug = coalesce($2, slug), \
         timezone = coalesce($3, timezone), language = coalesce($4, language)",
    )
    .bind(&name)
    .bind(&input.slug)
    .bind(&input.timezone)
    .bind(&input.language)
    .execute(&mut *tx)
    .await
    .map_err(db("modification des parametres"))?
    .rows_affected();
    if updated == 0 {
        return Err(not_configured());
    }
    audit(
        &mut tx,
        Some(auth.user.id),
        "settings.general.update",
        json!({ "fields": changed }),
    )
    .await?;
    let body = load_general(&mut tx).await?;
    tx.commit().await.map_err(db("transaction"))?;
    Ok(Json(body).into_response())
}

async fn load_deployments(conn: &mut PgConnection) -> Result<DeploySettings, ApiError> {
    let auto: Option<bool> = sqlx::query_scalar("SELECT deploy_auto FROM instance LIMIT 1")
        .fetch_optional(conn)
        .await
        .map_err(db("lecture des parametres"))?;
    Ok(DeploySettings {
        auto: auto.ok_or_else(not_configured)?,
        retention: DEPLOY_RETENTION,
    })
}

/// `GET /api/settings/deployments`
async fn deployments(State(state): State<AppState>, _auth: Admin) -> Result<Response, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    Ok(Json(load_deployments(&mut conn).await?).into_response())
}

/// `PATCH /api/settings/deployments`
async fn update_deployments(
    State(state): State<AppState>,
    auth: Admin,
    input: Result<Json<UpdateDeploySettings>, JsonRejection>,
) -> Result<Response, ApiError> {
    let input = body(input)?;
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    if let Some(auto) = input.auto {
        let updated = sqlx::query("UPDATE instance SET deploy_auto = $1")
            .bind(auto)
            .execute(&mut *tx)
            .await
            .map_err(db("modification des parametres"))?
            .rows_affected();
        if updated == 0 {
            return Err(not_configured());
        }
        audit(
            &mut tx,
            Some(auth.user.id),
            "settings.deployments.update",
            json!({ "auto": auto }),
        )
        .await?;
    }
    let body = load_deployments(&mut tx).await?;
    tx.commit().await.map_err(db("transaction"))?;
    Ok(Json(body).into_response())
}

async fn load_security(conn: &mut PgConnection, ip: &str) -> Result<OrgSecurity, ApiError> {
    let row: Option<(bool, Vec<String>)> =
        sqlx::query_as("SELECT mfa_required, ip_allowlist FROM instance LIMIT 1")
            .fetch_optional(conn)
            .await
            .map_err(db("lecture des parametres"))?;
    let (mfa_required, ip_allowlist) = row.ok_or_else(not_configured)?;
    Ok(OrgSecurity {
        mfa_required,
        ip_allowlist,
        current_ip: ip.to_string(),
    })
}

/// `GET /api/settings/security`
async fn security(
    State(state): State<AppState>,
    _auth: Admin,
    ClientIp(ip): ClientIp,
) -> Result<Response, ApiError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    Ok(Json(load_security(&mut conn, &ip.to_string()).await?).into_response())
}

/// Entrees validees, canoniques et sans doublon.
fn parse_allowlist(entries: &[String]) -> Result<Vec<IpNet>, ApiError> {
    if entries.len() > ip_allowlist::MAX_ENTRIES {
        return Err(invalid(format!(
            "{} adresses au plus.",
            ip_allowlist::MAX_ENTRIES
        )));
    }
    let mut list: Vec<IpNet> = Vec::with_capacity(entries.len());
    for entry in entries {
        let net = IpNet::parse(entry.trim()).ok_or_else(|| {
            let shown: String = entry.chars().take(64).filter(|c| !c.is_control()).collect();
            invalid(format!(
                "« {shown} » n'est pas une adresse IP ni un bloc CIDR (ex. 203.0.113.7 ou 198.51.100.0/24)."
            ))
        })?;
        if !list.contains(&net) {
            list.push(net);
        }
    }
    Ok(list)
}

/// `PATCH /api/settings/security`
async fn update_security(
    State(state): State<AppState>,
    auth: Admin,
    ClientIp(ip): ClientIp,
    input: Result<Json<UpdateOrgSecurity>, JsonRejection>,
) -> Result<Response, ApiError> {
    let input = body(input)?;
    let allowlist = input
        .ip_allowlist
        .as_deref()
        .map(parse_allowlist)
        .transpose()?;
    // Garde-fou : jamais de liste qui coupe l'acces de la requete en cours.
    if let Some(list) = &allowlist
        && !list.is_empty()
        && !list.iter().any(|net| net.contains(ip))
    {
        return Err(ApiError::new(
            422,
            "ip_allowlist_lockout",
            format!(
                "Cette liste exclut votre adresse actuelle ({ip}) : ajoutez-la pour ne pas perdre l'accès."
            ),
        ));
    }
    let stored: Option<Vec<String>> = allowlist
        .as_ref()
        .map(|l| l.iter().map(ToString::to_string).collect());
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    let updated = sqlx::query(
        "UPDATE instance SET mfa_required = coalesce($1, mfa_required), \
         ip_allowlist = coalesce($2, ip_allowlist)",
    )
    .bind(input.mfa_required)
    .bind(&stored)
    .execute(&mut *tx)
    .await
    .map_err(db("modification des parametres"))?
    .rows_affected();
    if updated == 0 {
        return Err(not_configured());
    }
    audit(
        &mut tx,
        Some(auth.user.id),
        "settings.security.update",
        json!({ "ip": ip.to_string(), "mfa_required": input.mfa_required, "ip_allowlist": &stored }),
    )
    .await?;
    let body = load_security(&mut tx, &ip.to_string()).await?;
    tx.commit().await.map_err(db("transaction"))?;
    if let Some(list) = allowlist {
        state.ip_allowlist.set(state.clock.unix_secs(), list);
        state.access.notify();
    }
    Ok(Json(body).into_response())
}

/// `GET /api/settings/danger`
async fn danger(State(state): State<AppState>, auth: Admin) -> Result<Response, ApiError> {
    owner_only(&auth)?;
    let row: Option<(String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT name, (SELECT count(*) FROM projects), (SELECT count(*) FROM users), \
                (SELECT name FROM github_app) \
         FROM instance LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(db("lecture de la zone de danger"))?;
    let (name, projects, members, github_app) = row.ok_or_else(not_configured)?;
    Ok(Json(DangerZone {
        name,
        projects,
        members,
        github_app,
    })
    .into_response())
}
