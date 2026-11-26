//! Mot de passe oublie et reinitialisation :
//! - `GET /api/auth/password/forgot` : l'envoi par e-mail est-il possible
//!   (SMTP active et `CIPHERA_PUBLIC_URL` defini) ? Sinon l'ecran invite a
//!   contacter un administrateur ;
//! - `POST /api/auth/password/forgot` : toujours 202, meme travail en base
//!   que le compte existe ou non (une seule instruction), e-mail envoye hors
//!   du chemin de la requete ;
//! - `GET /api/auth/password/reset/{token}`, `POST /api/auth/password/reset` :
//!   nouveau mot de passe (politique de la connexion), toutes les sessions du
//!   compte revoquees, double authentification conservee, aucune session
//!   ouverte ;
//! - `POST /api/team/members/{id}/reset-link` : lien genere par un
//!   administrateur (jamais pour un role au-dessus du sien), a transmettre ;
//! - `POST /api/team/members/{id}/reset-mfa` : remise a zero du second
//!   facteur d'un membre (memes regles de rang), sessions du membre revoquees.
//!
//! Ces deux routes exigent la confirmation renforcee de l'auteur
//! (`auth::step_up` : mot de passe, plus le code de son second facteur s'il
//! en a un), code consomme dans la transaction de l'action ; les autres
//! proprietaires sont prevenus (`team::record`).
//!
//! Lien a usage unique valable 30 minutes. Le jeton (32 octets aleatoires)
//! n'est jamais stocke, journalise ni ecrit dans l'audit : la base n'en garde
//! que le SHA-256. Une nouvelle demande anonyme supprime les liens `forgot`
//! inutilises du compte ; un lien d'administrateur ou de la CLI remplace tous
//! les liens inutilises. Consommation par `UPDATE ... WHERE used_at IS NULL AND expires_at
//! > maintenant`, atomique. Echeances : horloge injectable de `AppState`.

use api_types::password::{
    ForgotPassword, PasswordResetStatus, ResetLink, ResetMfa, ResetPassword, ResetTokenInfo,
};
use api_types::team::StepUpConfirmation;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::auth::session::{self, SessionToken};
use crate::auth::step_up::{self, StepUp};
use crate::auth::{internal, password, rate_limit};
use crate::authz::{Require, lvl, res};
use crate::http::client_ip::ClientIp;
use crate::mail;
use crate::state::AppState;

/// Duree de validite d'un lien, en secondes (30 minutes).
pub const RESET_TTL_SECS: i64 = 30 * 60;
/// Corps JSON maximal (mot de passe de 1024 caracteres compris).
const MAX_BODY: usize = 16 * 1024;
const MAX_EMAIL_LEN: usize = 254;
/// Prefixe de la cle de limitation des demandes : 5 par (IP, e-mail) et
/// 50 par IP dans la fenetre de `rate_limit`.
const FORGOT_KEY: &str = "ciphera:password-forgot:";
/// Delai minimal entre deux liens demandes pour un meme compte (« mot de
/// passe oublie ») : pas de rafale d'e-mails ni de lien remplace en boucle.
pub const FORGOT_COOLDOWN_SECS: f64 = 60.0;
/// Cle de limitation des reinitialisations : 5 echecs par IP.
const RESET_KEY: &str = "ciphera:password-reset";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/password/forgot", get(status).post(forgot))
        .route("/api/auth/password/reset/{token}", get(info))
        .route("/api/auth/password/reset", post(reset))
        .route("/api/team/members/{id}/reset-link", post(member_link))
        .route("/api/team/members/{id}/reset-mfa", post(member_reset_mfa))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message.to_string())
}

fn reset_invalid() -> ApiError {
    ApiError::new(
        404,
        "reset_invalid",
        "Lien de réinitialisation invalide ou déjà utilisé.",
    )
}

fn reset_expired() -> ApiError {
    ApiError::new(
        410,
        "reset_expired",
        "Ce lien de réinitialisation a expiré. Faites une nouvelle demande.",
    )
}

fn rate_limited(retry_after: u64) -> Response {
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

fn no_store(status: StatusCode) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")]).into_response()
}

