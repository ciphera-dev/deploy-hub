//! Migration 0036 : un secret ne vise plus « tous les projets ». Les lignes
//! existantes recoivent la liste explicite des projets, puis une contrainte
//! interdit la combinaison. Appliquee, apres coup, sur une base qui a deja
//! les migrations suivantes (0037 et plus) : l'ordre des numeros est libre.

mod common;

use uuid::Uuid;

async fn global(
    db: &sqlx::PgPool,
    key: &str,
    secret: bool,
    envs: &[&str],
    all: bool,
    age_days: i32,
) -> Uuid {
    let id = Uuid::new_v4();
    let (value, sealed): (Option<&str>, Option<Vec<u8>>) = if secret {
        (None, Some(vec![1, 2, 3]))
    } else {
        (Some("v"), None)
    };
    sqlx::query(
        "INSERT INTO global_variables \
           (id, key, secret, value, value_sealed, environments, all_projects, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, now() - make_interval(days => $8))",
    )
    .bind(id)
    .bind(key)
    .bind(secret)
    .bind(value)
    .bind(sealed)
    .bind(envs.iter().map(|e| e.to_string()).collect::<Vec<_>>())
    .bind(all)
    .bind(age_days)
    .execute(db)
    .await
    .unwrap();
    id
}

async fn scope(db: &sqlx::PgPool, id: Uuid) -> (bool, Vec<Uuid>) {
    let all: bool = sqlx::query_scalar("SELECT all_projects FROM global_variables WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap();
    let mut projects: Vec<Uuid> = sqlx::query_scalar(
        "SELECT project_id FROM global_variable_projects WHERE variable_id = $1",
    )
    .bind(id)
    .fetch_all(db)
    .await
    .unwrap();
    projects.sort();
    (all, projects)
}

#[tokio::test]
async fn migration_expands_secret_all_projects() {
    let (db, _pg) = common::bare_database().await;
    // Base deja migree sauf 0036 : 0037 et les suivantes sont appliquees.
    let mut before = sqlx::migrate!("./migrations");
    before.migrations = before
        .migrations
        .iter()
        .filter(|m| m.version != 36)
        .cloned()
        .collect();
    before.run(&db).await.expect("migrations sans 0036");

    let mut projects = Vec::new();
    for name in ["a", "b", "c"] {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, name, source_kind, image_ref) VALUES ($1, $2, 'image', 'img:1')",
        )
        .bind(id)
        .bind(name)
        .execute(&db)
        .await
        .unwrap();
        projects.push(id);
    }
    projects.sort();
    let (a, b, c) = (projects[0], projects[1], projects[2]);

    // Secret pour tous les projets, sans autre globale de la meme cle.
    let plain_secret = global(&db, "TOKEN", true, &["production"], true, 5).await;
    // Publique pour tous : inchangee.
    let public_all = global(&db, "REGION", false, &["production"], true, 5).await;
    // Secret deja explicite : inchange.
    let explicit = global(&db, "DB_URL", true, &["production"], false, 5).await;
    sqlx::query("INSERT INTO global_variable_projects VALUES ($1, $2)")
        .bind(explicit)
        .bind(a)
        .execute(&db)
        .await
        .unwrap();
    // Recoupement deja present : le projet `b` a sa propre globale, plus
    // precise, qui l'emportait deja sur le secret.
    let wide = global(&db, "API_KEY", true, &["production", "staging"], true, 5).await;
    let narrow = global(&db, "API_KEY", false, &["production"], false, 5).await;
    sqlx::query("INSERT INTO global_variable_projects VALUES ($1, $2)")
        .bind(narrow)
        .bind(b)
        .execute(&db)
        .await
        .unwrap();

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration 0036 apres 0037 et suivantes");

    assert_eq!(scope(&db, plain_secret).await, (false, vec![a, b, c]));
    assert_eq!(scope(&db, public_all).await, (true, vec![]));
    assert_eq!(scope(&db, explicit).await, (false, vec![a]));
    // `narrow` (1 environnement) l'emportait sur `wide` (2) pour `b` : le
    // secret garde les autres projets, sans nouveau recoupement.
    assert_eq!(scope(&db, wide).await, (false, vec![a, c]));
    assert_eq!(scope(&db, narrow).await, (false, vec![b]));

    // La contrainte interdit la combinaison ensuite.
    let again = sqlx::query("UPDATE global_variables SET all_projects = true WHERE id = $1")
        .bind(plain_secret)
        .execute(&db)
        .await;
    assert!(again.is_err(), "secret pour tous les projets accepte");
    // Une publique reste libre de viser tous les projets.
    sqlx::query("UPDATE global_variables SET all_projects = true WHERE id = $1")
        .bind(public_all)
        .execute(&db)
        .await
        .unwrap();
}
