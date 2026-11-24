//! Connexion par mot de passe : `POST /api/auth/login`, `POST /api/auth/logout`
//! (le compte connecte, `/api/me`, est dans `http::me`).
//!
//! Contre l'enumeration des comptes, un e-mail inconnu et un mauvais mot de
//! passe recoivent la meme reponse 401, apres le meme calcul Argon2 (un
//! hachage factice est verifie quand le compte n'existe pas). Ni le mot de
//! passe ni l'e-mail ne sont journalises ni ecrits dans l'audit.

use std::sync::LazyLock;

use api_types::auth::{LoginRequest, LoginResponse};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::auth::{csrf, internal, password, rate_limit, session};
use crate::http::client_ip::ClientIp;
use crate::state::AppState;

/// Corps JSON maximal (mot de passe de 1024 caracteres compris).
const MAX_BODY: usize = 16 * 1024;
const MAX_EMAIL_LEN: usize = 254;
const MAX_USER_AGENT_LEN: usize = 512;

/// Hachage verifie quand l'e-mail est inconnu : meme cout qu'un vrai compte.
static DUMMY_HASH: LazyLock<String> =
    LazyLock::new(|| password::hash("ciphera-compte-inexistant").expect("hachage Argon2 factice"));

/// Calcule le hachage factice (Argon2, ~50 ms) avant la premiere connexion.
pub fn warm_up() {
    LazyLock::force(&DUMMY_HASH);
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid_credentials() -> ApiError {
    ApiError::new(
        401,
        "invalid_credentials",
        "E-mail ou mot de passe incorrect.",
    )
}

fn rate_limited(retry_after: u64) -> Response {
    let mut res = ApiError::new(
        429,
        "rate_limited",
        "Trop de tentatives de connexion. Réessayez dans quelques minutes.",
    )
    .into_response();
    res.headers_mut()
        .insert(header::RETRY_AFTER, retry_after.into());
    res
}

/// Une connexion sur ce compte est deja en cours.
fn busy() -> Response {
    let mut res = ApiError::new(
        429,
        "busy",
        "Une connexion est déjà en cours sur ce compte : réessayez dans un instant.",
    )
    .into_response();
    res.headers_mut().insert(header::RETRY_AFTER, 1.into());
    res
}

/// Mot de passe a verifier : celui du compte, ou le hachage factice.
async fn verify(
    password: Zeroizing<String>,
    phc: Option<String>,
    in_flight: rate_limit::LoginFlight,
) -> Result<bool, password::OffloadError> {
    password::offload(move || {
        // Place rendue a la fin du calcul, meme si la requete est abandonnee.
        let _in_flight = in_flight;
        match phc {
            Some(phc) => password::verify(&password, &phc),
            None => {
                password::verify(&password, &DUMMY_HASH);
                false
            }
        }
    })
    .await
}

pub(crate) async fn audit(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    action: &str,
    detail: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, 'session', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(conn)
    .await?;
    Ok(())
}

/// En-tete `User-Agent` tronque, conserve avec la session.
pub(crate) fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|ua| ua.chars().take(MAX_USER_AGENT_LEN).collect())
}

/// (second facteur configure : TOTP confirmee ou passkey, 2FA imposee par
/// l'organisation) pour ce compte.
async fn mfa_state(conn: &mut PgConnection, user_id: Uuid) -> Result<(bool, bool), ApiError> {
    sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL) \
                OR EXISTS (SELECT 1 FROM webauthn_credentials WHERE user_id = $1), \
                coalesce((SELECT mfa_required FROM instance LIMIT 1 FOR SHARE), false)",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await
    .map_err(|e| internal("lecture de la double authentification", e))
}

