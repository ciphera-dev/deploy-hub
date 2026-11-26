//! Equipe : membres, roles, retrait, invitations en attente.

mod common;

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::state::AppState;
use serde_json::{Value, json};
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

/// Cree un compte et renvoie son identifiant.
async fn user(state: &AppState, email: &str, role: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, $5, now())",
    )
    .bind(id)
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .bind(format!("Nom {role}"))
    .execute(&state.db)
    .await
    .unwrap();
    id
}

/// (jeton de session, jeton CSRF)
async fn session(state: &AppState, email: &str) -> (String, String) {
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
    (r.token(), r.csrf())
}

async fn send(
    state: &AppState,
    by: &(String, String),
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&by.0),
            csrf: Some(&by.1),
            body,
            ..Req::default()
        },
    )
    .await
}

async fn set_role(state: &AppState, by: &(String, String), id: Uuid, role: &str) -> Reply {
    send(
        state,
        by,
        "PATCH",
        &format!("/api/team/members/{id}"),
        Some(json!({ "role": role, "password": PASSWORD })),
    )
    .await
}

async fn remove(state: &AppState, by: &(String, String), id: Uuid) -> Reply {
    send(
        state,
        by,
        "DELETE",
        &format!("/api/team/members/{id}"),
        Some(json!({ "password": PASSWORD })),
    )
    .await
}

async fn role_of(state: &AppState, id: Uuid) -> Option<String> {
    sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .unwrap()
}

async fn audit_count(state: &AppState, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = $1")
        .bind(action)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn list_members_requires_team_admin() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "dev@example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;
    let dev = session(&state, "dev@example.com").await;

    let r = send(&state, &dev, "GET", "/api/team/members", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let r = send(&state, &dev, "GET", "/api/team/invitations", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);

    let r = send(&state, &owner, "GET", "/api/team/members", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let list = r.json();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2);
    let me = list.iter().find(|m| m["role"] == "owner").unwrap();
    assert_eq!(me["email"], "owner@example.com");
    assert_eq!(me["name"], "Nom owner");
    assert_eq!(me["two_factor"], false);
    assert!(me["last_active_at"].as_i64().unwrap() > 0);
    assert!(me["created_at"].as_i64().unwrap() > 0);
    // Jamais connecte : pas de derniere activite.
    user(&state, "never@example.com", "viewer").await;
    let r = send(&state, &owner, "GET", "/api/team/members", None).await;
    let never = r
        .json()
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == "never@example.com")
        .unwrap()
        .clone();
    assert!(never["last_active_at"].is_null());
}

#[tokio::test]
async fn list_members_reports_two_factor() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let with_totp = user(&state, "totp@example.com", "viewer").await;
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, '\\x00', now())",
    )
    .bind(with_totp)
    .execute(&state.db)
    .await
    .unwrap();
    let pending = user(&state, "pending@example.com", "viewer").await;
    sqlx::query("INSERT INTO user_totp (user_id, secret_sealed) VALUES ($1, '\\x00')")
        .bind(pending)
        .execute(&state.db)
        .await
        .unwrap();
    let owner = session(&state, "owner@example.com").await;
    let r = send(&state, &owner, "GET", "/api/team/members", None).await;
    let list = r.json();
    let flag = |email: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|m| m["email"] == email)
            .unwrap()["two_factor"]
            .clone()
    };
    assert_eq!(flag("totp@example.com"), true);
    assert_eq!(flag("pending@example.com"), false);
    assert_eq!(flag("owner@example.com"), false);
}

#[tokio::test]
async fn admin_cannot_change_owner() {
    let (state, _pg) = common::state().await;
    let owner = user(&state, "owner@example.com", "owner").await;
    user(&state, "admin@example.com", "admin").await;
    let admin = session(&state, "admin@example.com").await;

    let r = set_role(&state, &admin, owner, "viewer").await;
    assert_eq!((r.status, r.code()), (403, json!("forbidden")), "{}", r.raw);
    let r = remove(&state, &admin, owner).await;
    assert_eq!((r.status, r.code()), (403, json!("forbidden")), "{}", r.raw);
    assert_eq!(role_of(&state, owner).await.as_deref(), Some("owner"));
}

#[tokio::test]
async fn admin_cannot_manage_another_admin() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "admin@example.com", "admin").await;
    let other = user(&state, "admin2@example.com", "admin").await;
    let admin = session(&state, "admin@example.com").await;
    assert_eq!(set_role(&state, &admin, other, "viewer").await.status, 403);
    assert_eq!(remove(&state, &admin, other).await.status, 403);
    assert_eq!(role_of(&state, other).await.as_deref(), Some("admin"));
}

