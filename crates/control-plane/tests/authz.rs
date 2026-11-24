mod common;

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use axum::routing::post;
use common::api::{Req, call};
use control_plane::auth::password;
use control_plane::authz::{self, Level, Require, Resource, Role, lvl, res};
use control_plane::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

/// Matrice par defaut de `prototypes/dashboard/Team.jsx` (`ROLES[].perms`,
/// 0 = Aucun, 1 = Lecture, 2 = Ecriture, 3 = Admin), dans l'ordre :
/// projects, deploy, servers, domains, vars, security, logs, team, settings.
/// Les colonnes `db` et `billing` de la maquette ne sont pas des ressources
/// de la v0.1. `settings` n'est pas une colonne de la maquette : Admin pour
/// owner et admin (`full(3)` hors facturation), Aucun pour les autres.
const TEAM_JSX: [(Role, [u8; 9]); 5] = [
    (Role::Owner, [3, 3, 3, 3, 3, 3, 3, 3, 3]),
    (Role::Admin, [3, 3, 3, 3, 3, 3, 3, 3, 3]),
    (Role::Developer, [2, 2, 1, 2, 2, 0, 1, 0, 0]),
    (Role::Ops, [1, 2, 3, 2, 1, 3, 1, 0, 0]),
    (Role::Viewer, [1, 1, 1, 1, 0, 1, 1, 0, 0]),
];

const RESOURCES: [Resource; 9] = [
    Resource::Projects,
    Resource::Deployments,
    Resource::Servers,
    Resource::Domains,
    Resource::Variables,
    Resource::Security,
    Resource::Logs,
    Resource::Team,
    Resource::Settings,
];

const LEVELS: [Level; 4] = [Level::None, Level::Read, Level::Write, Level::Admin];

#[test]
fn matrix_matches_team_jsx() {
    for (role, row) in TEAM_JSX {
        for (resource, granted) in RESOURCES.into_iter().zip(row) {
            for (n, level) in LEVELS.into_iter().enumerate() {
                assert_eq!(
                    authz::allowed(role, resource, level),
                    n as u8 <= granted,
                    "{role:?} {resource:?} {level:?}"
                );
            }
        }
    }
}

#[test]
fn roles_round_trip_and_grant_order() {
    for role in [
        Role::Owner,
        Role::Admin,
        Role::Ops,
        Role::Developer,
        Role::Viewer,
    ] {
        assert_eq!(Role::parse(role.as_str()), Some(role));
        // `can_grant` (invitations) autorise le meme rang ; la gestion d'un
        // membre (`can_manage`, liens de reinitialisation) est testee a part.
        assert!(role.can_grant(role), "{role:?}");
        assert!(role.can_grant(Role::Viewer), "{role:?}");
    }
    assert_eq!(Role::parse("dev"), None);
    assert_eq!(Role::parse("Owner"), None);
    // Owner > Admin > Ops | Developer > Viewer
    assert!(Role::Owner.can_grant(Role::Admin));
    assert!(!Role::Admin.can_grant(Role::Owner));
    assert!(Role::Admin.can_grant(Role::Ops));
    assert!(Role::Admin.can_grant(Role::Developer));
    assert!(!Role::Ops.can_grant(Role::Developer));
    assert!(!Role::Developer.can_grant(Role::Ops));
    assert!(!Role::Viewer.can_grant(Role::Developer));
}

#[test]
fn can_manage_needs_a_strictly_lower_rank_below_owner() {
    assert!(Role::Owner.can_manage(Role::Admin));
    assert!(!Role::Admin.can_manage(Role::Admin), "pas un autre admin");
    assert!(!Role::Admin.can_manage(Role::Owner));
    for target in [Role::Ops, Role::Developer, Role::Viewer] {
        assert!(Role::Admin.can_manage(target), "{target:?}");
    }
    for actor in [Role::Ops, Role::Developer, Role::Viewer] {
        for target in [Role::Viewer, Role::Developer, Role::Ops, actor] {
            assert!(!actor.can_manage(target), "{actor:?} {target:?}");
        }
    }
}

async fn user(state: &AppState, email: &str, role: &str) {
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, 'Test', now())",
    )
    .bind(Uuid::new_v4())
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
}

async fn login(state: &AppState, email: &str) -> String {
    let r = call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": email, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    r.token()
}

async fn write_project(_: Require<res::Projects, lvl::Write>) -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn read_project(_: Require<res::Projects, lvl::Read>) -> StatusCode {
    StatusCode::NO_CONTENT
}

/// Routeur de test : deux routes gardees par l'extracteur `Require`.
async fn probe(state: &AppState, path: &str, token: Option<&str>) -> (u16, Value) {
    let router = Router::new()
        .route("/probe/write", post(write_project))
        .route("/probe/read", post(read_project))
        .with_state(state.clone());
    let mut req = Request::builder().method("POST").uri(path);
    if let Some(t) = token {
        req = req.header(header::COOKIE, format!("__Host-ciphera_session={t}"));
    }
    let mut req = req.body(Body::empty()).unwrap();
    let peer: SocketAddr = common::api::DIRECT.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let res = router.oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn viewer_cannot_write_projects_403() {
    let (state, _pg) = common::state().await;
    user(&state, "viewer@example.com", "viewer").await;
    user(&state, "dev@example.com", "developer").await;
    let viewer = login(&state, "viewer@example.com").await;
    let dev = login(&state, "dev@example.com").await;

    let (status, body) = probe(&state, "/probe/write", Some(&viewer)).await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "forbidden");
    assert_eq!(probe(&state, "/probe/read", Some(&viewer)).await.0, 204);
    assert_eq!(probe(&state, "/probe/write", Some(&dev)).await.0, 204);

    let (status, body) = probe(&state, "/probe/write", None).await;
    assert_eq!(status, 401);
    assert_eq!(body["error"]["code"], "unauthenticated");

    // Un role inconnu en base ne donne aucun droit.
    sqlx::query("ALTER TABLE users DROP CONSTRAINT users_role_check")
        .execute(&state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET role = 'superuser' WHERE email = 'dev@example.com'")
        .execute(&state.db)
        .await
        .unwrap();
    assert_eq!(probe(&state, "/probe/read", Some(&dev)).await.0, 403);
}
