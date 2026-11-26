//! Creation de comptes hors assistant d'installation :
//! - `POST /api/team/invitations` : lien a usage unique valable 72 heures
//!   (Equipe : Admin requis ; jamais un role au-dessus de celui de l'invitant ;
//!   confirmation renforcee `auth::step_up`, code consomme dans la
//!   transaction de l'invitation ; autres proprietaires prevenus) ;
//! - `GET /api/auth/invitations/{token}`, `POST .../accept` : ecran
//!   d'acceptation ; le compte est cree avec le role invite, sans session (la
//!   connexion suit et applique la double authentification) ;
//! - `GET|POST /api/auth/register` : inscription publique (role Lecteur),
//!   fermee tant que `instance.public_signup` est faux (defaut). Le compte reste
//!   en attente jusqu'a la confirmation du lien envoye a l'adresse
//!   (`email_verification`) ; reponse 202 identique que l'adresse soit connue ou non.
//!
//! Le jeton (32 octets aleatoires) n'est jamais stocke, journalise ni ecrit
//! dans l'audit : la base n'en garde que le SHA-256. Il est consomme par un
//! `UPDATE ... WHERE used_at IS NULL AND expires_at > maintenant`, atomique.
//! Les echeances suivent l'horloge injectable de `AppState`.
//!
//! Livraison : le lien est renvoye a l'invitant, qui le copie ; il est aussi
//! envoye par e-mail a l'invite quand l'instance le permet (`mail::ready` :
//! SMTP active et `CIPHERA_PUBLIC_URL` defini), apres la validation de la
//! transaction et hors du chemin de la requete.

use api_types::invitations::{
    AcceptInvitation, CreateInvitation, InvitationInfo, InvitationLink, RegisterRequest,
    SignupStatus,
};
use axum::extract::rejection::JsonRejection;
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
use crate::authz::{Require, Role, forbidden, lvl, res};
use crate::http::client_ip::ClientIp;
use crate::http::email_verification;
use crate::http::setup::validate_email;
use crate::mail;
use crate::state::AppState;

/// Duree de validite d'une invitation, en secondes (72 heures).
pub const INVITATION_TTL_SECS: i64 = 72 * 3600;
/// Corps JSON maximal (mot de passe de 1024 caracteres compris).
const MAX_BODY: usize = 16 * 1024;
const MAX_NAME_CHARS: usize = 200;
/// Cles de limitation de debit (a la place de l'e-mail) : 5 echecs par IP
/// dans la fenetre de `rate_limit`.
const ACCEPT_KEY: &str = "ciphera:invitation-accept";
const REGISTER_KEY: &str = "ciphera:register";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/team/invitations",
            post(create).get(super::team::list_invitations),
        )
        .route("/api/auth/invitations/{token}", get(info))
        .route("/api/auth/invitations/{token}/accept", post(accept))
        .route("/api/auth/register", get(signup_status).post(register))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message.to_string())
}

fn email_taken() -> ApiError {
    ApiError::new(409, "email_taken", "Un compte existe déjà pour cet e-mail.")
}

fn invitation_invalid() -> ApiError {
    ApiError::new(
        404,
        "invitation_invalid",
        "Lien d'invitation invalide ou déjà utilisé.",
    )
}

fn invitation_expired() -> ApiError {
    ApiError::new(
        410,
        "invitation_expired",
        "Ce lien d'invitation a expiré. Demandez-en un nouveau à un administrateur.",
    )
}

/// Refus d'un jeton inutilisable : expire s'il n'a pas servi, sinon invalide.
async fn token_refusal(conn: &mut PgConnection, token_hash: &[u8]) -> Result<ApiError, ApiError> {
    let expired: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM invitations WHERE token_hash = $1 AND used_at IS NULL)",
    )
    .bind(token_hash)
    .fetch_one(conn)
    .await
    .map_err(|e| internal("lecture de l'invitation", e))?;
    Ok(if expired {
        invitation_expired()
    } else {
        invitation_invalid()
    })
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

/// Nom saisi : 1 a 200 caracteres (vide permis si `optional`), sans
/// caractere de controle (meme regle que le profil).
fn clean_name(name: &str, optional: bool) -> Result<String, ApiError> {
    let name = name.trim();
    if (name.is_empty() && !optional)
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "Nom : 1 à 200 caractères, sans caractère de contrôle.",
        ));
    }
    Ok(name.to_string())
}

fn unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|code| code == "23505")
}

