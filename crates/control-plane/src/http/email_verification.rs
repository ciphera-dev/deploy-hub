//! Verification d'adresse e-mail :
//! - `GET /api/auth/email-verification/{token}` : ecran de confirmation (adresse
//!   et motif du lien) ;
//! - `POST /api/auth/email-verification/{token}/confirm` : consomme le lien.
//!   Motif `change` : l'adresse du compte est remplacee, les autres sessions
//!   sont fermees, les liens de reinitialisation en cours tombent et l'ancienne
//!   adresse recoit un avis. Motif `signup` : le compte (cree en attente par
//!   l'inscription ouverte) peut se connecter.
//!
//! Lien a usage unique valable 24 heures, envoye a l'adresse a confirmer. Le
//! jeton (32 octets aleatoires) n'est jamais stocke, journalise ni ecrit dans
//! l'audit : la base n'en garde que le SHA-256. Consommation par
//! `UPDATE ... WHERE used_at IS NULL AND expires_at > maintenant`, atomique. La
//! confirmation n'exige pas de session (le lien s'ouvre souvent ailleurs) ; une
//! ouverture par GET ne change rien : seule la confirmation du POST agit.

use api_types::email_verification::EmailVerificationInfo;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::password_reset::audit;
use crate::auth::internal;
use crate::auth::session::{self, SessionToken};
use crate::state::AppState;

/// Duree de validite d'un lien, en secondes (24 heures).
pub const TTL_SECS: i64 = 24 * 3600;

pub const PURPOSE_CHANGE: &str = "change";
pub const PURPOSE_SIGNUP: &str = "signup";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/email-verification/{token}", get(info))
        .route(
            "/api/auth/email-verification/{token}/confirm",
            post(confirm),
        )
}

fn verification_invalid() -> ApiError {
    ApiError::new(
        404,
        "email_verification_invalid",
        "Lien de vérification invalide ou déjà utilisé.",
    )
}

fn verification_expired() -> ApiError {
    ApiError::new(
        410,
        "email_verification_expired",
        "Ce lien de vérification a expiré. Faites une nouvelle demande.",
    )
}

fn no_store(status: StatusCode) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")]).into_response()
}

/// Lien de l'ecran de confirmation : absolu avec l'origine publique, relatif
/// sinon ; jamais deduit de l'en-tete `Host`.
pub fn link(state: &AppState, token: &str) -> String {
    state.public_link(&format!("/verify-email/{token}"))
}

/// Cree un lien de verification de `new_email` pour ce compte, dans la
/// transaction de l'appelant, et supprime les liens inutilises du meme motif.
pub async fn issue(
    conn: &mut PgConnection,
    user_id: Uuid,
    new_email: &str,
    purpose: &str,
    now: f64,
) -> Result<SessionToken, sqlx::Error> {
    let token = SessionToken::generate().map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    sqlx::query(
        "DELETE FROM email_verifications WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL",
    )
    .bind(user_id)
    .bind(purpose)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO email_verifications (id, token_hash, user_id, new_email, purpose, created_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5, to_timestamp($6), to_timestamp($6) + make_interval(secs => $7))",
    )
    .bind(Uuid::new_v4())
    .bind(token.hash())
    .bind(user_id)
    .bind(new_email)
    .bind(purpose)
    .bind(now)
    .bind(TTL_SECS as f64)
    .execute(conn)
    .await?;
    Ok(token)
}

/// `GET /api/auth/email-verification/{token}`
async fn info(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let token = Zeroizing::new(token);
    let row: Option<(String, String, i64, bool, bool)> = sqlx::query_as(
        "SELECT purpose, new_email, extract(epoch FROM expires_at)::bigint, \
                used_at IS NOT NULL, expires_at <= to_timestamp($2) \
         FROM email_verifications WHERE token_hash = $1",
    )
    .bind(session::hash_token(&token))
    .bind(state.clock.unix_secs())
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du lien de verification", e))?;
    match row {
        None | Some((_, _, _, true, _)) => Err(verification_invalid()),
        Some((_, _, _, false, true)) => Err(verification_expired()),
        Some((purpose, email, expires_at, false, false)) => Ok((
            [(header::CACHE_CONTROL, "no-store")],
            Json(EmailVerificationInfo {
                purpose,
                email,
                expires_at,
            }),
        )
            .into_response()),
    }
}

