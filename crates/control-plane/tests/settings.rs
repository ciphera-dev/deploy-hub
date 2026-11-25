//! Parametres v0.1 (`/api/settings/{general,deployments,security,danger}`),
//! liste d'IP autorisees (middleware sur `/api/*` et le panneau), export et
//! suppression de l'organisation.

mod common;

use std::time::{Duration, SystemTime};

use common::api::{DIRECT, Reply, Req, call};
use control_plane::auth::password;
use control_plane::cli::admin;
use control_plane::state::{AppState, Clock};
use proto::ControlToAgent;
use serde_json::{Value, json};
use shared::ids::ServerId;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
/// Adresse du pair par defaut des appels (`common::api::DIRECT`).
const MY_IP: &str = "203.0.113.7";
const OTHER: &str = "198.51.100.9:40000";

type Session = (String, String);

async fn user(state: &AppState, role: &str) -> (Uuid, String) {
    let id = Uuid::new_v4();
    let email = format!("{role}-{}@example.com", id.simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, 'Test', now())",
    )
    .bind(id)
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
    (id, email)
}

async fn login_as(state: &AppState, email: &str) -> Reply {
    call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": email, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await
}

async fn login(state: &AppState, role: &str) -> (Uuid, Session) {
    let (id, email) = user(state, role).await;
    let r = login_as(state, &email).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    (id, (r.token(), r.csrf()))
}

async fn send(
    state: &AppState,
    s: &Session,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    send_from(state, s, DIRECT, method, path, body).await
}

async fn send_from(
    state: &AppState,
    s: &Session,
    peer: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            peer: Some(peer),
            body,
            ..Req::default()
        },
    )
    .await
}

async fn instance(state: &AppState) {
    sqlx::query(
        "INSERT INTO instance (id, name, slug, timezone, language) \
         VALUES ($1, 'Acme', 'acme', 'Europe/Zurich', 'fr')",
    )
    .bind(Uuid::new_v4())
    .execute(&state.db)
    .await
    .unwrap();
}

async fn anonymous(state: &AppState, peer: &str, method: &str, path: &str) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            peer: Some(peer),
            ..Req::default()
        },
    )
    .await
}

