//! Acceptation d'une invitation : le jeton est verifie avant tout calcul
//! Argon2. Seul test de ce binaire : le compteur global de calculs
//! (`password::offloaded`) n'y est jamais bouge par un autre test.

mod common;

use common::api::{Req, call};
use control_plane::auth::password;
use control_plane::state::AppState;
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

async fn accept(state: &AppState, token: &str, peer: &str) -> u16 {
    call(
        state,
        "POST",
        &format!("/api/auth/invitations/{token}/accept"),
        Req {
            peer: Some(peer),
            body: Some(
                json!({ "name": "Nora Kaci", "password": "une phrase de passe assez longue" }),
            ),
            ..Req::default()
        },
    )
    .await
    .status
}

#[tokio::test]
async fn invalid_token_no_argon2() {
    let (state, _pg) = common::state().await;
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) \
         VALUES ($1, 'owner@example.com', $2, 'owner', 'Owner', now())",
    )
    .bind(Uuid::new_v4())
    .bind(password::hash(PASSWORD).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    let login = call(
        &state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": "owner@example.com", "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(login.status, 200, "{}", login.raw);
    let invited = call(
        &state,
        "POST",
        "/api/team/invitations",
        Req {
            cookie: Some(&login.token()),
            csrf: Some(&login.csrf()),
            body: Some(json!({ "email": "nora@example.com", "role": "viewer" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(invited.status, 201, "{}", invited.raw);
    let link = invited.json()["link"].as_str().unwrap().to_string();
    let (_, token) = link.rsplit_once("/invite/").unwrap();

    // Jeton inconnu : refuse sans aucun calcul Argon2.
    let before = password::offloaded();
    assert_eq!(
        accept(&state, &"A".repeat(43), "198.51.100.1:1000").await,
        404
    );
    assert_eq!(password::offloaded(), before);

    // Jeton expire : refuse sans calcul non plus.
    sqlx::query("UPDATE invitations SET expires_at = now() - interval '1 minute'")
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(accept(&state, token, "198.51.100.2:1000").await, 410);
    assert_eq!(password::offloaded(), before);

    // Jeton valable : un calcul (hachage du mot de passe), compte cree.
    sqlx::query("UPDATE invitations SET expires_at = now() + interval '1 day'")
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(accept(&state, token, "198.51.100.3:1000").await, 204);
    assert_eq!(password::offloaded(), before + 1);
}
