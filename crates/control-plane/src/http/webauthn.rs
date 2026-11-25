//! Passkeys (WebAuthn) :
//! - `GET /api/auth/webauthn/status` : disponibilite pour l'hote du panneau ;
//! - gestion par le compte connecte (CSRF exige) :
//!   `POST /api/me/webauthn/register/{options,verify}`, `GET /api/me/webauthn`,
//!   `DELETE /api/me/webauthn/{id}` (mot de passe redemande) ;
//! - assertion (exemptee de CSRF, voir `auth::csrf`) :
//!   `POST /api/auth/webauthn/assert/{options,verify}`, comme second facteur
//!   d'une session en attente de MFA (la session tourne) ou comme connexion
//!   sans mot de passe (nouvelle session complete : la passkey verifie
//!   l'utilisateur, elle compte comme double authentification).
//!
//! Ni cle publique, ni identifiant de credential, ni mot de passe ne sont
//! journalises, ecrits dans l'audit ou renvoyes au-dela de ce que le
//! navigateur doit recevoir pour la ceremonie.

use api_types::mfa::{AddFactor, MfaVerified};
use api_types::webauthn::{
    AssertMode, AssertOptionsRequest, AssertVerify, DeleteWebauthn, WebauthnCredential,
    WebauthnCredentials, WebauthnOptions, WebauthnRegister, WebauthnRegistered, WebauthnStatus,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use webauthn_rs::prelude::{
    DiscoverableAuthentication, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, Webauthn,
};

use super::auth::{audit, user_agent};
use super::mfa::{invalid_request, not_pending, pending_session, rate_limited, with_session};
use crate::auth::webauthn::{self as store, Kind, Rp};
use crate::auth::{CurrentUser, csrf, factors, internal, password, rate_limit, recovery, session};
use crate::http::client_ip::ClientIp;
use crate::state::AppState;

/// Corps JSON maximal (reponse d'attestation d'un authentificateur comprise).
const MAX_BODY: usize = 64 * 1024;
/// Cle de limitation des connexions sans mot de passe (pas d'e-mail saisi) :
/// 5 echecs par IP dans la fenetre, dans le compteur des connexions.
const PASSWORDLESS_KEY: &str = "ciphera:webauthn-passwordless";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/webauthn/status", get(status))
        .route("/api/me/webauthn", get(list))
        .route("/api/me/webauthn/{id}", delete(remove))
        .route("/api/me/webauthn/register/options", post(register_options))
        .route("/api/me/webauthn/register/verify", post(register_verify))
        .route("/api/auth/webauthn/assert/options", post(assert_options))
        .route("/api/auth/webauthn/assert/verify", post(assert_verify))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn unavailable() -> ApiError {
    ApiError::new(
        409,
        "webauthn_unavailable",
        "Les clés de sécurité ne sont pas disponibles lorsque le panneau est ouvert par une adresse IP.",
    )
}

/// Assertion refusee (ceremonie inconnue, expiree ou deja utilisee, cle
/// inconnue, signature, origine ou compteur invalides) : un seul message.
fn assert_failed() -> ApiError {
    ApiError::new(401, "webauthn_failed", "Clé de sécurité refusée.")
}

fn register_failed() -> ApiError {
    ApiError::new(
        422,
        "webauthn_failed",
        "L'enregistrement de la clé a échoué. Recommencez.",
    )
}

fn no_store(body: impl IntoResponse) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], body).into_response()
}

/// Relying party pour cette requete : variables WebAuthn, sinon origine
/// publique (domaine du panneau confirme ou `CIPHERA_PUBLIC_URL`), sinon
/// hote de la requete.
pub fn rp_for(state: &AppState, headers: &HeaderMap, uri: &Uri) -> Option<Rp> {
    let public = state.panel.public_url(&state.cfg);
    store::relying_party(
        &state.cfg,
        public.as_deref(),
        store::request_host(headers, uri),
    )
}

