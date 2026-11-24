//! Profil du compte connecte : informations, avatar, mot de passe, sessions,
//! preferences de notification (`/api/me...`).

mod common;

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::mail::{Delivery, OutgoingMail};
use control_plane::state::AppState;
use image::{ImageEncoder, ImageFormat};
use serde_json::{Value, json};
use shared::config::Config;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const NEW_PASSWORD: &str = "une phrase de passe assez longue";
const EMAIL: &str = "alice@example.com";
/// Marqueur ecrit dans le segment EXIF de l'image de test.
const EXIF_MARKER: &[u8] = b"GPS-46.2044-6.1432-SECRET";

async fn add_user(state: &AppState, email: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, 'developer', 'Alice Martin', now())",
    )
    .bind(id)
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    id
}

/// (jeton de session, jeton CSRF)
type Who = (String, String);

async fn login_with(state: &AppState, email: &str, pw: &str) -> Reply {
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

async fn session(state: &AppState, email: &str) -> Who {
    let r = login_with(state, email, PASSWORD).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    (r.token(), r.csrf())
}

async fn send(state: &AppState, who: &Who, method: &str, path: &str, body: Option<Value>) -> Reply {
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

async fn upload(state: &AppState, who: &Who, content_type: &str, data: Vec<u8>) -> Reply {
    call(
        state,
        "PUT",
        "/api/me/avatar",
        Req {
            cookie: Some(&who.0),
            csrf: Some(&who.1),
            bytes: Some((content_type, data)),
            ..Req::default()
        },
    )
    .await
}

async fn me(state: &AppState, who: &Who) -> Reply {
    send(state, who, "GET", "/api/me", None).await
}

fn header<'a>(r: &'a Reply, name: &str) -> &'a str {
    r.headers
        .get(name)
        .unwrap_or_else(|| panic!("en-tete {name}"))
        .to_str()
        .unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([40, 90, 200, 255]));
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .unwrap();
    out
}

fn webp(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 90, 40, 255]));
    image::codecs::webp::WebPEncoder::new_lossless(&mut out)
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

/// JPEG 300x200 portant un segment APP1 EXIF (TIFF valide, sans entree)
/// suivi d'un marqueur reconnaissable.
fn jpeg_with_exif() -> Vec<u8> {
    let mut jpeg = Vec::new();
    let img = image::RgbImage::from_fn(300, 200, |x, y| image::Rgb([x as u8, y as u8, 128]));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
        .write_image(img.as_raw(), 300, 200, image::ExtendedColorType::Rgb8)
        .unwrap();
    let mut payload = b"Exif\0\0MM\0*\0\0\0\x08\0\0\0\0\0\0".to_vec();
    payload.extend_from_slice(EXIF_MARKER);
    let len = u16::try_from(payload.len() + 2).unwrap().to_be_bytes();
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&[0xFF, 0xE1, len[0], len[1]]);
    out.extend_from_slice(&payload);
    out.extend_from_slice(&jpeg[2..]);
    assert!(
        image::load_from_memory(&out).is_ok(),
        "JPEG de test lisible"
    );
    out
}

async fn count(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&state.db)
        .await
        .unwrap()
}

