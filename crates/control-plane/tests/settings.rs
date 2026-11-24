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
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, $4, 'Test')",
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
        Some(json!({ "ip_allowlist": list })),
    )
    .await
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
    // Par le proxy du noeud (boucle locale) : X-Forwarded-For fait foi.
    let via_proxy = |ip: &'static str| Req {
        cookie: Some(&owner.0),
        peer: Some("127.0.0.1:40000"),
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
async fn mfa_required_enforced_at_next_login() {
    let (state, _pg) = common::state().await;
    instance(&state).await;
    let (_, owner) = login(&state, "owner").await;
    let (_, email) = user(&state, "developer").await;
    let before = login_as(&state, &email).await;
    let before: Session = (before.token(), before.csrf());

    let r = send(
        &state,
        &owner,
        "PATCH",
        "/api/settings/security",
        Some(json!({ "mfa_required": true })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["mfa_required"], true);
    let r = send(&state, &owner, "GET", "/api/settings/general", None).await;
    assert_eq!(r.json()["mfa_required"], true);

    // Session ouverte avant : intacte.
    let r = send(&state, &before, "GET", "/api/projects", None).await;
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
                     "environments": ["production"], "all_projects": true }),
        ),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let r = send(
        &state,
        &owner,
        "POST",
        &format!("/api/projects/{id}/deploy-hooks"),
        Some(json!({ "name": "ci" })),
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
    let (_, other_owner) = login(&state, "owner").await;
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
    for s in [&owner, &admin, &other_owner] {
        assert_eq!(send(&state, s, "GET", "/api/me", None).await.status, 401);
    }
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
