//! Onglet Git d'un projet : reglages Git (chemins surveilles, declencheurs),
//! deploy hooks (secret montre une fois, 404 identique, limitation de
//! debit), redemarrage et redeploiement.

mod common;

use std::time::SystemTime;

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::state::AppState;
use hmac::{Hmac, KeyInit, Mac};
use proto::ControlToAgent;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use shared::ids::ServerId;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const WEBHOOK_SECRET: &str = "whsec-test-value-0123456789";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

async fn login(state: &AppState, role: &str) -> (String, String) {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, $4, 'Test')",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
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
    session: &(String, String),
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&session.0),
            csrf: Some(&session.1),
            body,
            ..Req::default()
        },
    )
    .await
}

/// Projet Git `ciphera/web` sur `main` ; renvoie (projet, environnement production).
async fn git_project(state: &AppState, name: &str) -> (Uuid, Uuid) {
    let dev = login(state, "developer").await;
    let r = send(
        state,
        &dev,
        "POST",
        "/api/projects",
        Some(json!({
            "name": name,
            "source": { "type": "git", "repository": "ciphera/web", "branch": "main" },
            "build": { "kind": "railpack" }
        })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let p = r.json();
    (
        Uuid::parse_str(p["id"].as_str().unwrap()).unwrap(),
        Uuid::parse_str(p["environments"][0]["id"].as_str().unwrap()).unwrap(),
    )
}

/// Deploy hook cree par un admin ; renvoie la reponse complete.
async fn create_hook(state: &AppState, project: Uuid) -> Value {
    let admin = login(state, "admin").await;
    let r = send(
        state,
        &admin,
        "POST",
        &format!("/api/projects/{project}/deploy-hooks"),
        Some(json!({ "name": "CI externe" })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    r.json()
}

async fn trigger(state: &AppState, path: &str, peer: &str) -> Reply {
    call(
        state,
        "POST",
        path,
        Req {
            peer: Some(peer),
            ..Req::default()
        },
    )
    .await
}

async fn count(state: &AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql.to_string()))
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn deploy_hook_secret_shown_once() {
    let (state, _pg) = common::state().await;
    let (project, _) = git_project(&state, "web").await;

    // Gestion des hooks : admin seulement.
    let dev = login(&state, "developer").await;
    let refused = send(
        &state,
        &dev,
        "POST",
        &format!("/api/projects/{project}/deploy-hooks"),
        Some(json!({ "name": "CI" })),
    )
    .await;
    assert_eq!(refused.status, 403, "{}", refused.raw);

    let created = create_hook(&state, project).await;
    let token = created["token"].as_str().unwrap().to_string();
    let hook_id = created["hook"]["id"].as_str().unwrap().to_string();
    assert!(token.starts_with("dh_"), "{token}");
    // 32 octets aleatoires en base64 URL sans remplissage : 43 caracteres.
    assert_eq!(token.len(), 3 + 43, "{token}");
    assert_eq!(
        created["url_path"],
        format!("/v1/hooks/deploy/{hook_id}?token={token}")
    );
    assert_eq!(
        created["hook"]["path"],
        format!("/v1/hooks/deploy/{hook_id}")
    );

    // Ni la liste ni la base ne contiennent le secret en clair.
    let admin = login(&state, "admin").await;
    let list = send(
        &state,
        &admin,
        "GET",
        &format!("/api/projects/{project}/deploy-hooks"),
        None,
    )
    .await;
    assert_eq!(list.status, 200, "{}", list.raw);
    assert_eq!(list.json().as_array().unwrap().len(), 1);
    assert!(!list.raw.contains(&token), "secret renvoye : {}", list.raw);
    assert!(!list.raw.contains("token"), "{}", list.raw);
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM deploy_hooks")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(stored, Sha256::digest(token.as_bytes()).to_vec());

    // Le secret declenche un deploiement `hook` de la production.
    let r = trigger(
        &state,
        created["url_path"].as_str().unwrap(),
        common::api::DIRECT,
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["status"], "queued");
    let trigger_kind: String = sqlx::query_scalar("SELECT trigger FROM deployments")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(trigger_kind, "hook");
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM audit_events WHERE action = 'deploy_hook.create'"
        )
        .await,
        1
    );

    // Un second appel pendant que le premier attend : pas de doublon.
    let again = trigger(
        &state,
        created["url_path"].as_str().unwrap(),
        common::api::DIRECT,
    )
    .await;
    assert_eq!(again.status, 202, "{}", again.raw);
    assert_eq!(again.json()["status"], "already_queued");
    assert_eq!(count(&state, "SELECT count(*) FROM deployments").await, 1);

    // Suppression : le hook ne repond plus.
    let del = send(
        &state,
        &admin,
        "DELETE",
        &format!("/api/projects/{project}/deploy-hooks/{hook_id}"),
        None,
    )
    .await;
    assert_eq!(del.status, 204, "{}", del.raw);
    let gone = trigger(
        &state,
        created["url_path"].as_str().unwrap(),
        common::api::DIRECT,
    )
    .await;
    assert_eq!(gone.status, 404, "{}", gone.raw);
}

#[tokio::test]
async fn deploy_hook_wrong_secret_404_constant_time() {
    let (state, _pg) = common::state().await;
    let (project, _) = git_project(&state, "web").await;
    let created = create_hook(&state, project).await;
    let hook_id = created["hook"]["id"].as_str().unwrap();
    let token = created["token"].as_str().unwrap();

    let wrong = trigger(
        &state,
        &format!("/v1/hooks/deploy/{hook_id}?token=dh_{}", "A".repeat(43)),
        "198.51.100.1:1000",
    )
    .await;
    let unknown = trigger(
        &state,
        &format!("/v1/hooks/deploy/{}?token={token}", Uuid::new_v4()),
        "198.51.100.2:1000",
    )
    .await;
    let missing = trigger(
        &state,
        &format!("/v1/hooks/deploy/{hook_id}"),
        "198.51.100.3:1000",
    )
    .await;
    let malformed = trigger(
        &state,
        &format!("/v1/hooks/deploy/not-a-uuid?token={token}"),
        "198.51.100.4:1000",
    )
    .await;
    for r in [&wrong, &unknown, &missing, &malformed] {
        assert_eq!(r.status, 404, "{}", r.raw);
        assert_eq!(r.raw, wrong.raw, "reponses distinguables");
    }
    assert_eq!(count(&state, "SELECT count(*) FROM deployments").await, 0);

    // Le secret passe aussi par l'en-tete.
    let header = call(
        &state,
        "POST",
        &format!("/v1/hooks/deploy/{hook_id}"),
        Req {
            headers: vec![("x-deploy-hook-token", token.to_string())],
            ..Req::default()
        },
    )
    .await;
    assert_eq!(header.status, 202, "{}", header.raw);
}

#[tokio::test]
async fn deploy_hook_rate_limited() {
    let (state, _pg) = common::state().await;
    let (project, _) = git_project(&state, "web").await;
    let created = create_hook(&state, project).await;
    let hook_id = created["hook"]["id"].as_str().unwrap();
    let good = created["url_path"].as_str().unwrap();
    let bad = format!("/v1/hooks/deploy/{hook_id}?token=dh_{}", "B".repeat(43));
    let attacker = "198.51.100.9:4000";

    for _ in 0..5 {
        let r = trigger(&state, &bad, attacker).await;
        assert_eq!(r.status, 404, "{}", r.raw);
    }
    let limited = trigger(&state, &bad, attacker).await;
    assert_eq!(limited.status, 429, "{}", limited.raw);
    let retry: u64 = limited
        .retry_after
        .as_deref()
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!((1..=900).contains(&retry), "{retry}");
    // Meme le bon secret est refuse depuis cette IP pendant la fenetre.
    let blocked = trigger(&state, good, attacker).await;
    assert_eq!(blocked.status, 429, "{}", blocked.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM deployments").await, 0);

    // Une autre IP n'est pas touchee.
    let other = trigger(&state, good, "198.51.100.10:4000").await;
    assert_eq!(other.status, 202, "{}", other.raw);
}

#[tokio::test]
async fn deploy_hook_does_not_reuse_a_queued_push() {
    let (state, _pg) = common::state().await;
    let (project, env) = git_project(&state, "web").await;
    let created = create_hook(&state, project).await;
    // Un push en file a un commit fixe : le hook doit deployer le dernier.
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, trigger, commit_sha, branch, \
         created_at) VALUES ($1, $2, $3, 'push', $4, 'main', now())",
    )
    .bind(Uuid::new_v4())
    .bind(project)
    .bind(env)
    .bind(COMMIT)
    .execute(&state.db)
    .await
    .unwrap();
    let r = trigger(
        &state,
        created["url_path"].as_str().unwrap(),
        common::api::DIRECT,
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["status"], "queued");
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM deployments WHERE trigger = 'hook'"
        )
        .await,
        1
    );
}