#[tokio::test]
async fn admin_cannot_promote_to_owner() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "admin@example.com", "admin").await;
    let dev = user(&state, "dev@example.com", "developer").await;
    let admin = session(&state, "admin@example.com").await;

    let r = set_role(&state, &admin, dev, "owner").await;
    assert_eq!((r.status, r.code()), (403, json!("forbidden")), "{}", r.raw);
    // Le refus ne change rien ; en revanche un admin peut accorder un role de
    // son niveau ou en dessous (ci-dessous : ops a un developer).
    assert_eq!(role_of(&state, dev).await.as_deref(), Some("developer"));
    let r = set_role(&state, &admin, dev, "ops").await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(role_of(&state, dev).await.as_deref(), Some("ops"));
}

#[tokio::test]
async fn owner_can_demote_admin() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;

    let r = set_role(&state, &owner, admin_id, "viewer").await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(role_of(&state, admin_id).await.as_deref(), Some("viewer"));
    assert_eq!(audit_count(&state, "team.member.role_changed").await, 1);
    let detail: Value = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE action = 'team.member.role_changed'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(
        (&detail["from"], &detail["to"]),
        (&json!("admin"), &json!("viewer"))
    );
    assert!(!detail.to_string().contains("admin@example.com"));

    // Role inconnu : 422.
    let r = set_role(&state, &owner, admin_id, "root").await;
    assert_eq!(r.status, 422, "{}", r.raw);
}

#[tokio::test]
async fn cannot_manage_self() {
    let (state, _pg) = common::state().await;
    let owner_id = user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;
    let admin = session(&state, "admin@example.com").await;

    for (by, id) in [(&owner, owner_id), (&admin, admin_id)] {
        let r = set_role(&state, by, id, "viewer").await;
        assert_eq!(
            (r.status, r.code()),
            (403, json!("self_forbidden")),
            "{}",
            r.raw
        );
        let r = remove(&state, by, id).await;
        assert_eq!(
            (r.status, r.code()),
            (403, json!("self_forbidden")),
            "{}",
            r.raw
        );
    }
    assert_eq!(role_of(&state, owner_id).await.as_deref(), Some("owner"));
    assert_eq!(role_of(&state, admin_id).await.as_deref(), Some("admin"));
}

#[tokio::test]
async fn last_owner_cannot_be_demoted_or_removed() {
    let (state, _pg) = common::state().await;
    let a = user(&state, "a@example.com", "owner").await;
    let b = user(&state, "b@example.com", "owner").await;
    let sa = session(&state, "a@example.com").await;
    let sb = session(&state, "b@example.com").await;

    // Deux proprietaires : A retrograde B puis le retire... ici A retire B.
    let r = remove(&state, &sa, b).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(role_of(&state, b).await, None);

    // B n'existe plus : sa session ne sert plus a rien.
    let r = remove(&state, &sb, a).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(role_of(&state, a).await.as_deref(), Some("owner"));

    // A seul : rien de possible sur lui-meme.
    let r = set_role(&state, &sa, a, "admin").await;
    assert_eq!((r.status, r.code()), (403, json!("self_forbidden")));
    let r = remove(&state, &sa, a).await;
    assert_eq!((r.status, r.code()), (403, json!("self_forbidden")));
    assert_eq!(role_of(&state, a).await.as_deref(), Some("owner"));
}

/// Deux proprietaires qui se retirent en meme temps : un seul reste.
#[tokio::test]
async fn simultaneous_owner_removals_keep_one_owner() {
    let (state, _pg) = common::state().await;
    let a = user(&state, "a@example.com", "owner").await;
    let b = user(&state, "b@example.com", "owner").await;
    let sa = session(&state, "a@example.com").await;
    let sb = session(&state, "b@example.com").await;
    let (r1, r2) = tokio::join!(remove(&state, &sa, b), remove(&state, &sb, a));
    let statuses = [r1.status, r2.status];
    assert!(statuses.contains(&200), "{statuses:?}");
    let owners: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE role = 'owner'")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(owners, 1, "{statuses:?}");
}

#[tokio::test]
async fn removed_member_loses_sessions_hooks_and_is_audited() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let dev = user(&state, "dev@example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;
    let dev_session = session(&state, "dev@example.com").await;
    let r = send(&state, &dev_session, "GET", "/api/me", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);

    let r = remove(&state, &owner, dev).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["deleted_hooks"], 0);
    assert_eq!(role_of(&state, dev).await, None);
    let r = send(&state, &dev_session, "GET", "/api/me", None).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(dev)
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    assert_eq!(audit_count(&state, "team.member.removed").await, 1);
    // Inconnu : 404.
    let r = remove(&state, &owner, dev).await;
    assert_eq!(r.status, 404, "{}", r.raw);
}