/// Lien de l'ecran de reinitialisation : absolu avec l'origine publique
/// (domaine confirme du panneau ou `CIPHERA_PUBLIC_URL`), relatif sinon ;
/// jamais deduit de l'en-tete `Host`.
pub fn link(public_url: Option<&str>, token: &str) -> String {
    crate::state::link(public_url, &format!("/reset/{token}"))
}

/// Ligne d'audit (ni jeton ni e-mail dans `detail`).
pub(crate) async fn audit(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    action: &str,
    detail: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, 'user', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(conn)
    .await?;
    Ok(())
}

/// Origine d'un lien : une demande anonyme ne supprime que les liens `forgot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Forgot,
    Admin,
    Cli,
}

impl Origin {
    fn as_str(self) -> &'static str {
        match self {
            Origin::Forgot => "forgot",
            Origin::Admin => "admin",
            Origin::Cli => "cli",
        }
    }
}

/// Lien cree pour un compte.
pub struct Issued {
    pub user_id: Uuid,
    /// E-mail du compte (tel qu'enregistre), destinataire du lien.
    pub email: String,
    pub token: SessionToken,
}

/// Cree un lien pour le compte de cet e-mail (casse ignoree), s'il existe, et
/// supprime ses liens encore inutilises ; dans la transaction de l'appelant.
/// Une demande anonyme (`Origin::Forgot`) ne supprime que les liens `forgot` ;
/// un lien `admin` ou `cli` remplace tous les liens inutilises. Rien n'est
/// cree si le dernier lien `forgot` du compte a moins de `cooldown_secs`
/// secondes (0 : pas de delai, liens generes par un administrateur).
pub async fn issue(
    conn: &mut PgConnection,
    email: &str,
    now: f64,
    cooldown_secs: f64,
    origin: Origin,
) -> Result<Option<Issued>, sqlx::Error> {
    let token = SessionToken::generate().map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    // Serialise les demandes d'un meme e-mail : un seul lien reste valable.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ciphera-reset:' || lower($1), 0))")
        .bind(email)
        .execute(&mut *conn)
        .await?;
    let issued: Option<(Uuid, String)> = sqlx::query_as(
        "WITH target AS (SELECT id, email FROM users \
                         WHERE lower(email) = lower($1) AND email_verified_at IS NOT NULL \
                           AND NOT EXISTS (SELECT 1 FROM password_resets r \
                                           WHERE r.user_id = users.id AND r.origin = 'forgot' \
                                             AND $6::float8 > 0 \
                                             AND r.created_at > to_timestamp($4) - make_interval(secs => $6))), \
              superseded AS (DELETE FROM password_resets \
                             WHERE used_at IS NULL AND user_id IN (SELECT id FROM target) \
                               AND (origin = 'forgot' OR $7::text <> 'forgot')), \
              inserted AS (INSERT INTO password_resets (id, token_hash, user_id, created_at, expires_at, origin) \
                           SELECT $2, $3, id, to_timestamp($4), \
                                  to_timestamp($4) + make_interval(secs => $5), $7::text \
                           FROM target RETURNING user_id) \
         SELECT target.id, target.email FROM target JOIN inserted ON inserted.user_id = target.id",
    )
    .bind(email)
    .bind(Uuid::new_v4())
    .bind(token.hash())
    .bind(now)
    .bind(RESET_TTL_SECS as f64)
    .bind(cooldown_secs)
    .bind(origin.as_str())
    .fetch_optional(&mut *conn)
    .await?;
    Ok(issued.map(|(user_id, email)| Issued {
        user_id,
        email,
        token,
    }))
}

/// `GET /api/auth/password/forgot`
async fn status(State(state): State<AppState>) -> Result<Json<PasswordResetStatus>, ApiError> {
    let email_available = mail::available(&state)
        .await
        .map_err(|e| internal("lecture des reglages SMTP", e))?;
    Ok(Json(PasswordResetStatus { email_available }))
}

