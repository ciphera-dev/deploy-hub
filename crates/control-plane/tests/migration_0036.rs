//! Migration 0036 : un secret ne vise plus « tous les projets ». Les lignes
//! existantes recoivent la liste explicite des projets, puis une contrainte
//! interdit la combinaison. Appliquee, apres coup, sur une base qui a deja
//! les migrations suivantes (0037 et plus) : l'ordre des numeros est libre.
//!
//! La valeur effective de chaque (projet, environnement), lue par le meme
//! resolveur que le panneau (`variables::list_project`), ne change pas.

mod common;

use std::collections::BTreeMap;

use control_plane::domain::variables;
use sqlx::PgPool;
use uuid::Uuid;

const ENVS: [&str; 2] = ["production", "staging"];

/// Insere une globale ; `scope` vide de sens (`None`) = tous les projets.
async fn global(
    db: &PgPool,
    key: &str,
    secret: bool,
    envs: &[&str],
    scope: Option<&[Uuid]>,
    age_days: i32,
) -> Uuid {
    let id = Uuid::new_v4();
    let (value, sealed): (Option<String>, Option<Vec<u8>>) = if secret {
        (None, Some(vec![1, 2, 3]))
    } else {
        (Some(format!("public-{key}-{}", envs.join("+"))), None)
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
    .bind(scope.is_none())
    .bind(age_days)
    .execute(db)
    .await
    .unwrap();
    for p in scope.unwrap_or(&[]) {
        sqlx::query("INSERT INTO global_variable_projects VALUES ($1, $2)")
            .bind(id)
            .bind(p)
            .execute(db)
            .await
            .unwrap();
    }
    id
}

async fn scope(db: &PgPool, id: Uuid) -> (bool, Vec<Uuid>) {
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

/// `n` projets, tries par identifiant, chacun avec ses environnements.
async fn projects(db: &PgPool, n: usize) -> Vec<Uuid> {
    let mut out = Vec::new();
    for i in 0..n {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO projects (id, name, source_kind, image_ref) VALUES ($1, $2, 'image', 'img:1')",
        )
        .bind(id)
        .bind(format!("p{i}"))
        .execute(db)
        .await
        .unwrap();
        for env in ENVS {
            sqlx::query("INSERT INTO environments (id, project_id, name) VALUES ($1, $2, $3)")
                .bind(Uuid::new_v4())
                .bind(id)
                .bind(env)
                .execute(db)
                .await
                .unwrap();
        }
        out.push(id);
    }
    out.sort();
    out
}

/// Valeur effective de chaque (projet, environnement) : cle, secret, valeur
/// publique (un secret n'a pas de valeur lisible), lue par le resolveur du
/// panneau.
type Effective = BTreeMap<(Uuid, String), BTreeMap<String, (bool, Option<String>)>>;

async fn effective(db: &PgPool, projects: &[Uuid]) -> Effective {
    let mut conn = db.acquire().await.unwrap();
    let mut out = BTreeMap::new();
    for p in projects {
        for env in ENVS {
            let id = variables::environment(&mut conn, *p, env)
                .await
                .unwrap()
                .unwrap();
            let rows = variables::list_project(&mut conn, *p, id, env)
                .await
                .unwrap();
            out.insert(
                (*p, env.to_string()),
                rows.into_iter()
                    .map(|r| (r.key, (r.secret, r.value)))
                    .collect(),
            );
        }
    }
    out
}

/// Base migree sauf 0036 : 0037 et les suivantes sont deja appliquees.
async fn before_0036() -> (PgPool, common::TestDb) {
    let (db, pg) = common::bare_database().await;
    let mut before = sqlx::migrate!("./migrations");
    before.migrations = before
        .migrations
        .iter()
        .filter(|m| m.version != 36)
        .cloned()
        .collect();
    before.run(&db).await.expect("migrations sans 0036");
    (db, pg)
}

fn without(all: &[Uuid], gone: Uuid) -> Vec<Uuid> {
    all.iter().copied().filter(|x| *x != gone).collect()
}

#[tokio::test]
async fn migration_expands_secret_all_projects() {
    let (db, _pg) = before_0036().await;
    let p = projects(&db, 4).await;

    // Plusieurs projets, aucune autre globale : tous les projets, partout.
    let plain = global(&db, "KEY_C", true, &["production", "staging"], None, 5).await;
    // Recoupement partiel : la publique ne couvre que `staging` de p[1], le
    // secret reste seul fournisseur de `production` pour p[1] : p[1] reste.
    let partial = global(&db, "KEY_A", true, &["production", "staging"], None, 5).await;
    global(&db, "KEY_A", false, &["staging"], Some(&[p[1]]), 5).await;
    // Ombre totale : chaque environnement du secret est couvert, par deux
    // globales plus precises, pour p[2] : p[2] peut etre retire.
    let shadow = global(&db, "KEY_D", true, &["production", "staging"], None, 5).await;
    global(&db, "KEY_D", false, &["production"], Some(&[p[2]]), 5).await;
    global(&db, "KEY_D", false, &["staging"], Some(&[p[2]]), 5).await;
    // Ombre totale par une seule globale de memes environnements.
    let same = global(&db, "KEY_B", true, &["production"], None, 5).await;
    global(&db, "KEY_B", false, &["production"], Some(&[p[3]]), 5).await;
    // Publique pour tous et secret deja explicite : inchanges.
    let public_all = global(&db, "REGION", false, &["production"], None, 5).await;
    let explicit = global(&db, "DB_URL", true, &["production"], Some(&[p[0]]), 5).await;

    let before = effective(&db, &p).await;
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration 0036 apres 0037 et suivantes");
    let after = effective(&db, &p).await;
    assert_eq!(before, after, "valeurs effectives changees");

    assert_eq!(scope(&db, plain).await, (false, p.clone()));
    assert_eq!(scope(&db, partial).await, (false, p.clone()));
    assert_eq!(scope(&db, shadow).await, (false, without(&p, p[2])));
    assert_eq!(scope(&db, same).await, (false, without(&p, p[3])));
    assert_eq!(scope(&db, public_all).await, (true, vec![]));
    assert_eq!(scope(&db, explicit).await, (false, vec![p[0]]));

    // La contrainte interdit la combinaison ensuite.
    let again = sqlx::query("UPDATE global_variables SET all_projects = true WHERE id = $1")
        .bind(plain)
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

#[tokio::test]
async fn migration_without_projects_leaves_empty_scopes() {
    let (db, _pg) = before_0036().await;
    let secret = global(&db, "TOKEN", true, &["production"], None, 5).await;
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("migration 0036 sans projet");
    assert_eq!(scope(&db, secret).await, (false, vec![]));
}
