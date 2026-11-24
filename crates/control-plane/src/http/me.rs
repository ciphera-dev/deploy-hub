//! Compte connecte (Profil) :
//! - `GET /api/me` (ouvert aussi a une session restreinte a l'enrolement) et
//!   `PATCH /api/me` (nom, e-mail, langue, fuseau horaire) ;
//! - `GET|PUT|DELETE /api/me/avatar` ;
//! - `POST /api/me/password` ;
//! - `GET /api/me/sessions`, `DELETE /api/me/sessions/{id}` ;
//! - `GET|PUT /api/me/notification-prefs` (canal e-mail seulement en v0.1).
//!
//! Les ecritures passent par la couche CSRF. Le changement d'e-mail et de mot
//! de passe redemandent le mot de passe actuel (limite de 5 echecs, comme la
//! desactivation de la TOTP). Ni mot de passe, ni adresse e-mail, ni jeton ne
//! sont journalises ou ecrits dans l'audit.

use std::sync::{Arc, LazyLock};
use std::time::Instant;

use api_types::auth::Me;
use api_types::me::{
    AvatarUpdated, ChangePassword, NotificationPrefs, NotificationPrefsView, SessionInfo, Sessions,
    UpdateMe,
};
use api_types::mfa::AddFactor;
use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, JsonRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use tokio::sync::Semaphore;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::auth::audit;
use super::mfa::{invalid_request, rate_limited};
use super::setup::validate_email;
use crate::auth::{CurrentUser, factors, internal, password, rate_limit, totp, webauthn};
use crate::avatar::{self, AvatarError, UploadLimiter};
use crate::http::client_ip::ClientIp;
use crate::mail;
use crate::state::AppState;

/// Corps JSON maximal (deux mots de passe de 1024 caracteres compris).
const MAX_BODY: usize = 16 * 1024;
const MAX_NAME_CHARS: usize = 200;
const MAX_TIMEZONE_LEN: usize = 64;
const LANGUAGES: &[&str] = &["fr", "en"];

/// Preferences d'un compte de role `role` qui n'en a jamais enregistre.
/// Certificats actives pour le proprietaire et l'administrateur (seuls
/// avertis du certificat du panneau), desactives pour les autres.
pub fn default_prefs(role: &str) -> NotificationPrefs {
    NotificationPrefs {
        email_enabled: true,
        deploy_succeeded: true,
        deploy_failed: true,
        certificates: matches!(role, "owner" | "admin"),
        team: true,
    }
}

/// Decodages d'avatar simultanes (jeton tenu jusqu'a la fin du calcul).
static AVATAR_SLOTS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(avatar::MAX_CONCURRENT)));
/// Envois d'avatar a decoder, par compte.
static AVATAR_UPLOADS: LazyLock<UploadLimiter> = LazyLock::new(UploadLimiter::default);

pub fn routes() -> Router<AppState> {
    let json = Router::new()
        .route("/api/me", get(me).patch(update))
        .route("/api/me/password", post(change_password))
        .route("/api/me/sessions", get(sessions))
        .route("/api/me/sessions/{id}", delete(revoke_session))
        .route("/api/me/notification-prefs", get(get_prefs).put(put_prefs))
        .layer(DefaultBodyLimit::max(MAX_BODY));
    let avatar = Router::new()
        .route(
            "/api/me/avatar",
            get(get_avatar).put(put_avatar).delete(delete_avatar),
        )
        .layer(DefaultBodyLimit::max(avatar::MAX_BYTES));
    json.merge(avatar)
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn no_store(body: impl IntoResponse) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], body).into_response()
}

async fn begin(state: &AppState) -> Result<sqlx::Transaction<'static, sqlx::Postgres>, ApiError> {
    state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction du profil", e))
}

