//! Zone de danger (proprietaire seulement, CSRF, audit) :
//! - `POST /api/organization/export` : export JSON de la configuration,
//!   sans aucun secret. Chaque champ est choisi un par un (liste blanche) :
//!   ni valeur de variable (meme non secrete), ni jeton, ni cle, ni hachage,
//!   ni graine TOTP, ni secret de webhook ou mot de passe SMTP ;
//! - `DELETE /api/organization { password, confirm: "SUPPRIMER" }` :
//!   re-authentification par le mot de passe (meme limite que les seconds
//!   facteurs), puis suppression de chaque projet comme
//!   `DELETE /api/projects/{id}` (conteneurs et images retires des agents,
//!   routes et certificats), des variables globales, des taches et metriques
//!   orphelines, des invitations, de tous les comptes sauf le proprietaire qui
//!   la demande et de la GitHub App locale (a supprimer a la main sur
//!   GitHub). Toutes les sessions sont revoquees, la sienne comprise.
//!   Restent : l'instance et sa configuration (domaine du panneau, TLS, SMTP),
//!   les serveurs (vides) et le journal d'audit. Pas de retour au setup.
//!
//! Un projet dont un agent est injoignable n'est pas supprime (ses conteneurs
//! tourneraient sans plus etre suivis) : il est renvoye dans
//! `remaining_projects` et la suppression peut etre relancee.