#[tokio::test]
async fn settings_require_admin_and_owner_sections_require_owner() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, dev) = login(&state, "developer").await;
    let (_, ops) = login(&state, "ops").await;
    let (_, admin) = login(&state, "admin").await;
    let (_, owner) = login(&state, "owner").await;
    for path in [
        "/api/settings/general",
        "/api/settings/deployments",
        "/api/settings/security",
    ] {
        for s in [&dev, &ops] {
            assert_eq!(
                send(&state, s, "GET", path, None).await.status,
                403,
                "{path}"
            );
        }
        assert_eq!(
            send(&state, &admin, "GET", path, None).await.status,
            200,
            "{path}"
        );
        assert_eq!(
            send(&state, &owner, "GET", path, None).await.status,
            200,
            "{path}"
        );
    }
    for (method, path) in [
        ("GET", "/api/settings/danger"),
        ("POST", "/api/organization/export"),
        ("DELETE", "/api/organization"),
        ("GET", "/api/settings/panel-domain"),
    ] {
        let r = send(&state, &admin, method, path, None).await;
        assert_eq!(r.status, 403, "{method} {path} : {}", r.raw);
    }
    let r = send(&state, &owner, "GET", "/api/settings/danger", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["name"], "Acme");
    assert_eq!(r.json()["members"], 4);
    // Ecritures sous CSRF.
    let r = call(
        &state,
        "PATCH",
        "/api/settings/general",
        Req {
            cookie: Some(&admin.0),
            body: Some(json!({ "name": "X" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
}

#[tokio::test]
async fn general_and_deploy_settings_round_trip() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, admin) = login(&state, "admin").await;
    let r = send(&state, &admin, "GET", "/api/settings/general", None).await;
    assert_eq!(r.json()["name"], "Acme");
    assert_eq!(r.json()["slug"], "acme");
    assert_eq!(r.json()["timezone"], "Europe/Zurich");
    assert_eq!(r.json()["members"], 1);

    let r = send(
        &state,
        &admin,
        "PATCH",
        "/api/settings/general",
        Some(json!({ "name": "Acme SA", "slug": "acme-sa", "timezone": "Europe/Paris", "language": "en" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["name"], "Acme SA");
    assert_eq!(r.json()["language"], "en");
    for bad in [
        json!({ "name": "" }),
        json!({ "slug": "Pas valide" }),
        json!({ "timezone": "Mars/Olympus" }),
        json!({ "language": "de" }),
        json!({ "region": "eu" }),
    ] {
        let r = send(
            &state,
            &admin,
            "PATCH",
            "/api/settings/general",
            Some(bad.clone()),
        )
        .await;
        assert_eq!(r.status, 422, "{bad} : {}", r.raw);
    }
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'settings.general.update'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(audit, 1);

    let r = send(&state, &admin, "GET", "/api/settings/deployments", None).await;
    assert_eq!(r.json(), json!({ "auto": true, "retention": 5 }));
    let r = send(
        &state,
        &admin,
        "PATCH",
        "/api/settings/deployments",
        Some(json!({ "auto": false })),
    )
    .await;
    assert_eq!(r.json(), json!({ "auto": false, "retention": 5 }));
    // La retention est fixe en v0.1.
    let r = send(
        &state,
        &admin,
        "PATCH",
        "/api/settings/deployments",
        Some(json!({ "retention": 10 })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Nouveau projet Git : deploiement automatique selon le defaut.
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/projects",
        Some(json!({
            "name": "web",
            "source": { "type": "git", "repository": "acme/web", "branch": "main" },
            "build": { "kind": "railpack" }
        })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let auto: bool = sqlx::query_scalar("SELECT auto_deploy FROM git_sources")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert!(!auto);
}

async fn set_allowlist(state: &AppState, s: &Session, list: Value) -> Reply {
    send(
        state,
        s,
        "PATCH",
        "/api/settings/security",
        Some(json!({ "ip_allowlist": list, "current_password": PASSWORD })),
    )
    .await
}

/// TOTP confirmee pour le compte (suffit a le dispenser de l'enrolement).
async fn give_totp(state: &AppState, user_id: Uuid) {
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, '\\x00', now())",
    )
    .bind(user_id)
    .execute(&state.db)
    .await
    .unwrap();
}

async fn patch_security(state: &AppState, s: &Session, body: Value) -> Reply {
    send(state, s, "PATCH", "/api/settings/security", Some(body)).await
}

#[tokio::test]
async fn ip_allowlist_blocks_others_403() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let r = set_allowlist(&state, &owner, json!(["203.0.113.0/24", "2001:db8::/32"])).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        r.json()["ip_allowlist"],
        json!(["203.0.113.0/24", "2001:db8::/32"])
    );
    assert_eq!(r.json()["current_ip"], MY_IP);

    // Autorisee : API et panneau.
    assert_eq!(
        send(&state, &owner, "GET", "/api/me", None).await.status,
        200
    );
    assert_eq!(
        send_from(&state, &owner, "[2001:db8::5]:4000", "GET", "/api/me", None)
            .await
            .status,
        200
    );
    // Hors liste : 403 sur l'API (meme avec une session valide) et le panneau.
    let r = send_from(&state, &owner, OTHER, "GET", "/api/me", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "ip_not_allowed");
    for path in ["/api/auth/login", "/api/setup/status"] {
        let r = anonymous(&state, OTHER, "GET", path).await;
        assert_eq!(r.status, 403, "{path} : {}", r.raw);
    }
    for path in ["/", "/projects", "/assets/app.js"] {
        let r = anonymous(&state, OTHER, "GET", path).await;
        assert_eq!(r.status, 403, "{path} : {}", r.raw);
    }
    // X-Forwarded-For d'un client direct : jamais cru.
    let r = call(
        &state,
        "GET",
        "/api/me",
        Req {
            cookie: Some(&owner.0),
            peer: Some(OTHER),
            headers: vec![("x-forwarded-for", MY_IP.into())],
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    // Par l'ecoute locale du proxy du noeud : X-Forwarded-For fait foi.
    let via_proxy = |ip: &'static str| Req {
        cookie: Some(&owner.0),
        peer: Some("127.0.0.1:40000"),
        proxy: true,
        headers: vec![("x-forwarded-for", ip.into())],
        ..Req::default()
    };
    assert_eq!(
        call(&state, "GET", "/api/me", via_proxy("198.51.100.9"))
            .await
            .status,
        403
    );
    assert_eq!(
        call(&state, "GET", "/api/me", via_proxy("203.0.113.99"))
            .await
            .status,
        200
    );

    // Liste vide : plus aucune restriction.
    let r = set_allowlist(&state, &owner, json!([])).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        send_from(&state, &owner, OTHER, "GET", "/api/me", None)
            .await
            .status,
        200
    );
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'settings.security.update'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(audit, 2);
}

#[tokio::test]
async fn ip_allowlist_spares_hooks_webhooks_acme_and_health() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let r = set_allowlist(&state, &owner, json!([MY_IP])).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    for (method, path) in [
        (
            "POST",
            "/v1/hooks/deploy/00000000-0000-0000-0000-000000000000?token=dh_x",
        ),
        ("POST", "/v1/webhooks/github"),
        ("GET", "/.well-known/acme-challenge/abc"),
        ("GET", "/api/health"),
    ] {
        let r = anonymous(&state, OTHER, method, path).await;
        assert_ne!(r.status, 403, "{method} {path} : {}", r.raw);
        assert_ne!(r.code(), "ip_not_allowed", "{method} {path}");
    }
    assert_eq!(
        anonymous(&state, OTHER, "GET", "/api/health").await.status,
        200
    );
}

#[tokio::test]
async fn ip_allowlist_cannot_lock_out_current_ip() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let r = set_allowlist(&state, &owner, json!(["198.51.100.0/24"])).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert_eq!(r.code(), "ip_allowlist_lockout");
    let message = r.json()["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(message.contains(MY_IP), "{message}");
    for bad in [
        json!(["pas-une-ip"]),
        json!(["203.0.113.0/33"]),
        json!(["2001:db8::/129"]),
        json!(["203.0.113.7/24 ; rm -rf /"]),
    ] {
        let r = set_allowlist(&state, &owner, bad.clone()).await;
        assert_eq!(r.status, 422, "{bad} : {}", r.raw);
        assert_eq!(r.code(), "validation_failed", "{bad}");
    }
    let r = set_allowlist(&state, &owner, json!(vec![MY_IP; 101])).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    // Rien n'a ete enregistre : toujours ouvert.
    assert_eq!(
        send_from(&state, &owner, OTHER, "GET", "/api/me", None)
            .await
            .status,
        200
    );
    // Bloc non canonique : ramene a son reseau.
    let r = set_allowlist(&state, &owner, json!(["203.0.113.7/24"])).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["ip_allowlist"], json!(["203.0.113.0/24"]));
}

#[tokio::test]
async fn clear_ip_allowlist_cli_reopens_access() {
    let (mut state, _pg) = common::state().await;
    let start = SystemTime::now();
    state.clock = Clock::from_fn(move || start);
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    assert_eq!(
        set_allowlist(&state, &owner, json!([MY_IP])).await.status,
        200
    );
    assert_eq!(
        send_from(&state, &owner, OTHER, "GET", "/api/me", None)
            .await
            .status,
        403
    );
    // L'administrateur a change d'adresse : la commande vide la liste.
    assert_eq!(admin::clear_ip_allowlist(&state.db).await.unwrap(), 1);
    assert_eq!(admin::clear_ip_allowlist(&state.db).await.unwrap(), 0);
    // Le serveur relit la liste en quelques secondes, sans redemarrage.
    let mut later = state.clone();
    later.clock = Clock::from_fn(move || start + Duration::from_secs(30));
    assert_eq!(
        send_from(&later, &owner, OTHER, "GET", "/api/me", None)
            .await
            .status,
        200
    );
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'settings.security.ip_allowlist.clear.cli'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn enabling_mfa_required_forces_open_sessions() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (owner_id, owner) = login(&state, "owner").await;
    give_totp(&state, owner_id).await;
    let (_, email) = user(&state, "developer").await;
    let before = login_as(&state, &email).await;
    let before: Session = (before.token(), before.csrf());
    let (with_factor_id, with_factor_email) = user(&state, "developer").await;
    give_totp(&state, with_factor_id).await;
    let kept = login_as(&state, &with_factor_email).await;
    // Un compte avec TOTP connecte avant : session intacte (apres l'etape 2FA).
    assert_eq!(kept.status, 200, "{}", kept.raw);

    let r = patch_security(&state, &owner, json!({ "mfa_required": true })).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["mfa_required"], true);
    assert_eq!(r.json()["self_enrollment_required"], false);
    let r = send(&state, &owner, "GET", "/api/settings/general", None).await;
    assert_eq!(r.json()["mfa_required"], true);

    // Session ouverte avant, compte sans facteur : enrolement obligatoire.
    let r = send(&state, &before, "GET", "/api/projects", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "mfa_enrollment_required");
    let r = send(&state, &before, "GET", "/api/me", None).await;
    assert_eq!(r.json()["mfa_enrollment_required"], true, "{}", r.raw);
    // Le proprietaire (avec TOTP) n'est pas touche.
    let r = send(&state, &owner, "GET", "/api/projects", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Connexion suivante sans 2FA : enrolement obligatoire avant tout acces.
    let r = login_as(&state, &email).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let after: Session = (r.token(), r.csrf());
    let r = send(&state, &after, "GET", "/api/me", None).await;
    assert_eq!(r.json()["mfa_enrollment_required"], true, "{}", r.raw);
    let r = send(&state, &after, "GET", "/api/projects", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "mfa_enrollment_required");
}

#[tokio::test]
async fn switched_session_needs_password_to_add_a_factor() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (owner_id, owner) = login(&state, "owner").await;
    give_totp(&state, owner_id).await;
    let (_, email) = user(&state, "developer").await;
    let open = login_as(&state, &email).await;
    let open: Session = (open.token(), open.csrf());
    let r = patch_security(&state, &owner, json!({ "mfa_required": true })).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let r = send(&state, &open, "GET", "/api/me", None).await;
    assert_eq!(r.json()["mfa_enrollment_required"], true, "{}", r.raw);

    // Pas de TOTP sans mot de passe, ni avec un mauvais (les passkeys :
    // `tests/webauthn.rs`, hote en nom de domaine requis).
    let path = "/api/me/mfa/totp/start";
    for body in [
        None,
        Some(json!({ "current_password": "mauvais mot de passe" })),
    ] {
        let r = send(&state, &open, "POST", path, body).await;
        assert_eq!(r.status, 401, "{}", r.raw);
        assert_eq!(r.code(), "invalid_credentials");
    }
    let seeds: i64 = sqlx::query_scalar("SELECT count(*) FROM user_totp")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(seeds, 1, "aucune graine creee sans mot de passe");
    // Avec le mot de passe : permis.
    let r = send(
        &state,
        &open,
        "POST",
        path,
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn author_without_factor_gets_enrollment() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let r = patch_security(&state, &owner, json!({ "mfa_required": true })).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["self_enrollment_required"], true, "{}", r.raw);
    let r = send(&state, &owner, "GET", "/api/projects", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "mfa_enrollment_required");
    // La page d'enrolement reste ouverte.
    let r = send(&state, &owner, "GET", "/api/me", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["mfa_enrollment_required"], true);
}

#[tokio::test]
async fn admin_cannot_disable_mfa_required() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (owner_id, owner) = login(&state, "owner").await;
    give_totp(&state, owner_id).await;
    let (admin_id, admin) = login(&state, "admin").await;
    give_totp(&state, admin_id).await;
    // L'administrateur peut imposer la 2FA (sans mot de passe)...
    let r = patch_security(&state, &admin, json!({ "mfa_required": true })).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // ... pas la desactiver, mot de passe fourni ou non.
    for body in [
        json!({ "mfa_required": false }),
        json!({ "mfa_required": false, "current_password": PASSWORD }),
    ] {
        let r = patch_security(&state, &admin, body).await;
        assert_eq!(r.status, 403, "{}", r.raw);
    }
    let on: bool = sqlx::query_scalar("SELECT mfa_required FROM instance")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert!(on);
    // Le proprietaire : mot de passe obligatoire.
    for body in [
        json!({ "mfa_required": false }),
        json!({ "mfa_required": false, "current_password": "mauvais mot de passe" }),
    ] {
        let r = patch_security(&state, &owner, body).await;
        assert_eq!(r.status, 401, "{}", r.raw);
        assert_eq!(r.code(), "invalid_credentials");
    }
    let r = patch_security(
        &state,
        &owner,
        json!({ "mfa_required": false, "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["mfa_required"], false);
}

#[tokio::test]
async fn ip_allowlist_owner_only_with_password() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let (_, admin) = login(&state, "admin").await;
    let list = json!([MY_IP]);
    let r = set_allowlist(&state, &admin, list.clone()).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    for body in [
        json!({ "ip_allowlist": list }),
        json!({ "ip_allowlist": list, "current_password": "mauvais mot de passe" }),
    ] {
        let r = patch_security(&state, &owner, body).await;
        assert_eq!(r.status, 401, "{}", r.raw);
        assert_eq!(r.code(), "invalid_credentials");
    }
    let n: i64 = sqlx::query_scalar("SELECT cardinality(ip_allowlist)::bigint FROM instance")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let r = set_allowlist(&state, &owner, list.clone()).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["ip_allowlist"], list);
    // Liste inchangee : l'administrateur peut enregistrer d'autres reglages.
    let r = patch_security(
        &state,
        &admin,
        json!({ "mfa_required": false, "ip_allowlist": list }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

/// Projet Git cree par l'API ; renvoie (id, id de l'environnement de production).
async fn project(state: &AppState, s: &Session, name: &str) -> (Uuid, Uuid) {
    let r = send(
        state,
        s,
        "POST",
        "/api/projects",
        Some(json!({
            "name": name,
            "source": { "type": "git", "repository": format!("acme/{name}"), "branch": "main" },
            "build": { "kind": "railpack" }
        })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let id: Uuid = r.json()["id"].as_str().unwrap().parse().unwrap();
    let env: Uuid = sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    (id, env)
}

#[tokio::test]
async fn export_has_no_secrets() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (owner_id, owner) = login(&state, "owner").await;
    let (_, admin) = login(&state, "admin").await;
    let (id, _) = project(&state, &owner, "web").await;
    let vars = format!("/api/projects/{id}/variables?env=production");
    for (key, value, secret) in [
        (
            "DATABASE_URL",
            "postgres://u:fixture-db-password@db/app",
            true,
        ),
        ("PUBLIC_NAME", "fixture-plain-value", false),
    ] {
        let r = send(
            &state,
            &owner,
            "PUT",
            &vars,
            Some(json!({ "key": key, "value": value, "secret": secret })),
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.raw);
    }
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/variables",
        Some(
            json!({ "key": "SENTRY_DSN", "value": "fixture-global-secret", "secret": true,
                     "environments": ["production"], "all_projects": false,
                     "projects": [id.to_string()] }),
        ),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let r = send(
        &state,
        &owner,
        "POST",
        &format!("/api/projects/{id}/deploy-hooks"),
        Some(json!({ "name": "ci", "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let hook_token = r.json()["token"].as_str().unwrap().to_string();
    let r = send(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        Some(
            json!({ "enabled": true, "host": "smtp.example.com", "port": 587, "tls": "starttls",
                     "username": "mailer", "password": "fixture-smtp-test-password",
                     "from_address": "ciphera@example.com", "from_name": "Ciphera" }),
        ),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, $2, now())",
    )
    .bind(owner_id)
    .bind(
        state
            .secrets
            .seal_bound(
                &control_plane::secrets::labels::totp_seed(owner_id),
                b"FIXTURETOTPSEED",
            )
            .unwrap(),
    )
    .execute(&state.db)
    .await
    .unwrap();
    // En-tete PEM factice coupe par concat! : aucune ligne de la source n'imite une vraie cle (scanner de secrets).
    sqlx::query(
        "INSERT INTO github_app (app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) VALUES (7, 'ciphera-x', 'Ciphera X', 'acme', \
         'https://github.com/apps/ciphera-x', $1, $2)",
    )
    .bind(
        state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_PRIVATE_KEY,
                concat!("-----BEGIN RSA ", "PRIVATE KEY-----fixture").as_bytes(),
            )
            .unwrap(),
    )
    .bind(
        state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_WEBHOOK_SECRET,
                b"fixture-webhook-secret",
            )
            .unwrap(),
    )
    .execute(&state.db)
    .await
    .unwrap();

    let r = send(&state, &admin, "POST", "/api/organization/export", None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let r = send(&state, &owner, "POST", "/api/organization/export", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.cache_control.as_deref(), Some("no-store"));
    assert!(
        r.headers["content-disposition"]
            .to_str()
            .unwrap()
            .contains("attachment")
    );
    let export = r.json();
    // Contenu attendu.
    assert_eq!(export["organization"]["name"], "Acme");
    assert_eq!(export["projects"][0]["name"], "web");
    let raw = r.raw.clone();
    for expected in [
        "DATABASE_URL",
        "PUBLIC_NAME",
        "SENTRY_DSN",
        "smtp.example.com",
        "Ciphera X",
        "ci",
    ] {
        assert!(raw.contains(expected), "{expected} absent de l'export");
    }
    // Aucun secret, aucune valeur de variable, aucun hachage.
    let phc: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(owner_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
    for secret in [
        "fixture-db-password",
        "fixture-plain-value",
        "fixture-global-secret",
        "fixture-smtp-test-password",
        "fixture-webhook-secret",
        "BEGIN RSA",
        "FIXTURETOTPSEED",
        hook_token.as_str(),
        phc.as_str(),
        "$argon2",
        PASSWORD,
    ] {
        assert!(!raw.contains(secret), "secret dans l'export : {secret}");
    }
    let mut keys = Vec::new();
    collect_keys(&export, &mut keys);
    for key in &keys {
        let k = key.to_ascii_lowercase();
        assert!(
            !(k.contains("hash")
                || k.contains("sealed")
                || k.contains("token")
                || k.contains("private")
                || k == "value"
                || k == "password"),
            "cle suspecte dans l'export : {key}"
        );
    }
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'organization.export'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(audit, 1);
}

fn collect_keys(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                collect_keys(v, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| collect_keys(v, out)),
        _ => {}
    }
}

async fn server(state: &AppState, name: &str, kind: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(name)
        .bind(kind)
        .bind(format!("FP:{}", id.simple()))
        .execute(&state.db)
        .await
        .unwrap();
    id
}

/// Deploiement envoye a l'agent `server` (conteneurs a retirer).
async fn deployed(state: &AppState, project: Uuid, env: Uuid, server: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, status, trigger, \
         created_at, started_at, finished_at, deploy_sent_at) \
         VALUES ($1, $2, $3, $4, 'success', 'manual', now(), now(), now(), now())",
    )
    .bind(id)
    .bind(project)
    .bind(env)
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();
    id
}

async fn count(state: &AppState, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&state.db).await.unwrap()
}

fn delete_body(password: &str, confirm: &str) -> Option<Value> {
    Some(json!({ "password": password, "confirm": confirm }))
}

#[tokio::test]
async fn delete_org_requires_reauth_and_phrase() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&state.db)
    .await
    .unwrap();
    let local = server(&state, "local", "local").await;
    let mut agent = state.agents.attach_channel(ServerId(local));
    let (owner_id, owner) = login(&state, "owner").await;
    let (_, admin) = login(&state, "admin").await;
    let (_, developer) = login(&state, "developer").await;
    let (web, web_env) = project(&state, &owner, "web").await;
    let (api, api_env) = project(&state, &owner, "api").await;
    let d1 = deployed(&state, web, web_env, local).await;
    let d2 = deployed(&state, api, api_env, local).await;
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/variables",
        Some(json!({ "key": "SHARED", "value": "x", "secret": false,
                     "environments": ["production"], "all_projects": true })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/team/invitations",
        Some(json!({ "email": "new@example.com", "role": "developer" })),
    )
    .await;
    assert!(r.status < 300, "{}", r.raw);
    sqlx::query(
        "INSERT INTO github_app (app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) VALUES (7, 'ciphera-x', 'Ciphera X', 'acme', \
         'https://github.com/apps/ciphera-x', '\\x00', '\\x00')",
    )
    .execute(&state.db)
    .await
    .unwrap();
    while agent.try_recv().is_ok() {}

    // Administrateur : refuse, meme avec la phrase et son mot de passe.
    let r = send(
        &state,
        &admin,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    // Phrase absente ou fausse.
    for (body, why) in [
        (delete_body(PASSWORD, ""), "vide"),
        (delete_body(PASSWORD, "supprimer"), "casse"),
        (delete_body(PASSWORD, " SUPPRIMER"), "espace"),
        (Some(json!({ "password": PASSWORD })), "absente"),
    ] {
        let r = send(&state, &owner, "DELETE", "/api/organization", body).await;
        assert_eq!(r.status, 422, "{why} : {}", r.raw);
    }
    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "pas ca"),
    )
    .await;
    assert_eq!(r.code(), "confirmation_required");
    // Mot de passe faux.
    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body("wrong password value", "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    // Rien n'a bouge.
    assert_eq!(count(&state, "SELECT count(*) FROM projects").await, 2);
    assert_eq!(count(&state, "SELECT count(*) FROM users").await, 3);
    assert!(agent.try_recv().is_err());

    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let body = r.json();
    assert_eq!(body["complete"], true, "{}", r.raw);
    assert_eq!(body["projects_deleted"], 2);
    assert_eq!(body["remaining_projects"], json!([]));
    assert_eq!(body["github_app"]["name"], "Ciphera X");
    assert_eq!(
        body["github_app"]["html_url"],
        "https://github.com/apps/ciphera-x"
    );
    assert!(
        r.set_cookie
            .as_deref()
            .unwrap_or_default()
            .contains("Max-Age=0")
    );

    // Conteneurs retires des agents.
    let mut removed = Vec::new();
    while let Ok(msg) = agent.try_recv() {
        if let ControlToAgent::RemoveDeployment { deployment_id } = msg {
            removed.push(deployment_id.0);
        }
    }
    removed.sort();
    let mut expected = vec![d1, d2];
    expected.sort();
    assert_eq!(removed, expected);
    // Supprimes.
    for sql in [
        "SELECT count(*) FROM projects",
        "SELECT count(*) FROM deployments",
        "SELECT count(*) FROM global_variables",
        "SELECT count(*) FROM invitations",
        "SELECT count(*) FROM github_app",
        "SELECT count(*) FROM sessions",
    ] {
        assert_eq!(count(&state, sql).await, 0, "{sql}");
    }
    // Seul le proprietaire qui a demande la suppression reste.
    let users: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(users, vec![owner_id]);
    // Conserves : instance, serveurs, SMTP, journal d'audit.
    for sql in [
        "SELECT count(*) FROM instance",
        "SELECT count(*) FROM servers",
        "SELECT count(*) FROM smtp_settings",
        "SELECT count(*) FROM audit_events WHERE action = 'organization.delete'",
    ] {
        assert_eq!(count(&state, sql).await, 1, "{sql}");
    }
    // Toutes les sessions revoquees, celle du proprietaire comprise.
    for s in [&owner, &admin, &developer] {
        assert_eq!(send(&state, s, "GET", "/api/me", None).await.status, 401);
    }
}

#[tokio::test]
async fn org_delete_refused_with_other_owners() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let (other_id, _) = login(&state, "owner").await;
    let attempts = || count(&state, "SELECT count(*) FROM mfa_attempts");
    let before = attempts().await;

    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "other_owners");
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Équipe")
    );
    // Refus avant la verification : aucun essai consomme, rien de supprime.
    assert_eq!(attempts().await, before);
    assert_eq!(count(&state, "SELECT count(*) FROM users").await, 2);

    // Autre proprietaire retire (Equipe) : la suppression passe.
    sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
        .bind(other_id)
        .execute(&state.db)
        .await
        .unwrap();
    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM users").await, 1);
}

