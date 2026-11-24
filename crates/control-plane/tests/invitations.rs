mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::mail::{Delivery, OutgoingMail};
use control_plane::state::{AppState, Clock};
use serde_json::{Value, json};
use shared::config::Config;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const NEW_PASSWORD: &str = "une phrase de passe assez longue";

async fn user(state: &AppState, email: &str, role: &str) {
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, 'Test', now())",
    )
    .bind(Uuid::new_v4())
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
}

async fn login_as(state: &AppState, email: &str, pw: &str) -> Reply {
    call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": email, "password": pw })),
            ..Req::default()
        },
    )
    .await
}

/// (jeton de session, jeton CSRF)
async fn session(state: &AppState, email: &str) -> (String, String) {
    let r = login_as(state, email, PASSWORD).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    (r.token(), r.csrf())
}

async fn invite(state: &AppState, by: &(String, String), body: Value) -> Reply {
    call(
        state,
        "POST",
        "/api/team/invitations",
        Req {
            cookie: Some(&by.0),
            csrf: Some(&by.1),
            body: Some(body),
            ..Req::default()
        },
    )
    .await
}

/// Jeton contenu dans le lien `.../invite/<jeton>`.
fn token_of(r: &Reply) -> String {
    assert_eq!(r.status, 201, "{}", r.raw);
    let link = r.json()["link"].as_str().expect("link").to_string();
    let (_, token) = link.rsplit_once("/invite/").expect("lien /invite/<jeton>");
    assert_eq!(token.len(), 43, "32 octets en base64url");
    token.to_string()
}

async fn accept(state: &AppState, token: &str, peer: Option<&str>) -> Reply {
    call(
        state,
        "POST",
        &format!("/api/auth/invitations/{token}/accept"),
        Req {
            peer,
            body: Some(json!({ "name": "Nora Kaci", "password": NEW_PASSWORD })),
            ..Req::default()
        },
    )
    .await
}

async fn lookup(state: &AppState, token: &str) -> Reply {
    call(
        state,
        "GET",
        &format!("/api/auth/invitations/{token}"),
        Req::default(),
    )
    .await
}