#[tokio::test]
async fn removed_member_hooks_deleted() {
    let (state, _pg) = common::state().await;
    let owner_id = user(&state, "owner@example.com", "owner").await;
    let dev = user(&state, "dev@example.com", "developer").await;
    let owner = session(&state, "owner@example.com").await;
    let project = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO projects (id, name, source_kind, image_ref) VALUES ($1, 'web', 'image', 'nginx:1')",
    )
    .bind(project)
    .execute(&state.db)
    .await
    .unwrap();
    for (i, (name, by)) in [("a", dev), ("b", dev), ("c", owner_id)]
        .into_iter()
        .enumerate()
    {
        // token_hash est UNIQUE (migration 0037) : une empreinte distincte par hook.
        sqlx::query(
            "INSERT INTO deploy_hooks (id, project_id, name, token_hash, created_by) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(project)
        .bind(name)
        .bind(vec![i as u8 + 7; 32])
        .bind(by)
        .execute(&state.db)
        .await
        .unwrap();
    }
    let r = remove(&state, &owner, dev).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["deleted_hooks"], 2);
    let left: Vec<String> = sqlx::query_scalar("SELECT name FROM deploy_hooks")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(left, vec!["c".to_string()]);
    let detail: Value =
        sqlx::query_scalar("SELECT detail FROM audit_events WHERE action = 'team.member.removed'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(detail["deleted_hooks"], 2);
}

#[tokio::test]
async fn role_change_takes_effect_on_next_request() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;
    let admin = session(&state, "admin@example.com").await;
    let r = send(&state, &admin, "GET", "/api/team/members", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);

    let r = set_role(&state, &owner, admin_id, "developer").await;
    assert_eq!(r.status, 204, "{}", r.raw);
    // Meme session, meme cookie : le nouveau role s'applique aussitot.
    let r = send(&state, &admin, "GET", "/api/team/members", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let r = send(&state, &admin, "GET", "/api/me", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["role"], "developer");
}

