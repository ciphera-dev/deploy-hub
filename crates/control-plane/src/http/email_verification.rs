//! Verification d'adresse e-mail :
//! - `GET /api/auth/email-verification/{token}` : ecran de confirmation (adresse
//!   et motif du lien) ;
//! - `POST /api/auth/email-verification/{token}/confirm` : consomme le lien.
//!   Motif `change` : l'adresse du compte est remplacee, les autres sessions
//!   sont fermees, les liens de reinitialisation en cours tombent et l'ancienne
//!   adresse recoit un avis. Motif `signup` : le corps porte le mot de passe du
//!   compte, choisi ici par le titulaire de la boite (l'inscription ouverte ne
//!   prend aucun mot de passe, comme l'acceptation d'une invitation, dont elle
//!   reprend la politique et la limite de debit) ; le compte peut alors se
//!   connecter.
//!
//! Lien a usage unique valable 24 heures, envoye a l'adresse a confirmer. Le
//! jeton (32 octets aleatoires) n'est jamais stocke, journalise ni ecrit dans
//! l'audit : la base n'en garde que le SHA-256. Consommation par
//! `UPDATE ... WHERE used_at IS NULL AND expires_at > maintenant`, atomique. Au
//! plus un lien par minute et par compte (`MAIL_COOLDOWN_SECS`) : au-dela, la
//! reponse est la meme mais rien n'est envoye. Les comptes d'inscription jamais
//! confirmes sont supprimes quand leur dernier lien a expire (`purge_pending`).
//! La confirmation n'exige pas de session (le lien s'ouvre souvent ailleurs) ;
//! une ouverture par GET ne change rien : seule la confirmation du POST agit.

use api_types::email_verification::{ConfirmEmailVerification, EmailVerificationInfo};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::invitations::hash_password;
use super::password_reset::audit;
use crate::auth::session::{self, SessionToken};
use crate::auth::{internal, password, rate_limit};
use crate::http::client_ip::ClientIp;
use crate::state::AppState;

/// Duree de validite d'un lien, en secondes (24 heures).
pub const TTL_SECS: i64 = 24 * 3600;
/// Delai minimal entre deux liens pour un meme compte, en secondes.
pub const MAIL_COOLDOWN_SECS: f64 = 60.0;
/// Corps JSON maximal (mot de passe de 1024 caracteres compris).
const MAX_BODY: usize = 16 * 1024;
/// Cle de limitation des confirmations d'inscription : 5 echecs par IP.
const CONFIRM_KEY: &str = "ciphera:email-verification-confirm";

pub const PURPOSE_CHANGE: &str = "change";
pub const PURPOSE_SIGNUP: &str = "signup";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/email-verification/{token}", get(info))
        .route(
            "/api/auth/email-verification/{token}/confirm",
            post(confirm),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY))
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
/// `None` (rien n'est cree ni envoye) si le dernier lien du compte a moins de
/// `MAIL_COOLDOWN_SECS` : pas de rafale d'e-mails vers une meme adresse.
pub async fn issue(
    conn: &mut PgConnection,
    user_id: Uuid,
    new_email: &str,
    purpose: &str,
    now: f64,
) -> Result<Option<SessionToken>, sqlx::Error> {
    let recent: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM email_verifications WHERE user_id = $1 \
                          AND created_at > to_timestamp($2) - make_interval(secs => $3))",
    )
    .bind(user_id)
    .bind(now)
    .bind(MAIL_COOLDOWN_SECS)
    .fetch_one(&mut *conn)
    .await?;
    if recent {
        return Ok(None);
    }
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
    Ok(Some(token))
}

/// Supprime les comptes d'inscription ouverte jamais confirmes : adresse non
/// verifiee et plus aucun lien valable (24 h apres le dernier envoi).
pub async fn purge_pending(db: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "DELETE FROM users u WHERE u.email_verified_at IS NULL \
           AND NOT EXISTS (SELECT 1 FROM email_verifications v WHERE v.user_id = u.id \
                             AND v.used_at IS NULL AND v.expires_at > now())",
    )
    .execute(db)
    .await
    .map(|done| done.rows_affected())
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
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    Path(token): Path<String>,
    body: Result<Option<Json<ConfirmEmailVerification>>, JsonRejection>,
) -> Result<Response, ApiError> {
    let token = Zeroizing::new(token);
    let token_hash = session::hash_token(&token);
    let now = state.clock.unix_secs();
    // Lecture sans consommation avant Argon2 : un lien inconnu, utilise ou
    // expire ne coute qu'une requete indexee.
    let usable: Option<(String, bool)> = sqlx::query_as(
        "SELECT purpose, expires_at <= to_timestamp($2) FROM email_verifications \
         WHERE token_hash = $1 AND used_at IS NULL",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du lien de verification", e))?;
    let purpose = match usable {
        None => return Err(verification_invalid()),
        Some((_, true)) => return Err(verification_expired()),
        Some((purpose, false)) => purpose,
    };
    // Inscription : le mot de passe est choisi ici, par le titulaire de la boite.
    let mut chosen: Option<(i64, String)> = None;
    if purpose == PURPOSE_SIGNUP {
        let Some(Json(input)) = body.ok().flatten() else {
            return Err(ApiError::new(
                422,
                "validation_failed",
                "Choisissez un mot de passe pour activer le compte.",
            ));
        };
        password::check_policy(&input.password)
            .map_err(|m| ApiError::new(422, "validation_failed", m))?;
        let attempt = match rate_limit::admit(
            &state.db,
            &rate_limit::IpKey::login(ip),
            &rate_limit::email_key(CONFIRM_KEY),
        )
        .await
        .map_err(|e| internal("limitation de debit", e))?
        {
            rate_limit::Admission::Allowed(id) => id,
            rate_limit::Admission::Limited(retry_after) => {
                let mut res = ApiError::new(
                    429,
                    "rate_limited",
                    "Trop de tentatives. Réessayez dans quelques minutes.",
                )
                .into_response();
                res.headers_mut()
                    .insert(header::RETRY_AFTER, retry_after.into());
                return Ok(res);
            }
        };
        let hash = hash_password(&state, attempt, input.password).await?;
        chosen = Some((attempt, hash));
    }

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
        let Some((attempt, password_hash)) = chosen else {
            return Err(verification_invalid());
        };
        // Seul un compte encore en attente, a cette adresse, est active.
        let activated = sqlx::query(
            "UPDATE users SET email_verified_at = to_timestamp($3), password_hash = $4 \
             WHERE id = $1 AND lower(email) = lower($2) AND email_verified_at IS NULL",
        )
        .bind(user_id)
        .bind(&new_email)
        .bind(now)
        .bind(&password_hash)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("activation du compte", e))?
        .rows_affected();
        if activated != 1 {
            return Err(verification_invalid());
        }
        rate_limit::mark_success(&mut tx, attempt)
            .await
            .map_err(|e| internal("enregistrement de la tentative", e))?;
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