async fn audit_actions(state: &AppState) -> Vec<String> {
    sqlx::query_scalar("SELECT action FROM audit_events ORDER BY at, action")
        .fetch_all(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn avatar_rejects_svg_and_oversize() {
    let (state, _pg) = common::state().await;
    add_user(&state, EMAIL).await;
    let who = session(&state, EMAIL).await;

    // SVG annonce comme PNG : le format est lu dans les octets, pas dans l'en-tete.
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
    let r = upload(&state, &who, "image/png", svg.to_vec()).await;
    assert_eq!(r.status, 415, "{}", r.raw);
    let r = upload(&state, &who, "image/svg+xml", svg.to_vec()).await;
    assert_eq!(r.status, 415, "{}", r.raw);
    // GIF : format non accepte.
    let r = upload(
        &state,
        &who,
        "image/gif",
        b"GIF89a\x01\0\x01\0\0\0\0;".to_vec(),
    )
    .await;
    assert_eq!(r.status, 415, "{}", r.raw);

    // Plus de 1 Mo : refuse avant tout decodage.
    let mut big = png(4, 4);
    big.resize(1024 * 1024 + 1, 0);
    let r = upload(&state, &who, "image/png", big).await;
    assert_eq!(r.status, 413, "{}", r.raw);

    // Signature PNG, contenu illisible.
    let mut broken = png(4, 4);
    broken.truncate(40);
    let r = upload(&state, &who, "image/png", broken).await;
    assert_eq!(r.status, 422, "{}", r.raw);

    // Dimensions hors limites (bombe de decompression) : refusees sans decodage complet.
    for (w, h) in [(8000, 1), (1, 8000)] {
        let r = upload(&state, &who, "image/png", png(w, h)).await;
        assert_eq!(r.status, 422, "{w}x{h} : {}", r.raw);
    }

    // Sans jeton CSRF : refuse.
    let r = call(
        &state,
        "PUT",
        "/api/me/avatar",
        Req {
            cookie: Some(&who.0),
            bytes: Some(("image/png", png(8, 8))),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);

    // Rien n'a ete enregistre.
    assert_eq!(count(&state, "SELECT count(*) FROM user_avatars").await, 0);
    let r = send(&state, &who, "GET", "/api/me/avatar", None).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(me(&state, &who).await.json()["avatar_url"], Value::Null);
}

#[tokio::test]
async fn avatar_strips_exif() {
    let (state, _pg) = common::state().await;
    add_user(&state, EMAIL).await;
    let who = session(&state, EMAIL).await;
    let original = jpeg_with_exif();
    assert!(contains(&original, EXIF_MARKER));

    // Content-Type trompeur : seul le contenu compte.
    let r = upload(&state, &who, "application/octet-stream", original).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let url = r.json()["avatar_url"]
        .as_str()
        .expect("avatar_url")
        .to_string();
    assert!(url.starts_with("/api/me/avatar?v="), "{url}");
    assert_eq!(me(&state, &who).await.json()["avatar_url"], json!(url));

    let r = send(&state, &who, "GET", &url, None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(header(&r, "content-type"), "image/png");
    assert_eq!(header(&r, "x-content-type-options"), "nosniff");
    assert!(header(&r, "cache-control").starts_with("private"));
    assert!(!contains(&r.bytes, EXIF_MARKER), "marqueur EXIF retire");
    assert!(!contains(&r.bytes, b"Exif"), "aucun segment EXIF");
    assert!(!contains(&r.bytes, b"eXIf"), "aucun bloc eXIf");
    assert_eq!(image::guess_format(&r.bytes).unwrap(), ImageFormat::Png);
    let img = image::load_from_memory(&r.bytes).unwrap();
    assert_eq!((img.width(), img.height()), (256, 256), "carre reduit");

    // Un autre compte ne lit pas cet avatar (chacun lit le sien).
    add_user(&state, "bob@example.com").await;
    let bob = session(&state, "bob@example.com").await;
    let r = send(&state, &bob, "GET", "/api/me/avatar", None).await;
    assert_eq!(r.status, 404, "{}", r.raw);

    // WebP accepte ; remplace l'avatar precedent.
    let r = upload(&state, &who, "image/webp", webp(40, 90)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM user_avatars").await, 1);

    // Suppression.
    let r = send(&state, &who, "DELETE", "/api/me/avatar", None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let r = send(&state, &who, "GET", "/api/me/avatar", None).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(me(&state, &who).await.json()["avatar_url"], Value::Null);
    let actions = audit_actions(&state).await;
    assert!(actions.contains(&"profile.avatar.updated".to_string()));
    assert!(actions.contains(&"profile.avatar.deleted".to_string()));
}

#[tokio::test]
async fn revoke_other_session() {
    let (state, _pg) = common::state().await;
    add_user(&state, EMAIL).await;
    add_user(&state, "bob@example.com").await;
    let laptop = session(&state, EMAIL).await;
    let phone = session(&state, EMAIL).await;
    let bob = session(&state, "bob@example.com").await;

    let r = send(&state, &laptop, "GET", "/api/me/sessions", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.cache_control.as_deref(), Some("no-store"));
    let sessions = r.json()["sessions"].as_array().unwrap().clone();
    assert_eq!(sessions.len(), 2, "seulement les sessions du compte");
    assert_eq!(
        sessions
            .iter()
            .filter(|s| s["current"] == json!(true))
            .count(),
        1
    );
    for s in &sessions {
        let keys: Vec<&String> = s.as_object().unwrap().keys().collect();
        for key in &keys {
            assert!(
                !key.contains("token") && !key.contains("hash"),
                "champ expose : {key}"
            );
        }
        assert_eq!(s["ip"], json!("203.0.113.7"));
        assert!(s["created_at"].is_i64() && s["last_seen_at"].is_i64());
    }
    assert!(!r.raw.contains(&laptop.0) && !r.raw.contains(&phone.0));
    let current = sessions
        .iter()
        .find(|s| s["current"] == json!(true))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let other = sessions
        .iter()
        .find(|s| s["current"] == json!(false))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // La session d'un autre compte : 404, et elle reste valide.
    let bob_id: String =
        sqlx::query_scalar("SELECT s.id::text FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email = 'bob@example.com'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    let r = send(
        &state,
        &laptop,
        "DELETE",
        &format!("/api/me/sessions/{bob_id}"),
        None,
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(me(&state, &bob).await.status, 200);
    // Identifiant invalide : 404.
    let r = send(
        &state,
        &laptop,
        "DELETE",
        "/api/me/sessions/pas-un-uuid",
        None,
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
    // La session courante : 409 (utiliser la deconnexion).
    let r = send(
        &state,
        &laptop,
        "DELETE",
        &format!("/api/me/sessions/{current}"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(me(&state, &laptop).await.status, 200);
    // Sans jeton CSRF : refuse.
    let r = call(
        &state,
        "DELETE",
        &format!("/api/me/sessions/{other}"),
        Req {
            cookie: Some(&laptop.0),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);

    // L'autre session du compte : revoquee.
    let r = send(
        &state,
        &laptop,
        "DELETE",
        &format!("/api/me/sessions/{other}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(me(&state, &phone).await.status, 401);
    assert_eq!(me(&state, &laptop).await.status, 200);
    let r = send(
        &state,
        &laptop,
        "DELETE",
        &format!("/api/me/sessions/{other}"),
        None,
    )
    .await;
    assert_eq!(r.status, 404, "deja revoquee : {}", r.raw);
    assert!(
        audit_actions(&state)
            .await
            .contains(&"auth.session.revoked".to_string())
    );
}

#[tokio::test]
async fn change_password_requires_current() {
    let (state, _pg) = common::state().await;
    let user_id = add_user(&state, EMAIL).await;
    let current = session(&state, EMAIL).await;
    let other = session(&state, EMAIL).await;
    let hash_before: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let change = |cur: &str, new: &str| json!({ "current_password": cur, "new_password": new });

    // Mot de passe actuel faux ou absent : refuse, rien ne change.
    let r = send(
        &state,
        &current,
        "POST",
        "/api/me/password",
        Some(change("mauvais mot de passe", NEW_PASSWORD)),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), json!("invalid_credentials"));
    let r = send(
        &state,
        &current,
        "POST",
        "/api/me/password",
        Some(json!({ "new_password": NEW_PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Nouveau mot de passe trop court.
    let r = send(
        &state,
        &current,
        "POST",
        "/api/me/password",
        Some(change(PASSWORD, "court")),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Sans jeton CSRF.
    let r = call(
        &state,
        "POST",
        "/api/me/password",
        Req {
            cookie: Some(&current.0),
            body: Some(change(PASSWORD, NEW_PASSWORD)),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let hash_now: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(hash_now, hash_before);
    assert_eq!(me(&state, &other).await.status, 200);

    // Bon mot de passe : la session courante reste, les autres sont revoquees.
    let r = send(
        &state,
        &current,
        "POST",
        "/api/me/password",
        Some(change(PASSWORD, NEW_PASSWORD)),
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(me(&state, &current).await.status, 200);
    assert_eq!(me(&state, &other).await.status, 401);
    assert_eq!(login_with(&state, EMAIL, PASSWORD).await.status, 401);
    assert_eq!(login_with(&state, EMAIL, NEW_PASSWORD).await.status, 200);
    let actions = audit_actions(&state).await;
    assert!(actions.contains(&"auth.password.changed".to_string()));
    assert!(actions.contains(&"auth.password.change.failure".to_string()));
    let detail: String = sqlx::query_scalar(
        "SELECT detail::text FROM audit_events WHERE action = 'auth.password.changed'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert!(!detail.contains(NEW_PASSWORD) && !detail.contains(PASSWORD));

    // Limitation : 5 echecs, puis 429.
    let r = login_with(&state, EMAIL, NEW_PASSWORD).await;
    let fresh = (r.token(), r.csrf());
    let mut last = 0;
    for _ in 0..6 {
        last = send(
            &state,
            &fresh,
            "POST",
            "/api/me/password",
            Some(change("toujours pas le bon", NEW_PASSWORD)),
        )
        .await
        .status;
    }
    assert_eq!(last, 429);
}

#[tokio::test]
async fn profile_update_validates_fields() {
    let (state, _pg) = common::state().await;
    add_user(&state, EMAIL).await;
    let who = session(&state, EMAIL).await;

    let r = me(&state, &who).await;
    assert_eq!(r.json()["language"], json!("fr"));
    assert_eq!(r.json()["timezone"], json!("UTC"));
    assert!(r.json()["csrf_token"].is_string(), "GET /api/me inchange");

    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        Some(json!({ "name": "  Alice Durand ", "language": "en", "timezone": "Europe/Zurich" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let r = me(&state, &who).await;
    assert_eq!(r.json()["name"], json!("Alice Durand"));
    assert_eq!(r.json()["language"], json!("en"));
    assert_eq!(r.json()["timezone"], json!("Europe/Zurich"));

    for body in [
        json!({ "language": "de" }),
        json!({ "timezone": "Mars/Olympus_Mons" }),
        json!({ "timezone": "europe/zurich" }),
        json!({ "timezone": "../etc/passwd" }),
        json!({ "name": "" }),
        json!({ "name": "a\u{0007}b" }),
        json!({ "name": "x".repeat(201) }),
        json!({ "role": "owner" }),
    ] {
        let r = send(&state, &who, "PATCH", "/api/me", Some(body.clone())).await;
        assert_eq!(r.status, 422, "{body} : {}", r.raw);
    }
    let role: String = sqlx::query_scalar("SELECT role FROM users WHERE email = $1")
        .bind(EMAIL)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(role, "developer");
    assert_eq!(
        me(&state, &who).await.json()["timezone"],
        json!("Europe/Zurich")
    );
}

type Outbox = Arc<Mutex<Vec<OutgoingMail>>>;

fn capture(state: &mut AppState) -> Outbox {
    let outbox: Outbox = Arc::default();
    let sink = outbox.clone();
    state.mail = Delivery::from_fn(move |_mailer, mail| {
        sink.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    });
    outbox
}

async fn wait_for_mail(outbox: &Outbox, n: usize) -> Vec<OutgoingMail> {
    for _ in 0..100 {
        if outbox.lock().unwrap().len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    outbox.lock().unwrap().clone()
}

/// Instance qui peut envoyer des e-mails (origine publique + SMTP actif).
async fn with_smtp(state: &mut AppState) {
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
}

/// Jeton du lien `/verify-email/<jeton>` d'un e-mail.
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

async fn confirm_email(state: &AppState, token: &str, cookie: Option<&str>) -> Reply {
    call(
        state,
        "POST",
        &format!("/api/auth/email-verification/{token}/confirm"),
        Req {
            cookie,
            ..Req::default()
        },
    )
    .await
}

fn change_email(email: &str) -> Option<Value> {
    Some(json!({ "email": email, "current_password": PASSWORD }))
}

#[tokio::test]
async fn email_change_requires_confirmation() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    with_smtp(&mut state).await;
    let user_id = add_user(&state, EMAIL).await;
    add_user(&state, "bob@example.com").await;
    let who = session(&state, EMAIL).await;
    sqlx::query(
        "INSERT INTO password_resets (id, token_hash, user_id, created_at, expires_at) \
         VALUES ($1, '\\x01', $2, now(), now() + interval '30 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .execute(&state.db)
    .await
    .unwrap();

    // Sans mot de passe, ou mot de passe faux, ou adresse invalide.
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        Some(json!({ "email": "alice@new.example.com" })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        Some(
            json!({ "email": "alice@new.example.com", "current_password": "mauvais mot de passe" }),
        ),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        Some(json!({ "email": "pas une adresse", "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Adresse d'un autre compte (casse differente) : meme reponse qu'une
    // adresse libre, mais aucun lien ni e-mail.
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("Bob@Example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(
        count(&state, "SELECT count(*) FROM email_verifications").await,
        0
    );
    assert!(outbox.lock().unwrap().is_empty());
    age_change_requests(&state).await;

    // Meme adresse que l'actuelle : rien a changer.
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("ALICE@example.com"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);

    // Adresse libre : demande enregistree, l'adresse ne change pas encore.
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        Some(json!({
            "name": "Alice Durand",
            "email": "Alice@New.example.com",
            "current_password": PASSWORD
        })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["email"], json!(EMAIL), "pas encore remplacee");
    assert_eq!(r.json()["name"], json!("Alice Durand"));
    assert_eq!(me(&state, &who).await.json()["email"], json!(EMAIL));
    let mails = wait_for_mail(&outbox, 1).await;
    assert_eq!(
        mails.len(),
        1,
        "lien envoye a la nouvelle adresse seulement"
    );
    assert_eq!(mails[0].to, "alice@new.example.com");
    assert!(!mails[0].text.contains(EMAIL));
    let token = verification_token(&mails[0]);
    // Seule l'empreinte est stockee ; liens de reinitialisation intacts.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM email_verifications")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(stored.len(), 32);
    assert_ne!(stored, token.as_bytes());
    assert_eq!(
        count(&state, "SELECT count(*) FROM password_resets").await,
        1
    );

    // Ouvrir le lien (GET) ne change rien.
    let r = call(
        &state,
        "GET",
        &format!("/api/auth/email-verification/{token}"),
        Req::default(),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["purpose"], json!("change"));
    assert_eq!(r.json()["email"], json!("alice@new.example.com"));
    assert_eq!(me(&state, &who).await.json()["email"], json!(EMAIL));

    // Confirmation, sans session : l'adresse est remplacee.
    let r = confirm_email(&state, &token, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(login_with(&state, EMAIL, PASSWORD).await.status, 401);
    assert_eq!(
        login_with(&state, "alice@new.example.com", PASSWORD)
            .await
            .status,
        200
    );
    // Avis a l'ancienne adresse (sans la nouvelle).
    let mails = wait_for_mail(&outbox, 2).await;
    assert_eq!(mails.len(), 2);
    assert_eq!(mails[1].to, EMAIL);
    assert!(!mails[1].text.contains("alice@new.example.com"));
    // Liens de reinitialisation en cours annules.
    assert_eq!(
        count(&state, "SELECT count(*) FROM password_resets").await,
        0
    );
    let detail: String = sqlx::query_scalar(
        "SELECT detail::text FROM audit_events WHERE action = 'profile.email.changed'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert!(
        !detail.contains("example.com"),
        "aucune adresse dans l'audit : {detail}"
    );
    // Usage unique.
    let r = confirm_email(&state, &token, None).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(r.code(), "email_verification_invalid");
}

#[tokio::test]
async fn email_change_revokes_other_sessions() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    with_smtp(&mut state).await;
    add_user(&state, EMAIL).await;
    let who = session(&state, EMAIL).await;
    let other = session(&state, EMAIL).await;
    let third = session(&state, EMAIL).await;
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("alice@new.example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let token = verification_token(&wait_for_mail(&outbox, 1).await[0]);
    // Les sessions ne tombent qu'a la confirmation.
    assert_eq!(me(&state, &other).await.status, 200);

    // Confirmation depuis le navigateur de `who` : sa session reste ouverte.
    let r = confirm_email(&state, &token, Some(&who.0)).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(me(&state, &who).await.status, 200);
    assert_eq!(me(&state, &other).await.status, 401);
    assert_eq!(me(&state, &third).await.status, 401);
    assert_eq!(
        me(&state, &who).await.json()["email"],
        json!("alice@new.example.com")
    );

    // Confirmation sans session : toutes les sessions tombent.
    sqlx::query("UPDATE users SET email_change_requested_at = email_change_requested_at - interval '5 minutes'")
        .execute(&state.db)
        .await
        .unwrap();
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("a2@new.example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let mails = wait_for_mail(&outbox, 3).await;
    let mail = mails
        .iter()
        .find(|m| m.to == "a2@new.example.com")
        .expect("lien envoye a la nouvelle adresse");
    let token = verification_token(mail);
    let r = confirm_email(&state, &token, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(me(&state, &who).await.status, 401);
}

#[tokio::test]
async fn email_change_needs_smtp_and_link_expires() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    add_user(&state, EMAIL).await;
    let who = session(&state, EMAIL).await;
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("alice@new.example.com"),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "smtp_required");
    assert_eq!(
        count(&state, "SELECT count(*) FROM email_verifications").await,
        0
    );
    assert_eq!(me(&state, &who).await.json()["email"], json!(EMAIL));

    with_smtp(&mut state).await;
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("alice@new.example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let token = verification_token(&wait_for_mail(&outbox, 1).await[0]);
    sqlx::query("UPDATE email_verifications SET expires_at = now() - interval '1 minute'")
        .execute(&state.db)
        .await
        .unwrap();
    let r = confirm_email(&state, &token, None).await;
    assert_eq!(r.status, 410, "{}", r.raw);
    assert_eq!(r.code(), "email_verification_expired");
    assert_eq!(me(&state, &who).await.json()["email"], json!(EMAIL));
}
#[tokio::test]
async fn notification_prefs_email_only() {
    let (state, _pg) = common::state().await;
    add_user(&state, EMAIL).await;
    add_user(&state, "bob@example.com").await;
    let who = session(&state, EMAIL).await;
    let bob = session(&state, "bob@example.com").await;

    let r = send(&state, &who, "GET", "/api/me/notification-prefs", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        r.json(),
        json!({
            "email_available": false,
            "email_enabled": true,
            "deploy_succeeded": true,
            "deploy_failed": true,
            "certificates": false,
            "team": true,
        })
    );
    let prefs = json!({
        "email_enabled": false,
        "deploy_succeeded": false,
        "deploy_failed": true,
        "certificates": true,
        "team": false,
    });
    let r = send(
        &state,
        &who,
        "PUT",
        "/api/me/notification-prefs",
        Some(prefs.clone()),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let r = send(&state, &who, "GET", "/api/me/notification-prefs", None).await;
    assert_eq!(r.json()["email_enabled"], json!(false));
    assert_eq!(r.json()["certificates"], json!(true));
    assert_eq!(r.json()["deploy_succeeded"], json!(false));
    // Un autre compte garde ses valeurs par defaut.
    let r = send(&state, &bob, "GET", "/api/me/notification-prefs", None).await;
    assert_eq!(r.json()["email_enabled"], json!(true));
    assert_eq!(r.json()["certificates"], json!(false), "developer");
    // Proprietaire et administrateur : certificats actives par defaut.
    for role in ["owner", "admin"] {
        sqlx::query("UPDATE users SET role = $1 WHERE email = 'bob@example.com'")
            .bind(role)
            .execute(&state.db)
            .await
            .unwrap();
        let r = send(&state, &bob, "GET", "/api/me/notification-prefs", None).await;
        assert_eq!(r.json()["certificates"], json!(true), "{role}");
    }
    // Canal autre que l'e-mail, ou champ manquant : refuse.
    let mut slack = prefs.clone();
    slack["dashboard"] = json!(true);
    let r = send(
        &state,
        &who,
        "PUT",
        "/api/me/notification-prefs",
        Some(slack),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let r = send(
        &state,
        &who,
        "PUT",
        "/api/me/notification-prefs",
        Some(json!({ "email_enabled": true })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
}

/// Attend qu'une requete soit bloquee sur un verrou de la base (5 s au plus).
async fn wait_for_lock_waiter(state: &AppState) {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE wait_event_type = 'Lock' AND datname = current_database()",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        if waiting >= 1 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("aucune requete en attente de verrou");
}

#[tokio::test]
async fn password_change_refused_if_hash_or_session_changed_meanwhile() {
    let (state, _pg) = common::state().await;
    let user_id = add_user(&state, EMAIL).await;
    let who = session(&state, EMAIL).await;
    let other = session(&state, EMAIL).await;
    let body = json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD });

    // Le hachage change (reinitialisation concurrente) apres la verification.
    let mut gate = state.db.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM users WHERE id = $1 FOR NO KEY UPDATE")
        .bind(user_id)
        .execute(&mut *gate)
        .await
        .unwrap();
    let request = {
        let (state, who, body) = (state.clone(), who.clone(), body.clone());
        tokio::spawn(
            async move { send(&state, &who, "POST", "/api/me/password", Some(body)).await },
        )
    };
    wait_for_lock_waiter(&state).await;
    let concurrent = password::hash("un autre mot de passe choisi").unwrap();
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(user_id)
        .bind(&concurrent)
        .execute(&mut *gate)
        .await
        .unwrap();
    gate.commit().await.unwrap();
    let r = request.await.unwrap();
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), json!("password_changed"));
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(stored, concurrent, "le changement concurrent est garde");
    assert_eq!(me(&state, &other).await.status, 200, "rien n'a ete revoque");

    // La session courante est revoquee entre la verification et l'ecriture.
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(user_id)
        .bind(password::hash(PASSWORD).unwrap())
        .execute(&state.db)
        .await
        .unwrap();
    let session_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM sessions WHERE token_hash = sha256(convert_to($1, 'UTF8'))",
    )
    .bind(&who.0)
    .fetch_one(&state.db)
    .await
    .unwrap();
    let mut gate = state.db.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM sessions WHERE id = $1 FOR NO KEY UPDATE")
        .bind(session_id)
        .execute(&mut *gate)
        .await
        .unwrap();
    let request = {
        let (state, who, body) = (state.clone(), who.clone(), body.clone());
        tokio::spawn(
            async move { send(&state, &who, "POST", "/api/me/password", Some(body)).await },
        )
    };
    wait_for_lock_waiter(&state).await;
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session_id)
        .execute(&mut *gate)
        .await
        .unwrap();
    gate.commit().await.unwrap();
    let r = request.await.unwrap();
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(
        me(&state, &other).await.status,
        200,
        "autres sessions intactes"
    );
    assert_eq!(
        login_with(&state, EMAIL, NEW_PASSWORD).await.status,
        401,
        "mot de passe inchange"
    );
}

#[tokio::test]
async fn avatar_uploads_are_rate_limited_per_account() {
    let (state, _pg) = common::state().await;
    add_user(&state, EMAIL).await;
    add_user(&state, "bob@example.com").await;
    let who = session(&state, EMAIL).await;
    let bob = session(&state, "bob@example.com").await;
    for _ in 0..5 {
        let r = upload(&state, &who, "image/png", png(8, 8)).await;
        assert_eq!(r.status, 200, "{}", r.raw);
    }
    let r = upload(&state, &who, "image/png", png(8, 8)).await;
    assert_eq!(r.status, 429, "{}", r.raw);
    assert!(header(&r, "retry-after").parse::<u64>().unwrap() >= 1);
    // Autre compte : non concerne.
    let r = upload(&state, &bob, "image/png", png(8, 8)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn second_email_change_within_the_minute_cancels_the_first_link() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    with_smtp(&mut state).await;
    add_user(&state, EMAIL).await;
    add_user(&state, "bob@example.com").await;
    let who = session(&state, EMAIL).await;

    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("alice@gmial.example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let typo = verification_token(&wait_for_mail(&outbox, 1).await[0]);

    // Correction dans la minute : 429 explicite avec Retry-After, rien n'est
    // envoye, et le premier lien est annule.
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("alice@gmail.example.com"),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
    assert_eq!(r.code(), "rate_limited");
    let retry: u64 = r
        .retry_after
        .as_deref()
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry), "{retry}");
    assert_eq!(wait_for_mail(&outbox, 1).await.len(), 1);
    let r = confirm_email(&state, &typo, None).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(
        count(&state, "SELECT count(*) FROM email_verifications").await,
        0
    );
    assert_eq!(me(&state, &who).await.json()["email"], json!(EMAIL));

    // Meme annulation quand la nouvelle adresse est celle d'un autre compte.
    sqlx::query("UPDATE users SET email_change_requested_at = email_change_requested_at - interval '5 minutes'")
        .execute(&state.db)
        .await
        .unwrap();
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("alice@later.example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let later = verification_token(&wait_for_mail(&outbox, 2).await[1]);
    sqlx::query("UPDATE users SET email_change_requested_at = email_change_requested_at - interval '5 minutes'")
        .execute(&state.db)
        .await
        .unwrap();
    let r = send(
        &state,
        &who,
        "PATCH",
        "/api/me",
        change_email("bob@example.com"),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(confirm_email(&state, &later, None).await.status, 404);
}

async fn age_change_requests(state: &AppState) {
    sqlx::query("UPDATE users SET email_change_requested_at = email_change_requested_at - interval '5 minutes'")
        .execute(&state.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn email_change_delay_counts_requests_not_links() {
    let (mut state, _pg) = common::state().await;
    let outbox = capture(&mut state);
    with_smtp(&mut state).await;
    add_user(&state, EMAIL).await;
    add_user(&state, "bob@example.com").await;
    let who = session(&state, EMAIL).await;
    let ask = |email: &'static str| {
        let (state, who) = (state.clone(), who.clone());
        async move { send(&state, &who, "PATCH", "/api/me", change_email(email)).await }
    };

    // Adresse prise, puis adresse libre dans la minute : 429 (comme apres une adresse libre).
    assert_eq!(ask("bob@example.com").await.status, 202);
    let r = ask("alice@free.example.com").await;
    assert_eq!(r.status, 429, "{}", r.raw);
    assert!(r.retry_after.is_some());
    assert!(outbox.lock().unwrap().is_empty());

    // Adresse libre, puis adresse prise dans la minute : 429.
    age_change_requests(&state).await;
    assert_eq!(ask("alice@free.example.com").await.status, 202);
    let r = ask("bob@example.com").await;
    assert_eq!(r.status, 429, "{}", r.raw);

    // Trois demandes dans la minute (libre, libre, prise) : un seul e-mail.
    age_change_requests(&state).await;
    assert_eq!(ask("alice@a.example.com").await.status, 202);
    assert_eq!(ask("alice@b.example.com").await.status, 429);
    assert_eq!(ask("bob@example.com").await.status, 429);
    assert_eq!(ask("alice@c.example.com").await.status, 429);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mails = outbox.lock().unwrap().clone();
    assert_eq!(
        mails.iter().filter(|m| m.to.starts_with("alice@")).count(),
        2,
        "un envoi pour la 2e suite, un pour la 3e : {:?}",
        mails.iter().map(|m| m.to.clone()).collect::<Vec<_>>()
    );
    // Aucun lien n'est valable apres les annulations successives, hors le dernier envoi.
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM email_verifications WHERE used_at IS NULL"
        )
        .await,
        0,
        "la 429 annule aussi le dernier lien"
    );
}
