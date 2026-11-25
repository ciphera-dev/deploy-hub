mod common;

use axum::body::Body;
use axum::http::{Request, header};
use common::{TempDir, state_in};
use control_plane::app::build_router;
use control_plane::state::AppState;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const TOKEN: &str = "tok_Zm9vYmFyYmF6cXV4MTIzNDU2Nzg5MGFiY2RlZmdoaWo";
const PASSWORD: &str = "correct horse battery";

struct Env {
    state: AppState,
    config: TempDir,
    _data: TempDir,
    _pg: common::TestDb,
}

async fn env() -> Env {
    let config = TempDir::new();
    let data = TempDir::new();
    std::fs::write(config.0.join("setup_token"), format!("{TOKEN}\n")).unwrap();
    std::fs::create_dir_all(config.0.join("tls")).unwrap();
    std::fs::write(config.0.join("tls/panel.crt"), "x").unwrap();
    let (state, pg) = state_in(&config.0, &data.0).await;
    Env {
        state,
        config,
        _data: data,
        _pg: pg,
    }
}

struct Reply {
    status: u16,
    set_cookie: Option<String>,
    raw: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.raw).unwrap()
    }
}

async fn call(
    state: &AppState,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        req = req.header("x-setup-token", t);
    }
    let body = match body {
        Some(b) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let res = build_router(state.clone())
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = res.status().as_u16();
    let set_cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    Reply {
        status,
        set_cookie,
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

fn payload(email: &str, password: &str) -> Value {
    json!({
        "admin": { "name": "Alice Martin", "email": email, "password": password },
        "organization": { "name": "Acme", "slug": "acme", "timezone": "Europe/Paris", "language": "fr" }
    })
}

async fn complete(state: &AppState, token: Option<&str>, body: Value) -> Reply {
    call(state, "POST", "/api/setup/complete", token, Some(body)).await
}

async fn count_users(state: &AppState) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn complete_creates_single_owner() {
    let env = env().await;
    let r = complete(
        &env.state,
        Some(TOKEN),
        payload("alice@example.com", PASSWORD),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert!(!r.raw.contains(PASSWORD));
    assert_eq!(r.json()["user"]["role"], "owner");

    let cookie = r.set_cookie.expect("cookie de session");
    for attr in [
        "__Host-ciphera_session=",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(cookie.contains(attr), "{attr} absent de {cookie}");
    }
    assert!(!cookie.contains("Domain"));
    let raw_token = cookie
        .strip_prefix("__Host-ciphera_session=")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let (role, hash, name): (String, String, String) =
        sqlx::query_as("SELECT role, password_hash, name FROM users")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    assert_eq!(role, "owner");
    assert_eq!(name, "Alice Martin");
    assert!(hash.starts_with("$argon2id$"), "{hash}");
    assert!(!hash.contains(PASSWORD));

    // Seul le SHA-256 du jeton est stocke.
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(stored, Sha256::digest(raw_token.as_bytes()).to_vec());
    assert_ne!(stored, raw_token.as_bytes());

    let (org, slug, tz, lang): (String, String, String, String) =
        sqlx::query_as("SELECT name, slug, timezone, language FROM instance")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    assert_eq!(
        (org.as_str(), slug.as_str(), tz.as_str(), lang.as_str()),
        ("Acme", "acme", "Europe/Paris", "fr")
    );

    let actor_is_owner: bool = sqlx::query_scalar(
        "SELECT actor_id = (SELECT id FROM users) FROM audit_events WHERE action = 'setup.completed'",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert!(actor_is_owner);
    let detail: String = sqlx::query_scalar("SELECT detail::text FROM audit_events")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert!(!detail.contains(PASSWORD));

    let status = call(&env.state, "GET", "/api/setup/status", None, None).await;
    assert_eq!(status.json(), json!({ "initialized": true }));
}

#[tokio::test]
async fn status_is_public_and_false_on_fresh_instance() {
    let env = env().await;
    let r = call(&env.state, "GET", "/api/setup/status", None, None).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json(), json!({ "initialized": false }));
}

#[tokio::test]
async fn complete_twice_returns_410() {
    let env = env().await;
    let first = complete(&env.state, Some(TOKEN), payload("a@example.com", PASSWORD)).await;
    assert_eq!(first.status, 201);
    let second = complete(&env.state, Some(TOKEN), payload("b@example.com", PASSWORD)).await;
    assert_eq!(second.status, 410);
    assert_eq!(second.json()["error"]["code"], "setup_closed");
    assert_eq!(count_users(&env.state).await, 1);
}

async fn race(env: &Env, same_email: bool) -> Vec<u16> {
    let mut tasks = Vec::new();
    for i in 0..10 {
        let state = env.state.clone();
        let email = if same_email {
            "same@example.com".to_string()
        } else {
            format!("admin{i}@example.com")
        };
        tasks.push(tokio::spawn(async move {
            complete(&state, Some(TOKEN), payload(&email, PASSWORD)).await
        }));
    }
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap().status);
    }
    statuses
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_complete_creates_one_admin() {
    let env = env().await;
    let statuses = race(&env, false).await;
    assert_eq!(
        statuses.iter().filter(|s| **s == 201).count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 410).count(),
        9,
        "{statuses:?}"
    );
    assert_eq!(count_users(&env.state).await, 1);
    for table in ["sessions"] {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&env.state.db)
                .await
                .unwrap();
        assert_eq!(n, 1, "{table}");
    }
    let audits: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = 'setup.completed'")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    assert_eq!(audits, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_complete_same_email_creates_one_admin() {
    let env = env().await;
    let statuses = race(&env, true).await;
    assert_eq!(
        statuses.iter().filter(|s| **s == 201).count(),
        1,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 410).count(),
        9,
        "{statuses:?}"
    );
    assert_eq!(count_users(&env.state).await, 1);
}

