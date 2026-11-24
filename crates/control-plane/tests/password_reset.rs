mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::mail::{Delivery, Mailer, OutgoingMail, SmtpSettings, TlsMode};
use control_plane::state::{AppState, Clock};
use serde_json::{Value, json};
use shared::config::Config;
use uuid::Uuid;
use zeroize::Zeroizing;

const PASSWORD: &str = "correct horse battery staple";
const NEW_PASSWORD: &str = "une phrase de passe assez longue";
const SMTP_PASSWORD: &str = "mot-de-passe-smtp-tres-secret";
const PUBLIC_URL: &str = "https://panel.example.com";

async fn user(state: &AppState, email: &str, role: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, $4, 'Test')",
    )
    .bind(id)
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
    id
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

async fn as_user(
    state: &AppState,
    who: &(String, String),
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&who.0),
            csrf: Some(&who.1),
            body,
            ..Req::default()
        },
    )
    .await
}

async fn anonymous(state: &AppState, method: &str, path: &str, body: Option<Value>) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            body,
            ..Req::default()
        },
    )
    .await
}

type Outbox = Arc<Mutex<Vec<OutgoingMail>>>;

/// Remplace l'envoi SMTP par une boite d'envoi en memoire.
fn capture(state: &mut AppState) -> Outbox {
    let outbox: Outbox = Arc::default();
    let sink = outbox.clone();
    state.mail = Delivery::from_fn(move |_mailer, mail| {
        sink.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    });
    outbox
}

fn with_public_url(state: &mut AppState) {
    state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_PUBLIC_URL" => Some(PUBLIC_URL.to_string()),
            _ => None,
        })
        .unwrap(),
    );
}

fn smtp_body() -> Value {
    json!({
        "enabled": true,
        "host": "smtp.example.com",
        "port": 587,
        "tls": "starttls",
        "username": "ciphera@example.com",
        "password": SMTP_PASSWORD,
        "from_address": "ciphera@example.com",
        "from_name": "Ciphera",
    })
}

