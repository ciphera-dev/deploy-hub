//! Equipe : membres et invitations en attente (Equipe : Admin requis).
//!
//! - `GET /api/team/members` ; `PATCH` (role) et `DELETE /api/team/members/{id}` ;
//! - `GET /api/team/invitations` (non utilisees, non expirees) et
//!   `DELETE /api/team/invitations/{id}` (la creation est dans `invitations`).
//!
//! Regles de rang : [`Role::can_manage`] sur le role actuel de la cible, et
//! [`Role::can_grant`] sur le nouveau role ; jamais soi-meme (403
//! `self_forbidden`), jamais le dernier proprietaire (409 `last_owner`). Dans
//! la transaction, les lignes des proprietaires sont verrouillees (`FOR
//! UPDATE`, par identifiant) puis le role de l'acteur est relu : deux
//! proprietaires qui se retirent en meme temps ne laissent pas zero
//! proprietaire.
//!
//! Retrait : le compte est supprime (sessions, facteurs, jetons de
//! reinitialisation suivent en cascade), ainsi que ses deploy hooks ; les
//! flux de logs ouverts se ferment (`access.notify()` apres la validation).
//! Changement de role : les sessions sont gardees, le role est relu a chaque
//! requete. Chaque action ecrit une ligne d'audit, sans e-mail ni jeton.

use api_types::team::{PendingInvitation, RemovedMember, TeamMember, UpdateMemberRole};
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::internal;
use crate::authz::{Require, Role, forbidden, lvl, res};
use crate::http::client_ip::ClientIp;
use crate::http::invitations::audit;
use crate::state::AppState;

const MAX_BODY: usize = 4 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/team/members", get(list_members))
        .route(
            "/api/team/members/{id}",
            delete(remove_member).patch(update_member),
        )
        .route("/api/team/invitations/{id}", delete(cancel_invitation))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn member_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Membre introuvable.")
}

fn invitation_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Invitation introuvable.")
}

fn self_forbidden() -> ApiError {
    ApiError::new(
        403,
        "self_forbidden",
        "Vous ne pouvez pas modifier votre propre compte ici.",
    )
}

fn last_owner() -> ApiError {
    ApiError::new(
        409,
        "last_owner",
        "L'organisation doit garder au moins un propriétaire.",
    )
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

/// `GET /api/team/members`
async fn list_members(
    State(state): State<AppState>,
    _auth: Require<res::Team, lvl::Admin>,
) -> Result<Response, ApiError> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, String, String, bool, Option<i64>, i64)> = sqlx::query_as(
        "SELECT u.id, u.name, u.email, u.role, \
                (EXISTS (SELECT 1 FROM user_totp t \
                          WHERE t.user_id = u.id AND t.confirmed_at IS NOT NULL) \
                 OR EXISTS (SELECT 1 FROM webauthn_credentials w WHERE w.user_id = u.id)), \
                (SELECT extract(epoch FROM max(s.last_seen_at))::bigint \
                   FROM sessions s WHERE s.user_id = u.id AND NOT s.mfa_pending), \
                extract(epoch FROM u.created_at)::bigint \
         FROM users u ORDER BY u.created_at, u.email",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| internal("lecture des membres", e))?;
    let members: Vec<TeamMember> = rows
        .into_iter()
        .map(
            |(id, name, email, role, two_factor, last_active_at, created_at)| TeamMember {
                id: id.to_string(),
                name,
                email,
                role,
                two_factor,
                last_active_at,
                created_at,
            },
        )
        .collect();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(members)).into_response())
}

/// Cible lue et verrouillee dans la transaction, role de l'acteur relu.
struct Locked {
    actor: Role,
    target: Role,
    owners: usize,
}

/// Verrouille les proprietaires (par identifiant), relit le role de l'acteur
/// et celui de la cible (verrouillee aussi), puis applique `can_manage`.
async fn lock_and_check(
    tx: &mut PgConnection,
    actor_id: Uuid,
    target_id: Uuid,
) -> Result<Locked, ApiError> {
    let owners: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE role = 'owner' ORDER BY id FOR UPDATE")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| internal("verrou des proprietaires", e))?;
    // Un acteur retire entre-temps n'a plus aucun droit.
    let actor: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
        .bind(actor_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| internal("lecture de l'acteur", e))?;
    let actor = actor.and_then(|r| Role::parse(&r)).ok_or_else(forbidden)?;
    let target: Option<String> =
        sqlx::query_scalar("SELECT role FROM users WHERE id = $1 FOR UPDATE")
            .bind(target_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| internal("lecture du membre", e))?;
    let target = target.ok_or_else(member_not_found)?;
    // Un role inconnu en base n'est gere par personne.
    let target = Role::parse(&target).ok_or_else(forbidden)?;
    if !actor.can_manage(target) {
        return Err(forbidden());
    }
    Ok(Locked {
        actor,
        target,
        owners: owners.len(),
    })
}