#[tokio::test]
async fn wrong_token_401() {
    let env = env().await;
    for token in [Some("nope"), Some(""), None] {
        let r = complete(&env.state, token, payload("a@example.com", PASSWORD)).await;
        assert_eq!(r.status, 401, "{token:?}");
        assert_eq!(r.json()["error"]["code"], "invalid_setup_token");
    }
    assert_eq!(count_users(&env.state).await, 0);
    assert!(env.config.0.join("setup_token").exists());
}

#[tokio::test]
async fn setup_token_expires() {
    let env = env().await;
    let file = env.config.0.join("setup_token");
    let now = control_plane::setup_token::unix_now();
    let checks = || call(&env.state, "POST", "/api/setup/checks", Some(TOKEN), None);

    // Echeance ecrite dans le fichier : valable avant, refuse apres.
    std::fs::write(
        &file,
        control_plane::setup_token::render(TOKEN, now + 3600).as_str(),
    )
    .unwrap();
    assert_eq!(checks().await.status, 200);
    std::fs::write(
        &file,
        control_plane::setup_token::render(TOKEN, now - 1).as_str(),
    )
    .unwrap();
    let r = checks().await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.json()["error"]["code"], "setup_token_expired");
    assert!(r.raw.contains("ciphera admin setup-token"));
    let r = complete(&env.state, Some(TOKEN), payload("a@example.com", PASSWORD)).await;
    assert_eq!(r.json()["error"]["code"], "setup_token_expired");
    assert_eq!(count_users(&env.state).await, 0);
    // Un autre jeton reste « invalide » : l'echeance ne se revele qu'au detenteur.
    let r = call(&env.state, "POST", "/api/setup/checks", Some("nope"), None).await;
    assert_eq!(r.json()["error"]["code"], "invalid_setup_token");

    // Fichier sans echeance (version precedente) : 24 h apres sa modification.
    std::fs::write(&file, format!("{TOKEN}\n")).unwrap();
    assert_eq!(checks().await.status, 200);
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 3600))
        .unwrap();
    assert_eq!(
        checks().await.json()["error"]["code"],
        "setup_token_expired"
    );
}

#[tokio::test]
async fn setup_token_regenerated_by_cli() {
    let env = env().await;
    let file = env.config.0.join("setup_token");
    std::fs::write(
        &file,
        control_plane::setup_token::render(TOKEN, control_plane::setup_token::unix_now() - 1)
            .as_str(),
    )
    .unwrap();

    let fresh = control_plane::cli::admin::setup_token(&env.state.db, &env.config.0, None)
        .await
        .unwrap();
    assert_ne!(fresh.as_str(), TOKEN);
    let text = std::fs::read_to_string(&file).unwrap();
    let stored = control_plane::setup_token::parse(&text).unwrap();
    assert_eq!(stored.token.as_str(), fresh.as_str());
    let left = stored.expires_at.unwrap() - control_plane::setup_token::unix_now();
    assert!((24 * 3600 - 60..=24 * 3600).contains(&left), "{left}");
    // L'ancien jeton ne vaut plus rien ; le nouveau ouvre l'assistant.
    let r = call(&env.state, "POST", "/api/setup/checks", Some(TOKEN), None).await;
    assert_eq!(r.json()["error"]["code"], "invalid_setup_token");
    let r = call(&env.state, "POST", "/api/setup/checks", Some(&fresh), None).await;
    assert_eq!(r.status, 200, "{}", r.raw);

    // Un compte existe : plus de jeton, fichier inchange.
    let r = complete(&env.state, Some(&fresh), payload("a@example.com", PASSWORD)).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let err = control_plane::cli::admin::setup_token(&env.state.db, &env.config.0, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("compte"), "{err}");
    assert!(!file.exists());
}