async fn configure_smtp(state: &AppState, admin: &(String, String)) {
    let r = as_user(
        state,
        admin,
        "PUT",
        "/api/settings/notifications",
        Some(smtp_body()),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

/// Attend que la tache d'envoi ait depose `n` messages.
async fn wait_for_mail(outbox: &Outbox, n: usize) -> Vec<OutgoingMail> {
    for _ in 0..100 {
        if outbox.lock().unwrap().len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    outbox.lock().unwrap().clone()
}

/// Jeton du lien `.../reset/<jeton>` contenu dans un texte.
fn reset_token_in(text: &str) -> String {
    let (_, rest) = text.split_once("/reset/").expect("lien /reset/<jeton>");
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    assert_eq!(token.len(), 43, "32 octets en base64url");
    token
}

async fn forgot(state: &AppState, email: &str) -> Reply {
    anonymous(
        state,
        "POST",
        "/api/auth/password/forgot",
        Some(json!({ "email": email })),
    )
    .await
}

async fn reset(state: &AppState, token: &str, pw: &str) -> Reply {
    anonymous(
        state,
        "POST",
        "/api/auth/password/reset",
        Some(json!({ "token": token, "password": pw })),
    )
    .await
}

async fn mail_available(state: &AppState) -> bool {
    let r = anonymous(state, "GET", "/api/auth/password/forgot", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    r.json()["email_available"]
        .as_bool()
        .expect("email_available")
}

async fn count(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn forgot_always_202_no_enumeration() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    user(&state, "owner@example.com", "owner").await;
    user(&state, "Nora@Example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;

    // Sans SMTP : « contactez un administrateur », rien n'est cree.
    assert!(!mail_available(&state).await);
    assert_eq!(forgot(&state, "nora@example.com").await.status, 202);
    // SMTP sans origine publique : toujours indisponible (lien impossible).
    configure_smtp(&state, &owner).await;
    assert!(!mail_available(&state).await);
    assert_eq!(forgot(&state, "nora@example.com").await.status, 202);
    assert_eq!(
        count(&state, "SELECT count(*) FROM password_resets").await,
        0
    );

    with_public_url(&mut state);
    assert!(mail_available(&state).await);
    let known = forgot(&state, "NORA@example.com").await;
    let unknown = forgot(&state, "ghost@example.com").await;
    for r in [&known, &unknown] {
        assert_eq!(r.status, 202, "{}", r.raw);
        assert_eq!(r.cache_control.as_deref(), Some("no-store"));
    }
    assert_eq!(known.raw, unknown.raw, "meme reponse");
    assert!(known.set_cookie.is_none());

    let sent = wait_for_mail(&outbox, 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let sent_after = outbox.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "un seul e-mail (compte existant)");
    assert_eq!(sent_after.len(), 1, "rien pour l'e-mail inconnu");
    let mail = &sent[0];
    assert_eq!(
        mail.to, "Nora@Example.com",
        "adresse du compte, pas celle saisie"
    );
    assert!(mail.text.contains(&format!("{PUBLIC_URL}/reset/")));
    assert!(mail.text.contains("30 minutes"));
    let token = reset_token_in(&mail.text);

    // Seule l'empreinte est stockee.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM password_resets")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_bytes());
    let audit: Vec<String> = sqlx::query_scalar("SELECT detail::text FROM audit_events")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert!(
        audit
            .iter()
            .all(|d| !d.contains(&token) && !d.to_lowercase().contains("nora@"))
    );

    // La requete repond avant toute recherche du compte : le verrou de
    // l'e-mail est tenu ailleurs, la reponse arrive quand meme et aucun lien
    // n'existe encore ; il apparait une fois le verrou relache.
    let mut blocker = state.db.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ciphera-reset:' || lower($1), 0))")
        .bind("nora@example.com")
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("DELETE FROM password_resets")
        .execute(&state.db)
        .await
        .unwrap();
    let r = tokio::time::timeout(Duration::from_secs(5), forgot(&state, "nora@example.com"))
        .await
        .expect("reponse sans attendre la recherche du compte");
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(
        count(&state, "SELECT count(*) FROM password_resets").await,
        0
    );
    blocker.rollback().await.unwrap();
    assert_eq!(wait_for_mail(&outbox, 2).await.len(), 2);
    assert_eq!(
        count(&state, "SELECT count(*) FROM password_resets").await,
        1
    );

    // Corps invalide : 422 (sans rien reveler non plus).
    let bad = anonymous(
        &state,
        "POST",
        "/api/auth/password/forgot",
        Some(json!({ "mail": "x" })),
    )
    .await;
    assert_eq!(bad.status, 422, "{}", bad.raw);

    // Limitation de debit par (IP, e-mail) : la 6e demande est refusee.
    for _ in 0..4 {
        assert_eq!(forgot(&state, "ghost@example.com").await.status, 202);
    }
    let limited = forgot(&state, "ghost@example.com").await;
    assert_eq!(limited.status, 429, "{}", limited.raw);
    assert!(limited.retry_after.is_some());
}

#[tokio::test]
async fn reset_token_single_use_30min() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    with_public_url(&mut state);
    let offset = Arc::new(AtomicU64::new(0));
    let o = offset.clone();
    state.clock =
        Clock::from_fn(move || SystemTime::now() + Duration::from_secs(o.load(Ordering::SeqCst)));
    user(&state, "owner@example.com", "owner").await;
    user(&state, "nora@example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;
    configure_smtp(&state, &owner).await;

    assert_eq!(forgot(&state, "nora@example.com").await.status, 202);
    let token = reset_token_in(&wait_for_mail(&outbox, 1).await[0].text);

    offset.store(30 * 60 - 60, Ordering::SeqCst);
    let info = anonymous(
        &state,
        "GET",
        &format!("/api/auth/password/reset/{token}"),
        None,
    )
    .await;
    assert_eq!(info.status, 200, "{}", info.raw);
    assert_eq!(info.cache_control.as_deref(), Some("no-store"));
    let short = reset(&state, &token, "trop court").await;
    assert_eq!(short.status, 422, "{}", short.raw);

    let r = reset(&state, &token, NEW_PASSWORD).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert!(
        r.set_cookie.is_none(),
        "la reinitialisation ne connecte pas"
    );
    assert_eq!(
        login_as(&state, "nora@example.com", PASSWORD).await.status,
        401
    );
    assert_eq!(
        login_as(&state, "nora@example.com", NEW_PASSWORD)
            .await
            .status,
        200
    );

    // Usage unique.
    let again = reset(&state, &token, "encore une autre phrase").await;
    assert_eq!(again.status, 404, "{}", again.raw);
    assert_eq!(again.code(), "reset_invalid");

    // Une nouvelle demande remplace le lien encore inutilise.
    assert_eq!(forgot(&state, "nora@example.com").await.status, 202);
    let first = reset_token_in(&wait_for_mail(&outbox, 2).await[1].text);
    // Moins de 60 s apres : 202, mais ni nouveau lien ni e-mail ; le
    // precedent reste valable.
    assert_eq!(forgot(&state, "nora@example.com").await.status, 202);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(outbox.lock().unwrap().len(), 2, "delai de 60 s par compte");
    let info = anonymous(
        &state,
        "GET",
        &format!("/api/auth/password/reset/{first}"),
        None,
    )
    .await;
    assert_eq!(info.status, 200, "{}", info.raw);
    offset.fetch_add(61, Ordering::SeqCst);
    assert_eq!(forgot(&state, "nora@example.com").await.status, 202);
    let second = reset_token_in(&wait_for_mail(&outbox, 3).await[2].text);
    let replaced = reset(&state, &first, "encore une autre phrase").await;
    assert_eq!(replaced.status, 404, "{}", replaced.raw);

    // Au-dela de 30 minutes : expire.
    offset.fetch_add(30 * 60 + 60, Ordering::SeqCst);
    let info = anonymous(
        &state,
        "GET",
        &format!("/api/auth/password/reset/{second}"),
        None,
    )
    .await;
    assert_eq!(info.status, 410, "{}", info.raw);
    assert_eq!(info.code(), "reset_expired");
    let late = reset(&state, &second, "encore une autre phrase").await;
    assert_eq!(late.status, 410, "{}", late.raw);
    assert_eq!(late.code(), "reset_expired");
    assert_eq!(
        login_as(&state, "nora@example.com", NEW_PASSWORD)
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn reset_revokes_sessions() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let nora_id = user(&state, "nora@example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;
    let nora_a = session(&state, "nora@example.com").await;
    // Double authentification active : la session suivante attend la MFA.
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, '\\x00', now())",
    )
    .bind(nora_id)
    .execute(&state.db)
    .await
    .unwrap();
    let pending = login_as(&state, "nora@example.com", PASSWORD).await;
    assert_eq!(pending.status, 200, "{}", pending.raw);
    assert_eq!(pending.json()["mfa_required"], true);
    let nora_b = (pending.token(), pending.csrf());

    // Lien genere par un administrateur (pas de SMTP : lien relatif a copier).
    let r = as_user(
        &state,
        &owner,
        "POST",
        &format!("/api/team/members/{nora_id}/reset-link"),
        None,
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.cache_control.as_deref(), Some("no-store"));
    let link = r.json()["link"].as_str().expect("link").to_string();
    assert!(link.starts_with("/reset/"), "{link}");
    let token = reset_token_in(&link);

    let r = reset(&state, &token, NEW_PASSWORD).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    for s in [&nora_a, &nora_b] {
        let me = as_user(&state, s, "GET", "/api/me", None).await;
        assert_eq!(me.status, 401, "{}", me.raw);
        assert_eq!(
            me.code(),
            "unauthenticated",
            "session complete et session en attente de MFA"
        );
    }
    assert_eq!(
        count(
            &state,
            &format!("SELECT count(*) FROM sessions WHERE user_id = '{nora_id}'")
        )
        .await,
        0
    );
    assert_eq!(
        as_user(&state, &owner, "GET", "/api/me", None).await.status,
        200,
        "les autres comptes gardent leurs sessions"
    );
    assert_eq!(
        count(
            &state,
            &format!("SELECT count(*) FROM user_totp WHERE user_id = '{nora_id}'")
        )
        .await,
        1,
        "la double authentification est conservee"
    );

    // Audit : lien genere et reinitialisation, jamais le jeton.
    let audit: Vec<(String, String)> = sqlx::query_as(
        "SELECT action, detail::text FROM audit_events \
         WHERE action IN ('team.member.reset_link', 'auth.password.reset') ORDER BY at",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    let actions: Vec<&str> = audit.iter().map(|(a, _)| a.as_str()).collect();
    assert_eq!(actions, ["team.member.reset_link", "auth.password.reset"]);
    assert!(audit.iter().all(|(_, d)| !d.contains(&token)));
}

#[tokio::test]
async fn reset_token_consumed_once_under_concurrency() {
    let (state, _pg) = common::state().await;
    user(&state, "nora@example.com", "developer").await;
    let link = control_plane::cli::admin::reset_password(&state.db, &state.cfg, "nora@example.com")
        .await
        .unwrap();
    let token = reset_token_in(&link);
    let (a, b) = tokio::join!(
        reset(&state, &token, NEW_PASSWORD),
        reset(&state, &token, "encore une autre phrase")
    );
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [204, 404], "{} / {}", a.raw, b.raw);
}

#[tokio::test]
async fn garbage_reset_token_is_refused_cheaply() {
    let (state, _pg) = common::state().await;
    let r = reset(&state, "pas-un-jeton", NEW_PASSWORD).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(r.code(), "reset_invalid");
}

#[tokio::test]
async fn admin_reset_link_respects_roles() {
    let (state, _pg) = common::state().await;
    let owner_id = user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let dev_id = user(&state, "dev@example.com", "developer").await;
    let admin = session(&state, "admin@example.com").await;
    let dev = session(&state, "dev@example.com").await;
    let path = |id: Uuid| format!("/api/team/members/{id}/reset-link");

    let r = as_user(&state, &admin, "POST", &path(owner_id), None).await;
    assert_eq!(
        r.status, 403,
        "un admin ne reinitialise pas le proprietaire"
    );
    let r = as_user(&state, &dev, "POST", &path(dev_id), None).await;
    assert_eq!(r.status, 403, "Equipe : Admin requis");
    let r = as_user(&state, &admin, "POST", &path(Uuid::new_v4()), None).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    let r = as_user(&state, &admin, "POST", &path(dev_id), None).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let r = as_user(&state, &admin, "POST", &path(admin_id), None).await;
    assert_eq!(r.status, 403, "jamais pour son propre compte : {}", r.raw);
    let other_admin = user(&state, "admin2@example.com", "admin").await;
    let r = as_user(&state, &admin, "POST", &path(other_admin), None).await;
    assert_eq!(r.status, 403, "un admin ne gere pas un autre admin");
    let owner = session(&state, "owner@example.com").await;
    let r = as_user(&state, &owner, "POST", &path(owner_id), None).await;
    assert_eq!(r.status, 403, "jamais pour son propre compte");
    let r = as_user(&state, &owner, "POST", &path(other_admin), None).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let r = call(
        &state,
        "POST",
        &path(dev_id),
        Req {
            cookie: Some(&admin.0),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "jeton CSRF exige");
}

#[tokio::test]
async fn smtp_password_write_only() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "dev@example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;
    let dev = session(&state, "dev@example.com").await;

    let empty = as_user(&state, &owner, "GET", "/api/settings/notifications", None).await;
    assert_eq!(empty.status, 200, "{}", empty.raw);
    assert_eq!(empty.json()["configured"], false);
    assert_eq!(empty.json()["password_set"], false);

    let put = as_user(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(smtp_body()),
    )
    .await;
    assert_eq!(put.status, 200, "{}", put.raw);
    assert!(!put.raw.contains(SMTP_PASSWORD));

    let get = as_user(&state, &owner, "GET", "/api/settings/notifications", None).await;
    assert_eq!(get.status, 200, "{}", get.raw);
    assert_eq!(get.cache_control.as_deref(), Some("no-store"));
    assert!(!get.raw.contains(SMTP_PASSWORD), "{}", get.raw);
    assert!(get.json().get("password").is_none());
    let body = get.json();
    assert_eq!(body["configured"], true);
    assert_eq!(body["password_set"], true);
    assert_eq!(body["host"], "smtp.example.com");
    assert_eq!(body["port"], 587);
    assert_eq!(body["tls"], "starttls");

    // Chiffre en base (age), jamais en clair.
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT password_sealed FROM smtp_settings")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert!(
        !sealed
            .windows(SMTP_PASSWORD.len())
            .any(|w| w == SMTP_PASSWORD.as_bytes())
    );
    assert_eq!(
        state.secrets.open(&sealed).unwrap().as_slice(),
        SMTP_PASSWORD.as_bytes()
    );

    // PUT sans mot de passe, meme serveur et meme utilisateur : l'ancien est
    // conserve.
    let mut keep = smtp_body();
    keep.as_object_mut().unwrap().remove("password");
    keep["from_name"] = json!("Ciphera Ops");
    let put = as_user(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(keep),
    )
    .await;
    assert_eq!(put.status, 200, "{}", put.raw);
    assert_eq!(put.json()["password_set"], true);
    assert_eq!(put.json()["from_name"], "Ciphera Ops");
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT password_sealed FROM smtp_settings")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(
        state.secrets.open(&sealed).unwrap().as_slice(),
        SMTP_PASSWORD.as_bytes()
    );

    let audit: Vec<String> = sqlx::query_scalar("SELECT detail::text FROM audit_events")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert!(audit.iter().all(|d| !d.contains(SMTP_PASSWORD)));

    // Changer d'hote ou d'utilisateur sans ressaisir le mot de passe : refuse
    // (sinon le mot de passe enregistre partirait vers un autre serveur).
    for (field, value) in [
        ("host", "attacker.example.net"),
        ("username", "autre@example.com"),
    ] {
        let mut body = smtp_body();
        body.as_object_mut().unwrap().remove("password");
        body[field] = json!(value);
        let r = as_user(
            &state,
            &owner,
            "PUT",
            "/api/settings/notifications",
            Some(body),
        )
        .await;
        assert_eq!(r.status, 422, "{field} : {}", r.raw);
        assert!(
            r.raw.contains("Ressaisissez le mot de passe SMTP"),
            "{}",
            r.raw
        );
    }
    let host: String = sqlx::query_scalar("SELECT host FROM smtp_settings")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(host, "smtp.example.com", "rien n'est enregistre");
    // Nouveau serveur avec son mot de passe : accepte.
    let mut moved = smtp_body();
    moved["host"] = json!("smtp2.example.com");
    let r = as_user(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(moved),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Utilisateur vide : pas d'authentification, mot de passe efface.
    let mut anonymous_smtp = smtp_body();
    anonymous_smtp.as_object_mut().unwrap().remove("password");
    anonymous_smtp["host"] = json!("smtp2.example.com");
    anonymous_smtp["username"] = json!("");
    let r = as_user(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(anonymous_smtp),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["password_set"], false);
    // Utilisateur sans aucun mot de passe enregistre : refuse.
    let mut no_password = smtp_body();
    no_password.as_object_mut().unwrap().remove("password");
    no_password["host"] = json!("smtp2.example.com");
    let r = as_user(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(no_password),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);

    // Parametres : Admin requis.
    let r = as_user(&state, &dev, "GET", "/api/settings/notifications", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let r = as_user(
        &state,
        &dev,
        "PUT",
        "/api/settings/notifications",
        Some(smtp_body()),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
}

#[tokio::test]
async fn smtp_requires_tls() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;

    let variants: Vec<(&str, Value)> = vec![
        ("tls", json!("none")),
        ("tls", json!("STARTTLS")),
        ("tls", json!("ssl")),
        ("tls", json!("")),
        ("port", json!(0)),
        ("port", json!(70000)),
        ("host", json!("")),
        ("host", json!("smtp.example.com\r\nRCPT TO:x")),
        ("host", json!("smtp example.com")),
        ("from_address", json!("pas-une-adresse")),
        ("from_name", json!("Ciphera\r\nBcc: x@example.com")),
    ];
    for (field, value) in variants {
        let mut body = smtp_body();
        body[field] = value.clone();
        let r = as_user(
            &state,
            &owner,
            "PUT",
            "/api/settings/notifications",
            Some(body),
        )
        .await;
        assert_eq!(r.status, 422, "{field} = {value} : {}", r.raw);
    }
    let mut body = smtp_body();
    body.as_object_mut().unwrap().remove("tls");
    let r = as_user(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(body),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM smtp_settings").await, 0);

    // Seuls STARTTLS et TLS implicite existent ; un transport est construit pour chacun.
    assert_eq!(TlsMode::parse("starttls"), Some(TlsMode::StartTls));
    assert_eq!(TlsMode::parse("tls"), Some(TlsMode::Tls));
    assert_eq!(TlsMode::parse("none"), None);
    let mut settings = SmtpSettings {
        enabled: true,
        host: "smtp.example.com".into(),
        port: 587,
        tls: TlsMode::StartTls,
        username: "u".into(),
        password: Some(Zeroizing::new("p".into())),
        from_address: "ciphera@example.com".into(),
        from_name: "Ciphera".into(),
    };
    assert!(Mailer::from_settings(&settings).is_some());
    settings.tls = TlsMode::Tls;
    settings.port = 465;
    assert!(Mailer::from_settings(&settings).is_some());
    settings.enabled = false;
    assert!(Mailer::from_settings(&settings).is_none(), "desactive");
}

#[tokio::test]
async fn invitation_is_emailed_when_mail_available() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    with_public_url(&mut state);
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    configure_smtp(&state, &owner).await;

    let r = as_user(
        &state,
        &owner,
        "POST",
        "/api/team/invitations",
        Some(json!({ "email": "nora@example.com", "role": "viewer" })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let link = r.json()["link"].as_str().unwrap().to_string();
    assert!(link.starts_with(&format!("{PUBLIC_URL}/invite/")), "{link}");
    let sent = wait_for_mail(&outbox, 1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "nora@example.com");
    assert!(sent[0].text.contains(&link));
}

#[tokio::test]
async fn cli_reset_password_prints_link() {
    let (state, _pg) = common::state().await;
    user(&state, "Nora@Example.com", "developer").await;
    let link = control_plane::cli::admin::reset_password(&state.db, &state.cfg, "nora@example.com")
        .await
        .expect("lien");
    assert!(link.starts_with("/reset/"), "{link}");
    let err = control_plane::cli::admin::reset_password(&state.db, &state.cfg, "x@example.com")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("aucun compte"), "{err}");

    let token = reset_token_in(&link);
    assert_eq!(reset(&state, &token, NEW_PASSWORD).await.status, 204);
    let audit: Vec<String> = sqlx::query_scalar(
        "SELECT detail::text FROM audit_events WHERE action = 'auth.password.reset_link.cli'",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(audit.len(), 1);
    assert!(!audit[0].contains(&token));
}