pub(crate) async fn audit(
    conn: &mut PgConnection,
    actor: Uuid,
    action: &str,
    target: &str,
    detail: serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(target)
    .bind(detail)
    .execute(conn)
    .await?;
    Ok(())
}

/// Lien de l'ecran d'acceptation : absolu avec l'origine publique (domaine
/// confirme du panneau ou `CIPHERA_PUBLIC_URL`), jamais
/// deduit de l'en-tete `Host` (falsifiable) ; sinon chemin relatif, complete
/// par le panneau.
fn link(state: &AppState, token: &str) -> String {
    state.public_link(&format!("/invite/{token}"))
}

/// `POST /api/team/invitations`
async fn create(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Require<res::Team, lvl::Admin>,
    body: Result<Json<CreateInvitation>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let email = input.email.trim().to_string();
    if !validate_email(&email) {
        return Err(invalid("E-mail invalide."));
    }
    let name = clean_name(input.name.as_deref().unwrap_or(""), true)?;
    let role = Role::parse(&input.role).ok_or_else(|| invalid("Rôle invalide."))?;
    if !auth.role.can_grant(role) {
        return Err(forbidden());
    }

    // Meme format que les jetons de session : 32 octets aleatoires, base64url.
    let token = SessionToken::generate().map_err(|e| internal("jeton d'invitation", e))?;
    let invitation_id = Uuid::new_v4();
    let now = state.clock.unix_secs();
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction d'invitation", e))?;
    // Serialise les invitations d'un meme e-mail : une seule reste en attente.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-invite:' || lower($1), 0))",
    )
    .bind(&email)
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("verrou d'invitation", e))?;
    // Un compte d'inscription ouverte encore non confirme ne reserve pas
    // l'adresse : l'invitation le remplace.
    sqlx::query("DELETE FROM users WHERE lower(email) = lower($1) AND email_verified_at IS NULL")
        .bind(&email)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("remplacement du compte en attente", e))?;
    let taken: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE lower(email) = lower($1))")
            .bind(&email)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| internal("lecture des comptes", e))?;
    if taken {
        return Err(email_taken());
    }
    sqlx::query("DELETE FROM invitations WHERE lower(email) = lower($1) AND used_at IS NULL")
        .bind(&email)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("remplacement de l'invitation", e))?;
    // Step-up apres les controles ci-dessus (une demande refusee ne coute
    // aucun essai) ; un echec annule la transaction : rien n'est modifie.
    let proof = match step_up::verify(
        &state,
        &auth.user,
        ip,
        input.password,
        input.code,
        "team.invitation.created.failure",
    )
    .await?
    {
        StepUp::Verified(proof) => proof,
        StepUp::Limited(response) => return Ok(response),
    };
    sqlx::query(
        "INSERT INTO invitations (id, token_hash, email, name, role, invited_by, created_at, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7), to_timestamp($7) + make_interval(secs => $8))",
    )
    .bind(invitation_id)
    .bind(token.hash())
    .bind(&email)
    .bind(&name)
    .bind(role.as_str())
    .bind(auth.user.id)
    .bind(now)
    .bind(INVITATION_TTL_SECS as f64)
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("creation de l'invitation", e))?;
    proof.commit(&mut tx).await?;
    // Ni jeton ni e-mail dans l'audit.
    super::team::record(
        &mut tx,
        &state,
        auth.user.id,
        "team.invitation.created",
        "invitation",
        json!({ "invitation_id": invitation_id, "role": role.as_str(), "ip": ip.to_string() }),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction d'invitation", e))?;

    let body = InvitationLink {
        link: link(&state, token.expose()),
    };
    // L'invitation est creee : un echec de lecture des reglages SMTP ne la
    // fait pas echouer (le lien est renvoye a l'invitant).
    match mail::ready(&state).await {
        Ok(Some(mailer)) => state.mail.spawn(
            mailer,
            mail::invitation_mail(&email, &body.link),
            "invitation",
        ),
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "invitation : e-mail non envoye"),
    }
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
        .into_response())
}