/// TOTP du compte enrolee par l'API ; renvoie la session tournee, le
/// generateur et les codes de secours. La fenetre d'enrolement est oubliee
/// (un code de la fenetre courante reste utilisable).
async fn enrol_totp(state: &AppState, s: &Session) -> (Session, totp_rs::Totp, Vec<String>) {
    let start = send(
        state,
        s,
        "POST",
        "/api/me/mfa/totp/start",
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(start.status, 200, "{}", start.raw);
    let uri = start.json()["otpauth_uri"].as_str().unwrap().to_string();
    let secret = uri
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("secret="))
        .unwrap()
        .to_string();
    let totp = totp_rs::Builder::new()
        .with_secret(totp_rs::Secret::try_from_base32(&secret).unwrap())
        .with_skew(0)
        .build()
        .unwrap();
    away_from_step_boundary().await;
    let confirm = send(
        state,
        s,
        "POST",
        "/api/me/mfa/totp/confirm",
        Some(json!({ "code": totp_now(&totp) })),
    )
    .await;
    assert_eq!(confirm.status, 200, "{}", confirm.raw);
    let codes = confirm.json()["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    sqlx::query("UPDATE user_totp SET last_used_step = NULL")
        .execute(&state.db)
        .await
        .unwrap();
    ((confirm.token(), confirm.csrf()), totp, codes)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn totp_now(totp: &totp_rs::Totp) -> String {
    totp.generate(unix_now()).to_string()
}

/// Code a 6 chiffres sur d'aucune fenetre acceptee (precedente, courante,
/// suivante) : le code courant, chiffres decales jusqu'a sortir de ces trois.
fn wrong_code(totp: &totp_rs::Totp) -> String {
    let now = unix_now();
    let valid: Vec<String> = [now - 30, now, now + 30]
        .iter()
        .map(|t| totp.generate(*t).to_string())
        .collect();
    let mut code = totp_now(totp);
    while valid.contains(&code) {
        code = code
            .chars()
            .map(|c| char::from_digit((c.to_digit(10).unwrap() + 1) % 10, 10).unwrap())
            .collect();
    }
    code
}

/// Evite de calculer un code juste avant un changement de fenetre de 30 s.
async fn away_from_step_boundary() {
    let into = unix_now() % 30;
    if into >= 26 {
        tokio::time::sleep(Duration::from_secs(31 - into)).await;
    }
}

#[tokio::test]
async fn org_delete_requires_second_factor() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let (owner, totp, codes) = enrol_totp(&state, &owner).await;
    let delete = |body: Value| send(&state, &owner, "DELETE", "/api/organization", Some(body));

    // Mot de passe seul : le second facteur est exige.
    let r = delete(json!({ "password": PASSWORD, "confirm": "SUPPRIMER" })).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "second_factor_required");
    // Code faux : refuse et audite.
    away_from_step_boundary().await;
    let r =
        delete(json!({ "password": PASSWORD, "code": wrong_code(&totp), "confirm": "SUPPRIMER" }))
            .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_code");
    // Bon code mais mauvais mot de passe : refuse.
    away_from_step_boundary().await;
    let r = delete(
        json!({ "password": "wrong password value", "code": totp_now(&totp), "confirm": "SUPPRIMER" }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM audit_events WHERE action = 'organization.delete.failure'"
        )
        .await,
        2
    );
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM audit_events WHERE action = 'organization.delete'"
        )
        .await,
        0
    );

    // Mot de passe et code de secours : supprimee, le code est consomme.
    let r = delete(json!({ "password": PASSWORD, "code": codes[0], "confirm": "SUPPRIMER" })).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM recovery_codes WHERE used_at IS NOT NULL"
        )
        .await,
        1
    );
}