/// Relying party et instance webauthn-rs pour cette requete.
fn relying_party(
    state: &AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<(Rp, Webauthn), ApiError> {
    let rp = rp_for(state, headers, uri).ok_or_else(unavailable)?;
    let webauthn = store::webauthn(&rp).map_err(|e| {
        tracing::warn!(error = %e, "configuration WebAuthn refusee");
        unavailable()
    })?;
    Ok((rp, webauthn))
}

fn options_body(ceremony_id: Uuid, options: serde_json::Value) -> Response {
    no_store(Json(WebauthnOptions {
        ceremony_id: ceremony_id.to_string(),
        options,
    }))
}

/// `GET /api/auth/webauthn/status`
async fn status(State(state): State<AppState>, headers: HeaderMap, uri: Uri) -> Response {
    let available = rp_for(&state, &headers, &uri).is_some();
    no_store(Json(WebauthnStatus { available }))
}

/// `GET /api/me/webauthn`
async fn list(State(state): State<AppState>, user: CurrentUser) -> Result<Response, ApiError> {
    let credentials = store::list(&state.db, user.id)
        .await
        .map_err(|e| internal("lecture des passkeys", e))?
        .into_iter()
        .map(|c| WebauthnCredential {
            id: c.id.to_string(),
            name: c.name,
            created_at: c.created_at,
            last_used_at: c.last_used_at,
        })
        .collect();
    Ok(no_store(Json(WebauthnCredentials { credentials })))
}

/// `POST /api/me/webauthn/register/options`
async fn register_options(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    uri: Uri,
    user: CurrentUser,
    body: Result<Json<AddFactor>, JsonRejection>,
) -> Result<Response, ApiError> {
    let (_, webauthn) = relying_party(&state, &headers, &uri)?;
    if let Some(limited) = super::me::require_password_for_factor(
        &state,
        &user,
        &ip.to_string(),
        body,
        "mfa.webauthn.register.failure",
    )
    .await?
    {
        return Ok(limited);
    }
    let exclude: Vec<_> = store::passkeys(&state.db, user.id)
        .await
        .map_err(|e| internal("lecture des passkeys", e))?
        .iter()
        .map(|p| p.cred_id().clone())
        .collect();
    let display_name = if user.name.trim().is_empty() {
        &user.email
    } else {
        &user.name
    };
    let (ccr, registration) = webauthn
        .start_passkey_registration(
            user.id,
            &user.email,
            display_name,
            (!exclude.is_empty()).then_some(exclude),
        )
        .map_err(|e| internal("options d'enregistrement WebAuthn", e))?;
    let mut options =
        serde_json::to_value(&ccr).map_err(|e| internal("options d'enregistrement WebAuthn", e))?;
    // Cle residente si possible : permet ensuite la connexion sans mot de
    // passe (sans e-mail saisi) avec une cle physique aussi.
    options["publicKey"]["authenticatorSelection"]["residentKey"] = json!("preferred");
    if let Err(e) = store::purge_ceremonies(&state.db).await {
        tracing::warn!(error = %e, "purge des ceremonies WebAuthn");
    }
    let id = store::save_ceremony(
        &state.db,
        Kind::Register,
        Some(user.session.id),
        &ip.to_string(),
        &registration,
    )
    .await
    .map_err(|e| internal("enregistrement de la ceremonie WebAuthn", e))?;
    Ok(options_body(id, options))
}

/// `POST /api/me/webauthn/register/verify`
async fn register_verify(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    uri: Uri,
    user: CurrentUser,
    body: Result<Json<WebauthnRegister>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    let name = store::clean_name(&input.name).ok_or_else(invalid_request)?;
    let ceremony_id = Uuid::parse_str(&input.ceremony_id).map_err(|_| invalid_request())?;
    let credential: RegisterPublicKeyCredential =
        serde_json::from_value(input.credential).map_err(|_| invalid_request())?;
    let (_, webauthn) = relying_party(&state, &headers, &uri)?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction d'enregistrement WebAuthn", e))?;
    factors::lock(&mut tx, user.id)
        .await
        .map_err(|e| internal("verrou des seconds facteurs", e))?;
    // Ceremonie consommee meme en cas d'echec (usage unique).
    let registration: Option<PasskeyRegistration> =
        store::take_ceremony(&mut tx, ceremony_id, Kind::Register, Some(user.session.id))
            .await
            .map_err(|e| internal("lecture de la ceremonie WebAuthn", e))?;
    let passkey = registration.and_then(|r| {
        webauthn
            .finish_passkey_registration(&credential, &r)
            .inspect_err(|e| tracing::info!(error = %e, "enregistrement WebAuthn refuse"))
            .ok()
    });
    let Some(passkey) = passkey else {
        tx.commit()
            .await
            .map_err(|e| internal("transaction d'enregistrement WebAuthn", e))?;
        return Err(register_failed());
    };
    let Some(id) = store::insert(&mut tx, user.id, &passkey, &name)
        .await
        .map_err(|e| internal("enregistrement de la passkey", e))?
    else {
        tx.commit()
            .await
            .map_err(|e| internal("transaction d'enregistrement WebAuthn", e))?;
        return Err(ApiError::new(
            409,
            "credential_exists",
            "Cette clé est déjà enregistrée.",
        ));
    };
    // Compte sans code de secours utilisable (premiere passkey d'un compte
    // sans TOTP, ou codes epuises) : 10 codes, affiches une seule fois. Sans
    // eux, perdre la cle bloquerait le compte. Argon2 (~50 ms par code) hors
    // du thread asynchrone ; la transaction (et le verrou) attend.
    let has_codes = recovery::has_unused(&mut tx, user.id)
        .await
        .map_err(|e| internal("lecture des codes de secours", e))?;
    let recovery_codes = if has_codes {
        Vec::new()
    } else {
        let codes = recovery::generate().map_err(|e| internal("codes de secours", e))?;
        let (codes, hashes) = password::offload(move || {
            let hashes = recovery::hash_all(&codes);
            (codes, hashes)
        })
        .await
        .map_err(|e| e.into_api("hachage des codes de secours"))?;
        let hashes = hashes.map_err(|e| internal("hachage des codes de secours", e))?;
        recovery::replace(&mut tx, user.id, &hashes)
            .await
            .map_err(|e| internal("enregistrement des codes de secours", e))?;
        codes.iter().map(|c| c.to_string()).collect()
    };
    // Nouveau second facteur (et fin d'une eventuelle restriction a
    // l'enrolement) : nouveau jeton.
    let token = session::rotate(&mut tx, &user.session, false)
        .await
        .map_err(|e| internal("rotation de la session", e))?;
    session::end_other_enrollment_sessions(&mut tx, user.id, user.session.id)
        .await
        .map_err(|e| internal("fin des sessions d'enrolement", e))?;
    audit(
        &mut tx,
        Some(user.id),
        "mfa.webauthn.registered",
        json!({ "ip": ip.to_string(), "recovery_codes_issued": !has_codes }),
    )
    .await
    .map_err(|e| internal("audit d'enregistrement WebAuthn", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction d'enregistrement WebAuthn", e))?;

    let body = WebauthnRegistered {
        id: id.to_string(),
        csrf_token: csrf::token_for(token.expose()),
        recovery_codes,
    };
    super::me::notify_factor_added(&state, &user.email, crate::mail::Factor::Passkey).await;
    Ok(with_session(&token, user.session.persistent, Json(body)))
}

/// `DELETE /api/me/webauthn/{id}`
async fn remove(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    user: CurrentUser,
    Path(id): Path<String>,
    body: Result<Json<DeleteWebauthn>, JsonRejection>,
) -> Result<Response, ApiError> {
    let not_found = || ApiError::new(404, "not_found", "Clé de sécurité introuvable.");
    let id = Uuid::parse_str(&id).map_err(|_| not_found())?;
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
        .map_err(|e| internal("transaction de suppression WebAuthn", e))?;
    if !ok {
        audit(
            &mut tx,
            Some(user.id),
            "mfa.webauthn.delete.failure",
            json!({ "ip": ip.to_string(), "reason": "invalid_credentials" }),
        )
        .await
        .map_err(|e| internal("audit de suppression WebAuthn", e))?;
        tx.commit()
            .await
            .map_err(|e| internal("transaction de suppression WebAuthn", e))?;
        return Err(ApiError::new(
            401,
            "invalid_credentials",
            "Mot de passe incorrect.",
        ));
    }
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    // Verrou des seconds facteurs du compte : deux suppressions simultanees
    // (deux passkeys, ou passkey et TOTP) voient chacune l'etat de l'autre.
    factors::lock(&mut tx, user.id)
        .await
        .map_err(|e| internal("verrou des seconds facteurs", e))?;
    let factors = factors::read(&mut tx, user.id)
        .await
        .map_err(|e| internal("lecture des seconds facteurs", e))?;
    let last = !factors.totp && factors.passkeys <= 1;
    // 2FA imposee par l'organisation : la derniere methode ne peut pas partir.
    // (Verifie avant la suppression : la tentative reussie reste enregistree.)
    if factors.org_requires
        && last
        && store::owns(&mut tx, id, user.id)
            .await
            .map_err(|e| internal("lecture de la passkey", e))?
    {
        tx.commit()
            .await
            .map_err(|e| internal("transaction de suppression WebAuthn", e))?;
        return Err(ApiError::new(
            409,
            "mfa_required_by_org",
            "Votre organisation impose la vérification en deux étapes : gardez au moins une méthode.",
        ));
    }
    if !store::delete(&mut tx, id, user.id)
        .await
        .map_err(|e| internal("suppression de la passkey", e))?
    {
        tx.commit()
            .await
            .map_err(|e| internal("transaction de suppression WebAuthn", e))?;
        return Err(not_found());
    }
    // Plus aucun second facteur : les codes de secours n'ont plus d'objet.
    if last {
        recovery::delete_all(&mut tx, user.id)
            .await
            .map_err(|e| internal("suppression des codes de secours", e))?;
    }
    audit(
        &mut tx,
        Some(user.id),
        "mfa.webauthn.deleted",
        json!({ "ip": ip.to_string() }),
    )
    .await
    .map_err(|e| internal("audit de suppression WebAuthn", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de suppression WebAuthn", e))?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response())
}

/// `POST /api/auth/webauthn/assert/options`
async fn assert_options(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    uri: Uri,
    body: Result<Json<AssertOptionsRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    let (_, webauthn) = relying_party(&state, &headers, &uri)?;
    let ip = ip.to_string();
    if let Err(e) = store::purge_ceremonies(&state.db).await {
        tracing::warn!(error = %e, "purge des ceremonies WebAuthn");
    }
    match input.mode {
        AssertMode::SecondFactor => {
            let session = pending_session(&state, &headers).await?;
            let passkeys = store::passkeys(&state.db, session.user_id)
                .await
                .map_err(|e| internal("lecture des passkeys", e))?;
            if passkeys.is_empty() {
                return Err(ApiError::new(
                    409,
                    "webauthn_no_credentials",
                    "Aucune clé de sécurité n'est enregistrée pour ce compte.",
                ));
            }
            let (rcr, authentication) = webauthn
                .start_passkey_authentication(&passkeys)
                .map_err(|e| internal("options d'assertion WebAuthn", e))?;
            let id = store::save_ceremony(
                &state.db,
                Kind::SecondFactor,
                Some(session.id),
                &ip,
                &authentication,
            )
            .await
            .map_err(|e| internal("enregistrement de la ceremonie WebAuthn", e))?;
            let options =
                serde_json::to_value(&rcr).map_err(|e| internal("options d'assertion", e))?;
            Ok(options_body(id, options))
        }
        AssertMode::Login => {
            let in_flight = store::passwordless_in_flight(&state.db, &ip)
                .await
                .map_err(|e| internal("lecture des ceremonies WebAuthn", e))?;
            if in_flight >= store::MAX_PASSWORDLESS_PER_IP {
                tracing::warn!(%ip, "connexion par passkey refusee : trop de ceremonies");
                return Ok(rate_limited(store::CEREMONY_TTL_SECS as u64));
            }
            let (mut rcr, authentication) = webauthn
                .start_discoverable_authentication()
                .map_err(|e| internal("options d'assertion WebAuthn", e))?;
            // Bouton « Clé de sécurité » (fenetre modale), pas l'autoremplissage.
            rcr.mediation = None;
            let id =
                store::save_ceremony(&state.db, Kind::Passwordless, None, &ip, &authentication)
                    .await
                    .map_err(|e| internal("enregistrement de la ceremonie WebAuthn", e))?;
            let options =
                serde_json::to_value(&rcr).map_err(|e| internal("options d'assertion", e))?;
            Ok(options_body(id, options))
        }
    }
}

/// `POST /api/auth/webauthn/assert/verify`
async fn assert_verify(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    uri: Uri,
    body: Result<Json<AssertVerify>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid_request())?;
    let ceremony_id = Uuid::parse_str(&input.ceremony_id).map_err(|_| invalid_request())?;
    let credential: PublicKeyCredential =
        serde_json::from_value(input.credential).map_err(|_| invalid_request())?;
    let (_, webauthn) = relying_party(&state, &headers, &uri)?;
    let assertion = Assertion {
        state,
        ip: ip.to_string(),
        ip_key: rate_limit::IpKey::login(ip),
        webauthn,
        ceremony_id,
        credential,
    };
    match store::ceremony_kind(&assertion.state.db, ceremony_id)
        .await
        .map_err(|e| internal("lecture de la ceremonie WebAuthn", e))?
    {
        Some((Kind::SecondFactor, Some(session_id))) => {
            assertion.second_factor(&headers, session_id).await
        }
        Some((Kind::Passwordless, None)) => assertion.passwordless(&headers, input.remember).await,
        _ => Err(assert_failed()),
    }
}

/// Assertion recue, a verifier.
struct Assertion {
    state: AppState,
    ip: String,
    ip_key: rate_limit::IpKey,
    webauthn: Webauthn,
    ceremony_id: Uuid,
    credential: PublicKeyCredential,
}

/// Issue de la verification cryptographique.
enum Checked {
    Ok {
        user_id: Uuid,
    },
    Refused {
        user_id: Option<Uuid>,
        reason: &'static str,
    },
}

impl Assertion {
    /// Second facteur d'une session en attente de MFA : meme schema que
    /// `http::mfa::second_factor` (limitation, puis une seule transaction qui
    /// verrouille la session, consomme la ceremonie, met a jour la passkey,
    /// fait tourner la session et ecrit l'audit).
    async fn second_factor(
        self,
        headers: &HeaderMap,
        ceremony_session: Uuid,
    ) -> Result<Response, ApiError> {
        let session = pending_session(&self.state, headers).await?;
        if session.id != ceremony_session {
            return Err(assert_failed());
        }
        let db = self.state.db.clone();
        if let Err(e) = rate_limit::purge_mfa(&db).await {
            tracing::warn!(error = %e, "purge des tentatives de second facteur");
        }
        let attempt = match rate_limit::admit_mfa(
            &db,
            session.user_id,
            session.id,
            rate_limit::MfaFactor::Passkey,
            Some(&self.ip_key),
        )
        .await
        .map_err(|e| internal("limitation de debit", e))?
        {
            rate_limit::Admission::Allowed(id) => id,
            rate_limit::Admission::Limited(retry_after) => {
                tracing::warn!(ip = %self.ip, "second facteur refuse : trop de tentatives");
                return Ok(rate_limited(retry_after));
            }
        };

        let mut tx = db
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
        let authentication: Option<PasskeyAuthentication> = store::take_ceremony(
            &mut tx,
            self.ceremony_id,
            Kind::SecondFactor,
            Some(session.id),
        )
        .await
        .map_err(|e| internal("lecture de la ceremonie WebAuthn", e))?;
        let checked = match authentication {
            None => Checked::Refused {
                user_id: Some(session.user_id),
                reason: "ceremony",
            },
            Some(a) => {
                self.check(&mut tx, session.user_id, |w, cred, _| {
                    w.finish_passkey_authentication(cred, &a)
                })
                .await?
            }
        };
        if let Checked::Refused { reason, .. } = checked {
            self.refuse(tx, Some(session.user_id), "second_factor", reason)
                .await?;
            return Err(assert_failed());
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
            "auth.webauthn.success",
            json!({ "ip": self.ip }),
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

    /// Connexion sans mot de passe : passkey decouvrable, verification de
    /// l'utilisateur exigee ; nouvelle session complete (la 2FA imposee par
    /// l'organisation est satisfaite).
    async fn passwordless(self, headers: &HeaderMap, remember: bool) -> Result<Response, ApiError> {
        let db = self.state.db.clone();
        if let Err(e) = rate_limit::purge(&db).await {
            tracing::warn!(error = %e, "purge des tentatives de connexion");
        }
        let key = rate_limit::email_key(PASSWORDLESS_KEY);
        let attempt = match rate_limit::admit(&db, &self.ip_key, &key)
            .await
            .map_err(|e| internal("limitation de debit", e))?
        {
            rate_limit::Admission::Allowed(id) => id,
            rate_limit::Admission::Limited(retry_after) => {
                tracing::warn!(ip = %self.ip, "connexion par passkey refusee : trop de tentatives");
                return Ok(rate_limited(retry_after));
            }
        };

        let mut tx = db
            .begin()
            .await
            .map_err(|e| internal("transaction de connexion", e))?;
        let authentication: Option<DiscoverableAuthentication> =
            store::take_ceremony(&mut tx, self.ceremony_id, Kind::Passwordless, None)
                .await
                .map_err(|e| internal("lecture de la ceremonie WebAuthn", e))?;
        // Le compte vient de l'identifiant renvoye par l'authentificateur
        // (user handle), puis le credential doit lui appartenir.
        let user_id = self
            .webauthn
            .identify_discoverable_authentication(&self.credential)
            .ok()
            .map(|(user_id, _)| user_id);
        let checked = match (authentication, user_id) {
            (Some(a), Some(user_id)) => {
                self.check(&mut tx, user_id, |w, cred, passkey| {
                    w.finish_discoverable_authentication_v2(cred, a, passkey)
                })
                .await?
            }
            (None, _) => Checked::Refused {
                user_id: None,
                reason: "ceremony",
            },
            (Some(_), None) => Checked::Refused {
                user_id: None,
                reason: "user_handle",
            },
        };
        let user_id = match checked {
            Checked::Ok { user_id } => user_id,
            Checked::Refused { user_id, reason } => {
                self.refuse(tx, user_id, "login", reason).await?;
                return Err(assert_failed());
            }
        };

        // Nouvelle session a chaque connexion : celle que presente le
        // navigateur (eventuellement fixee par un tiers) est supprimee.
        if let Some(previous) = session::token_from_headers(headers) {
            session::delete_by_token(&mut tx, previous)
                .await
                .map_err(|e| internal("suppression de l'ancienne session", e))?;
        }
        let token =
            session::SessionToken::generate().map_err(|e| internal("jeton de session", e))?;
        session::create(
            &mut tx,
            user_id,
            &token,
            session::NewSession {
                mfa_pending: false,
                enrollment_required: false,
                persistent: remember,
                ip: Some(self.ip.clone()),
                user_agent: user_agent(headers),
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
            "auth.webauthn.login",
            json!({ "ip": self.ip, "persistent": remember }),
        )
        .await
        .map_err(|e| internal("audit de connexion", e))?;
        tx.commit()
            .await
            .map_err(|e| internal("transaction de connexion", e))?;
        let body = MfaVerified {
            csrf_token: csrf::token_for(token.expose()),
        };
        Ok(with_session(&token, remember, Json(body)))
    }

    /// Verifie l'assertion contre la passkey du compte (verrouillee) et, si
    /// elle est valide, enregistre le nouveau compteur dans la transaction.
    async fn check(
        &self,
        tx: &mut PgConnection,
        user_id: Uuid,
        finish: impl FnOnce(
            &Webauthn,
            &PublicKeyCredential,
            &Passkey,
        ) -> Result<
            webauthn_rs::prelude::AuthenticationResult,
            webauthn_rs::prelude::WebauthnError,
        >,
    ) -> Result<Checked, ApiError> {
        let refused = |reason| Checked::Refused {
            user_id: Some(user_id),
            reason,
        };
        let Some(mut locked) = store::lock(tx, user_id, self.credential.get_credential_id())
            .await
            .map_err(|e| internal("lecture de la passkey", e))?
        else {
            return Ok(refused("unknown_credential"));
        };
        let result = match finish(&self.webauthn, &self.credential, &locked.passkey) {
            Ok(result) => result,
            Err(e) => {
                tracing::info!(error = %e, "assertion WebAuthn refusee");
                return Ok(refused("invalid_assertion"));
            }
        };
        if !result.user_verified() {
            return Ok(refused("user_verification"));
        }
        // Compteur compare a la valeur stockee maintenant (et pas seulement a
        // celle figee dans la ceremonie) : un clone de la cle est refuse.
        if !store::counter_ok(locked.counter, result.counter()) {
            return Ok(refused("counter"));
        }
        if locked.passkey.update_credential(&result).is_none() {
            return Ok(refused("unknown_credential"));
        }
        store::record_use(tx, locked.id, &locked.passkey)
            .await
            .map_err(|e| internal("mise a jour de la passkey", e))?;
        Ok(Checked::Ok { user_id })
    }

    /// Echec : audit (la ceremonie reste consommee), puis reponse 401.
    async fn refuse(
        &self,
        mut tx: sqlx::Transaction<'static, sqlx::Postgres>,
        user_id: Option<Uuid>,
        mode: &str,
        reason: &str,
    ) -> Result<(), ApiError> {
        audit(
            &mut tx,
            user_id,
            "auth.webauthn.failure",
            json!({ "ip": self.ip, "mode": mode, "reason": reason }),
        )
        .await
        .map_err(|e| internal("audit WebAuthn", e))?;
        tx.commit()
            .await
            .map_err(|e| internal("transaction WebAuthn", e))?;
        Ok(())
    }
}