/// `PATCH /api/team/members/{id}`
async fn update_member(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Require<res::Team, lvl::Admin>,
    id: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<UpdateMemberRole>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = id.map_err(|_| member_not_found())?;
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let new_role = Role::parse(&input.role).ok_or_else(|| invalid("Rôle invalide."))?;
    if id == auth.user.id {
        return Err(self_forbidden());
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de changement de role", e))?;
    let locked = lock_and_check(&mut tx, auth.user.id, id).await?;
    if !locked.actor.can_grant(new_role) {
        return Err(forbidden());
    }
    // Filet de securite : `can_manage(Owner)` exige un proprietaire different de
    // la cible, donc il en reste deja un autre ; cette garde ne se declenche pas
    // aujourd'hui et n'est pas couverte par un test.
    if locked.target == Role::Owner && new_role != Role::Owner && locked.owners < 2 {
        return Err(last_owner());
    }
    sqlx::query("UPDATE users SET role = $2 WHERE id = $1")
        .bind(id)
        .bind(new_role.as_str())
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("changement de role", e))?;
    audit(
        &mut tx,
        auth.user.id,
        "team.member.role_changed",
        "user",
        json!({
            "user_id": id,
            "from": locked.target.as_str(),
            "to": new_role.as_str(),
            "ip": ip.to_string(),
        }),
    )
    .await
    .map_err(|e| internal("audit du changement de role", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de changement de role", e))?;
    // Les flux ouverts reverifient le role du compte.
    state.access.notify();
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `DELETE /api/team/members/{id}`
async fn remove_member(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Require<res::Team, lvl::Admin>,
    id: Result<Path<Uuid>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = id.map_err(|_| member_not_found())?;
    if id == auth.user.id {
        return Err(self_forbidden());
    }
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de retrait", e))?;
    let locked = lock_and_check(&mut tx, auth.user.id, id).await?;
    // Filet de securite (voir `update_member`) : inatteignable aujourd'hui.
    if locked.target == Role::Owner && locked.owners < 2 {
        return Err(last_owner());
    }
    // Les invitations encore utilisables du membre ne lui survivent pas.
    let invitations =
        sqlx::query("DELETE FROM invitations WHERE invited_by = $1 AND used_at IS NULL")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| internal("suppression des invitations", e))?
            .rows_affected();
    let hooks = sqlx::query("DELETE FROM deploy_hooks WHERE created_by = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("suppression des deploy hooks", e))?
        .rows_affected();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("suppression du membre", e))?;
    audit(
        &mut tx,
        auth.user.id,
        "team.member.removed",
        "user",
        json!({
            "user_id": id,
            "role": locked.target.as_str(),
            "deleted_hooks": hooks,
            "deleted_invitations": invitations,
            "ip": ip.to_string(),
        }),
    )
    .await
    .map_err(|e| internal("audit du retrait", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de retrait", e))?;
    // Sessions supprimees : les flux de logs du membre se ferment.
    state.access.notify();
    Ok(Json(RemovedMember {
        deleted_hooks: hooks,
        deleted_invitations: invitations,
    })
    .into_response())
}

/// `GET /api/team/invitations`
pub(crate) async fn list_invitations(
    State(state): State<AppState>,
    _auth: Require<res::Team, lvl::Admin>,
) -> Result<Response, ApiError> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, String, String, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT i.id, i.email, i.name, i.role, u.name, \
                extract(epoch FROM i.created_at)::bigint, \
                extract(epoch FROM i.expires_at)::bigint \
         FROM invitations i LEFT JOIN users u ON u.id = i.invited_by \
         WHERE i.used_at IS NULL AND i.expires_at > to_timestamp($1) \
         ORDER BY i.created_at DESC, i.id",
    )
    .bind(state.clock.unix_secs())
    .fetch_all(&state.db)
    .await
    .map_err(|e| internal("lecture des invitations", e))?;
    let invitations: Vec<PendingInvitation> = rows
        .into_iter()
        .map(
            |(id, email, name, role, invited_by, created_at, expires_at)| PendingInvitation {
                id: id.to_string(),
                email,
                name,
                role,
                invited_by,
                created_at,
                expires_at,
            },
        )
        .collect();
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(invitations)).into_response())
}

/// `DELETE /api/team/invitations/{id}`
async fn cancel_invitation(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Require<res::Team, lvl::Admin>,
    id: Result<Path<Uuid>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = id.map_err(|_| invitation_not_found())?;
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction d'annulation", e))?;
    let role: Option<String> = sqlx::query_scalar(
        "SELECT role FROM invitations \
         WHERE id = $1 AND used_at IS NULL AND expires_at > to_timestamp($2) FOR UPDATE",
    )
    .bind(id)
    .bind(state.clock.unix_secs())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| internal("lecture de l'invitation", e))?;
    let role = role.ok_or_else(invitation_not_found)?;
    // Role de l'acteur relu dans la transaction (comme les routes membres).
    let actor: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
        .bind(auth.user.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| internal("lecture de l'acteur", e))?;
    let actor = actor.and_then(|r| Role::parse(&r)).ok_or_else(forbidden)?;
    if !Role::parse(&role).is_some_and(|r| actor.can_grant(r)) {
        return Err(forbidden());
    }
    sqlx::query("DELETE FROM invitations WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("annulation de l'invitation", e))?;
    audit(
        &mut tx,
        auth.user.id,
        "team.invitation.cancelled",
        "invitation",
        json!({ "invitation_id": id, "role": role, "ip": ip.to_string() }),
    )
    .await
    .map_err(|e| internal("audit d'annulation", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction d'annulation", e))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