/// Un code deja consomme (ici par la creation d'un deploy hook) est refuse
/// avant toute suppression : projets et comptes restent.
#[tokio::test]
async fn org_delete_replayed_code_deletes_nothing() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let (owner, totp, codes) = enrol_totp(&state, &owner).await;
    let (project_id, _) = project(&state, &owner, "web").await;
    login(&state, "developer").await;
    away_from_step_boundary().await;
    let code = totp_now(&totp);
    let hook = send(
        &state,
        &owner,
        "POST",
        &format!("/api/projects/{project_id}/deploy-hooks"),
        Some(json!({ "name": "CI", "password": PASSWORD, "code": code })),
    )
    .await;
    assert_eq!(hook.status, 201, "{}", hook.raw);

    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        Some(json!({ "password": PASSWORD, "code": code, "confirm": "SUPPRIMER" })),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_code");
    assert_eq!(count(&state, "SELECT count(*) FROM projects").await, 1);
    assert_eq!(count(&state, "SELECT count(*) FROM users").await, 2);
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM audit_events WHERE action = 'organization.delete'"
        )
        .await,
        0
    );

    // Un code neuf (code de secours) : supprimee.
    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        Some(json!({ "password": PASSWORD, "code": codes[0], "confirm": "SUPPRIMER" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM projects").await, 0);
}

/// Deux demandes simultanees avec le meme code : une seule supprime.
#[tokio::test]
async fn org_delete_same_code_twice_concurrently_runs_once() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let (owner, totp, _) = enrol_totp(&state, &owner).await;
    away_from_step_boundary().await;
    let body = json!({ "password": PASSWORD, "code": totp_now(&totp), "confirm": "SUPPRIMER" });
    let (a, b) = tokio::join!(
        send(
            &state,
            &owner,
            "DELETE",
            "/api/organization",
            Some(body.clone())
        ),
        send(&state, &owner, "DELETE", "/api/organization", Some(body)),
    );
    let ok = [a.status, b.status].iter().filter(|s| **s == 200).count();
    assert_eq!(ok, 1, "{} / {}", a.raw, b.raw);
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM audit_events WHERE action = 'organization.delete'"
        )
        .await,
        1
    );
}