/// `POST /api/auth/password/forgot` (exempte de CSRF : aucune session).
async fn forgot(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    body: Result<Json<ForgotPassword>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let email = input.email.trim().to_lowercase();
    if email.is_empty() || email.len() > MAX_EMAIL_LEN {
        return Err(invalid("Requête invalide."));
    }
    // Chaque demande compte (jamais marquee reussie), dans un espace separe
    // de celui des connexions.
    let ip_key = rate_limit::IpKey::forgot(ip);
    let ip = ip.to_string();
    let key = rate_limit::email_key(&format!("{FORGOT_KEY}{email}"));
    if let rate_limit::Admission::Limited(retry_after) = rate_limit::admit(&state.db, &ip_key, &key)
        .await
        .map_err(|e| internal("limitation de debit", e))?
    {
        tracing::warn!(%ip, "mot de passe oublie : trop de demandes");
        return Ok(rate_limited(retry_after));
    }

    // Sans envoi possible, aucun lien n'est cree : l'ecran a deja invite a
    // contacter un administrateur (reglage de l'instance, pas du compte).
    let Some(mailer) = mail::ready(&state)
        .await
        .map_err(|e| internal("lecture des reglages SMTP", e))?
    else {
        return Ok(no_store(StatusCode::ACCEPTED));
    };
    // La reponse part avant toute recherche du compte : recherche, creation
    // du lien, validation et envoi se font dans une tache separee. Le temps
    // de reponse ne depend donc pas de l'existence du compte.
    let db = state.db.clone();
    let public_url = state.public_url();
    let delivery = state.mail.clone();
    let now = state.clock.unix_secs();
    tokio::spawn(async move {
        let issued = async {
            let mut tx = db.begin().await?;
            let issued = issue(&mut tx, &email, now, FORGOT_COOLDOWN_SECS, Origin::Forgot).await?;
            tx.commit().await?;
            Ok::<_, sqlx::Error>(issued)
        }
        .await;
        match issued {
            Ok(Some(issued)) => {
                let mail = mail::reset_mail(
                    &issued.email,
                    &link(public_url.as_deref(), issued.token.expose()),
                );
                if let Err(e) = delivery.send(mailer, mail).await {
                    tracing::warn!(error = %e, "e-mail de reinitialisation non envoye");
                }
            }
            Ok(None) => {}
            // Ni e-mail ni jeton dans le journal.
            Err(e) => tracing::warn!(error = %e, "lien de reinitialisation non cree"),
        }
    });
    Ok(no_store(StatusCode::ACCEPTED))
}

/// `GET /api/auth/password/reset/{token}`
async fn info(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let token = Zeroizing::new(token);
    let row: Option<(i64, bool, bool)> = sqlx::query_as(
        "SELECT extract(epoch FROM expires_at)::bigint, used_at IS NOT NULL, \
                expires_at <= to_timestamp($2) \
         FROM password_resets WHERE token_hash = $1",
    )
    .bind(session::hash_token(&token))
    .bind(state.clock.unix_secs())
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du lien de reinitialisation", e))?;
    match row {
        None | Some((_, true, _)) => Err(reset_invalid()),
        Some((_, false, true)) => Err(reset_expired()),
        Some((expires_at, false, false)) => Ok((
            [(header::CACHE_CONTROL, "no-store")],
            Json(ResetTokenInfo { expires_at }),
        )
            .into_response()),
    }
}

/// Hachage Argon2id hors du thread asynchrone (~50 ms, 19 Mio).
/// Un refus « occupe » retire la tentative `attempt` (pas un echec du client).
async fn hash_password(
    state: &AppState,
    attempt: i64,
    password: Zeroizing<String>,
) -> Result<String, ApiError> {
    let hashed = match password::offload(move || password::hash(&password)).await {
        Ok(hashed) => hashed,
        Err(e) => {
            return Err(rate_limit::offload_error(
                &state.db,
                rate_limit::Attempt::Login(attempt),
                e,
                "hachage du mot de passe",
            )
            .await);
        }
    };
    hashed.map_err(|e| internal("hachage du mot de passe", e))
}

