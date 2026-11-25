//! Migration 0043 : option HSTS `instance.hsts_include_subdomains`,
//! desactivee par defaut, y compris sur une instance deja configuree.

mod common;

use uuid::Uuid;

#[tokio::test]
async fn hsts_include_subdomains_defaults_to_false_on_an_existing_instance() {
    let (db, _pg) = common::bare_database().await;
    let mut before = sqlx::migrate!("./migrations");
    before.migrations = before
        .migrations
        .iter()
        .filter(|m| m.version < 43)
        .cloned()
        .collect();
    before.run(&db).await.expect("migrations jusqu'a 0042");
    sqlx::query("INSERT INTO instance (id, name) VALUES ($1, 'Org')")
        .bind(Uuid::new_v4())
        .execute(&db)
        .await
        .unwrap();

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration 0043");
    let on: bool = sqlx::query_scalar("SELECT hsts_include_subdomains FROM instance")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(!on);
    // NOT NULL : jamais d'etat indetermine.
    let refused = sqlx::query("UPDATE instance SET hsts_include_subdomains = NULL")
        .execute(&db)
        .await;
    assert!(refused.is_err());
}