/// Passkey seule, sans code de secours : aucun code possible, 409.
#[tokio::test]
async fn org_delete_second_factor_unavailable() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (owner_id, owner) = login(&state, "owner").await;
    sqlx::query(
        "INSERT INTO webauthn_credentials (id, user_id, credential_id, passkey, name)          VALUES ($1, $2, $3, '{}'::jsonb, 'cle')",
    )
    .bind(Uuid::new_v4())
    .bind(owner_id)
    .bind(Uuid::new_v4().as_bytes().to_vec())
    .execute(&state.db)
    .await
    .unwrap();
    let delete = |body: Value| send(&state, &owner, "DELETE", "/api/organization", Some(body));
    let r = delete(json!({ "password": PASSWORD, "confirm": "SUPPRIMER" })).await;
    assert_eq!(r.code(), "second_factor_required", "{}", r.raw);
    let r = delete(json!({ "password": PASSWORD, "code": "123456", "confirm": "SUPPRIMER" })).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "second_factor_unavailable");
    assert_eq!(count(&state, "SELECT count(*) FROM users").await, 1);
}

#[tokio::test]
async fn delete_org_keeps_and_reports_projects_of_offline_agents() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let online = server(&state, "local", "local").await;
    let offline = server(&state, "edge", "remote").await;
    let mut agent = state.agents.attach_channel(ServerId(online));
    let (_, email) = user(&state, "owner").await;
    let r = login_as(&state, &email).await;
    let owner: Session = (r.token(), r.csrf());
    let (web, web_env) = project(&state, &owner, "web").await;
    let (edge, edge_env) = project(&state, &owner, "edge").await;
    deployed(&state, web, web_env, online).await;
    deployed(&state, edge, edge_env, offline).await;

    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["complete"], false, "{}", r.raw);
    assert_eq!(r.json()["projects_deleted"], 1);
    assert_eq!(
        r.json()["remaining_projects"],
        json!([{ "id": edge.to_string(), "name": "edge" }])
    );
    // Le projet de l'agent injoignable reste suivi (rien n'est perdu).
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM projects")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(names, vec!["edge".to_string()]);
    let detail: Value =
        sqlx::query_scalar("SELECT detail FROM audit_events WHERE action = 'organization.delete'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(detail["remaining_projects"], json!(["edge"]));

    // Agent revenu : une nouvelle suppression termine.
    let _edge_agent = state.agents.attach_channel(ServerId(offline));
    while agent.try_recv().is_ok() {}
    let r = login_as(&state, &email).await;
    let owner: Session = (r.token(), r.csrf());
    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["complete"], true, "{}", r.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM projects").await, 0);
}