/// `POST /api/auth/password/reset` (exempte de CSRF : aucune session).
async fn reset(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    body: Result<Json<ResetPassword>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    password::check_policy(&input.password).map_err(invalid)?;

    let ip_key = rate_limit::IpKey::login(ip);
    let ip = ip.to_string();
    let attempt = match rate_limit::admit(&state.db, &ip_key, &rate_limit::email_key(RESET_KEY))
        .await
        .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => {
            tracing::warn!(%ip, "reinitialisation refusee : trop de tentatives");
            return Ok(rate_limited(retry_after));
        }
    };
    // Lecture sans consommation avant Argon2 : un jeton inconnu, utilise ou
    // expire ne coute qu'une requete indexee.
    let token_hash = session::hash_token(&input.token);
    let usable: Option<bool> = sqlx::query_scalar(
        "SELECT expires_at > to_timestamp($2) FROM password_resets \
         WHERE token_hash = $1 AND used_at IS NULL",
    )
    .bind(&token_hash)
    .bind(state.clock.unix_secs())
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du lien de reinitialisation", e))?;
    match usable {
        None => return Err(reset_invalid()),
        Some(false) => return Err(reset_expired()),
        Some(true) => {}
    }
    let password_hash = hash_password(&state, attempt, input.password).await?;

    let now = state.clock.unix_secs();
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de reinitialisation", e))?;
    let consumed: Option<(Uuid, Uuid)> = sqlx::query_as(
        "UPDATE password_resets SET used_at = to_timestamp($2) \
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > to_timestamp($2) \
         RETURNING id, user_id",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| internal("consommation du lien de reinitialisation", e))?;
    let Some((reset_id, user_id)) = consumed else {
        let expired: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM password_resets \
                            WHERE token_hash = $1 AND used_at IS NULL)",
        )
        .bind(&token_hash)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| internal("lecture du lien de reinitialisation", e))?;
        return Err(if expired {
            reset_expired()
        } else {
            reset_invalid()
        });
    };

    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&password_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("changement du mot de passe", e))?;
    sqlx::query("DELETE FROM password_resets WHERE user_id = $1 AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("suppression des autres liens", e))?;
    // Toutes les sessions du compte, completes ou en attente de MFA.
    let revoked = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("revocation des sessions", e))?
        .rows_affected();
    // L'ancien detenteur du mot de passe est dehors : ses echecs de second
    // facteur (plafond journalier compris) ne bloquent plus le titulaire.
    rate_limit::clear_mfa(&mut tx, user_id)
        .await
        .map_err(|e| internal("remise a zero du second facteur", e))?;
    rate_limit::mark_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    audit(
        &mut tx,
        Some(user_id),
        "auth.password.reset",
        json!({ "reset_id": reset_id, "sessions_revoked": revoked, "ip": ip }),
    )
    .await
    .map_err(|e| internal("audit de reinitialisation", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de reinitialisation", e))?;
    state.access.notify();
    Ok(no_store(StatusCode::NO_CONTENT))
}

fn member_not_found() -> ApiError {
    ApiError::new(404, "not_found", "Membre introuvable.")
}

/// `POST /api/team/members/{id}/reset-link`
async fn member_link(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Require<res::Team, lvl::Admin>,
    id: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<StepUpConfirmation>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = id.map_err(|_| member_not_found())?;
    // Jamais pour son propre compte (le flux « mot de passe oublie » existe).
    if id == auth.user.id {
        return Err(ApiError::new(
            403,
            "use_password_change",
            "Pour votre propre compte, changez votre mot de passe depuis le profil.",
        ));
    }
    // Memes regles de rang que la gestion des membres : un admin ne
    // reinitialise ni le proprietaire ni un autre admin ; un role inconnu en
    // base n'est gere par personne. Premier controle sans verrou, avant le
    // step-up : une demande refusee ne coute aucun essai.
    super::team::precheck(&state, auth.role, id).await?;
    {
        let mut conn = state
            .db
            .acquire()
            .await
            .map_err(|e| internal("connexion a la base", e))?;
        if super::team::is_pending(&mut conn, id).await? {
            return Err(super::team::account_pending());
        }
    }
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    // Step-up hors transaction (aucune connexion tenue pendant Argon2).
    let proof = match step_up::verify(
        &state,
        &auth.user,
        ip,
        input.password,
        input.code,
        "team.member.reset_link.failure",
    )
    .await?
    {
        StepUp::Verified(proof) => proof,
        StepUp::Limited(response) => return Ok(response),
    };
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de reinitialisation", e))?;
    // Rang de l'acteur et de la cible relus sous verrou (comme Equipe) : un
    // changement de role pendant le step-up est pris en compte, rien n'est
    // emis si le controle echoue maintenant.
    super::team::lock_and_check(&mut tx, auth.user.id, id).await?;
    if super::team::is_pending(&mut tx, id).await? {
        return Err(super::team::account_pending());
    }
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| internal("lecture du membre", e))?;
    let recipients = super::team::owners_to_notify(&mut tx, auth.user.id).await?;
    let issued = issue(&mut tx, &email, state.clock.unix_secs(), 0.0, Origin::Admin)
        .await
        .map_err(|e| internal("creation du lien de reinitialisation", e))?
        .ok_or_else(member_not_found)?;
    proof.commit(&mut tx).await?;
    super::team::record(
        &mut tx,
        &state,
        auth.user.id,
        "team.member.reset_link",
        "user",
        json!({ "user_id": issued.user_id, "ip": ip.to_string() }),
        recipients,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de reinitialisation", e))?;
    let body = ResetLink {
        link: link(state.public_url().as_deref(), issued.token.expose()),
    };
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response())
}