#[tokio::test]
async fn token_file_deleted_after_complete() {
    let env = env().await;
    assert!(env.config.0.join("setup_token").exists());
    let r = complete(&env.state, Some(TOKEN), payload("a@example.com", PASSWORD)).await;
    assert_eq!(r.status, 201);
    assert!(!env.config.0.join("setup_token").exists());
}

#[tokio::test]
async fn token_invalid_as_soon_as_a_user_exists_even_if_file_lingers() {
    let env = env().await;
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, email_verified_at) VALUES ($1, 'x@example.com', 'h', 'owner', now())",
    )
    .bind(uuid::Uuid::new_v4())
    .execute(&env.state.db)
    .await
    .unwrap();
    assert!(env.config.0.join("setup_token").exists());
    let r = complete(&env.state, Some(TOKEN), payload("a@example.com", PASSWORD)).await;
    assert_eq!(r.status, 410);
    let checks = call(&env.state, "POST", "/api/setup/checks", Some(TOKEN), None).await;
    assert_eq!(checks.status, 410);
    assert_eq!(count_users(&env.state).await, 1);
}

#[tokio::test]
async fn missing_token_file_without_users_makes_setup_impossible() {
    let env = env().await;
    std::fs::remove_file(env.config.0.join("setup_token")).unwrap();
    let r = complete(&env.state, Some(TOKEN), payload("a@example.com", PASSWORD)).await;
    assert_eq!(r.status, 503);
    assert_eq!(r.json()["error"]["code"], "setup_token_missing");
    assert_eq!(count_users(&env.state).await, 0);
}

#[tokio::test]
async fn password_policy_rejects_short() {
    let env = env().await;
    let r = complete(
        &env.state,
        Some(TOKEN),
        payload("a@example.com", "onzecaracte"),
    )
    .await;
    assert_eq!(r.status, 422);
    assert_eq!(r.json()["error"]["code"], "validation_failed");
    assert!(!r.raw.contains("onzecaracte"));
    let long = "x".repeat(1025);
    assert_eq!(
        complete(&env.state, Some(TOKEN), payload("a@example.com", &long))
            .await
            .status,
        422
    );
    // Echec de validation : rien n'est consomme.
    assert_eq!(count_users(&env.state).await, 0);
    assert!(env.config.0.join("setup_token").exists());
    // Exactement 12 caracteres (multi-octets) : accepte.
    let twelve = "é".repeat(12);
    assert_eq!(
        complete(&env.state, Some(TOKEN), payload("a@example.com", &twelve))
            .await
            .status,
        201
    );
}

#[tokio::test]
async fn invalid_fields_are_422() {
    let env = env().await;
    for (field, value) in [("email", "pas-un-email"), ("name", "  ")] {
        let mut body = payload("a@example.com", PASSWORD);
        body["admin"][field] = json!(value);
        assert_eq!(
            complete(&env.state, Some(TOKEN), body).await.status,
            422,
            "{field}"
        );
    }
    for (field, value) in [
        ("slug", "Mauvais Slug"),
        ("language", "xx"),
        ("timezone", "../etc"),
        ("name", ""),
    ] {
        let mut body = payload("a@example.com", PASSWORD);
        body["organization"][field] = json!(value);
        assert_eq!(
            complete(&env.state, Some(TOKEN), body).await.status,
            422,
            "{field}"
        );
    }
    assert_eq!(count_users(&env.state).await, 0);
}

#[tokio::test]
async fn checks_require_token_and_report_local_state() {
    let env = env().await;
    assert_eq!(
        call(&env.state, "POST", "/api/setup/checks", None, None)
            .await
            .status,
        401
    );
    assert_eq!(
        call(&env.state, "POST", "/api/setup/checks", Some("nope"), None)
            .await
            .status,
        401
    );

    let r = call(&env.state, "POST", "/api/setup/checks", Some(TOKEN), None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let checks = r.json()["checks"].as_array().unwrap().clone();
    let by_id = |id: &str| {
        checks
            .iter()
            .find(|c| c["id"] == id)
            .unwrap_or_else(|| panic!("{id}"))
            .clone()
    };
    assert_eq!(by_id("database")["ok"], true);
    assert_eq!(by_id("tls")["ok"], true);
    assert_eq!(by_id("data_dir")["ok"], true);
    for c in &checks {
        assert!(c["detail"].as_str().is_some_and(|d| !d.is_empty()));
    }
    // Docker absent ou present : jamais un echec bloquant.
    assert_eq!(by_id("docker")["ok"], true);

    // Certificat absent : echec.
    std::fs::remove_file(env.config.0.join("tls/panel.crt")).unwrap();
    let r = call(&env.state, "POST", "/api/setup/checks", Some(TOKEN), None).await;
    let tls = r.json()["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "tls")
        .unwrap()
        .clone();
    assert_eq!(tls["ok"], false);
}