/// Reponse de `GET /api/me`, relue en base (apres une modification aussi).
async fn load_me(state: &AppState, user: &CurrentUser) -> Result<Me, ApiError> {
    let (name, email, role, language, timezone, avatar_version): (
        String,
        String,
        String,
        String,
        String,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT u.name, u.email, u.role, u.language, u.timezone, \
                (extract(epoch FROM a.updated_at) * 1000)::bigint \
         FROM users u LEFT JOIN user_avatars a ON a.user_id = u.id WHERE u.id = $1",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| internal("lecture du compte", e))?;
    let totp_enabled = totp::is_enabled(&state.db, user.id)
        .await
        .map_err(|e| internal("lecture de la double authentification", e))?;
    let webauthn_enabled = webauthn::has_any(&state.db, user.id)
        .await
        .map_err(|e| internal("lecture des passkeys", e))?;
    Ok(Me {
        id: user.id.to_string(),
        name,
        email,
        role,
        csrf_token: user.csrf_token().to_string(),
        totp_enabled,
        webauthn_enabled,
        mfa_enrollment_required: user.session.enrollment_required,
        language,
        timezone,
        avatar_url: avatar_version.map(avatar_url),
    })
}

fn avatar_url(version: i64) -> String {
    format!("/api/me/avatar?v={version}")
}

/// `GET /api/me`
async fn me(State(state): State<AppState>, user: CurrentUser) -> Result<Response, ApiError> {
    Ok(no_store(Json(load_me(&state, &user).await?)))
}

/// Re-authentification par le mot de passe actuel (changement d'e-mail ou de
/// mot de passe) : meme limite que les seconds facteurs, echec audite.
pub(crate) enum Reauth {
    /// Tentative a marquer reussie dans la transaction de la modification, et
    /// hachage verifie (a recomparer dans cette transaction).
    Verified(i64, String),
    /// Limite atteinte : reponse 429 prete.
    Limited(Response),
}

pub(crate) async fn reauthenticate(
    state: &AppState,
    user: &CurrentUser,
    ip: &str,
    password_input: Zeroizing<String>,
    failure_action: &str,
) -> Result<Reauth, ApiError> {
    if password_input.is_empty() || password_input.chars().count() > password::MAX_LEN {
        return Err(invalid_request());
    }
    let attempt = match rate_limit::admit_mfa(
        &state.db,
        user.id,
        user.session.id,
        rate_limit::MfaFactor::Other,
    )
    .await
    .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => {
            return Ok(Reauth::Limited(rate_limited(retry_after)));
        }
    };
    let phc: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| internal("lecture du compte", e))?;
    let checked = phc.clone();
    let ok = match password::offload(move || password::verify(&password_input, &checked)).await {
        Ok(ok) => ok,
        Err(e) => {
            return Err(rate_limit::offload_error(
                &state.db,
                rate_limit::Attempt::Mfa(attempt),
                e,
                "verification du mot de passe",
            )
            .await);
        }
    };
    if ok {
        return Ok(Reauth::Verified(attempt, phc));
    }
    let mut tx = begin(state).await?;
    audit(
        &mut tx,
        Some(user.id),
        failure_action,
        json!({ "ip": ip, "reason": "invalid_credentials" }),
    )
    .await
    .map_err(|e| internal("audit du profil", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction du profil", e))?;
    Err(ApiError::new(
        401,
        "invalid_credentials",
        "Mot de passe actuel incorrect.",
    ))
}

/// Mot de passe actuel exige avant d'ajouter un second facteur (passkey ou
/// TOTP), sauf pour une session restreinte a l'enrolement obligatoire (le
/// compte n'a alors aucun facteur et vient de se connecter par mot de
/// passe). `Ok(Some(reponse))` : limite atteinte, reponse 429 prete. Une
/// verification reussie ne compte pas comme un echec.
pub(crate) async fn require_password_for_factor(
    state: &AppState,
    user: &CurrentUser,
    ip: &str,
    body: Result<Json<AddFactor>, JsonRejection>,
    failure_action: &str,
) -> Result<Option<Response>, ApiError> {
    // Exemption seulement tant que le compte n'a aucun facteur (relu ici) :
    // le drapeau de la session est fige a la connexion.
    if user.session.enrollment_required {
        let mut conn = state
            .db
            .acquire()
            .await
            .map_err(|e| internal("connexion a la base", e))?;
        let f = factors::read(&mut conn, user.id)
            .await
            .map_err(|e| internal("lecture des seconds facteurs", e))?;
        if !f.totp && f.passkeys == 0 {
            return Ok(None);
        }
    }
    let missing = || {
        ApiError::new(
            401,
            "invalid_credentials",
            "Saisissez votre mot de passe actuel pour ajouter un second facteur.",
        )
    };
    let Json(input) = body.map_err(|_| missing())?;
    if input.current_password.is_empty() {
        return Err(missing());
    }
    match reauthenticate(state, user, ip, input.current_password, failure_action).await? {
        Reauth::Verified(attempt, _) => {
            let mut conn = state
                .db
                .acquire()
                .await
                .map_err(|e| internal("connexion a la base", e))?;
            rate_limit::mark_mfa_success(&mut conn, attempt)
                .await
                .map_err(|e| internal("enregistrement de la tentative", e))?;
            Ok(None)
        }
        Reauth::Limited(res) => Ok(Some(res)),
    }
}

/// Avis au titulaire apres l'ajout d'un second facteur, hors du chemin de la
/// requete, si l'instance peut envoyer des e-mails. Aucun echec ne remonte.
pub(crate) async fn notify_factor_added(state: &AppState, email: &str, factor: mail::Factor) {
    match mail::ready(state).await {
        Ok(Some(mailer)) => {
            state.mail.spawn(
                mailer,
                mail::factor_added_mail(email, factor),
                "factor_added",
            );
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "avis d'ajout de facteur non envoye"),
    }
}

fn clean_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(invalid("Le nom doit contenir de 1 à 200 caractères."));
    }
    Ok(name.to_string())
}

