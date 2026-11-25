mod common;

use common::api::{Req, call};
use common::{self_signed, serve_https, state, tls_get};
use control_plane::auth::password;
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const PUBLIC_PEER: &str = "203.0.113.7:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

#[tokio::test]
async fn health_ok() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    let r = tls_get(addr, &cert, "/api/health").await;
    assert_eq!(r.status, 200);
    assert!(
        r.header("content-type")
            .unwrap()
            .starts_with("application/json")
    );
    let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(v["status"], "ok");
    assert!(v["version"].is_string());
}

#[tokio::test]
async fn unknown_api_path_is_json_404_never_index() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    for path in ["/api/nope", "/api/projects/x/y", "/api"] {
        let r = tls_get(addr, &cert, path).await;
        assert_eq!(r.status, 404, "{path}");
        assert!(
            r.header("content-type")
                .unwrap()
                .starts_with("application/json")
        );
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["error"]["code"], "not_found");
    }
}

#[tokio::test]
async fn initial_migration_creates_the_tables() {
    let (state, _pg) = state().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY 1",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    for t in ["audit_events", "instance", "sessions", "users"] {
        assert!(
            tables.iter().any(|x| x == t),
            "table {t} absente : {tables:?}"
        );
    }
    // Le role est contraint.
    let bad = sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, email_verified_at) \
         VALUES (gen_random_uuid(), 'a@b.c', 'x', 'root', now())",
    )
    .execute(&state.db)
    .await;
    assert!(bad.is_err());
}

/// Base injoignable : 503 (l'updater ne declare jamais reussie une version
/// qui ne joint pas sa base).
#[tokio::test]
async fn health_is_503_without_the_database() {
    let (state, _pg) = state().await;
    state.db.close().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    let r = tls_get(addr, &cert, "/api/health").await;
    assert_eq!(r.status, 503);
    let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
    assert_eq!(v["status"], "database_unavailable");
    assert!(v["version"].is_string());
}

/// Pair public, ou pair local arrive par l'ecoute du proxy : la version reste cachee.
#[tokio::test]
async fn health_hides_version_publicly() {
    let (state, _pg) = state().await;
    for (peer, proxy) in [(PUBLIC_PEER, false), (PUBLIC_PEER, true), (LOOPBACK, true)] {
        let r = call(
            &state,
            "GET",
            "/api/health",
            Req {
                peer: Some(peer),
                proxy,
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 200, "{peer} proxy={proxy}");
        assert_eq!(r.json()["status"], "ok");
        assert!(
            r.json().get("version").is_none(),
            "{peer} proxy={proxy} : {}",
            r.raw
        );
    }
}

/// Boucle locale sur l'acces direct, ou session ouverte : la version est donnee.
#[tokio::test]
async fn health_version_local() {
    let (state, _pg) = state().await;
    let r = call(
        &state,
        "GET",
        "/api/health",
        Req {
            peer: Some(LOOPBACK),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.json()["version"], env!("CARGO_PKG_VERSION"));

    let email = format!("owner-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, 'owner', 'Test', now())",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    let login = call(
        &state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": email, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(login.status, 200, "{}", login.raw);
    let token = login.token();
    let r = call(
        &state,
        "GET",
        "/api/health",
        Req {
            peer: Some(PUBLIC_PEER),
            proxy: true,
            cookie: Some(&token),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.json()["version"], env!("CARGO_PKG_VERSION"));
}