/// `POST /api/auth/login`
async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    body: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) =
        body.map_err(|_| ApiError::new(422, "validation_failed", "Requête invalide."))?;
    let email = input.email.trim().to_lowercase();
    if email.is_empty()
        || email.len() > MAX_EMAIL_LEN
        || input.password.is_empty()
        || input.password.chars().count() > password::MAX_LEN
    {
        return Err(ApiError::new(422, "validation_failed", "Requête invalide."));
    }
    let ip_key = rate_limit::IpKey::login(ip);
    let ip = ip.to_string();
    let email_key = rate_limit::email_key(&email);
    if let Err(e) = rate_limit::purge(&state.db).await {
        tracing::warn!(error = %e, "purge des tentatives de connexion");
    }
    // Au-dela de 10 echecs de mot de passe recents sur ce compte (toutes IP),
    // chaque tentative est ralentie, sans jamais bloquer le titulaire (qui
    // garde la passkey). Le delai ne depend pas de l'existence du compte et
    // s'applique AVANT la place de calcul : une requete qui dort ne la tient pas.
    let failures = rate_limit::recent_login_failures(&state.db, &email_key)
        .await
        .map_err(|e| internal("limitation de debit", e))?;
    let delay = rate_limit::login_delay(failures + 1);
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let attempt = match rate_limit::admit(&state.db, &ip_key, &email_key)
        .await
        .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => {
            tracing::warn!(%ip, "connexion refusee : trop de tentatives");
            return Ok(rate_limited(retry_after));
        }
    };
    // Un seul calcul Argon2 de connexion en vol par compte : les requetes
    // simultanees sur le meme e-mail ne multiplient pas le cout du serveur.
    // La place est prise apres le controle de debit (une requete refusee ne la
    // tient jamais) et passe dans le calcul (`verify`), rendue a sa fin.
    let Some(in_flight) = state.login_flights.try_begin(&email_key) else {
        // Refus « occupe » : la tentative n'est pas un echec du client.
        if let Err(e) = sqlx::query("DELETE FROM login_attempts WHERE id = $1")
            .bind(attempt)
            .execute(&state.db)
            .await
        {
            tracing::warn!(error = %e, "tentative non retiree apres un refus « occupe »");
        }
        return Ok(busy());
    };

    // lower() de PostgreSQL : la meme fonction que l'index unique des e-mails.
    let user: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE lower(email) = lower($1)")
            .bind(&email)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| internal("lecture du compte", e))?;
    let (user_id, phc) = match user {
        Some((id, phc)) => (Some(id), Some(phc)),
        None => (None, None),
    };
    let ok = match verify(input.password, phc, in_flight).await {
        Ok(ok) => ok,
        Err(e) => {
            return Err(rate_limit::offload_error(
                &state.db,
                rate_limit::Attempt::Login(attempt),
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
        .map_err(|e| internal("transaction de connexion", e))?;
    let mfa = match user_id.filter(|_| ok) {
        Some(id) => Some(mfa_state(&mut tx, id).await?),
        None => None,
    };
    let Some(user_id) = user_id.filter(|_| ok) else {
        audit(
            &mut tx,
            user_id,
            "auth.login.failure",
            json!({ "ip": ip, "reason": "invalid_credentials" }),
        )
        .await
        .map_err(|e| internal("audit de connexion", e))?;
        tx.commit()
            .await
            .map_err(|e| internal("transaction de connexion", e))?;
        return Err(invalid_credentials());
    };

    // Nouvelle session a chaque connexion : celle que presente le navigateur
    // (eventuellement fixee par un tiers) est supprimee.
    if let Some(previous) = session::token_from_headers(&headers) {
        session::delete_by_token(&mut tx, previous)
            .await
            .map_err(|e| internal("suppression de l'ancienne session", e))?;
    }
    // TOTP confirmee ou passkey : session en attente du second facteur. 2FA
    // imposee par l'organisation sans second facteur : session restreinte a
    // l'enrolement.
    let (has_second_factor, org_requires_mfa) = mfa.unwrap_or_default();
    let mfa_required = has_second_factor;
    let enrollment_required = org_requires_mfa && !has_second_factor;
    let token = session::SessionToken::generate().map_err(|e| internal("jeton de session", e))?;
    let user_agent = user_agent(&headers);
    session::create(
        &mut tx,
        user_id,
        &token,
        session::NewSession {
            mfa_pending: mfa_required,
            enrollment_required,
            persistent: input.remember,
            ip: Some(ip.clone()),
            user_agent,
        },
    )
    .await
    .map_err(|e| internal("creation de la session", e))?;
    rate_limit::mark_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    audit(
        &mut tx,
        Some(user_id),
        "auth.login.success",
        json!({
            "ip": ip,
            "mfa_pending": mfa_required,
            "enrollment_required": enrollment_required,
            "persistent": input.remember,
        }),
    )
    .await
    .map_err(|e| internal("audit de connexion", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de connexion", e))?;

    let body = LoginResponse {
        mfa_required,
        csrf_token: csrf::token_for(token.expose()),
    };
    Ok((
        [
            (header::SET_COOKIE, token.set_cookie(input.remember)),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        Json(body),
    )
        .into_response())
}

/// `POST /api/auth/logout` (jeton CSRF exige par la couche CSRF).
async fn logout(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(token) = session::token_from_headers(&headers) {
        let mut tx = state
            .db
            .begin()
            .await
            .map_err(|e| internal("transaction de deconnexion", e))?;
        if let Some(user_id) = session::delete_by_token(&mut tx, token)
            .await
            .map_err(|e| internal("suppression de la session", e))?
        {
            audit(
                &mut tx,
                user_id,
                "auth.logout",
                json!({ "ip": ip.to_string() }),
            )
            .await
            .map_err(|e| internal("audit de deconnexion", e))?;
        }
        tx.commit()
            .await
            .map_err(|e| internal("transaction de deconnexion", e))?;
        state.access.notify();
    }
    Ok((
        StatusCode::NO_CONTENT,
        [
            (header::SET_COOKIE, session::clear_cookie()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
    )
        .into_response())
}
