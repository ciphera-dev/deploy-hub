//! Secrets lies a leur ligne : mot de passe SMTP, GitHub App, graines TOTP.
//! Rechiffrement au demarrage des anciens chiffres (`variables::rebind_all`).

mod common;

use control_plane::domain::variables;
use control_plane::mail;
use control_plane::secrets::labels;
use control_plane::state::AppState;
use sqlx::PgPool;
use uuid::Uuid;

const SMTP_PASSWORD: &str = "fixture-smtp-password";

async fn insert_smtp(db: &PgPool, sealed: Vec<u8>) {
    sqlx::query(
        "INSERT INTO smtp_settings (id, enabled, host, port, tls, username, password_sealed, \
                                    from_address, from_name) \
         VALUES (true, true, 'smtp.example.com', 587, 'starttls', 'mailer', $1, \
                 'ciphera@example.com', 'Ciphera') \
         ON CONFLICT (id) DO UPDATE SET password_sealed = EXCLUDED.password_sealed",
    )
    .bind(sealed)
    .execute(db)
    .await
    .unwrap();
}

async fn insert_github(db: &PgPool, key: Vec<u8>, hook: Vec<u8>) {
    sqlx::query(
        "INSERT INTO github_app (id, app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) \
         VALUES (true, 7, 'ciphera-x', 'Ciphera X', 'acme', 'https://github.com/apps/ciphera-x', $1, $2)",
    )
    .bind(key)
    .bind(hook)
    .execute(db)
    .await
    .unwrap();
}

async fn insert_user(db: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) \
         VALUES ($1, $2, 'x', 'owner', 'Test')",
    )
    .bind(id)
    .bind(format!("u-{}@example.com", id.simple()))
    .execute(db)
    .await
    .unwrap();
    id
}

async fn insert_totp(db: &PgPool, user: Uuid, sealed: Vec<u8>) {
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, $2, now())",
    )
    .bind(user)
    .bind(sealed)
    .execute(db)
    .await
    .unwrap();
}

async fn smtp_loads(state: &AppState) -> bool {
    mail::load_settings(&state.db, &state.secrets).await.is_ok()
}

/// Un chiffre de variable copie dans le mot de passe SMTP est refuse.
#[tokio::test]
async fn variable_ciphertext_in_smtp_fails_load_settings() {
    let (state, _pg) = common::state().await;
    let variable = state
        .secrets
        .seal_bound(b"project|env-1|STRIPE_KEY", SMTP_PASSWORD.as_bytes())
        .unwrap();
    insert_smtp(&state.db, variable).await;
    assert!(!smtp_loads(&state).await);
    let own = state
        .secrets
        .seal_bound(labels::SMTP_PASSWORD, SMTP_PASSWORD.as_bytes())
        .unwrap();
    insert_smtp(&state.db, own).await;
    let settings = mail::load_settings(&state.db, &state.secrets)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        settings.password.as_deref().map(String::as_str),
        Some(SMTP_PASSWORD)
    );
}

/// Un chiffre lie a une autre ligne n'est pas « adopte » par le
/// rechiffrement : il reste illisible.
#[tokio::test]
async fn rebind_refuses_bound_plaintext() {
    let (state, _pg) = common::state().await;
    let foreign = |label: &[u8]| state.secrets.seal_bound(label, b"valeur").unwrap();
    insert_smtp(&state.db, foreign(b"project|env-1|KEY")).await;
    insert_github(
        &state.db,
        foreign(b"project|env-1|KEY"),
        foreign(b"project|env-2|KEY"),
    )
    .await;
    let user = insert_user(&state.db).await;
    insert_totp(&state.db, user, foreign(b"project|env-3|KEY")).await;

    assert_eq!(
        variables::rebind_all(&state.db, &state.secrets)
            .await
            .unwrap(),
        0
    );
    assert!(!smtp_loads(&state).await);
    let (key, hook): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT private_key_sealed, webhook_secret_sealed FROM github_app")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(
        state
            .secrets
            .open_bound(labels::GITHUB_PRIVATE_KEY, &key)
            .is_err()
    );
    assert!(
        state
            .secrets
            .open_bound(labels::GITHUB_WEBHOOK_SECRET, &hook)
            .is_err()
    );
    let seed: Vec<u8> = sqlx::query_scalar("SELECT secret_sealed FROM user_totp")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert!(
        state
            .secrets
            .open_bound(&labels::totp_seed(user), &seed)
            .is_err()
    );
}

