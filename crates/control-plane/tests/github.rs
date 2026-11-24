//! GitHub App par instance (flux manifest) et webhooks entrants, contre un
//! serveur GitHub simule (wiremock). Review Focus 1 : webhook falsifie ou
//! rejoue -> 401/404/200 et aucune tache mise en file.

mod common;

use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::state::{AppState, Clock};
use hmac::{Hmac, KeyInit, Mac};
use rsa::pkcs1::{EncodeRsaPrivateKey, EncodeRsaPublicKey};
use serde_json::{Value, json};
use sha2::Sha256;
use shared::config::Config;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PASSWORD: &str = "correct horse battery staple";
const WEBHOOK_SECRET: &str = "whsec-test-value-0123456789";
const INSTALLATION_TOKEN: &str = "ghs_InstallationTokenNeverStored42";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

/// Cle RSA 2048 jetable (PKCS#1 PEM, comme celle que renvoie GitHub) et sa
/// cle publique PKCS#1 DER, generees une fois par binaire de test.
fn test_key() -> &'static (String, Vec<u8>) {
    static KEY: OnceLock<(String, Vec<u8>)> = OnceLock::new();
    KEY.get_or_init(|| {
        let mut rng = rand::rand_core::UnwrapErr(rand::rngs::SysRng);
        let key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let der = key.to_pkcs1_der().unwrap();
        let body = STANDARD.encode(der.as_bytes());
        // En-tete PEM factice coupe par concat! : aucune ligne de la source n'imite une vraie cle (scanner de secrets).
        let mut pem = String::from(concat!("-----BEGIN RSA ", "PRIVATE KEY-----\n"));
        for line in body.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(line).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END RSA PRIVATE KEY-----\n");
        let public = key.to_public_key().to_pkcs1_der().unwrap().into_vec();
        (pem, public)
    })
}

/// L'API GitHub de l'etat pointe vers le serveur simule.
fn use_github(state: &mut AppState, server: &MockServer) {
    let uri = server.uri();
    state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_GITHUB_API_URL" => Some(uri.clone()),
            _ => None,
        })
        .unwrap(),
    );
}

/// Session ouverte : (cookie, jeton CSRF).
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

