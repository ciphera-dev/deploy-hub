//! Double authentification :
//! - enrolement TOTP d'un compte connecte : `POST /api/me/mfa/totp/start`,
//!   `POST /api/me/mfa/totp/confirm`, `DELETE /api/me/mfa/totp` (CSRF exige) ;
//! - second facteur d'une session en attente de MFA :
//!   `POST /api/auth/mfa/verify`, `POST /api/auth/recovery-code` (exemptees de
//!   CSRF, voir `auth::csrf`). Un succes fait tourner la session.
//!
//! Ni les codes, ni la graine, ni le mot de passe ne sont journalises ou
//! ecrits dans l'audit.

use api_types::mfa::{
    AddFactor, DisableTotp, MfaCode, MfaMethods, MfaVerified, TotpConfirmed, TotpStart,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::auth::audit;
use crate::auth::session::{self, Session, SessionToken};
use crate::auth::{
    CurrentUser, csrf, factors, internal, password, rate_limit, recovery, totp, unauthenticated,
};
use crate::http::client_ip::ClientIp;
use crate::secrets::labels;
use crate::state::AppState;

/// Corps JSON maximal (mot de passe de 1024 caracteres compris).
const MAX_BODY: usize = 8 * 1024;
/// Longueur maximale d'un code saisi (espaces et tirets compris).
const MAX_CODE_LEN: usize = 64;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/me/mfa/totp/start", post(start))
        .route("/api/me/mfa/totp/confirm", post(confirm))
        .route("/api/me/mfa/totp", delete(disable))
        .route("/api/auth/mfa/verify", post(verify))
        .route("/api/auth/recovery-code", post(recovery_code))
        .route("/api/auth/mfa/methods", get(methods))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

pub(super) fn invalid_request() -> ApiError {
    ApiError::new(422, "validation_failed", "Requête invalide.")
}

fn invalid_code(status: u16) -> ApiError {
    ApiError::new(status, "invalid_code", "Code incorrect.")
}

pub(super) fn rate_limited(retry_after: u64) -> Response {
    let mut res = ApiError::new(
        429,
        "rate_limited",
        "Trop de tentatives. Réessayez dans quelques minutes.",
    )
    .into_response();
    res.headers_mut()
        .insert(header::RETRY_AFTER, retry_after.into());
    res
}

fn code_from(body: Result<Json<MfaCode>, JsonRejection>) -> Result<Zeroizing<String>, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    if input.code.len() > MAX_CODE_LEN {
        return Err(invalid_request());
    }
    Ok(input.code)
}