#[tokio::test]
async fn delete_org_reports_projects_that_fail_and_finishes_the_rest() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, email) = user(&state, "owner").await;
    let r = login_as(&state, &email).await;
    let owner: Session = (r.token(), r.csrf());
    let (boom, _) = project(&state, &owner, "boom").await;
    project(&state, &owner, "web").await;
    // Panne simulee : la suppression du projet « boom » echoue en base.
    sqlx::raw_sql(
        "CREATE FUNCTION test_refuse_boom() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'panne simulee'; END $$; \
         CREATE TRIGGER test_refuse_boom BEFORE DELETE ON projects \
         FOR EACH ROW WHEN (OLD.name = 'boom') EXECUTE FUNCTION test_refuse_boom();",
    )
    .execute(&state.db)
    .await
    .unwrap();

    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    // Pas de 500 apres une demi-suppression : la suite est faite, le projet
    // en echec est signale.
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["complete"], false, "{}", r.raw);
    assert_eq!(r.json()["projects_deleted"], 1);
    assert_eq!(
        r.json()["remaining_projects"],
        json!([{ "id": boom.to_string(), "name": "boom" }])
    );
    assert_eq!(count(&state, "SELECT count(*) FROM sessions").await, 0);
    let detail: Value =
        sqlx::query_scalar("SELECT detail FROM audit_events WHERE action = 'organization.delete'")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(detail["remaining_projects"], json!(["boom"]));
}