use api_types::settings::{
    DELETE_PHRASE, DEPLOY_RETENTION, DeleteOrganization, GithubAppNotice, OrganizationDeleted,
    RemainingProject,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use shared::error::ApiError;
use uuid::Uuid;

use crate::auth::{internal, rate_limit, session};
use crate::domain::domains::{self, panel};
use crate::http::client_ip::ClientIp;
use crate::http::me::{Reauth, reauthenticate};
use crate::http::projects::{Removal, delete_project};
use crate::http::settings::{Admin, audit, owner_only};
use crate::state::AppState;

const MAX_BODY: usize = 8 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/organization/export", post(export))
        .route("/api/organization", delete(remove))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn db(context: &'static str) -> impl Fn(sqlx::Error) -> ApiError {
    move |e| internal(context, e)
}

/// Requetes de l'export : (section, requete renvoyant une valeur JSON).
/// Colonnes listees une a une : aucun champ secret n'y figure.
const EXPORT: &[(&str, &str)] = &[
    (
        "organization",
        "SELECT json_build_object('id', id, 'name', name, 'slug', slug, 'timezone', timezone, \
         'language', language, 'created_at', created_at, 'mfa_required', mfa_required, \
         'ip_allowlist', ip_allowlist, 'deploy_auto', deploy_auto, 'public_signup', public_signup) \
         FROM instance LIMIT 1",
    ),
    (
        "members",
        "SELECT coalesce(json_agg(json_build_object('id', u.id, 'name', u.name, 'email', u.email, \
         'role', u.role, 'created_at', u.created_at, \
         'two_factor', EXISTS (SELECT 1 FROM user_totp t WHERE t.user_id = u.id AND t.confirmed_at IS NOT NULL) \
            OR EXISTS (SELECT 1 FROM webauthn_credentials w WHERE w.user_id = u.id)) \
         ORDER BY u.created_at), '[]'::json) FROM users u",
    ),
    (
        "projects",
        "SELECT coalesce(json_agg(json_build_object('id', p.id, 'name', p.name, \
         'source', p.source_kind, 'image', p.image_ref, \
         'build', json_build_object('kind', p.build_kind, 'path', p.build_path, 'service', p.build_service), \
         'generated_domain', p.generated_domain, 'created_at', p.created_at, \
         'git', (SELECT json_build_object('repository', g.repository, 'branch', g.branch, \
                 'root_directory', g.root_directory, 'watch_paths', g.watch_paths, \
                 'auto_deploy', g.auto_deploy, 'skip_ci', g.skip_ci, 'ignore_bots', g.ignore_bots) \
                 FROM git_sources g WHERE g.project_id = p.id), \
         'environments', (SELECT coalesce(json_agg(json_build_object('name', e.name, \
                 'services', (SELECT coalesce(json_agg(s.name ORDER BY s.name), '[]'::json) \
                              FROM services s WHERE s.environment_id = e.id), \
                 'variables', (SELECT coalesce(json_agg(json_build_object('key', v.key, 'secret', v.secret) \
                               ORDER BY v.key), '[]'::json) \
                               FROM project_variables v WHERE v.environment_id = e.id)) \
                 ORDER BY e.name), '[]'::json) FROM environments e WHERE e.project_id = p.id), \
         'domains', (SELECT coalesce(json_agg(json_build_object('host', d.host, 'path_prefix', d.path_prefix, \
                 'generated', d.generated, 'primary', d.is_primary, 'redirect', d.redirect, \
                 'dns_status', d.dns_status, 'cert_status', d.cert_status) ORDER BY d.host), '[]'::json) \
                 FROM domains d WHERE d.project_id = p.id), \
         'deploy_hooks', (SELECT coalesce(json_agg(json_build_object('id', h.id, 'name', h.name, \
                 'created_at', h.created_at, 'last_used_at', h.last_used_at) ORDER BY h.created_at), '[]'::json) \
                 FROM deploy_hooks h WHERE h.project_id = p.id)) \
         ORDER BY p.name), '[]'::json) FROM projects p",
    ),
    (
        "global_variables",
        "SELECT coalesce(json_agg(json_build_object('key', g.key, 'secret', g.secret, \
         'environments', g.environments, 'all_projects', g.all_projects, \
         'projects', (SELECT coalesce(json_agg(p.name ORDER BY p.name), '[]'::json) \
                      FROM global_variable_projects gp JOIN projects p ON p.id = gp.project_id \
                      WHERE gp.variable_id = g.id)) \
         ORDER BY g.key), '[]'::json) FROM global_variables g",
    ),
    (
        "servers",
        "SELECT coalesce(json_agg(json_build_object('id', id, 'name', name, 'kind', kind, \
         'created_at', created_at, 'revoked', revoked_at IS NOT NULL) ORDER BY created_at), '[]'::json) \
         FROM servers",
    ),
    (
        "notifications",
        "SELECT json_build_object('enabled', enabled, 'host', host, 'port', port, 'tls', tls, \
         'username', username, 'password_set', password_sealed IS NOT NULL, \
         'from_address', from_address, 'from_name', from_name) FROM smtp_settings LIMIT 1",
    ),
    (
        "github_app",
        "SELECT json_build_object('app_id', app_id, 'slug', slug, 'name', name, \
         'owner_login', owner_login, 'html_url', html_url, 'created_at', created_at) \
         FROM github_app LIMIT 1",
    ),
];

/// `POST /api/organization/export`
async fn export(
    State(state): State<AppState>,
    auth: Admin,
    ClientIp(ip): ClientIp,
) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    // Instantane coherent des sections.
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(db("transaction de l'export"))?;
    let mut doc = serde_json::Map::new();
    doc.insert("format".into(), json!("ciphera-export"));
    doc.insert("version".into(), json!(1));
    doc.insert("exported_at".into(), json!(state.clock.unix_secs() as i64));
    for (section, sql) in EXPORT {
        let value: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(*sql))
            .fetch_optional(&mut *tx)
            .await
            .map_err(db("lecture de l'export"))?;
        doc.insert((*section).into(), value.unwrap_or(Value::Null));
    }
    let panel = panel::view(&mut tx)
        .await
        .map_err(db("domaine du panneau"))?;
    doc.insert(
        "panel_domain".into(),
        json!({ "domain": panel.domain, "status": panel.status }),
    );
    doc.insert(
        "deployments".into(),
        json!({ "retention": DEPLOY_RETENTION }),
    );
    tx.commit().await.map_err(db("transaction"))?;

    let slug = doc["organization"]["slug"]
        .as_str()
        .filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .unwrap_or("organisation")
        .to_string();
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    audit(
        &mut conn,
        Some(actor),
        "organization.export",
        json!({ "ip": ip.to_string() }),
    )
    .await?;
    Ok((
        [
            (header::CACHE_CONTROL, "no-store".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"ciphera-{slug}.json\""),
            ),
        ],
        Json(Value::Object(doc)),
    )
        .into_response())
}