/// Anciens chiffres (age seul) rechiffres une fois ; ensuite un chiffre non
/// lie introduit reste illisible.
#[tokio::test]
async fn rebind_migrates_smtp_github_totp_once() {
    let (state, _pg) = common::state().await;
    let s = &state.secrets;
    insert_smtp(&state.db, s.seal(SMTP_PASSWORD.as_bytes()).unwrap()).await;
    insert_github(
        &state.db,
        s.seal(b"cle-privee").unwrap(),
        s.seal(b"secret-webhook").unwrap(),
    )
    .await;
    let user = insert_user(&state.db).await;
    insert_totp(&state.db, user, s.seal(b"GRAINETOTP").unwrap()).await;
    // Graine en attente d'enrolement : meme etiquette.
    let pending = insert_user(&state.db).await;
    sqlx::query("INSERT INTO user_totp (user_id, secret_sealed) VALUES ($1, $2)")
        .bind(pending)
        .bind(s.seal(b"ATTENTE").unwrap())
        .execute(&state.db)
        .await
        .unwrap();

    assert_eq!(variables::rebind_all(&state.db, s).await.unwrap(), 5);
    let settings = mail::load_settings(&state.db, s).await.unwrap().unwrap();
    assert_eq!(
        settings.password.as_deref().map(String::as_str),
        Some(SMTP_PASSWORD)
    );
    let (key, hook): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT private_key_sealed, webhook_secret_sealed FROM github_app")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(
        s.open_bound(labels::GITHUB_PRIVATE_KEY, &key)
            .unwrap()
            .as_slice(),
        b"cle-privee"
    );
    assert_eq!(
        s.open_bound(labels::GITHUB_WEBHOOK_SECRET, &hook)
            .unwrap()
            .as_slice(),
        b"secret-webhook"
    );
    for (u, clear) in [(user, &b"GRAINETOTP"[..]), (pending, &b"ATTENTE"[..])] {
        let seed: Vec<u8> =
            sqlx::query_scalar("SELECT secret_sealed FROM user_totp WHERE user_id = $1")
                .bind(u)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(
            s.open_bound(&labels::totp_seed(u), &seed)
                .unwrap()
                .as_slice(),
            clear
        );
    }
    let markers: i64 = sqlx::query_scalar("SELECT count(*) FROM secret_rebinds")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(markers, 4);

    // Deuxieme passage : rien. Un chiffre non lie introduit ensuite reste refuse.
    assert_eq!(variables::rebind_all(&state.db, s).await.unwrap(), 0);
    insert_smtp(&state.db, s.seal(SMTP_PASSWORD.as_bytes()).unwrap()).await;
    assert_eq!(variables::rebind_all(&state.db, s).await.unwrap(), 0);
    assert!(!smtp_loads(&state).await);
}

/// Cle privee et secret de webhook echanges entre eux : illisibles.
#[tokio::test]
async fn github_secret_swap_fails() {
    let (state, _pg) = common::state().await;
    let s = &state.secrets;
    insert_github(
        &state.db,
        s.seal_bound(labels::GITHUB_PRIVATE_KEY, b"cle-privee")
            .unwrap(),
        s.seal_bound(labels::GITHUB_WEBHOOK_SECRET, b"secret-webhook")
            .unwrap(),
    )
    .await;
    sqlx::query(
        "UPDATE github_app SET private_key_sealed = webhook_secret_sealed, \
         webhook_secret_sealed = private_key_sealed",
    )
    .execute(&state.db)
    .await
    .unwrap();
    let (key, hook): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT private_key_sealed, webhook_secret_sealed FROM github_app")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(s.open_bound(labels::GITHUB_PRIVATE_KEY, &key).is_err());
    assert!(s.open_bound(labels::GITHUB_WEBHOOK_SECRET, &hook).is_err());
}