// --- Webhook GitHub : chemins surveilles et declencheurs -----------------

async fn install_app(state: &AppState) {
    sqlx::query(
        "INSERT INTO github_app (app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) \
         VALUES (123, 'ciphera-ab12cd', 'Ciphera ab12cd', 'acme', \
         'https://github.com/apps/ciphera-ab12cd', $1, $2)",
    )
    .bind(
        state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_PRIVATE_KEY,
                b"inutilise par le webhook",
            )
            .unwrap(),
    )
    .bind(
        state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_WEBHOOK_SECRET,
                WEBHOOK_SECRET.as_bytes(),
            )
            .unwrap(),
    )
    .execute(&state.db)
    .await
    .unwrap();
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Corps unique a chaque appel (compteur) : le webhook rejette un corps deja vu.
fn push(files: &[&str], message: &str, sender: &str) -> Vec<u8> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    json!({
        "nonce": SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        "ref": "refs/heads/main",
        "after": COMMIT,
        "deleted": false,
        "repository": { "full_name": "ciphera/web", "pushed_at": unix_now() },
        "head_commit": { "id": COMMIT, "message": message },
        "sender": { "login": sender },
        "commits": [
            { "id": COMMIT, "added": [], "removed": [], "modified": files }
        ]
    })
    .to_string()
    .into_bytes()
}