/// App deja enregistree (secrets chiffres), sans passer par le flux manifest.
async fn install_app(state: &AppState) {
    let (pem, _) = test_key();
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
                pem.as_bytes(),
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

/// Projet Git sur `repository`/`branch` ; renvoie (projet, environnement production).
async fn project(state: &AppState, name: &str, repository: &str, branch: &str) -> (Uuid, Uuid) {
    let dev = login(state, "developer").await;
    let r = send(
        state,
        &dev,
        "POST",
        "/api/projects",
        Some(json!({
            "name": name,
            "source": { "type": "git", "repository": repository, "branch": branch },
            "build": { "kind": "railpack" }
        })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let p = r.json();
    let id = Uuid::parse_str(p["id"].as_str().unwrap()).unwrap();
    let env = Uuid::parse_str(p["environments"][0]["id"].as_str().unwrap()).unwrap();
    (id, env)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Push signe date de maintenant (`repository.pushed_at`, secondes Unix).
fn push_body(repository: &str, branch: &str) -> Vec<u8> {
    push_value(repository, branch, json!(unix_now()))
        .to_string()
        .into_bytes()
}

fn push_value(repository: &str, branch: &str, pushed_at: Value) -> Value {
    json!({
        "ref": format!("refs/heads/{branch}"),
        "before": "0000000000000000000000000000000000000000",
        "after": COMMIT,
        "deleted": false,
        "repository": { "full_name": repository, "pushed_at": pushed_at },
        "head_commit": { "id": COMMIT, "message": "feat: x" }
    })
}

fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256={hex}")
}

async fn webhook(
    state: &AppState,
    body: Vec<u8>,
    signature: Option<String>,
    delivery: &str,
) -> Reply {
    let mut headers = vec![
        ("x-github-event", "push".to_string()),
        ("x-github-delivery", delivery.to_string()),
    ];
    if let Some(sig) = signature {
        headers.push(("x-hub-signature-256", sig));
    }
    call(
        state,
        "POST",
        "/v1/webhooks/github",
        Req {
            bytes: Some(("application/json", body)),
            headers,
            ..Req::default()
        },
    )
    .await
}

async fn job_count(state: &AppState) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn webhook_bad_signature_401_no_job() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let body = push_body("ciphera/web", "main");

    let wrong_secret = webhook(&state, body.clone(), Some(sign("other", &body)), "d-1").await;
    assert_eq!(wrong_secret.status, 401, "{}", wrong_secret.raw);
    assert_eq!(wrong_secret.code(), "invalid_signature");

    // Corps modifie apres signature.
    let tampered = push_body("ciphera/web", "main-x");
    let r = webhook(&state, tampered, Some(sign(WEBHOOK_SECRET, &body)), "d-2").await;
    assert_eq!(r.status, 401, "{}", r.raw);

    let malformed = webhook(&state, body.clone(), Some("sha256=zz".to_string()), "d-3").await;
    assert_eq!(malformed.status, 401);

    assert_eq!(job_count(&state).await, 0);
    let seen: i64 = sqlx::query_scalar("SELECT count(*) FROM github_deliveries")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(seen, 0, "une livraison non signee n'est pas enregistree");
}

#[tokio::test]
async fn webhook_missing_signature_401() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;

    let r = webhook(&state, push_body("ciphera/web", "main"), None, "d-1").await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_signature");
    assert_eq!(job_count(&state).await, 0);
}

#[tokio::test]
async fn webhook_push_enqueues_deploy() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    // Correspondance du depot insensible a la casse.
    let (project_id, env_id) = project(&state, "web", "Ciphera/Web", "main").await;
    let body = push_body("ciphera/web", "main");

    let r = webhook(
        &state,
        body.clone(),
        Some(sign(WEBHOOK_SECRET, &body)),
        "d-ok",
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    let (kind, payload, dedupe): (String, Value, Option<String>) =
        sqlx::query_as("SELECT kind, payload, dedupe_key FROM jobs")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert_eq!(kind, "deploy");
    assert_eq!(
        payload,
        json!({
            "project_id": project_id,
            "environment_id": env_id,
            "commit_sha": COMMIT,
        })
    );
    assert_eq!(dedupe, Some(format!("d-ok:{project_id}")));
}

#[tokio::test]
async fn webhook_replayed_delivery_ignored() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let body = push_body("ciphera/web", "main");
    let sig = sign(WEBHOOK_SECRET, &body);

    let first = webhook(&state, body.clone(), Some(sig.clone()), "d-replay").await;
    assert_eq!(first.status, 202, "{}", first.raw);
    assert_eq!(job_count(&state).await, 1);
    // Meme une fois la tache terminee (deduplication de la file levee), la
    // livraison rejouee ne cree rien.
    sqlx::query("UPDATE jobs SET status = 'done', finished_at = now()")
        .execute(&state.db)
        .await
        .unwrap();

    let replay = webhook(&state, body, Some(sig), "d-replay").await;
    assert_eq!(replay.status, 200, "{}", replay.raw);
    assert_eq!(replay.json()["status"], "duplicate");
    assert_eq!(job_count(&state).await, 1);
}

#[tokio::test]
async fn webhook_replay_window_is_pruned_after_24h() {
    let (mut state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let body = push_body("ciphera/web", "main");
    let sig = sign(WEBHOOK_SECRET, &body);
    assert_eq!(
        webhook(&state, body.clone(), Some(sig.clone()), "d-old")
            .await
            .status,
        202
    );

    // 25 h plus tard, une autre livraison purge l'ancienne entree.
    state.clock = Clock::from_fn(|| SystemTime::now() + Duration::from_secs(25 * 3600));
    let other = push_value("ciphera/web", "other", json!(unix_now() + 25 * 3600))
        .to_string()
        .into_bytes();
    let r = webhook(
        &state,
        other.clone(),
        Some(sign(WEBHOOK_SECRET, &other)),
        "d-new",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let kept: Vec<String> = sqlx::query_scalar("SELECT delivery_id FROM github_deliveries")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(kept, vec!["d-new".to_string()]);
}

#[tokio::test]
async fn webhook_unknown_repo_404_no_job() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let body = push_body("someone/else", "main");

    let r = webhook(
        &state,
        body.clone(),
        Some(sign(WEBHOOK_SECRET, &body)),
        "d-1",
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(r.code(), "unknown_repository");
    assert_eq!(job_count(&state).await, 0);
}

#[tokio::test]
async fn webhook_push_other_branch_ignored() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let body = push_body("ciphera/web", "feature/x");

    let r = webhook(
        &state,
        body.clone(),
        Some(sign(WEBHOOK_SECRET, &body)),
        "d-1",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "ignored");
    assert_eq!(job_count(&state).await, 0);
}

#[tokio::test]
async fn webhook_body_over_5_mib_rejected() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    const CAP: usize = 5 * 1024 * 1024;
    // Exactement 5 Mio (bien au-dela des 2 Mo par defaut d'axum) : accepte.
    let mut body = push_body("ciphera/web", "main");
    body.resize(CAP, b' ');
    let sig = sign(WEBHOOK_SECRET, &body);
    let r = webhook(&state, body, Some(sig), "d-cap").await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(job_count(&state).await, 1);

    let mut over = push_body("ciphera/web", "main");
    over.resize(CAP + 1, b' ');
    let sig = sign(WEBHOOK_SECRET, &over);
    let r = webhook(&state, over, Some(sig), "d-big").await;
    assert_eq!(r.status, 413, "{}", r.raw);
    assert_eq!(job_count(&state).await, 1);
}

