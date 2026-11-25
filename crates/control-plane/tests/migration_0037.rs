//! Migration 0037 : purge des doublons, detachement des orphelines, puis
//! contraintes `deploy_hooks.token_hash` unique et `mfa_attempts.session_id`
//! lie aux sessions. Appliquee sur une base qui contient des donnees factices.

mod common;

use uuid::Uuid;

#[tokio::test]
async fn duplicates_and_orphans_are_purged_then_constraints_hold() {
    let (db, _pg) = common::bare_database().await;
    // Schema d'avant 0037 : les migrations precedentes seulement.
    let mut before = sqlx::migrate!("./migrations");
    before.migrations = before
        .migrations
        .iter()
        .filter(|m| m.version < 37)
        .cloned()
        .collect();
    before.run(&db).await.expect("migrations jusqu'a 0036");

    let user = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) \
         VALUES ($1, 'm@example.com', 'x', 'owner', 'M', now())",
    )
    .bind(user)
    .execute(&db)
    .await
    .unwrap();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO projects (id, name, source_kind, image_ref) VALUES ($1, 'p', 'image', 'img:1')")
        .bind(project)
        .execute(&db)
        .await
        .unwrap();
    // Trois hooks : deux au meme hash (le plus ancien reste), un distinct.
    let (old, young, other) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    for (id, hash, age) in [(old, 1u8, 3), (young, 1, 1), (other, 2, 2)] {
        sqlx::query(
            "INSERT INTO deploy_hooks (id, project_id, name, token_hash, created_at) \
             VALUES ($1, $2, 'h', $3, now() - make_interval(days => $4))",
        )
        .bind(id)
        .bind(project)
        .bind(vec![hash; 32])
        .bind(age)
        .execute(&db)
        .await
        .unwrap();
    }
    // Tentatives : une session reelle, une orpheline.
    let session = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at) \
         VALUES ($1, $2, $3, now() + interval '1 day')",
    )
    .bind(session)
    .bind(user)
    .bind(vec![9u8; 32])
    .execute(&db)
    .await
    .unwrap();
    for s in [session, Uuid::new_v4()] {
        sqlx::query(
            "INSERT INTO mfa_attempts (user_id, session_id, succeeded) VALUES ($1, $2, false)",
        )
        .bind(user)
        .bind(s)
        .execute(&db)
        .await
        .unwrap();
    }

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration 0037");

    let hooks: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM deploy_hooks ORDER BY created_at")
        .fetch_all(&db)
        .await
        .unwrap();
    assert_eq!(hooks, [old, other]);
    // Orpheline detachee, tentative de la session reelle gardee.
    let attached: Vec<Option<Uuid>> =
        sqlx::query_scalar("SELECT session_id FROM mfa_attempts ORDER BY id")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(attached, [Some(session), None]);

    // Contraintes en place.
    let dup = sqlx::query(
        "INSERT INTO deploy_hooks (id, project_id, name, token_hash) VALUES ($1, $2, 'h', $3)",
    )
    .bind(Uuid::new_v4())
    .bind(project)
    .bind(vec![2u8; 32])
    .execute(&db)
    .await;
    assert!(dup.is_err(), "hash en double accepte");
    let orphan = sqlx::query(
        "INSERT INTO mfa_attempts (user_id, session_id, succeeded) VALUES ($1, $2, false)",
    )
    .bind(user)
    .bind(Uuid::new_v4())
    .execute(&db)
    .await;
    assert!(orphan.is_err(), "tentative orpheline acceptee");
    // La session partie detache ses tentatives sans les effacer : le plafond
    // journalier du compte les compte encore.
    sqlx::query("DELETE FROM sessions WHERE id = $1")
        .bind(session)
        .execute(&db)
        .await
        .unwrap();
    let left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mfa_attempts WHERE session_id IS NULL")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(left, 2);
}