fn sign(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(WEBHOOK_SECRET.as_bytes()).unwrap();
    mac.update(body);
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256={hex}")
}

async fn webhook(state: &AppState, body: Vec<u8>) -> Reply {
    call(
        state,
        "POST",
        "/v1/webhooks/github",
        Req {
            headers: vec![
                ("x-github-event", "push".to_string()),
                ("x-github-delivery", Uuid::new_v4().to_string()),
                ("x-hub-signature-256", sign(&body)),
            ],
            bytes: Some(("application/json", body)),
            ..Req::default()
        },
    )
    .await
}

async fn jobs(state: &AppState) -> i64 {
    count(state, "SELECT count(*) FROM jobs").await
}

#[tokio::test]
async fn watch_paths_skip_deploy_when_unmatched() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    let (project, _) = git_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let r = send(
        &state,
        &dev,
        "PATCH",
        &format!("/api/projects/{project}/git"),
        Some(json!({ "watch_paths": ["apps/web/**", "packages/ui/**"] })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        r.json()["watch_paths"],
        json!(["apps/web/**", "packages/ui/**"])
    );

    let skipped = webhook(
        &state,
        push(&["docs/readme.md", "apps/api/main.rs"], "docs", "alice"),
    )
    .await;
    assert_eq!(skipped.status, 200, "{}", skipped.raw);
    assert_eq!(skipped.json()["status"], "ignored");
    assert_eq!(skipped.json()["reason"], "watch_paths");
    assert_eq!(jobs(&state).await, 0);

    let matched = webhook(&state, push(&["apps/web/src/index.ts"], "feat", "alice")).await;
    assert_eq!(matched.status, 202, "{}", matched.raw);
    assert_eq!(jobs(&state).await, 1);

    // Liste vide : chaque push deploie.
    let r = send(
        &state,
        &dev,
        "PATCH",
        &format!("/api/projects/{project}/git"),
        Some(json!({ "watch_paths": [] })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let any = webhook(&state, push(&["docs/readme.md"], "docs", "alice")).await;
    assert_eq!(any.status, 202, "{}", any.raw);
    assert_eq!(jobs(&state).await, 2);
}

#[tokio::test]
async fn push_respects_auto_deploy_skip_ci_and_bots() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    let (project, _) = git_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let patch = |body: Value| {
        let state = state.clone();
        let dev = dev.clone();
        async move {
            let r = send(
                &state,
                &dev,
                "PATCH",
                &format!("/api/projects/{project}/git"),
                Some(body),
            )
            .await;
            assert_eq!(r.status, 200, "{}", r.raw);
        }
    };

    let r = webhook(&state, push(&["a"], "fix: x [skip ci]", "alice")).await;
    assert_eq!(r.json()["reason"], "skip_ci", "{}", r.raw);
    patch(json!({ "ignore_bots": true })).await;
    let r = webhook(&state, push(&["a"], "chore: deps", "dependabot[bot]")).await;
    assert_eq!(r.json()["reason"], "bot", "{}", r.raw);
    patch(json!({ "auto_deploy": false })).await;
    let r = webhook(&state, push(&["a"], "feat", "alice")).await;
    assert_eq!(r.json()["reason"], "auto_deploy", "{}", r.raw);
    assert_eq!(jobs(&state).await, 0);

    patch(json!({ "auto_deploy": true, "skip_ci": false })).await;
    let r = webhook(&state, push(&["a"], "fix: x [skip ci]", "alice")).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(jobs(&state).await, 1);
}

#[tokio::test]
async fn git_settings_roles_and_validation() {
    let (state, _pg) = common::state().await;
    let (project, _) = git_project(&state, "web").await;
    let path = format!("/api/projects/{project}/git");

    let viewer = login(&state, "viewer").await;
    let r = send(&state, &viewer, "GET", &path, None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        r.json(),
        json!({
            "repository": "ciphera/web", "branch": "main", "root_directory": "",
            "watch_paths": [], "auto_deploy": true, "skip_ci": true, "ignore_bots": false
        })
    );
    let r = send(
        &state,
        &viewer,
        "PATCH",
        &path,
        Some(json!({ "auto_deploy": false })),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);

    let dev = login(&state, "developer").await;
    for bad in [
        json!({ "watch_paths": ["../etc/**"] }),
        json!({ "watch_paths": ["/abs/**"] }),
        json!({ "watch_paths": ["a b"] }),
        json!({ "watch_paths": ["[abc]"] }),
        json!({ "root_directory": "../x" }),
        json!({ "branch": "main; rm -rf /" }),
        json!({}),
    ] {
        let r = send(&state, &dev, "PATCH", &path, Some(bad.clone())).await;
        assert_eq!(r.status, 422, "{bad} : {}", r.raw);
    }
    let r = send(
        &state,
        &dev,
        "PATCH",
        &path,
        Some(json!({ "branch": "release", "root_directory": "./apps/web/" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["branch"], "release");
    assert_eq!(r.json()["root_directory"], "apps/web");
    let audited = count(
        &state,
        "SELECT count(*) FROM audit_events WHERE action = 'project.git.update'",
    )
    .await;
    assert_eq!(audited, 1);

    // Projet image : pas de reglages Git.
    let r = send(
        &state,
        &dev,
        "POST",
        "/api/projects",
        Some(json!({ "name": "img", "source": { "type": "image", "image": "ghcr.io/a/b:1" } })),
    )
    .await;
    let img = r.json()["id"].as_str().unwrap().to_string();
    let r = send(
        &state,
        &viewer,
        "GET",
        &format!("/api/projects/{img}/git"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "not_git");
}

// --- Redemarrage et redeploiement ------------------------------------------

#[tokio::test]
async fn restart_targets_serving_deployment_or_409() {
    let (state, _pg) = common::state().await;
    let (project, env) = git_project(&state, "web").await;
    let dev = login(&state, "developer").await;
    let path = format!("/api/projects/{project}/restart");

    let r = send(&state, &dev, "POST", &path, None).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "nothing_serving");

    let viewer = login(&state, "viewer").await;
    let r = send(&state, &viewer, "POST", &path, None).await;
    assert_eq!(r.status, 403, "{}", r.raw);

    let server = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', 'local', 'AA:BB')",
    )
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();
    let serving = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, status, trigger, \
         created_at, started_at, finished_at) \
         VALUES ($1, $2, $3, $4, 'success', 'manual', now(), now(), now())",
    )
    .bind(serving)
    .bind(project)
    .bind(env)
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();

    // Agent deconnecte : 503.
    let r = send(&state, &dev, "POST", &path, None).await;
    assert_eq!(r.status, 503, "{}", r.raw);

    let mut agent = state.agents.attach_channel(ServerId(server));
    let r = send(&state, &dev, "POST", &path, None).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["deployment_id"], serving.to_string());
    match agent.try_recv() {
        Ok(ControlToAgent::Restart { deployment_id }) => assert_eq!(deployment_id.0, serving),
        other => panic!("ordre attendu : Restart, recu {other:?}"),
    }
    assert_eq!(
        count(
            &state,
            "SELECT count(*) FROM audit_events WHERE action = 'project.restart'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn redeploy_refused_while_one_is_active() {
    let (state, _pg) = common::state().await;
    let (project, _) = git_project(&state, "web").await;
    let path = format!("/api/projects/{project}/redeploy");

    let viewer = login(&state, "viewer").await;
    let r = send(&state, &viewer, "POST", &path, None).await;
    assert_eq!(r.status, 403, "{}", r.raw);

    let dev = login(&state, "developer").await;
    let r = send(&state, &dev, "POST", &path, None).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["trigger"], "manual");
    assert_eq!(r.json()["status"], "queued");
    let again = send(&state, &dev, "POST", &path, None).await;
    assert_eq!(again.status, 409, "{}", again.raw);
    assert_eq!(again.code(), "deployment_in_progress");
    assert_eq!(count(&state, "SELECT count(*) FROM deployments").await, 1);
}

/// Routes publiques limitees par IP en memoire, avant toute ecriture en
/// base : au-dela de `HOOK_PER_IP` appels par minute, 429 sans tentative
/// enregistree ; un hook valide ne declenche pas plus de `HOOK_PER_HOOK`
/// deploiements par minute.
#[tokio::test]
async fn public_routes_are_rate_limited_before_the_database() {
    use control_plane::http::public_limit::{HOOK_PER_HOOK, HOOK_PER_IP};
    let (state, _pg) = common::state().await;
    let (project, _) = git_project(&state, "web").await;
    let created = create_hook(&state, project).await;
    let good = created["url_path"].as_str().unwrap().to_string();

    // Identifiants differents a chaque appel : le plafond (IP, hook) en base
    // n'intervient pas.
    let scanner = "198.51.100.20:4000";
    for _ in 0..HOOK_PER_IP {
        let path = format!(
            "/v1/hooks/deploy/{}?token=dh_{}",
            Uuid::new_v4(),
            "C".repeat(43)
        );
        let r = trigger(&state, &path, scanner).await;
        assert_eq!(r.status, 404, "{}", r.raw);
    }
    let before = count(&state, "SELECT count(*) FROM login_attempts").await;
    let limited = trigger(&state, &good, scanner).await;
    assert_eq!(limited.status, 429, "{}", limited.raw);
    assert!(limited.retry_after.is_some());
    assert_eq!(
        count(&state, "SELECT count(*) FROM login_attempts").await,
        before,
        "rien n'est ecrit en base au-dela de la limite"
    );

    // Hook valide appele depuis plusieurs IP : borne par hook.
    for i in 0..HOOK_PER_HOOK {
        let r = trigger(&state, &good, &format!("198.51.100.{}:4000", 30 + i)).await;
        assert_eq!(r.status, 202, "{}", r.raw);
    }
    let r = trigger(&state, &good, "198.51.100.200:4000").await;
    assert_eq!(r.status, 429, "{}", r.raw);
}

/// Webhook GitHub : au-dela de `WEBHOOK_PER_IP` livraisons par minute
/// depuis une IP, 429 avant la lecture du secret.
#[tokio::test]
async fn github_webhook_is_rate_limited_per_ip() {
    use control_plane::http::public_limit::WEBHOOK_PER_IP;
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    let send = |sig: &'static str| {
        let state = state.clone();
        async move {
            call(
                &state,
                "POST",
                "/v1/webhooks/github",
                Req {
                    headers: vec![
                        ("x-github-event", "push".to_string()),
                        ("x-github-delivery", Uuid::new_v4().to_string()),
                        ("x-hub-signature-256", sig.to_string()),
                    ],
                    bytes: Some(("application/json", b"{}".to_vec())),
                    peer: Some("203.0.113.99:4000"),
                    ..Req::default()
                },
            )
            .await
        }
    };
    for _ in 0..WEBHOOK_PER_IP {
        let r = send("sha256=00").await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    let r = send("sha256=00").await;
    assert_eq!(r.status, 429, "{}", r.raw);
    // Les livraisons d'une autre IP passent (signature verifiee ; aucun
    // projet ne suit le depot).
    let ok = webhook(&state, push(&["a"], "feat: x", "alice")).await;
    assert_eq!(ok.status, 404, "{}", ok.raw);
}