/// Livraison signee avec une autre date de push (`pushed_at`) ou sans date.
async fn dated_push(state: &AppState, body: Value, delivery: &str) -> Reply {
    let body = body.to_string().into_bytes();
    let sig = sign(WEBHOOK_SECRET, &body);
    webhook(state, body, Some(sig), delivery).await
}

#[tokio::test]
async fn webhook_stale_push_ignored_no_job() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let old = unix_now() - 25 * 3600;
    let iso = jiff::Timestamp::from_second(old).unwrap().to_string();
    for (pushed_at, delivery) in [(json!(old), "d-stale-int"), (json!(iso), "d-stale-iso")] {
        // Livraison capturee rejouee apres la purge : identifiant inconnu,
        // signature valide, mais push trop ancien.
        let r = dated_push(
            &state,
            push_value("ciphera/web", "main", pushed_at),
            delivery,
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.raw);
        assert_eq!(r.json()["status"], "ignored");
        assert_eq!(r.json()["reason"], "stale");
    }
    // Plus de 5 min dans le futur : refuse aussi.
    let future = push_value("ciphera/web", "main", json!(unix_now() + 600));
    let r = dated_push(&state, future, "d-future").await;
    assert_eq!(r.json()["reason"], "stale", "{}", r.raw);
    assert_eq!(job_count(&state).await, 0);
}

#[tokio::test]
async fn webhook_push_without_timestamp_ignored_no_job() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let mut body = push_value("ciphera/web", "main", Value::Null);
    body["repository"]
        .as_object_mut()
        .unwrap()
        .remove("pushed_at");
    let r = dated_push(&state, body, "d-undated").await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["reason"], "stale");
    assert_eq!(job_count(&state).await, 0);
}

#[tokio::test]
async fn webhook_fresh_push_iso_or_commit_timestamp_enqueues() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    project(&state, "web", "ciphera/web", "main").await;
    let iso = jiff::Timestamp::from_second(unix_now() - 60)
        .unwrap()
        .to_string();
    let r = dated_push(
        &state,
        push_value("ciphera/web", "main", json!(iso)),
        "d-iso",
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    // Sans `pushed_at` : date du commit de tete (avec decalage horaire).
    let mut body = push_value("ciphera/web", "main", Value::Null);
    body["repository"]
        .as_object_mut()
        .unwrap()
        .remove("pushed_at");
    // Meme instant, ecrit en heure locale +02:00.
    let commit_time = jiff::Timestamp::from_second(unix_now() - 120 + 2 * 3600)
        .unwrap()
        .to_string()
        .replace('Z', "+02:00");
    body["head_commit"]["timestamp"] = json!(commit_time);
    let r = dated_push(&state, body, "d-commit").await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(job_count(&state).await, 2);
}