#[tokio::test]
async fn list_and_cancel_invitation() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    for (email, role) in [("x@example.com", "developer"), ("y@example.com", "viewer")] {
        let r = send(
            &state,
            &owner,
            "POST",
            "/api/team/invitations",
            Some(json!({ "email": email, "role": role, "name": "Invite", "password": PASSWORD })),
        )
        .await;
        assert_eq!(r.status, 201, "{}", r.raw);
    }
    // Une invitation expiree n'est pas listee.
    sqlx::query(
        "INSERT INTO invitations (id, token_hash, email, name, role, invited_by, created_at, expires_at) \
         VALUES ($1, $2, 'old@example.com', '', 'viewer', NULL, now() - interval '5 days', now() - interval '2 days')",
    )
    .bind(Uuid::new_v4())
    .bind(vec![1u8; 32])
    .execute(&state.db)
    .await
    .unwrap();

    let r = send(&state, &owner, "GET", "/api/team/invitations", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let list = r.json();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2, "{}", r.raw);
    assert!(!r.raw.contains("token") && !r.raw.contains("link"));
    let x = list.iter().find(|i| i["email"] == "x@example.com").unwrap();
    assert_eq!(x["role"], "developer");
    assert_eq!(x["name"], "Invite");
    assert_eq!(x["invited_by"], "Nom owner");
    assert!(x["expires_at"].as_i64().unwrap() > x["created_at"].as_i64().unwrap());

    let id = x["id"].as_str().unwrap().to_string();
    let r = send(
        &state,
        &owner,
        "DELETE",
        &format!("/api/team/invitations/{id}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(audit_count(&state, "team.invitation.cancelled").await, 1);
    let r = send(&state, &owner, "GET", "/api/team/invitations", None).await;
    assert_eq!(r.json().as_array().unwrap().len(), 1);
    // Deja annulee : 404.
    let r = send(
        &state,
        &owner,
        "DELETE",
        &format!("/api/team/invitations/{id}"),
        None,
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
}

#[tokio::test]
async fn admin_cannot_cancel_owner_invitation() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;
    let admin = session(&state, "admin@example.com").await;
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/team/invitations",
        Some(json!({ "email": "boss@example.com", "role": "owner", "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let id: Uuid = sqlx::query_scalar("SELECT id FROM invitations WHERE role = 'owner'")
        .fetch_one(&state.db)
        .await
        .unwrap();

    // L'admin la voit mais ne peut pas l'annuler.
    let r = send(&state, &admin, "GET", "/api/team/invitations", None).await;
    assert_eq!(r.json().as_array().unwrap().len(), 1);
    let r = send(
        &state,
        &admin,
        "DELETE",
        &format!("/api/team/invitations/{id}"),
        None,
    )
    .await;
    assert_eq!((r.status, r.code()), (403, json!("forbidden")), "{}", r.raw);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM invitations")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(left, 1);
    let r = send(
        &state,
        &owner,
        "DELETE",
        &format!("/api/team/invitations/{id}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
}

#[tokio::test]
async fn unknown_or_malformed_ids_are_404() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    for (method, path) in [
        ("DELETE", format!("/api/team/members/{}", Uuid::new_v4())),
        ("DELETE", "/api/team/members/pas-un-uuid".to_string()),
        (
            "DELETE",
            format!("/api/team/invitations/{}", Uuid::new_v4()),
        ),
        ("DELETE", "/api/team/invitations/pas-un-uuid".to_string()),
    ] {
        let r = send(&state, &owner, method, &path, None).await;
        assert_eq!(r.status, 404, "{method} {path} : {}", r.raw);
    }
    let r = send(
        &state,
        &owner,
        "PATCH",
        &format!("/api/team/members/{}", Uuid::new_v4()),
        Some(json!({ "role": "viewer" })),
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
}

#[tokio::test]
async fn removed_member_invitations_deleted() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;
    let admin = session(&state, "admin@example.com").await;
    let mut tokens = Vec::new();
    for (by, email) in [
        (&admin, "a1@example.com"),
        (&admin, "a2@example.com"),
        (&owner, "o1@example.com"),
    ] {
        let r = send(
            &state,
            by,
            "POST",
            "/api/team/invitations",
            Some(json!({ "email": email, "role": "viewer", "password": PASSWORD })),
        )
        .await;
        assert_eq!(r.status, 201, "{}", r.raw);
        let link = r.json()["link"].as_str().unwrap().to_string();
        tokens.push(link.rsplit_once("/invite/").unwrap().1.to_string());
    }

    let r = remove(&state, &owner, admin_id).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["deleted_invitations"], 2);
    let detail: Value =
        sqlx::query_scalar("SELECT detail FROM audit_events WHERE action = 'team.member.removed'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(detail["deleted_invitations"], 2);

    // Les liens du membre retire ne fonctionnent plus ; celui du proprietaire oui.
    for (token, expected) in [(&tokens[0], 404), (&tokens[1], 404), (&tokens[2], 204)] {
        let r = call(
            &state,
            "POST",
            &format!("/api/auth/invitations/{token}/accept"),
            Req {
                body: Some(json!({ "name": "Nora Kaci", "password": PASSWORD })),
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, expected, "{}", r.raw);
    }
    let r = send(&state, &owner, "GET", "/api/team/invitations", None).await;
    assert_eq!(r.json().as_array().unwrap().len(), 0, "{}", r.raw);
}

#[tokio::test]
async fn cancel_invitation_rereads_actor_role() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/team/invitations",
        Some(json!({ "email": "v@example.com", "role": "viewer", "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let id: Uuid = sqlx::query_scalar("SELECT id FROM invitations")
        .fetch_one(&state.db)
        .await
        .unwrap();
    // Un acteur retire entre l'authentification et la transaction n'a aucun droit :
    // la requete passe par le vrai routeur, donc on verifie le cas observable
    // (acteur retrograde avant la requete -> 403 des le garde-fou).
    let admin = session(&state, "admin@example.com").await;
    let r = set_role(&state, &owner, admin_id, "developer").await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let r = send(
        &state,
        &admin,
        "DELETE",
        &format!("/api/team/invitations/{id}"),
        None,
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM invitations")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(left, 1);
}

#[tokio::test]
async fn pending_account_is_flagged_and_refuses_role_and_reset_link() {
    let (state, _pg) = common::state().await;
    user(&state, "owner@example.com", "owner").await;
    let pending = user(&state, "eve@example.com", "viewer").await;
    sqlx::query("UPDATE users SET email_verified_at = NULL WHERE id = $1")
        .bind(pending)
        .execute(&state.db)
        .await
        .unwrap();
    let owner = session(&state, "owner@example.com").await;

    let r = send(&state, &owner, "GET", "/api/team/members", None).await;
    let list = r.json();
    let flag = |email: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .find(|m| m["email"] == email)
            .unwrap()["pending"]
            .clone()
    };
    assert_eq!(flag("eve@example.com"), true);
    assert_eq!(flag("owner@example.com"), false);

    let r = set_role(&state, &owner, pending, "developer").await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "account_pending");
    assert_eq!(role_of(&state, pending).await.as_deref(), Some("viewer"));
    let r = send(
        &state,
        &owner,
        "POST",
        &format!("/api/team/members/{pending}/reset-link"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "account_pending");
    // Le retrait reste possible.
    assert_eq!(remove(&state, &owner, pending).await.status, 200);
}
