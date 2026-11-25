//! Migration 0038 : `deploy_hooks.expires_at`. Appliquee apres 0039 et les
//! suivantes (l'ordre des numeros est libre) ; les hooks existants gardent
//! une expiration vide (ils n'expirent pas).

mod common;

use uuid::Uuid;

#[tokio::test]
async fn existing_hooks_do_not_expire() {
    let (db, _pg) = common::bare_database().await;
    let mut before = sqlx::migrate!("./migrations");
    before.migrations = before
        .migrations
        .iter()
        .filter(|m| m.version != 38)
        .cloned()
        .collect();
    before.run(&db).await.expect("migrations sans 0038");

    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name, source_kind, image_ref) VALUES ($1, 'p', 'image', 'img:1')")
        .bind(project)
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO deploy_hooks (id, project_id, name, token_hash) VALUES ($1, $2, 'h', $3)",
    )
    .bind(Uuid::new_v4())
    .bind(project)
    .bind(vec![9u8; 32])
    .execute(&db)
    .await
    .unwrap();

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration 0038 apres 0039 et suivantes");
    let unset: bool = sqlx::query_scalar("SELECT expires_at IS NULL FROM deploy_hooks")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(unset);
}