#[tokio::test]
async fn webhook_without_app_401_no_job() {
    let (state, _pg) = common::state().await;
    project(&state, "web", "ciphera/web", "main").await;
    let body = push_body("ciphera/web", "main");
    let r = webhook(
        &state,
        body.clone(),
        Some(sign(WEBHOOK_SECRET, &body)),
        "d-1",
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_signature");
    assert_eq!(job_count(&state).await, 0);
}

#[tokio::test]
async fn webhook_project_without_production_env_no_job() {
    let (state, _pg) = common::state().await;
    install_app(&state).await;
    let (project_id, _) = project(&state, "web", "ciphera/web", "main").await;
    sqlx::query("DELETE FROM environments WHERE project_id = $1")
        .bind(project_id)
        .execute(&state.db)
        .await
        .unwrap();
    let body = push_body("ciphera/web", "main");
    let r = webhook(
        &state,
        body.clone(),
        Some(sign(WEBHOOK_SECRET, &body)),
        "d-1",
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["reason"], "environment");
    assert_eq!(job_count(&state).await, 0);
}

/// Manifest puis retour GitHub ; renvoie la reponse du retour.
async fn connect(state: &AppState, admin: &(String, String)) -> Reply {
    let r = send(
        state,
        admin,
        "POST",
        "/api/connectors/github/manifest",
        Some(json!({ "panel_url": "https://panel.example.com" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let post_url = r.json()["post_url"].as_str().unwrap().to_string();
    let token = post_url.split_once("state=").unwrap().1.to_string();
    send(
        state,
        admin,
        "GET",
        &format!("/api/connectors/github/callback?code=thecode&state={token}"),
        None,
    )
    .await
}

fn conversion_mock() -> Mock {
    let (pem, _) = test_key();
    Mock::given(method("POST"))
        .and(path("/app-manifests/thecode/conversions"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 123,
            "slug": "ciphera-ab12cd",
            "node_id": "MDM6QXBwNTk=",
            "owner": { "login": "acme" },
            "name": "Ciphera ab12cd",
            "html_url": "https://github.com/apps/ciphera-ab12cd",
            "client_id": "Iv1.0123",
            "client_secret": "client-secret-test-value",
            "webhook_secret": WEBHOOK_SECRET,
            "pem": pem,
        })))
}

#[tokio::test]
async fn manifest_requests_read_permissions_and_push_events() {
    let (state, _pg) = common::state().await;
    let admin = login(&state, "admin").await;
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/connectors/github/manifest",
        Some(json!({ "panel_url": "https://panel.example.com/" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let m = &r.json()["manifest"];
    assert_eq!(
        m["default_permissions"],
        json!({ "contents": "read", "metadata": "read", "pull_requests": "read" })
    );
    assert_eq!(m["default_events"], json!(["push"]));
    assert_eq!(
        m["hook_attributes"]["url"],
        "https://panel.example.com/v1/webhooks/github"
    );
    assert_eq!(m["redirect_url"], "https://panel.example.com/connectors");
    let post_url = r.json()["post_url"].as_str().unwrap().to_string();
    assert!(post_url.starts_with("https://github.com/settings/apps/new?state="));
    // Seule l'empreinte du state est stockee.
    let token = post_url.split_once("state=").unwrap().1;
    let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT state_hash FROM github_manifest_states")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1);
    assert_ne!(stored[0], token.as_bytes());

    let bad = send(
        &state,
        &admin,
        "POST",
        "/api/connectors/github/manifest",
        Some(json!({ "panel_url": "javascript:alert(1)" })),
    )
    .await;
    assert_eq!(bad.status, 422, "{}", bad.raw);
}

#[tokio::test]
async fn private_key_never_returned() {
    let (mut state, _pg) = common::state().await;
    let server = MockServer::start().await;
    use_github(&mut state, &server);
    conversion_mock().expect(1).mount(&server).await;
    let admin = login(&state, "admin").await;

    let r = connect(&state, &admin).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["connected"], true);
    assert_eq!(r.json()["app"]["slug"], "ciphera-ab12cd");
    assert_eq!(
        r.json()["app"]["install_url"],
        "https://github.com/apps/ciphera-ab12cd/installations/new"
    );
    let status = send(&state, &admin, "GET", "/api/connectors/github", None).await;
    assert_eq!(status.status, 200);
    let (pem, _) = test_key();
    let pem_body = pem.lines().nth(1).unwrap();
    for reply in [&r, &status] {
        for secret in [
            pem_body,
            "PRIVATE KEY",
            WEBHOOK_SECRET,
            "client-secret-test-value",
        ] {
            assert!(
                !reply.raw.contains(secret),
                "secret renvoye : {}",
                reply.raw
            );
        }
    }

    // Chiffres en base, et rien du secret client n'est garde.
    let (key, hook): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT private_key_sealed, webhook_secret_sealed FROM github_app")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(!String::from_utf8_lossy(&key).contains(pem_body));
    assert!(!String::from_utf8_lossy(&hook).contains(WEBHOOK_SECRET));
    assert_eq!(
        state
            .secrets
            .open_bound(control_plane::secrets::labels::GITHUB_PRIVATE_KEY, &key)
            .unwrap()
            .as_slice(),
        pem.as_bytes()
    );
    assert_eq!(
        state
            .secrets
            .open_bound(control_plane::secrets::labels::GITHUB_WEBHOOK_SECRET, &hook)
            .unwrap()
            .as_slice(),
        WEBHOOK_SECRET.as_bytes()
    );

    // Une seule App par instance.
    let again = send(
        &state,
        &admin,
        "POST",
        "/api/connectors/github/manifest",
        Some(json!({ "panel_url": "https://panel.example.com" })),
    )
    .await;
    assert_eq!(again.status, 409, "{}", again.raw);
}

#[tokio::test]
async fn manifest_bad_state_creates_no_app() {
    let (mut state, _pg) = common::state().await;
    let server = MockServer::start().await;
    use_github(&mut state, &server);
    // Le code n'est jamais converti sans state valide.
    conversion_mock().expect(0).mount(&server).await;
    let admin = login(&state, "admin").await;
    let other_admin = login(&state, "admin").await;

    let r = send(
        &state,
        &admin,
        "POST",
        "/api/connectors/github/manifest",
        Some(json!({ "panel_url": "https://panel.example.com" })),
    )
    .await;
    let post_url = r.json()["post_url"].as_str().unwrap().to_string();
    let token = post_url.split_once("state=").unwrap().1.to_string();

    for (who, url) in [
        (
            &admin,
            "/api/connectors/github/callback?code=thecode".to_string(),
        ),
        (
            &admin,
            "/api/connectors/github/callback?code=thecode&state=forged".to_string(),
        ),
        // State d'une autre session : refuse (et consomme rien d'autre).
        (
            &other_admin,
            format!("/api/connectors/github/callback?code=thecode&state={token}"),
        ),
    ] {
        let r = send(&state, who, "GET", &url, None).await;
        assert_eq!(r.status, 400, "{url} : {}", r.raw);
        assert_eq!(r.code(), "invalid_state");
    }

    // Expire (plus d'une heure).
    state.clock = Clock::from_fn(|| SystemTime::now() + Duration::from_secs(3601));
    let expired = send(
        &state,
        &admin,
        "GET",
        &format!("/api/connectors/github/callback?code=thecode&state={token}"),
        None,
    )
    .await;
    assert_eq!(expired.status, 400, "{}", expired.raw);

    let apps: i64 = sqlx::query_scalar("SELECT count(*) FROM github_app")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(apps, 0);
}

#[tokio::test]
async fn manifest_state_is_single_use() {
    let (mut state, _pg) = common::state().await;
    let server = MockServer::start().await;
    use_github(&mut state, &server);
    conversion_mock().expect(1).mount(&server).await;
    let admin = login(&state, "admin").await;
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/connectors/github/manifest",
        Some(json!({ "panel_url": "https://panel.example.com" })),
    )
    .await;
    let post_url = r.json()["post_url"].as_str().unwrap().to_string();
    let url = format!(
        "/api/connectors/github/callback?code=thecode&state={}",
        post_url.split_once("state=").unwrap().1
    );
    assert_eq!(send(&state, &admin, "GET", &url, None).await.status, 200);
    let again = send(&state, &admin, "GET", &url, None).await;
    assert_eq!(again.status, 400, "{}", again.raw);
}

fn repos_mocks() -> Vec<Mock> {
    let expires = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_hours(1))
        .unwrap();
    vec![
        Mock::given(method("GET"))
            .and(path("/app/installations"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                { "id": 42, "account": { "login": "acme" } }
            ]))),
        Mock::given(method("POST"))
            .and(path("/app/installations/42/access_tokens"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "token": INSTALLATION_TOKEN,
                "expires_at": expires.to_string(),
            })))
            // Le second appel reutilise le jeton garde en memoire.
            .expect(1),
        Mock::given(method("GET"))
            .and(path("/installation/repositories"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 2,
                "repositories": [
                    { "full_name": "acme/web", "name": "web", "owner": { "login": "acme" },
                      "private": true, "default_branch": "main", "updated_at": "2026-11-20T10:00:00Z" },
                    { "full_name": "acme/api", "name": "api", "owner": { "login": "acme" },
                      "private": false, "default_branch": "develop", "updated_at": null }
                ]
            }))),
    ]
}

