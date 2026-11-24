mod common;

use common::{self_signed, serve_https, state, tls_get};

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
        "INSERT INTO users (id, email, password_hash, role) \
         VALUES (gen_random_uuid(), 'a@b.c', 'x', 'root')",
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