/// Fuseau horaire de la base IANA embarquee, nom exact (ex. `Europe/Zurich`) :
/// la recherche de jiff ignore la casse, le nom canonique doit donc etre
/// identique a la saisie.
pub(crate) fn valid_timezone(tz: &str) -> bool {
    tz.len() <= MAX_TIMEZONE_LEN
        && jiff::tz::db()
            .get(tz)
            .is_ok_and(|zone| zone.iana_name() == Some(tz))
}

fn password_changed() -> ApiError {
    ApiError::new(
        409,
        "password_changed",
        "Le mot de passe a été modifié entre-temps. Réessayez.",
    )
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|code| code == "23505")
}

/// `PATCH /api/me`
async fn update(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    body: Result<Json<UpdateMe>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    let ip = ip.to_string();
    let name = input.name.as_deref().map(clean_name).transpose()?;
    if let Some(language) = &input.language
        && !LANGUAGES.contains(&language.as_str())
    {
        return Err(invalid("Langue non prise en charge."));
    }
    if let Some(tz) = &input.timezone
        && !valid_timezone(tz)
    {
        return Err(invalid("Fuseau horaire inconnu."));
    }
    let new_email = match &input.email {
        Some(email) => {
            let email = email.trim().to_lowercase();
            if !validate_email(&email) {
                return Err(invalid("Adresse e-mail invalide."));
            }
            // Meme adresse (a la casse pres) : rien a changer.
            (email != user.email.to_lowercase()).then_some(email)
        }
        None => None,
    };
    let attempt = match &new_email {
        Some(_) => {
            let Some(current) = input.current_password else {
                return Err(invalid(
                    "Saisissez votre mot de passe actuel pour changer d'adresse e-mail.",
                ));
            };
            match reauthenticate(&state, &user, &ip, current, "profile.email.change.failure")
                .await?
            {
                Reauth::Verified(attempt, phc) => Some((attempt, phc)),
                Reauth::Limited(res) => return Ok(res),
            }
        }
        None => None,
    };

    let mut tx = begin(&state).await?;
    let (old_email, current_hash): (String, String) =
        sqlx::query_as("SELECT email, password_hash FROM users WHERE id = $1 FOR UPDATE")
            .bind(user.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| internal("lecture du compte", e))?;
    // Mot de passe change entre la verification et cette transaction.
    if let Some((_, verified)) = &attempt
        && *verified != current_hash
    {
        return Err(password_changed());
    }
    let updated = sqlx::query(
        "UPDATE users SET name = coalesce($2, name), email = coalesce($3, email), \
                language = coalesce($4, language), timezone = coalesce($5, timezone) \
         WHERE id = $1",
    )
    .bind(user.id)
    .bind(&name)
    .bind(&new_email)
    .bind(&input.language)
    .bind(&input.timezone)
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(_) => {}
        Err(e) if is_unique_violation(&e) => {
            // Mot de passe juste : la tentative ne compte pas comme un echec.
            drop(tx);
            if let Some((attempt, _)) = attempt {
                let mut conn = state
                    .db
                    .acquire()
                    .await
                    .map_err(|e| internal("connexion a la base", e))?;
                rate_limit::mark_mfa_success(&mut conn, attempt)
                    .await
                    .map_err(|e| internal("enregistrement de la tentative", e))?;
            }
            return Err(ApiError::new(
                409,
                "email_taken",
                "Cette adresse e-mail est déjà utilisée par un autre compte.",
            ));
        }
        Err(e) => return Err(internal("modification du profil", e)),
    }
    let fields: Vec<&str> = [
        ("name", name.is_some()),
        ("language", input.language.is_some()),
        ("timezone", input.timezone.is_some()),
    ]
    .into_iter()
    .filter_map(|(field, changed)| changed.then_some(field))
    .collect();
    if !fields.is_empty() {
        audit(
            &mut tx,
            Some(user.id),
            "profile.updated",
            json!({ "ip": ip, "fields": fields }),
        )
        .await
        .map_err(|e| internal("audit du profil", e))?;
    }
    if new_email.is_some() {
        // Les liens de reinitialisation envoyes a l'ancienne adresse tombent.
        sqlx::query("DELETE FROM password_resets WHERE user_id = $1 AND used_at IS NULL")
            .bind(user.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| internal("suppression des liens de reinitialisation", e))?;
        audit(
            &mut tx,
            Some(user.id),
            "profile.email.changed",
            json!({ "ip": ip }),
        )
        .await
        .map_err(|e| internal("audit du profil", e))?;
    }
    if let Some((attempt, _)) = attempt {
        rate_limit::mark_mfa_success(&mut tx, attempt)
            .await
            .map_err(|e| internal("enregistrement de la tentative", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| internal("transaction du profil", e))?;

    if new_email.is_some() {
        notify_old_address(&state, &old_email).await;
    }
    Ok(no_store(Json(load_me(&state, &user).await?)))
}

/// Avis a l'ancienne adresse, hors du chemin de la requete, si l'instance
/// peut envoyer des e-mails. Aucun echec ne remonte (ni adresse au journal).
async fn notify_old_address(state: &AppState, old_email: &str) {
    match mail::ready(state).await {
        Ok(Some(mailer)) => {
            state
                .mail
                .spawn(mailer, mail::email_changed_mail(old_email), "email_changed");
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "avis de changement d'e-mail non envoye"),
    }
}

/// Version demandee (`?v=`) dans l'URL de l'avatar.
fn requested_version(uri: &Uri) -> Option<i64> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("v="))?
        .parse()
        .ok()
}