/// `POST /api/auth/email-verification/{token}/confirm` (exempte de CSRF : le
/// lien porte sa propre preuve).
async fn confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let token = Zeroizing::new(token);
    let token_hash = session::hash_token(&token);
    let now = state.clock.unix_secs();
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de verification", e))?;
    let consumed: Option<(Uuid, String, String)> = sqlx::query_as(
        "UPDATE email_verifications SET used_at = to_timestamp($2) \
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > to_timestamp($2) \
         RETURNING user_id, new_email, purpose",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| internal("consommation du lien de verification", e))?;
    let Some((user_id, new_email, purpose)) = consumed else {
        let expired: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM email_verifications \
                            WHERE token_hash = $1 AND used_at IS NULL)",
        )
        .bind(&token_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| internal("lecture du lien de verification", e))?;
        return Err(if expired {
            verification_expired()
        } else {
            verification_invalid()
        });
    };

    let mut old_email = None;
    if purpose == PURPOSE_SIGNUP {
        // Seul un compte encore en attente, a cette adresse, est active.
        let activated = sqlx::query(
            "UPDATE users SET email_verified_at = to_timestamp($3) \
             WHERE id = $1 AND lower(email) = lower($2) AND email_verified_at IS NULL",
        )
        .bind(user_id)
        .bind(&new_email)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("activation du compte", e))?
        .rows_affected();
        if activated != 1 {
            return Err(verification_invalid());
        }
        audit(&mut tx, Some(user_id), "auth.email.verified", json!({}))
            .await
            .map_err(|e| internal("audit de verification", e))?;
    } else {
        let previous: Option<String> =
            sqlx::query_scalar("SELECT email FROM users WHERE id = $1 FOR UPDATE")
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| internal("lecture du compte", e))?;
        let Some(previous) = previous else {
            return Err(verification_invalid());
        };
        let updated = sqlx::query(
            "UPDATE users SET email = $2, email_verified_at = coalesce(email_verified_at, to_timestamp($3)) \
             WHERE id = $1",
        )
        .bind(user_id)
        .bind(&new_email)
        .bind(now)
        .execute(&mut *tx)
        .await;
        match updated {
            Ok(_) => {}
            Err(e) if is_unique_violation(&e) => {
                return Err(ApiError::new(
                    409,
                    "email_taken",
                    "Cette adresse e-mail est déjà utilisée par un autre compte.",
                ));
            }
            Err(e) => return Err(internal("changement d'adresse", e)),
        }
        // Les autres sessions tombent ; celle qui porte ce navigateur, si elle
        // est au nom du meme compte, reste ouverte.
        let keep = session::token_from_headers(&headers).map(session::hash_token);
        let revoked = sqlx::query(
            "DELETE FROM sessions WHERE user_id = $1 AND ($2::bytea IS NULL OR token_hash <> $2)",
        )
        .bind(user_id)
        .bind(keep)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("revocation des sessions", e))?
        .rows_affected();
        // Liens de reinitialisation envoyes a l'ancienne adresse, autres
        // demandes de changement en attente.
        sqlx::query("DELETE FROM password_resets WHERE user_id = $1 AND used_at IS NULL")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| internal("suppression des liens de reinitialisation", e))?;
        sqlx::query(
            "DELETE FROM email_verifications WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL",
        )
        .bind(user_id)
        .bind(PURPOSE_CHANGE)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("suppression des liens de verification", e))?;
        audit(
            &mut tx,
            Some(user_id),
            "profile.email.changed",
            json!({ "revoked_sessions": revoked }),
        )
        .await
        .map_err(|e| internal("audit du profil", e))?;
        old_email = Some(previous);
    }
    tx.commit()
        .await
        .map_err(|e| internal("transaction de verification", e))?;
    if let Some(old_email) = old_email {
        state.access.notify();
        super::me::notify_old_address(&state, &old_email).await;
    }
    Ok(no_store(StatusCode::NO_CONTENT))
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|code| code == "23505")
}