#[tokio::test]
async fn delete_org_removes_all_metrics_and_keeps_servers() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let node = server(&state, "local", "local").await;
    let (_, email) = user(&state, "owner").await;
    let r = login_as(&state, &email).await;
    let owner: Session = (r.token(), r.csrf());
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO node_metrics_raw VALUES ('{node}', now(), 1, 1, 2, 1, 2, 0, 0); \
         INSERT INTO node_metrics_hourly VALUES ('{node}', date_trunc('hour', now()), 1, 1, 2, 1, 2, 1); \
         INSERT INTO container_metrics_raw VALUES ('{node}', gen_random_uuid(), gen_random_uuid(), now(), 1, 1, true); \
         INSERT INTO container_metrics_hourly VALUES (gen_random_uuid(), date_trunc('hour', now()), 1, 1, 1, 1);"
    )))
    .execute(&state.db)
    .await
    .unwrap();

    let r = send(
        &state,
        &owner,
        "DELETE",
        "/api/organization",
        delete_body(PASSWORD, "SUPPRIMER"),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    for sql in [
        "SELECT count(*) FROM node_metrics_raw",
        "SELECT count(*) FROM node_metrics_hourly",
        "SELECT count(*) FROM container_metrics_raw",
        "SELECT count(*) FROM container_metrics_hourly",
    ] {
        assert_eq!(count(&state, sql).await, 0, "{sql}");
    }
    assert_eq!(count(&state, "SELECT count(*) FROM servers").await, 1);
}