/// `DELETE /api/organization`
async fn remove(
    State(state): State<AppState>,
    auth: Admin,
    ClientIp(ip): ClientIp,
    input: Result<Json<DeleteOrganization>, JsonRejection>,
) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    let Json(input) =
        input.map_err(|_| ApiError::new(422, "validation_failed", "Requête invalide."))?;
    if input.confirm != DELETE_PHRASE {
        return Err(ApiError::new(
            422,
            "confirmation_required",
            format!("Tapez {DELETE_PHRASE} pour confirmer."),
        ));
    }
    let ip = ip.to_string();
    let attempt = match reauthenticate(
        &state,
        &auth.user,
        &ip,
        input.password,
        "organization.delete.failure",
    )
    .await?
    {
        Reauth::Verified(attempt, _) => attempt,
        Reauth::Limited(response) => return Ok(response),
    };

    // 1. Projets, un par un (chacun dans sa transaction : ce qui est fait le
    // reste si la suite echoue, et une nouvelle demande reprend le reste).
    // Un projet qui echoue (agent injoignable, erreur) est garde et signale
    // dans `remaining_projects` ; la suite continue.
    let projects: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM projects ORDER BY name")
            .fetch_all(&state.db)
            .await
            .map_err(db("liste des projets"))?;
    let mut hosts = Vec::new();
    let mut deleted = 0_i64;
    for (id, name) in &projects {
        // Organisation supprimee : les volumes des projets aussi.
        match delete_project(&state, actor, *id, true, true).await {
            Ok(Removal::Deleted(h)) => {
                hosts.extend(h);
                deleted += 1;
            }
            Ok(Removal::NotFound) => {}
            Ok(Removal::AgentUnreachable(servers)) => {
                tracing::warn!(project = %name, ?servers, "projet garde : agent injoignable");
            }
            Err(e) => {
                tracing::error!(project = %name, error = ?e, "projet garde : suppression en echec");
            }
        }
    }

    // 2. Le reste de l'organisation, en une transaction.
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ciphera.organization_delete'))")
        .execute(&mut *tx)
        .await
        .map_err(db("verrou de la suppression"))?;
    for (context, sql) in [
        ("variables globales", "DELETE FROM global_variables"),
        ("invitations", "DELETE FROM invitations"),
        // Taches des projets supprimes (jamais une tache en cours : son
        // travailleur la termine et trouve les lignes absentes).
        (
            "taches orphelines",
            "DELETE FROM jobs WHERE status <> 'running' AND ( \
               (payload ? 'deployment_id' AND NOT EXISTS (SELECT 1 FROM deployments d \
                  WHERE d.id::text = payload->>'deployment_id')) \
               OR (payload ? 'environment_id' AND NOT EXISTS (SELECT 1 FROM environments e \
                  WHERE e.id::text = payload->>'environment_id')) \
               OR (kind = 'acme_issue' AND NOT EXISTS (SELECT 1 FROM domains x \
                  WHERE x.id::text = payload->>'domain_id')))",
        ),
        // Toutes les metriques (decision du 2026-11-23), serveurs compris :
        // les serveurs gardes repartent d'un historique vide.
        ("metriques", "DELETE FROM container_metrics_raw"),
        ("metriques", "DELETE FROM container_metrics_hourly"),
        ("metriques", "DELETE FROM node_metrics_raw"),
        ("metriques", "DELETE FROM node_metrics_hourly"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *tx)
            .await
            .map_err(db(context))?;
    }
    let users = sqlx::query("DELETE FROM users WHERE id <> $1")
        .bind(actor)
        .execute(&mut *tx)
        .await
        .map_err(db("suppression des comptes"))?
        .rows_affected();
    let github: Option<(String, String)> =
        sqlx::query_as("DELETE FROM github_app RETURNING name, html_url")
            .fetch_optional(&mut *tx)
            .await
            .map_err(db("suppression de la GitHub App"))?;
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(db("enregistrement de la tentative"))?;
    // Toutes les sessions, celle de la demande comprise.
    sqlx::query("DELETE FROM sessions")
        .execute(&mut *tx)
        .await
        .map_err(db("revocation des sessions"))?;
    let remaining: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT id, name FROM projects ORDER BY name")
            .fetch_all(&mut *tx)
            .await
            .map_err(db("liste des projets"))?;
    audit(
        &mut tx,
        Some(actor),
        "organization.delete",
        json!({
            "ip": ip,
            "projects_deleted": deleted,
            "remaining_projects": remaining.iter().map(|(_, n)| n).collect::<Vec<_>>(),
            "users_deleted": users,
            "github_app": github.as_ref().map(|(name, _)| name),
        }),
    )
    .await?;
    tx.commit().await.map_err(db("transaction"))?;
    state.access.notify();

    if !hosts.is_empty() || deleted > 0 {
        domains::push_routes(&state).await;
        domains::remove_certificates(&state, &hosts);
    }
    if github.is_some() {
        state.github.forget_tokens();
    }
    let body = OrganizationDeleted {
        complete: remaining.is_empty(),
        projects_deleted: deleted,
        remaining_projects: remaining
            .into_iter()
            .map(|(id, name)| RemainingProject {
                id: id.to_string(),
                name,
            })
            .collect(),
        github_app: github.map(|(name, html_url)| GithubAppNotice { name, html_url }),
    };
    Ok((
        [
            (header::SET_COOKIE, session::clear_cookie()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        Json(body),
    )
        .into_response())
}