/// Reponse qui pose le cookie d'une session tournee.
pub(super) fn with_session(
    token: &SessionToken,
    persistent: bool,
    body: impl IntoResponse,
) -> Response {
    (
        [
            (header::SET_COOKIE, token.set_cookie(persistent)),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response()
}

/// `POST /api/me/mfa/totp/start`
async fn start(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    body: Result<Json<AddFactor>, JsonRejection>,
) -> Result<Response, ApiError> {
    if let Some(limited) = super::me::require_password_for_factor(
        &state,
        &user,
        &ip.to_string(),
        body,
        "mfa.totp.start.failure",
    )
    .await?
    {
        return Ok(limited);
    }
    let already = || {
        ApiError::new(
            409,
            "totp_already_enabled",
            "La vérification en deux étapes est déjà activée.",
        )
    };
    if totp::is_enabled(&state.db, user.id)
        .await
        .map_err(|e| internal("lecture de la TOTP", e))?
    {
        return Err(already());
    }
    let secret = totp::generate_secret().map_err(|e| internal("graine TOTP", e))?;
    let uri = totp::otpauth_uri(&secret, &user.email).map_err(|e| internal("URI TOTP", e))?;
    let qr_svg = totp::qr_svg(&uri).ok_or_else(|| internal("QR code TOTP", "encodage"))?;
    let sealed = state
        .secrets
        .seal_bound(&labels::totp_seed(user.id), &secret)
        .map_err(|e| internal("scellement de la graine TOTP", e))?;
    if !totp::save_pending(&state.db, user.id, &sealed)
        .await
        .map_err(|e| internal("enregistrement de la graine TOTP", e))?
    {
        return Err(already());
    }
    let body = TotpStart {
        otpauth_uri: uri.to_string(),
        qr_svg,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// `POST /api/me/mfa/totp/confirm`
async fn confirm(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    body: Result<Json<MfaCode>, JsonRejection>,
) -> Result<Response, ApiError> {
    let code = code_from(body)?;
    let (sealed, _) = totp::load(&state.db, user.id, false)
        .await
        .map_err(|e| internal("lecture de la TOTP", e))?
        .ok_or_else(|| {
            ApiError::new(
                409,
                "totp_not_started",
                "Aucune activation en cours : recommencez.",
            )
        })?;
    let secret = state
        .secrets
        .open_bound(&labels::totp_seed(user.id), &sealed)
        .map_err(|e| internal("ouverture de la graine TOTP", e))?;
    let step = totp::matching_step(&secret, &code, totp::unix_now(), None)
        .ok_or_else(|| invalid_code(422))?;

    // Codes de secours : Argon2 (~50 ms chacun), hors du thread asynchrone et
    // avant la transaction.
    let codes = recovery::generate().map_err(|e| internal("codes de secours", e))?;
    let (codes, hashes) = password::offload(move || {
        let hashes = recovery::hash_all(&codes);
        (codes, hashes)
    })
    .await
    .map_err(|e| e.into_api("hachage des codes de secours"))?;
    let hashes = hashes.map_err(|e| internal("hachage des codes de secours", e))?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction d'activation TOTP", e))?;
    factors::lock(&mut tx, user.id)
        .await
        .map_err(|e| internal("verrou des seconds facteurs", e))?;
    // Graine remplacee entre-temps par un nouveau `start` : refus.
    if !totp::confirm(&mut tx, user.id, &sealed, step)
        .await
        .map_err(|e| internal("confirmation de la TOTP", e))?
    {
        return Err(invalid_code(422));
    }
    recovery::replace(&mut tx, user.id, &hashes)
        .await
        .map_err(|e| internal("enregistrement des codes de secours", e))?;
    // Changement de niveau de la session (et fin d'une eventuelle restriction
    // a l'enrolement) : nouveau jeton.
    let token = session::rotate(&mut tx, &user.session, false)
        .await
        .map_err(|e| internal("rotation de la session", e))?;
    session::end_other_enrollment_sessions(&mut tx, user.id, user.session.id)
        .await
        .map_err(|e| internal("fin des sessions d'enrolement", e))?;
    audit(
        &mut tx,
        Some(user.id),
        "mfa.totp.enabled",
        json!({ "ip": ip.to_string() }),
    )
    .await
    .map_err(|e| internal("audit d'activation TOTP", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction d'activation TOTP", e))?;

    let body = TotpConfirmed {
        recovery_codes: codes.iter().map(|c| c.to_string()).collect(),
        csrf_token: csrf::token_for(token.expose()),
    };
    super::me::notify_factor_added(&state, &user.email, crate::mail::Factor::Totp).await;
    Ok(with_session(&token, user.session.persistent, Json(body)))
}

/// `DELETE /api/me/mfa/totp`
async fn disable(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    body: Result<Json<DisableTotp>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    if input.password.chars().count() > password::MAX_LEN {
        return Err(invalid_request());
    }
    let attempt = match rate_limit::admit_mfa(
        &state.db,
        user.id,
        user.session.id,
        rate_limit::MfaFactor::Reauth,
        None,
    )
    .await
    .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => return Ok(rate_limited(retry_after)),
    };
    let phc: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| internal("lecture du compte", e))?;
    let pw = input.password;
    let ok = match password::offload(move || password::verify(&pw, &phc)).await {
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

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de desactivation TOTP", e))?;
    if !ok {
        audit(
            &mut tx,
            Some(user.id),
            "mfa.totp.disable.failure",
            json!({ "ip": ip.to_string(), "reason": "invalid_credentials" }),
        )
        .await
        .map_err(|e| internal("audit de desactivation TOTP", e))?;
        tx.commit()
            .await
            .map_err(|e| internal("transaction de desactivation TOTP", e))?;
        return Err(ApiError::new(
            401,
            "invalid_credentials",
            "Mot de passe incorrect.",
        ));
    }
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    // Verrou des seconds facteurs du compte : une suppression simultanee de
    // la derniere passkey voit cette desactivation, et inversement.
    factors::lock(&mut tx, user.id)
        .await
        .map_err(|e| internal("verrou des seconds facteurs", e))?;
    let factors = factors::read(&mut tx, user.id)
        .await
        .map_err(|e| internal("lecture des seconds facteurs", e))?;
    // 2FA imposee par l'organisation : la derniere methode ne peut pas partir
    // (une passkey restante suffit).
    if factors.org_requires && factors.totp && factors.passkeys == 0 {
        tx.commit()
            .await
            .map_err(|e| internal("transaction de desactivation TOTP", e))?;
        return Err(ApiError::new(
            409,
            "mfa_required_by_org",
            "Votre organisation impose la vérification en deux étapes : elle ne peut pas être désactivée.",
        ));
    }
    let existed = totp::delete(&mut tx, user.id)
        .await
        .map_err(|e| internal("suppression de la TOTP", e))?;
    // Les codes de secours restent tant qu'une passkey protege le compte.
    if factors.passkeys == 0 {
        recovery::delete_all(&mut tx, user.id)
            .await
            .map_err(|e| internal("suppression des codes de secours", e))?;
    }
    if existed {
        audit(
            &mut tx,
            Some(user.id),
            "mfa.totp.disabled",
            json!({ "ip": ip.to_string() }),
        )
        .await
        .map_err(|e| internal("audit de desactivation TOTP", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| internal("transaction de desactivation TOTP", e))?;
    if !existed {
        return Err(ApiError::new(
            404,
            "not_found",
            "Aucune vérification en deux étapes à désactiver.",
        ));
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response())
}

/// Session en attente de MFA presentee par le navigateur.
pub(super) async fn pending_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Session, ApiError> {
    let token = session::token_from_headers(headers)
        .map(|t| Zeroizing::new(t.to_string()))
        .ok_or_else(unauthenticated)?;
    let session = session::lookup(&state.db, &token)
        .await
        .map_err(|e| internal("lecture de la session", e))?
        .ok_or_else(unauthenticated)?;
    if !session.mfa_pending {
        return Err(not_pending());
    }
    Ok(session)
}

#[derive(Clone, Copy)]
enum Factor {
    Totp,
    Recovery,
}

impl Factor {
    fn rate_limit(self) -> rate_limit::MfaFactor {
        match self {
            Factor::Totp => rate_limit::MfaFactor::Totp,
            Factor::Recovery => rate_limit::MfaFactor::Recovery,
        }
    }

    fn audit_action(self, ok: bool) -> &'static str {
        match (self, ok) {
            (Factor::Totp, true) => "auth.mfa.success",
            (Factor::Totp, false) => "auth.mfa.failure",
            (Factor::Recovery, true) => "auth.recovery_code.success",
            (Factor::Recovery, false) => "auth.recovery_code.failure",
        }
    }
}

pub(super) fn not_pending() -> ApiError {
    ApiError::new(
        409,
        "mfa_not_pending",
        "Aucune vérification en deux étapes n'est attendue pour cette session.",
    )
}

/// Ce que le code permettrait de consommer, s'il est valide.
enum Candidate {
    /// Fenetre TOTP acceptee (posterieure a la derniere utilisee).
    Step(u64),
    /// Code de secours non utilise correspondant.
    Code(Uuid),
}

/// Fenetre TOTP correspondant au code (rien n'est encore enregistre).
async fn totp_candidate(
    state: &AppState,
    user_id: Uuid,
    code: &str,
) -> Result<Option<Candidate>, ApiError> {
    let Some((sealed, last_used)) = totp::load(&state.db, user_id, true)
        .await
        .map_err(|e| internal("lecture de la TOTP", e))?
    else {
        return Ok(None);
    };
    let secret = state
        .secrets
        .open_bound(&labels::totp_seed(user_id), &sealed)
        .map_err(|e| internal("ouverture de la graine TOTP", e))?;
    Ok(totp::matching_step(&secret, code, totp::unix_now(), last_used).map(Candidate::Step))
}

/// Code de secours non utilise correspondant a la saisie (rien n'est encore consomme).
async fn recovery_candidate(
    state: &AppState,
    user_id: Uuid,
    code: &str,
    attempt: i64,
) -> Result<Option<Candidate>, ApiError> {
    let Some(code) = recovery::normalize(code) else {
        return Ok(None);
    };
    let candidates = recovery::unused(&state.db, user_id)
        .await
        .map_err(|e| internal("lecture des codes de secours", e))?;
    let found = match password::offload(move || recovery::find(&code, &candidates)).await {
        Ok(found) => found,
        Err(e) => {
            return Err(rate_limit::offload_error(
                &state.db,
                rate_limit::Attempt::Mfa(attempt),
                e,
                "verification du code de secours",
            )
            .await);
        }
    };
    Ok(found.map(Candidate::Code))
}

/// Second facteur commun aux deux routes : limitation, verification, audit,
/// puis rotation de la session en cas de succes.
///
/// La consommation du code (fenetre TOTP ou code de secours) et la rotation se
/// font dans une seule transaction, apres verrouillage de la ligne de la
/// session : de deux requetes simultanees sur la meme session (TOTP et code de
/// secours, ou double envoi), la seconde trouve la session deja remplacee et
/// recoit 409 `mfa_not_pending` sans rien consommer.
async fn second_factor(
    state: AppState,
    ip: std::net::IpAddr,
    headers: HeaderMap,
    code: Zeroizing<String>,
    factor: Factor,
) -> Result<Response, ApiError> {
    let ip_key = rate_limit::IpKey::login(ip);
    let ip = ip.to_string();
    let session = pending_session(&state, &headers).await?;
    if let Err(e) = rate_limit::purge_mfa(&state.db).await {
        tracing::warn!(error = %e, "purge des tentatives de second facteur");
    }
    let attempt = match rate_limit::admit_mfa(
        &state.db,
        session.user_id,
        session.id,
        factor.rate_limit(),
        Some(&ip_key),
    )
    .await
    .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => {
            tracing::warn!(%ip, "second facteur refuse : trop de tentatives");
            return Ok(rate_limited(retry_after));
        }
    };
    // Verification couteuse (Argon2 pour un code de secours) hors transaction.
    let candidate = match factor {
        Factor::Totp => totp_candidate(&state, session.user_id, &code).await?,
        Factor::Recovery => recovery_candidate(&state, session.user_id, &code, attempt).await?,
    };

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de second facteur", e))?;
    let still_pending: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM sessions WHERE id = $1 AND mfa_pending FOR UPDATE")
            .bind(session.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| internal("verrouillage de la session", e))?;
    if still_pending.is_none() {
        tx.rollback()
            .await
            .map_err(|e| internal("transaction de second facteur", e))?;
        return Err(not_pending());
    }
    let ok = match candidate {
        Some(Candidate::Step(step)) => totp::consume_step(&mut tx, session.user_id, step)
            .await
            .map_err(|e| internal("enregistrement de la fenetre TOTP", e))?,
        Some(Candidate::Code(id)) => recovery::mark_used(&mut tx, id)
            .await
            .map_err(|e| internal("consommation du code de secours", e))?,
        None => false,
    };
    if !ok {
        audit(
            &mut tx,
            Some(session.user_id),
            factor.audit_action(false),
            json!({ "ip": ip, "reason": "invalid_code" }),
        )
        .await
        .map_err(|e| internal("audit de second facteur", e))?;
        tx.commit()
            .await
            .map_err(|e| internal("transaction de second facteur", e))?;
        return Err(invalid_code(401));
    }
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    let token = session::rotate(&mut tx, &session, false)
        .await
        .map_err(|e| internal("rotation de la session", e))?;
    audit(
        &mut tx,
        Some(session.user_id),
        factor.audit_action(true),
        json!({ "ip": ip }),
    )
    .await
    .map_err(|e| internal("audit de second facteur", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de second facteur", e))?;

    let body = MfaVerified {
        csrf_token: csrf::token_for(token.expose()),
    };
    Ok(with_session(&token, session.persistent, Json(body)))
}

/// `POST /api/auth/mfa/verify`
async fn verify(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Result<Json<MfaCode>, JsonRejection>,
) -> Result<Response, ApiError> {
    let code = code_from(body)?;
    second_factor(state, ip, headers, code, Factor::Totp).await
}

/// `GET /api/auth/mfa/methods`
async fn methods(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Response, ApiError> {
    let session = pending_session(&state, &headers).await?;
    let (totp, passkey, recovery): (bool, bool, bool) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL), \
                EXISTS (SELECT 1 FROM webauthn_credentials WHERE user_id = $1), \
                EXISTS (SELECT 1 FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL)",
    )
    .bind(session.user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| internal("lecture des seconds facteurs", e))?;
    let available = super::webauthn::rp_for(&state, &headers, &uri).is_some();
    let body = MfaMethods {
        totp,
        webauthn: passkey && available,
        recovery,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// `POST /api/auth/recovery-code`
async fn recovery_code(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Result<Json<MfaCode>, JsonRejection>,
) -> Result<Response, ApiError> {
    let code = code_from(body)?;
    second_factor(state, ip, headers, code, Factor::Recovery).await
}