/// `POST /api/team/members/{id}/reset-mfa`
async fn member_reset_mfa(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Require<res::Team, lvl::Admin>,
    id: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<ResetMfa>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Path(id) = id.map_err(|_| member_not_found())?;
    if id == auth.user.id {
        return Err(ApiError::new(
            403,
            "use_profile",
            "Pour votre propre compte, gérez vos facteurs depuis le profil.",
        ));
    }
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    // Memes regles de rang que `reset-link` ; un role inconnu n'est gere par
    // personne. Premier controle sans verrou, avant le step-up.
    super::team::precheck(&state, auth.role, id).await?;
    let proof = match step_up::verify(
        &state,
        &auth.user,
        ip,
        input.password,
        input.code,
        "team.member.reset_mfa.failure",
    )
    .await?
    {
        StepUp::Verified(proof) => proof,
        StepUp::Limited(response) => return Ok(response),
    };
    let ip = ip.to_string();
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction de remise a zero du second facteur", e))?;
    // Rangs de l'acteur et de la cible relus sous verrou (comme Equipe) : un
    // changement de role survenu pendant le step-up est pris en compte.
    super::team::lock_and_check(&mut tx, auth.user.id, id).await?;
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| internal("lecture du membre", e))?;
    let recipients = super::team::owners_to_notify(&mut tx, auth.user.id).await?;
    let mut removed = [0_u64; 3];
    for (slot, (context, sql)) in [
        ("TOTP", "DELETE FROM user_totp WHERE user_id = $1"),
        (
            "passkeys",
            "DELETE FROM webauthn_credentials WHERE user_id = $1",
        ),
        (
            "codes de secours",
            "DELETE FROM recovery_codes WHERE user_id = $1",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        removed[slot] = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| internal(context, e))?
            .rows_affected();
    }
    let revoked = sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("revocation des sessions", e))?
        .rows_affected();
    // Les echecs de second facteur d'avant ne bloquent plus le titulaire.
    rate_limit::clear_mfa(&mut tx, id)
        .await
        .map_err(|e| internal("remise a zero du second facteur", e))?;
    // Code de l'auteur consomme avec la remise a zero : rejoue, il annule tout.
    proof.commit(&mut tx).await?;
    super::team::record(
        &mut tx,
        &state,
        auth.user.id,
        "team.member.reset_mfa",
        "user",
        json!({
            "user_id": id,
            "totp": removed[0],
            "passkeys": removed[1],
            "recovery_codes": removed[2],
            "sessions_revoked": revoked,
            "ip": ip,
        }),
        recipients,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction de remise a zero du second facteur", e))?;
    state.access.notify();
    // Avis au titulaire, hors du chemin de la requete ; aucun echec ne remonte.
    match mail::ready(&state).await {
        Ok(Some(mailer)) => state
            .mail
            .spawn(mailer, mail::mfa_reset_mail(&email), "mfa_reset"),
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "avis de remise a zero du second facteur non envoye"),
    }
    Ok(no_store(StatusCode::NO_CONTENT))
}