#[tokio::test]
async fn installation_token_not_persisted() {
    let (mut state, _pg) = common::state().await;
    let server = MockServer::start().await;
    use_github(&mut state, &server);
    for mock in repos_mocks() {
        mock.mount(&server).await;
    }
    install_app(&state).await;
    let dev = login(&state, "developer").await;

    for _ in 0..2 {
        let r = send(&state, &dev, "GET", "/api/git/providers/github/repos", None).await;
        assert_eq!(r.status, 200, "{}", r.raw);
        let repos = r.json();
        assert_eq!(repos.as_array().unwrap().len(), 2);
        assert_eq!(repos[0]["full_name"], "acme/api");
        assert_eq!(repos[0]["default_branch"], "develop");
        assert_eq!(repos[1]["full_name"], "acme/web");
        assert_eq!(repos[1]["private"], true);
        assert!(!r.raw.contains(INSTALLATION_TOKEN));
    }

    // Le jeton d'installation sert aux depots ; le JWT de l'App (RS256,
    // signe par la cle de l'App) sert aux installations et au jeton.
    let requests = server.received_requests().await.unwrap();
    let auth = |p: &str| {
        requests
            .iter()
            .find(|r| r.url.path() == p)
            .and_then(|r| r.headers.get("authorization"))
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap()
    };
    assert_eq!(
        auth("/installation/repositories"),
        format!("Bearer {INSTALLATION_TOKEN}")
    );
    let jwt = auth("/app/installations");
    let jwt = jwt.strip_prefix("Bearer ").unwrap();
    let (signed, signature) = jwt.rsplit_once('.').unwrap();
    let (header, claims) = signed.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
    assert_eq!(header["alg"], "RS256");
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap();
    assert_eq!(claims["iss"], "123");
    let lifetime = claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap();
    assert!(lifetime <= 600, "{lifetime}");
    let (_, public) = test_key();
    ring::signature::UnparsedPublicKey::new(&ring::signature::RSA_PKCS1_2048_8192_SHA256, public)
        .verify(
            signed.as_bytes(),
            &URL_SAFE_NO_PAD.decode(signature).unwrap(),
        )
        .expect("signature RS256 valide");

    // Aucune table ne contient le jeton (ni en texte, ni en hexadecimal).
    let hex: String = INSTALLATION_TOKEN
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert!(tables.contains(&"github_app".to_string()));
    for table in tables {
        let rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT row_to_json(t)::text FROM \"{table}\" t"
        )))
        .fetch_all(&state.db)
        .await
        .unwrap();
        for row in rows {
            assert!(
                !row.contains(INSTALLATION_TOKEN) && !row.contains(&hex),
                "jeton trouve dans {table}"
            );
        }
    }
}

#[tokio::test]
async fn github_routes_enforce_roles() {
    let (state, _pg) = common::state().await;
    let viewer = login(&state, "viewer").await;
    let dev = login(&state, "developer").await;

    let r = send(
        &state,
        &viewer,
        "GET",
        "/api/git/providers/github/repos",
        None,
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    // Developer : autorise, mais aucune App connectee.
    let r = send(&state, &dev, "GET", "/api/git/providers/github/repos", None).await;
    assert_eq!(r.status, 404, "{}", r.raw);
    assert_eq!(r.code(), "github_not_connected");

    for (method, path) in [
        ("POST", "/api/connectors/github/manifest"),
        ("GET", "/api/connectors/github/callback?code=a&state=b"),
        ("GET", "/api/connectors/github"),
        ("DELETE", "/api/connectors/github"),
    ] {
        let r = send(&state, &dev, method, path, Some(json!({}))).await;
        assert_eq!(r.status, 403, "{method} {path} : {}", r.raw);
    }
}