async fn owner_env() -> (AppState, common::TestDb, (String, String)) {
    let (state, pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    (state, pg, owner)
}

#[tokio::test]
async fn invitation_link_single_use() {
    let (state, _pg, owner) = owner_env().await;
    let r = invite(
        &state,
        &owner,
        json!({ "email": "Nora@Example.com", "role": "developer" }),
    )
    .await;
    let token = token_of(&r);
    assert_eq!(r.json()["link"], format!("/invite/{token}"));

    // Seule l'empreinte est stockee.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM invitations")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_bytes());

    let info = lookup(&state, &token).await;
    assert_eq!(info.status, 200, "{}", info.raw);
    assert_eq!(info.json()["email"], "Nora@Example.com");
    assert_eq!(info.json()["role"], "developer");

    let r = accept(&state, &token, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert!(
        r.set_cookie.is_none(),
        "l'acceptation n'ouvre pas de session"
    );
    let (role, name): (String, String) =
        sqlx::query_as("SELECT role, name FROM users WHERE lower(email) = 'nora@example.com'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!((role.as_str(), name.as_str()), ("developer", "Nora Kaci"));
    assert_eq!(
        login_as(&state, "nora@example.com", NEW_PASSWORD)
            .await
            .status,
        200
    );

    // Second usage : refuse, aucun autre compte.
    let again = accept(&state, &token, None).await;
    assert_eq!(again.status, 404, "{}", again.raw);
    assert_eq!(again.code(), "invitation_invalid");
    assert_eq!(lookup(&state, &token).await.status, 404);
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(users, 2);

    // Audit : creation et acceptation, jamais le jeton.
    let audit: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, detail::text FROM audit_events WHERE action LIKE 'team.%' ORDER BY at",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    let actions: Vec<&str> = audit.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(
        actions,
        ["team.invitation.created", "team.invitation.accepted"]
    );
    assert!(audit.iter().all(|(_, d)| !d.contains(&token)));
}

#[tokio::test]
async fn invitation_expires_after_72h() {
    let (mut state, _pg, owner) = owner_env().await;
    let offset = Arc::new(AtomicU64::new(0));
    let o = offset.clone();
    state.clock =
        Clock::from_fn(move || SystemTime::now() + Duration::from_secs(o.load(Ordering::SeqCst)));
    let token = token_of(
        &invite(
            &state,
            &owner,
            json!({ "email": "late@example.com", "role": "viewer" }),
        )
        .await,
    );

    offset.store(72 * 3600 - 60, Ordering::SeqCst);
    assert_eq!(lookup(&state, &token).await.status, 200);

    offset.store(72 * 3600 + 60, Ordering::SeqCst);
    let info = lookup(&state, &token).await;
    assert_eq!(info.status, 410, "{}", info.raw);
    assert_eq!(info.code(), "invitation_expired");
    let r = accept(&state, &token, None).await;
    assert_eq!(r.status, 410, "{}", r.raw);
    assert_eq!(r.code(), "invitation_expired");
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE email = 'late@example.com')")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn owner_role_cannot_be_granted_by_admin() {
    let (state, _pg, owner) = owner_env().await;
    user(&state, "admin@example.com", "admin").await;
    user(&state, "dev@example.com", "developer").await;
    let admin = session(&state, "admin@example.com").await;
    let dev = session(&state, "dev@example.com").await;

    let r = invite(
        &state,
        &admin,
        json!({ "email": "boss@example.com", "role": "owner" }),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "forbidden");
    let r = invite(
        &state,
        &admin,
        json!({ "email": "ops@example.com", "role": "admin" }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let r = invite(
        &state,
        &owner,
        json!({ "email": "boss@example.com", "role": "owner" }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);

    // Equipe : Aucun pour un developpeur.
    let r = invite(
        &state,
        &dev,
        json!({ "email": "x@example.com", "role": "viewer" }),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "forbidden");

    let r = invite(
        &state,
        &owner,
        json!({ "email": "x@example.com", "role": "root" }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
}

#[tokio::test]
async fn invitation_for_existing_account_409() {
    let (state, _pg, owner) = owner_env().await;
    let r = invite(
        &state,
        &owner,
        json!({ "email": "OWNER@example.com", "role": "viewer" }),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "email_taken");

    // Compte cree entre l'invitation et son acceptation : 409, jeton intact.
    let token = token_of(
        &invite(
            &state,
            &owner,
            json!({ "email": "race@example.com", "role": "viewer" }),
        )
        .await,
    );
    user(&state, "race@example.com", "viewer").await;
    let r = accept(&state, &token, None).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "email_taken");
}

#[tokio::test]
async fn new_invitation_replaces_pending_one() {
    let (state, _pg, owner) = owner_env().await;
    let body = json!({ "email": "nora@example.com", "role": "viewer" });
    let first = token_of(&invite(&state, &owner, body.clone()).await);
    let second = token_of(&invite(&state, &owner, body).await);
    assert_eq!(lookup(&state, &first).await.status, 404);
    assert_eq!(lookup(&state, &second).await.status, 200);
}

#[tokio::test]
async fn accept_validates_and_is_rate_limited() {
    let (state, _pg, owner) = owner_env().await;
    let token = token_of(
        &invite(
            &state,
            &owner,
            json!({ "email": "nora@example.com", "role": "viewer" }),
        )
        .await,
    );
    let short = call(
        &state,
        "POST",
        &format!("/api/auth/invitations/{token}/accept"),
        Req {
            body: Some(json!({ "name": "Nora", "password": "court" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(short.status, 422, "{}", short.raw);

    let peer = Some("198.51.100.9:1234");
    for _ in 0..5 {
        assert_eq!(accept(&state, "jeton-inconnu", peer).await.status, 404);
    }
    let limited = accept(&state, "jeton-inconnu", peer).await;
    assert_eq!(limited.status, 429, "{}", limited.raw);
    assert!(limited.retry_after.is_some());
    // Meme un jeton valide est refuse tant que la limite court (pour cette IP).
    assert_eq!(accept(&state, &token, peer).await.status, 429);
    assert_eq!(accept(&state, &token, None).await.status, 204);
}

#[tokio::test]
async fn invitations_require_session_and_csrf() {
    let (state, _pg, owner) = owner_env().await;
    let body = json!({ "email": "nora@example.com", "role": "viewer" });
    let r = call(
        &state,
        "POST",
        "/api/team/invitations",
        Req {
            cookie: Some(&owner.0),
            body: Some(body.clone()),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403);
    assert_eq!(r.code(), "csrf");
    let r = call(
        &state,
        "POST",
        "/api/team/invitations",
        Req {
            body: Some(body),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "sans session, la couche CSRF refuse d'abord");
}

#[tokio::test]
async fn register_disabled_by_default_403() {
    let (state, _pg) = common::state().await;
    // Instance creee comme par l'assistant : public_signup prend la valeur
    // par defaut de la colonne (pas le repli de coalesce sans instance).
    sqlx::query(
        "INSERT INTO instance (id, name, slug, timezone, language) VALUES ($1, 'Org', 'org', 'UTC', 'fr')",
    )
    .bind(Uuid::new_v4())
    .execute(&state.db)
    .await
    .unwrap();
    let status = call(&state, "GET", "/api/auth/register", Req::default()).await;
    assert_eq!(status.status, 200, "{}", status.raw);
    assert_eq!(status.json()["public_signup"], false);

    let body = json!({ "name": "Eve", "email": "eve@example.com", "password": NEW_PASSWORD });
    let r = call(
        &state,
        "POST",
        "/api/auth/register",
        Req {
            body: Some(body.clone()),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "signup_disabled");
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(users, 0);
}

type Outbox = Arc<Mutex<Vec<OutgoingMail>>>;

/// Inscription ouverte, SMTP actif et origine publique definie : les e-mails
/// de verification sont captes.
async fn signup_env() -> (AppState, common::TestDb, Outbox) {
    let (mut state, pg) = common::state().await;
    let outbox: Outbox = Arc::default();
    let sink = outbox.clone();
    state.mail = Delivery::from_fn(move |_mailer, mail| {
        sink.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    });
    state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_PUBLIC_URL" => Some("https://panel.example.com".to_string()),
            _ => None,
        })
        .unwrap(),
    );
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO instance (id, name, public_signup) VALUES ($1, 'Org', true)")
        .bind(Uuid::new_v4())
        .execute(&state.db)
        .await
        .unwrap();
    (state, pg, outbox)
}

async fn register(state: &AppState, name: &str, email: &str, peer: &str) -> Reply {
    call(
        state,
        "POST",
        "/api/auth/register",
        Req {
            peer: Some(peer),
            body: Some(json!({ "name": name, "email": email })),
            ..Req::default()
        },
    )
    .await
}

/// Rend le dernier envoi assez ancien pour que le delai minimal entre deux
/// liens ne s'applique plus.
async fn age_links(state: &AppState) {
    sqlx::query("UPDATE email_verifications SET created_at = created_at - interval '5 minutes'")
        .execute(&state.db)
        .await
        .unwrap();
}

async fn mails_to(outbox: &Outbox, to: &str, n: usize) -> Vec<OutgoingMail> {
    for _ in 0..100 {
        if outbox.lock().unwrap().iter().filter(|m| m.to == to).count() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    outbox
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.to == to)
        .cloned()
        .collect()
}

fn verification_token(mail: &OutgoingMail) -> String {
    let (_, rest) = mail
        .text
        .split_once("https://panel.example.com/verify-email/")
        .expect("lien de verification");
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    assert_eq!(token.len(), 43, "32 octets en base64url");
    token
}

async fn confirm_with(state: &AppState, token: &str, password: Option<&str>) -> Reply {
    call(
        state,
        "POST",
        &format!("/api/auth/email-verification/{token}/confirm"),
        Req {
            body: password.map(|p| json!({ "password": p })),
            ..Req::default()
        },
    )
    .await
}

async fn users_with(state: &AppState, email: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM users WHERE lower(email) = lower($1)")
        .bind(email)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn register_when_enabled_creates_pending_viewer() {
    let (state, _pg, outbox) = signup_env().await;
    let status = call(&state, "GET", "/api/auth/register", Req::default()).await;
    assert_eq!(status.json()["public_signup"], true);

    let r = register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert!(r.set_cookie.is_none());
    let (role, verified): (String, bool) = sqlx::query_as(
        "SELECT role, email_verified_at IS NOT NULL FROM users WHERE email = 'eve@example.com'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(role, "viewer");
    assert!(!verified, "compte en attente de verification");
    let mails = mails_to(&outbox, "eve@example.com", 1).await;
    assert_eq!(mails.len(), 1);
    let token = verification_token(&mails[0]);
    let info = call(
        &state,
        "GET",
        &format!("/api/auth/email-verification/{token}"),
        Req::default(),
    )
    .await;
    assert_eq!(info.status, 200, "{}", info.raw);
    assert_eq!(info.json()["purpose"], "signup");
    assert_eq!(
        confirm_with(&state, &token, Some(NEW_PASSWORD))
            .await
            .status,
        204
    );
    let verified: bool = sqlx::query_scalar(
        "SELECT email_verified_at IS NOT NULL FROM users WHERE email = 'eve@example.com'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert!(verified);
    assert_eq!(
        confirm_with(&state, &token, Some(NEW_PASSWORD))
            .await
            .status,
        404,
        "usage unique"
    );
}

#[tokio::test]
async fn register_same_response_existing_email() {
    let (state, _pg, outbox) = signup_env().await;
    user(&state, "nora@example.com", "developer").await;

    let fresh = register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    let known = register(&state, "Intruse", "Nora@Example.com", "198.51.100.1:1000").await;
    assert_eq!(fresh.status, 202, "{}", fresh.raw);
    assert_eq!(known.status, fresh.status, "{}", known.raw);
    assert_eq!(known.raw, fresh.raw);
    assert_eq!(known.set_cookie, fresh.set_cookie);

    // Le compte existant est intact et ne recoit rien.
    let (name, role): (String, String) =
        sqlx::query_as("SELECT name, role FROM users WHERE email = 'nora@example.com'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!((name.as_str(), role.as_str()), ("Test", "developer"));
    assert_eq!(
        login_as(&state, "nora@example.com", PASSWORD).await.status,
        200
    );
    assert_eq!(mails_to(&outbox, "eve@example.com", 1).await.len(), 1);
    assert!(mails_to(&outbox, "nora@example.com", 0).await.is_empty());
    assert_eq!(users_with(&state, "eve@example.com").await, 1);
    assert_eq!(users_with(&state, "nora@example.com").await, 1);
}

#[tokio::test]
async fn reregistration_cannot_hijack_and_keeps_existing_links_valid() {
    let (state, _pg, outbox) = signup_env().await;
    // La victime s'inscrit, puis un tiers refait l'inscription de la meme adresse.
    let r = register(&state, "Victime", "eve@example.com", "198.51.100.1:1000").await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let first = verification_token(&mails_to(&outbox, "eve@example.com", 1).await[0]);
    let password_before: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE email = 'eve@example.com'")
            .fetch_one(&state.db)
            .await
            .unwrap();

    // Delai minimal entre deux liens : meme reponse, rien n'est envoye.
    let again = register(&state, "Tiers", "eve@example.com", "198.51.100.2:1000").await;
    assert_eq!(again.raw, r.raw);
    assert_eq!(mails_to(&outbox, "eve@example.com", 1).await.len(), 1);

    // Apres le delai : un nouveau lien part, l'ancien reste valable jusqu'a son expiration.
    age_links(&state).await;
    let again = register(&state, "Tiers", "eve@example.com", "198.51.100.2:1000").await;
    assert_eq!(again.raw, r.raw);
    let mails = mails_to(&outbox, "eve@example.com", 2).await;
    assert_eq!(mails.len(), 2);
    let second = verification_token(&mails[1]);
    assert_ne!(first, second);
    let info = call(
        &state,
        "GET",
        &format!("/api/auth/email-verification/{first}"),
        Req::default(),
    )
    .await;
    assert_eq!(info.status, 200, "{}", info.raw);
    // Ni le nom ni le mot de passe du compte en attente n'ont ete repris par le tiers.
    let (name, password_after): (String, String) =
        sqlx::query_as("SELECT name, password_hash FROM users WHERE email = 'eve@example.com'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(name, "Victime");
    assert_eq!(password_after, password_before);
    assert_eq!(users_with(&state, "eve@example.com").await, 1);

    // Le titulaire de la boite fixe le mot de passe avec le lien qu'il ouvre ;
    // l'autre lien ne sert plus a rien une fois le compte active.
    assert_eq!(
        confirm_with(&state, &first, Some(NEW_PASSWORD))
            .await
            .status,
        204
    );
    assert_eq!(
        login_as(&state, "eve@example.com", NEW_PASSWORD)
            .await
            .status,
        200
    );
    assert_eq!(
        login_as(&state, "eve@example.com", "mot de passe du tiers inconnu")
            .await
            .status,
        401
    );
    // Les autres liens d'inscription sont supprimes a l'activation.
    let info = call(
        &state,
        "GET",
        &format!("/api/auth/email-verification/{second}"),
        Req::default(),
    )
    .await;
    assert_eq!(info.status, 404, "{}", info.raw);
    assert_eq!(
        confirm_with(&state, &second, Some("un autre mot de passe long"))
            .await
            .status,
        404
    );
    assert_eq!(
        login_as(&state, "eve@example.com", NEW_PASSWORD)
            .await
            .status,
        200,
        "le mot de passe choisi n'a pas change"
    );
}

#[tokio::test]
async fn confirm_needs_a_valid_password() {
    let (state, _pg, outbox) = signup_env().await;
    register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    let token = verification_token(&mails_to(&outbox, "eve@example.com", 1).await[0]);
    // Sans mot de passe, ou trop court : refus, le lien reste utilisable.
    let r = confirm_with(&state, &token, None).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let r = confirm_with(&state, &token, Some("court")).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let verified: bool = sqlx::query_scalar(
        "SELECT email_verified_at IS NOT NULL FROM users WHERE email = 'eve@example.com'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert!(!verified);
    assert_eq!(
        login_as(&state, "eve@example.com", "court").await.status,
        401
    );
    assert_eq!(
        confirm_with(&state, &token, Some(NEW_PASSWORD))
            .await
            .status,
        204
    );
}

#[tokio::test]
async fn unverified_signup_cannot_login() {
    let (state, _pg, outbox) = signup_env().await;
    let r = register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    assert_eq!(r.status, 202, "{}", r.raw);
    // Aucun mot de passe n'existe avant la confirmation.
    let r = login_as(&state, "eve@example.com", NEW_PASSWORD).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    // Meme avec un hachage connu, un compte non confirme n'ouvre pas de session.
    sqlx::query("UPDATE users SET password_hash = $1 WHERE email = 'eve@example.com'")
        .bind(password::hash(NEW_PASSWORD).unwrap())
        .execute(&state.db)
        .await
        .unwrap();
    let r = login_as(&state, "eve@example.com", NEW_PASSWORD).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "email_not_verified");
    assert!(r.set_cookie.is_none());
    let r = login_as(&state, "eve@example.com", "mauvais mot de passe").await;
    assert_eq!(r.status, 401, "{}", r.raw);

    let token = verification_token(&mails_to(&outbox, "eve@example.com", 1).await[0]);
    assert_eq!(
        confirm_with(&state, &token, Some(NEW_PASSWORD))
            .await
            .status,
        204
    );
    let r = login_as(&state, "eve@example.com", NEW_PASSWORD).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn register_requires_smtp() {
    let (state, _pg) = common::state().await;
    sqlx::query("INSERT INTO instance (id, name, public_signup) VALUES ($1, 'Org', true)")
        .bind(Uuid::new_v4())
        .execute(&state.db)
        .await
        .unwrap();
    let r = register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "smtp_required");
    assert_eq!(users_with(&state, "eve@example.com").await, 0);
}

#[tokio::test]
async fn forgot_password_for_pending_account_sends_nothing() {
    let (state, _pg, outbox) = signup_env().await;
    register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    assert_eq!(mails_to(&outbox, "eve@example.com", 1).await.len(), 1);
    let r = call(
        &state,
        "POST",
        "/api/auth/password/forgot",
        Req {
            body: Some(json!({ "email": "eve@example.com" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        mails_to(&outbox, "eve@example.com", 0).await.len(),
        1,
        "aucun lien de reinitialisation"
    );
    let resets: i64 = sqlx::query_scalar("SELECT count(*) FROM password_resets")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(resets, 0);
}

#[tokio::test]
async fn invitation_replaces_pending_account() {
    let (state, _pg, _outbox) = signup_env().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    register(&state, "Intrus", "eve@example.com", "198.51.100.1:1000").await;
    assert_eq!(users_with(&state, "eve@example.com").await, 1);

    let r = invite(
        &state,
        &owner,
        json!({ "email": "eve@example.com", "role": "developer" }),
    )
    .await;
    let token = token_of(&r);
    assert_eq!(
        users_with(&state, "eve@example.com").await,
        0,
        "compte en attente supprime"
    );
    let r = accept(&state, &token, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let (role, verified): (String, bool) = sqlx::query_as(
        "SELECT role, email_verified_at IS NOT NULL FROM users WHERE email = 'eve@example.com'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!((role.as_str(), verified), ("developer", true));
}

#[tokio::test]
async fn unconfirmed_signups_are_purged_after_the_link_expires() {
    let (state, _pg, outbox) = signup_env().await;
    user(&state, "nora@example.com", "developer").await;
    register(&state, "Eve", "eve@example.com", "198.51.100.1:1000").await;
    register(&state, "Zoe", "zoe@example.com", "198.51.100.1:1000").await;
    let token = verification_token(&mails_to(&outbox, "zoe@example.com", 1).await[0]);
    assert_eq!(
        confirm_with(&state, &token, Some(NEW_PASSWORD))
            .await
            .status,
        204
    );

    // Lien encore valable : rien n'est supprime.
    let purge = || control_plane::http::email_verification::purge_pending(&state.db);
    assert_eq!(purge().await.unwrap(), 0);
    sqlx::query("UPDATE email_verifications SET expires_at = now() - interval '1 minute'")
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(purge().await.unwrap(), 1);
    assert_eq!(users_with(&state, "eve@example.com").await, 0);
    assert_eq!(
        users_with(&state, "zoe@example.com").await,
        1,
        "compte confirme conserve"
    );
    assert_eq!(users_with(&state, "nora@example.com").await, 1);
}

#[tokio::test]
async fn concurrent_accepts_create_one_account() {
    let (state, _pg, owner) = owner_env().await;
    let token = token_of(
        &invite(
            &state,
            &owner,
            json!({ "email": "nora@example.com", "role": "viewer" }),
        )
        .await,
    );
    let (a, b) = tokio::join!(
        accept(&state, &token, Some("198.51.100.1:1000")),
        accept(&state, &token, Some("198.51.100.2:1000")),
    );
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [204, 404], "{} / {}", a.raw, b.raw);
    let users: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE lower(email) = 'nora@example.com'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(users, 1);
}