/// `GET /api/auth/invitations/{token}`
async fn info(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let token = Zeroizing::new(token);
    let row: Option<(String, String, String, i64, bool, bool)> = sqlx::query_as(
        "SELECT email, name, role, extract(epoch FROM expires_at)::bigint, \
                used_at IS NOT NULL, expires_at <= to_timestamp($2) \
         FROM invitations WHERE token_hash = $1",
    )
    .bind(session::hash_token(&token))
    .bind(state.clock.unix_secs())
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture de l'invitation", e))?;
    let Some((email, name, role, expires_at, used, expired)) = row else {
        return Err(invitation_invalid());
    };
    if used {
        return Err(invitation_invalid());
    }
    if expired {
        return Err(invitation_expired());
    }
    let body = InvitationInfo {
        email,
        name,
        role,
        expires_at,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// Hachage Argon2id hors du thread asynchrone (~50 ms, 19 Mio).
/// Un refus « occupe » retire la tentative `attempt` (pas un echec du client).
pub(crate) async fn hash_password(
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

/// `POST /api/auth/invitations/{token}/accept` (exempte de CSRF : aucune session).
async fn accept(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Path(token): Path<String>,
    body: Result<Json<AcceptInvitation>, JsonRejection>,
) -> Result<Response, ApiError> {
    let token = Zeroizing::new(token);
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let name = clean_name(&input.name, false)?;
    password::check_policy(&input.password).map_err(invalid)?;

    let ip_key = rate_limit::IpKey::login(ip);
    let ip = ip.to_string();
    let attempt = match rate_limit::admit(&state.db, &ip_key, &rate_limit::email_key(ACCEPT_KEY))
        .await
        .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => {
            tracing::warn!(%ip, "acceptation d'invitation refusee : trop de tentatives");
            return Ok(rate_limited(retry_after));
        }
    };
    // Jeton verifie avant le calcul Argon2 : un jeton faux ou expire ne coute
    // aucun hachage (il compte quand meme comme un echec de la limite).
    let token_hash = session::hash_token(&token);
    let now = state.clock.unix_secs();
    let usable: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM invitations \
                        WHERE token_hash = $1 AND used_at IS NULL AND expires_at > to_timestamp($2))",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_one(&state.db)
    .await
    .map_err(|e| internal("lecture de l'invitation", e))?;
    if !usable {
        let mut conn = state
            .db
            .acquire()
            .await
            .map_err(|e| internal("connexion a la base", e))?;
        return Err(token_refusal(&mut conn, &token_hash).await?);
    }
    let password_hash = hash_password(&state, attempt, input.password).await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction d'acceptation", e))?;
    let consumed: Option<(Uuid, String, String)> = sqlx::query_as(
        "UPDATE invitations SET used_at = to_timestamp($2) \
         WHERE token_hash = $1 AND used_at IS NULL AND expires_at > to_timestamp($2) \
         RETURNING id, email, role",
    )
    .bind(&token_hash)
    .bind(now)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| internal("consommation de l'invitation", e))?;
    let Some((invitation_id, email, role)) = consumed else {
        // Consomme ou expire entre la verification et la transaction.
        return Err(token_refusal(&mut tx, &token_hash).await?);
    };

    // Compte en attente a cette adresse (inscription ouverte) : remplace.
    sqlx::query("DELETE FROM users WHERE lower(email) = lower($1) AND email_verified_at IS NULL")
        .bind(&email)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal("remplacement du compte en attente", e))?;
    let user_id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, $5, now())",
    )
    .bind(user_id)
    .bind(&email)
    .bind(&password_hash)
    .bind(&role)
    .bind(&name)
    .execute(&mut *tx)
    .await;
    match inserted {
        Ok(_) => {}
        // Compte cree entre-temps : la transaction est annulee, l'invitation reste intacte.
        Err(e) if unique_violation(&e) => return Err(email_taken()),
        Err(e) => return Err(internal("creation du compte invite", e)),
    }
    rate_limit::mark_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    audit(
        &mut tx,
        user_id,
        "team.invitation.accepted",
        "invitation",
        json!({ "invitation_id": invitation_id, "role": role, "ip": ip }),
    )
    .await
    .map_err(|e| internal("audit d'acceptation", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction d'acceptation", e))?;
    Ok(no_store(StatusCode::NO_CONTENT))
}

async fn public_signup(state: &AppState) -> Result<bool, ApiError> {
    sqlx::query_scalar("SELECT coalesce((SELECT public_signup FROM instance LIMIT 1), false)")
        .fetch_one(&state.db)
        .await
        .map_err(|e| internal("lecture de la politique d'inscription", e))
}

/// `GET /api/auth/register`
async fn signup_status(State(state): State<AppState>) -> Result<Json<SignupStatus>, ApiError> {
    Ok(Json(SignupStatus {
        public_signup: public_signup(&state).await?,
    }))
}

/// `POST /api/auth/register` (exempte de CSRF : aucune session). Nom et adresse
/// seulement : le mot de passe est choisi par le titulaire de la boite, en
/// confirmant le lien (`email_verification`).
async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    body: Result<Json<RegisterRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    if !public_signup(&state).await? {
        return Err(ApiError::new(
            403,
            "signup_disabled",
            "Les comptes de cette instance sont créés sur invitation.",
        ));
    }
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let name = clean_name(&input.name, false)?;
    let email = input.email.trim().to_string();
    if !validate_email(&email) {
        return Err(invalid("E-mail invalide."));
    }
    // Sans envoi d'e-mails, l'adresse ne pourrait jamais etre verifiee. Reglage
    // de l'instance : la reponse ne depend pas du compte.
    let mailer = mail::ready(&state)
        .await
        .map_err(|e| internal("lecture des reglages SMTP", e))?
        .ok_or_else(|| {
            ApiError::new(
                409,
                "smtp_required",
                "L'inscription demande un serveur SMTP configuré : contactez un administrateur.",
            )
        })?;

    // Chaque inscription compte (jamais marquee reussie) : 5 par IP et par
    // fenetre de 15 minutes.
    let attempt = match rate_limit::admit(
        &state.db,
        &rate_limit::IpKey::login(ip),
        &rate_limit::email_key(REGISTER_KEY),
    )
    .await
    .map_err(|e| internal("limitation de debit", e))?
    {
        rate_limit::Admission::Allowed(id) => id,
        rate_limit::Admission::Limited(retry_after) => return Ok(rate_limited(retry_after)),
    };
    // Aucun mot de passe n'existe encore : un hachage d'une valeur aleatoire
    // jamais divulguee remplit la colonne (et garde le meme cout sur tous les
    // chemins). Le compte ne peut pas se connecter avant la confirmation.
    let placeholder = SessionToken::generate().map_err(|e| internal("jeton d'inscription", e))?;
    let password_hash = hash_password(
        &state,
        attempt,
        Zeroizing::new(placeholder.expose().to_string()),
    )
    .await?;

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction d'inscription", e))?;
    // Serialise les inscriptions d'une meme adresse.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-signup:' || lower($1), 0))",
    )
    .bind(&email)
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("verrou d'inscription", e))?;
    let existing: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT id, email_verified_at IS NULL FROM users WHERE lower(email) = lower($1) FOR UPDATE",
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| internal("lecture des comptes", e))?;
    // Adresse libre : compte en attente. Compte encore en attente a cette
    // adresse : il reste tel quel (ni nom ni mot de passe ne sont repris) et
    // un nouveau lien peut partir (hors delai ; les anciens restent valables
    // jusqu'a leur expiration). Compte deja verifie : rien ne change,
    // rien n'est envoye. Dans tous les cas, la reponse est la meme.
    let user_id = match existing {
        None => {
            let user_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) \
                 VALUES ($1, $2, $3, $4, $5, NULL)",
            )
            .bind(user_id)
            .bind(&email)
            .bind(&password_hash)
            .bind(Role::Viewer.as_str())
            .bind(&name)
            .execute(&mut *tx)
            .await
            .map_err(|e| internal("creation du compte", e))?;
            audit(
                &mut tx,
                user_id,
                "auth.register",
                "user",
                json!({ "role": Role::Viewer.as_str(), "ip": ip }),
            )
            .await
            .map_err(|e| internal("audit d'inscription", e))?;
            Some(user_id)
        }
        Some((user_id, true)) => Some(user_id),
        Some((_, false)) => None,
    };
    // Au plus un lien par minute et par adresse (`issue`) : meme reponse.
    let verification = match user_id {
        Some(user_id) => email_verification::issue(
            &mut tx,
            user_id,
            &email,
            email_verification::PURPOSE_SIGNUP,
            state.clock.unix_secs(),
        )
        .await
        .map_err(|e| internal("creation du lien de verification", e))?,
        None => None,
    };
    tx.commit()
        .await
        .map_err(|e| internal("transaction d'inscription", e))?;
    if let Some(token) = verification {
        let link = email_verification::link(&state, token.expose());
        state.mail.spawn(
            mailer,
            mail::email_verification_mail(&email, &link, true),
            "signup_verification",
        );
    }
    Ok(no_store(StatusCode::ACCEPTED))
}