/// `GET /api/me/avatar` : PNG re-encode, type fixe, `nosniff`. L'URL
/// versionnee (`?v=`) se garde en cache ; sans version, revalidation.
async fn get_avatar(
    State(state): State<AppState>,
    user: CurrentUser,
    uri: Uri,
) -> Result<Response, ApiError> {
    let row: Option<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT png, (extract(epoch FROM updated_at) * 1000)::bigint FROM user_avatars WHERE user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture de l'avatar", e))?;
    let Some((png, version)) = row else {
        return Err(ApiError::new(404, "not_found", "Aucun avatar."));
    };
    let versioned = requested_version(&uri) == Some(version);
    let cache = if versioned {
        "private, max-age=31536000, immutable"
    } else {
        "private, no-cache"
    };
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CACHE_CONTROL, cache),
            (header::CONTENT_DISPOSITION, "inline"),
        ],
        png,
    )
        .into_response())
}

/// `PUT /api/me/avatar` : corps brut, format lu dans les octets.
async fn put_avatar(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, ApiError> {
    let too_large = || ApiError::new(413, "payload_too_large", "L'image dépasse 1 Mo.");
    let bytes = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            too_large()
        } else {
            invalid_request()
        }
    })?;
    if bytes.len() > avatar::MAX_BYTES {
        return Err(too_large());
    }
    let unsupported = || {
        ApiError::new(
            415,
            "unsupported_media_type",
            "Formats acceptés : PNG, JPEG ou WebP.",
        )
    };
    avatar::sniff(&bytes).map_err(|_| unsupported())?;
    if let Err(retry_after) = AVATAR_UPLOADS.admit(user.id, Instant::now()) {
        let mut res = ApiError::new(
            429,
            "rate_limited",
            "Trop d'envois d'image. Réessayez dans une minute.",
        )
        .into_response();
        res.headers_mut()
            .insert(header::RETRY_AFTER, retry_after.into());
        return Ok(res);
    }
    let png = avatar::process_limited(AVATAR_SLOTS.clone(), bytes)
        .await
        .map_err(|e| internal("traitement de l'avatar", e))?
        .map_err(|e| match e {
            AvatarError::Unsupported => unsupported(),
            AvatarError::Invalid => {
                invalid("Image illisible ou trop grande (4096 x 4096 au plus).")
            }
        })?;

    let mut tx = begin(&state).await?;
    let version: i64 = sqlx::query_scalar(
        "INSERT INTO user_avatars (user_id, png, updated_at) VALUES ($1, $2, clock_timestamp()) \
         ON CONFLICT (user_id) DO UPDATE SET png = excluded.png, updated_at = excluded.updated_at \
         RETURNING (extract(epoch FROM updated_at) * 1000)::bigint",
    )
    .bind(user.id)
    .bind(&png)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| internal("enregistrement de l'avatar", e))?;
    audit(
        &mut tx,
        Some(user.id),
        "profile.avatar.updated",
        json!({ "ip": ip.to_string() }),
    )
    .await
    .map_err(|e| internal("audit du profil", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction du profil", e))?;
    Ok(no_store(Json(AvatarUpdated {
        avatar_url: avatar_url(version),
    })))
}

/// `DELETE /api/me/avatar`
async fn delete_avatar(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
) -> Result<Response, ApiError> {
    let mut tx = begin(&state).await?;
    let deleted = sqlx::query("DELETE FROM user_avatars WHERE user_id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("suppression de l'avatar", e))?
        .rows_affected();
    if deleted > 0 {
        audit(
            &mut tx,
            Some(user.id),
            "profile.avatar.deleted",
            json!({ "ip": ip.to_string() }),
        )
        .await
        .map_err(|e| internal("audit du profil", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| internal("transaction du profil", e))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST /api/me/password` : garde la session courante, revoque les autres.
async fn change_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    body: Result<Json<ChangePassword>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    let ip = ip.to_string();
    password::check_policy(&input.new_password).map_err(invalid)?;
    let (attempt, verified) = match reauthenticate(
        &state,
        &user,
        &ip,
        input.current_password,
        "auth.password.change.failure",
    )
    .await?
    {
        Reauth::Verified(attempt, verified) => (attempt, verified),
        Reauth::Limited(res) => return Ok(res),
    };
    let new_password = input.new_password;
    let phc = match password::offload(move || password::hash(&new_password)).await {
        Ok(phc) => phc.map_err(|e| internal("hachage du mot de passe", e))?,
        Err(e) => {
            return Err(rate_limit::offload_error(
                &state.db,
                rate_limit::Attempt::Mfa(attempt),
                e,
                "hachage du mot de passe",
            )
            .await);
        }
    };

    let mut tx = begin(&state).await?;
    // La session courante doit encore exister (pas revoquee entre-temps)...
    let session_alive: bool =
        sqlx::query_scalar("SELECT true FROM sessions WHERE id = $1 AND user_id = $2 FOR UPDATE")
            .bind(user.session.id)
            .bind(user.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| internal("lecture de la session", e))?
            .unwrap_or(false);
    if !session_alive {
        return Err(crate::auth::unauthenticated());
    }
    // ... et le hachage verifie doit etre toujours celui du compte.
    let updated =
        sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1 AND password_hash = $3")
            .bind(user.id)
            .bind(&phc)
            .bind(&verified)
            .execute(&mut *tx)
            .await
            .map_err(|e| internal("modification du mot de passe", e))?
            .rows_affected();
    if updated != 1 {
        return Err(password_changed());
    }
    let revoked = revoke_other_sessions(&mut tx, &user).await?;
    sqlx::query("DELETE FROM password_resets WHERE user_id = $1 AND used_at IS NULL")
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("suppression des liens de reinitialisation", e))?;
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    audit(
        &mut tx,
        Some(user.id),
        "auth.password.changed",
        json!({ "ip": ip, "revoked_sessions": revoked }),
    )
    .await
    .map_err(|e| internal("audit du profil", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction du profil", e))?;
    state.access.notify();
    Ok(no_store(StatusCode::NO_CONTENT))
}

async fn revoke_other_sessions(
    conn: &mut PgConnection,
    user: &CurrentUser,
) -> Result<u64, ApiError> {
    Ok(
        sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND id <> $2")
            .bind(user.id)
            .bind(user.session.id)
            .execute(conn)
            .await
            .map_err(|e| internal("revocation des sessions", e))?
            .rows_affected(),
    )
}

/// (id, ip, user_agent, created_at, last_seen_at)
type SessionRow = (Uuid, Option<String>, Option<String>, i64, i64);

/// `GET /api/me/sessions`
async fn sessions(State(state): State<AppState>, user: CurrentUser) -> Result<Response, ApiError> {
    let rows: Vec<SessionRow> = sqlx::query_as(
        "SELECT id, ip, user_agent, extract(epoch FROM created_at)::bigint, \
                extract(epoch FROM last_seen_at)::bigint \
         FROM sessions WHERE user_id = $1 AND expires_at > now() \
         ORDER BY last_seen_at DESC, created_at DESC",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| internal("lecture des sessions", e))?;
    let sessions = rows
        .into_iter()
        .map(
            |(id, ip, user_agent, created_at, last_seen_at)| SessionInfo {
                id: id.to_string(),
                ip,
                user_agent,
                created_at,
                last_seen_at,
                current: id == user.session.id,
            },
        )
        .collect();
    Ok(no_store(Json(Sessions { sessions })))
}

/// `DELETE /api/me/sessions/{id}` : une autre session du compte seulement.
async fn revoke_session(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let not_found = || ApiError::new(404, "not_found", "Session introuvable.");
    let id = Uuid::parse_str(&id).map_err(|_| not_found())?;
    if id == user.session.id {
        return Err(ApiError::new(
            409,
            "current_session",
            "C'est la session en cours : utilisez la déconnexion.",
        ));
    }
    let mut tx = begin(&state).await?;
    let deleted = sqlx::query("DELETE FROM sessions WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("revocation de la session", e))?
        .rows_affected();
    if deleted == 0 {
        return Err(not_found());
    }
    audit(
        &mut tx,
        Some(user.id),
        "auth.session.revoked",
        json!({ "ip": ip.to_string(), "session": id.to_string() }),
    )
    .await
    .map_err(|e| internal("audit du profil", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction du profil", e))?;
    state.access.notify();
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn prefs_view(state: &AppState, prefs: NotificationPrefs) -> Result<Response, ApiError> {
    let email_available = mail::available(state)
        .await
        .map_err(|e| internal("lecture des reglages SMTP", e))?;
    Ok(no_store(Json(NotificationPrefsView {
        email_available,
        email_enabled: prefs.email_enabled,
        deploy_succeeded: prefs.deploy_succeeded,
        deploy_failed: prefs.deploy_failed,
        certificates: prefs.certificates,
        team: prefs.team,
    })))
}

/// `GET /api/me/notification-prefs`
async fn get_prefs(State(state): State<AppState>, user: CurrentUser) -> Result<Response, ApiError> {
    let row: Option<(bool, bool, bool, bool, bool)> = sqlx::query_as(
        "SELECT email_enabled, deploy_succeeded, deploy_failed, certificates, team \
         FROM notification_prefs WHERE user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture des preferences", e))?;
    let prefs = row.map_or_else(
        || default_prefs(&user.role),
        |(email_enabled, deploy_succeeded, deploy_failed, certificates, team)| NotificationPrefs {
            email_enabled,
            deploy_succeeded,
            deploy_failed,
            certificates,
            team,
        },
    );
    prefs_view(&state, prefs).await
}

/// `PUT /api/me/notification-prefs`
async fn put_prefs(
    State(state): State<AppState>,
    user: CurrentUser,
    body: Result<Json<NotificationPrefs>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(prefs) = body.map_err(|_| invalid_request())?;
    sqlx::query(
        "INSERT INTO notification_prefs \
             (user_id, email_enabled, deploy_succeeded, deploy_failed, certificates, team) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (user_id) DO UPDATE SET email_enabled = excluded.email_enabled, \
             deploy_succeeded = excluded.deploy_succeeded, deploy_failed = excluded.deploy_failed, \
             certificates = excluded.certificates, team = excluded.team, updated_at = now()",
    )
    .bind(user.id)
    .bind(prefs.email_enabled)
    .bind(prefs.deploy_succeeded)
    .bind(prefs.deploy_failed)
    .bind(prefs.certificates)
    .bind(prefs.team)
    .execute(&state.db)
    .await
    .map_err(|e| internal("enregistrement des preferences", e))?;
    prefs_view(&state, prefs).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timezones_are_exact_iana_names() {
        for tz in [
            "UTC",
            "Europe/Zurich",
            "America/Argentina/Buenos_Aires",
            "Asia/Kolkata",
        ] {
            assert!(valid_timezone(tz), "{tz}");
        }
        for tz in [
            "",
            "europe/zurich",
            "Mars/Olympus",
            "../etc/passwd",
            "Europe/Zurich ",
            "UTC+1",
        ] {
            assert!(!valid_timezone(tz), "{tz}");
        }
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_name("  Alice  ").unwrap(), "Alice");
        assert!(clean_name("   ").is_err());
        assert!(clean_name("a\nb").is_err());
        assert!(clean_name(&"é".repeat(200)).is_ok());
        assert!(clean_name(&"é".repeat(201)).is_err());
    }
}
