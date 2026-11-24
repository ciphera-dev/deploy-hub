//! Version de l'instance et demande de mise a jour (`/api/system/*`).
//! La verification de la derniere version est testee contre un serveur
//! simule local (wiremock) : jamais le vrai reseau.

mod common;

use std::sync::Arc;

use common::TempDir;
use common::api::{DIRECT, Reply, Req, call};
use control_plane::auth::password;
use control_plane::state::AppState;
use serde_json::{Value, json};
use shared::config::Config;
use shared::update::{REQUEST_FILE, UpdateRequest, request_key};
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PASSWORD: &str = "correct horse battery staple";

type Session = (String, String);

async fn login(state: &AppState, role: &str) -> Session {
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
    s: &Session,
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
            peer: Some(DIRECT),
            body,
            ..Req::default()
        },
    )
    .await
}

fn with_check(state: &mut AppState, enabled: bool, base: &str) {
    state.cfg = Arc::new(Config {
        update_check: enabled,
        release_base_url: base.to_string(),
        ..(*state.cfg).clone()
    });
}

fn current() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

#[tokio::test]
async fn version_check_is_off_by_default_and_contacts_nothing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let (mut state, _db) = common::state().await;
    assert!(!state.cfg.update_check, "desactivee par defaut");
    with_check(&mut state, false, &server.uri());
    let admin = login(&state, "admin").await;

    let r = send(&state, &admin, "GET", "/api/system/version", None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json(), json!({ "current": current() }));
    server.verify().await;

    let viewer = login(&state, "viewer").await;
    let r = send(&state, &viewer, "GET", "/api/system/version", None).await;
    assert_eq!(r.status, 403);
}

#[tokio::test]
async fn version_check_opt_in_reads_latest_once() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/releases/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": "v9.9.9",
            "changelog_url": "https://get.ciphera.ch/releases/v9.9.9/CHANGELOG.md"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let (mut state, _db) = common::state().await;
    with_check(&mut state, true, &server.uri());
    let admin = login(&state, "admin").await;

    for _ in 0..2 {
        let r = send(&state, &admin, "GET", "/api/system/version", None).await;
        assert_eq!(r.status, 200, "{}", r.raw);
        assert_eq!(
            r.json(),
            json!({
                "current": current(),
                "latest": "v9.9.9",
                "changelog_url": "https://get.ciphera.ch/releases/v9.9.9/CHANGELOG.md"
            })
        );
    }
    server.verify().await;
    // Aucune donnee de l'instance : ni requete, ni version dans l'agent.
    let received = server.received_requests().await.unwrap();
    assert_eq!(received[0].url.query(), None);
    assert_eq!(received[0].headers["user-agent"], "ciphera");
    assert!(received[0].headers.get("cookie").is_none());
}

#[tokio::test]
async fn bad_latest_answer_is_ignored() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/releases/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": "v1.0.0",
            "changelog_url": "javascript:alert(1)"
        })))
        .mount(&server)
        .await;
    let (mut state, _db) = common::state().await;
    with_check(&mut state, true, &server.uri());
    let admin = login(&state, "admin").await;
    let r = send(&state, &admin, "GET", "/api/system/version", None).await;
    assert_eq!(
        r.json(),
        json!({ "current": current(), "latest": "v1.0.0" }),
        "lien non https retire"
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{\"version\":\"latest\"}"))
        .mount(&server)
        .await;
    let (mut state, _db) = common::state().await;
    with_check(&mut state, true, &server.uri());
    let admin = login(&state, "admin").await;
    let r = send(&state, &admin, "GET", "/api/system/version", None).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json(), json!({ "current": current() }));
}

#[tokio::test]
async fn update_request_is_owner_only_reauthenticated_and_signed() {
    let tmp = TempDir::new();
    let (mut state, _db) = common::state_in(&tmp.0, &tmp.0).await;
    // Dossier surveille par l'unite .path = dossier de donnees des tests.
    state.update_request_dir = tmp.0.clone();
    let file = tmp.0.join(REQUEST_FILE);
    let body = |version: &str, pw: &str| Some(json!({ "version": version, "password": pw }));

    // Administrateur non proprietaire : 403.
    let admin = login(&state, "admin").await;
    let r = send(
        &state,
        &admin,
        "POST",
        "/api/system/update",
        body("v99.0.0", PASSWORD),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);

    let owner = login(&state, "owner").await;
    // Sans jeton CSRF : 403.
    let r = call(
        &state,
        "POST",
        "/api/system/update",
        Req {
            cookie: Some(&owner.0),
            body: body("v99.0.0", PASSWORD),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403);
    // Mot de passe faux : 401.
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/system/update",
        body("v99.0.0", "faux"),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    // Version mal formee, egale ou inferieure : 422.
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/system/update",
        body("latest", PASSWORD),
    )
    .await;
    assert_eq!(r.status, 422);
    for v in [current(), "v0.0.1".to_string()] {
        let r = send(
            &state,
            &owner,
            "POST",
            "/api/system/update",
            body(&v, PASSWORD),
        )
        .await;
        assert_eq!(r.status, 422, "{v} : {}", r.raw);
        assert_eq!(r.code(), "not_newer");
    }
    assert!(!file.exists(), "aucune demande deposee");

    let r = send(
        &state,
        &owner,
        "POST",
        "/api/system/update",
        body("v99.0.0", PASSWORD),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["version"], "v99.0.0");
    // Le panneau previent : en cas d'echec, retour arriere avec la base
    // sauvegardee juste avant, ecritures faites pendant la mise a jour perdues.
    let notice = r.json()["notice"].as_str().unwrap().to_string();
    assert!(
        notice.contains("60 s") && notice.contains("perdues"),
        "{notice}"
    );

    // Demande signee avec la cle de l'instance, ecrite sans fichier temporaire restant.
    let req = UpdateRequest::from_json(&std::fs::read(&file).unwrap()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(
        req.verify(&*request_key(&state.secrets), now)
            .unwrap()
            .to_string(),
        "v99.0.0"
    );
    let leftovers: Vec<_> = std::fs::read_dir(&tmp.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != REQUEST_FILE)
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    // Audit de la demande, sans mot de passe.
    let detail: Value = sqlx::query_scalar(
        "SELECT detail FROM audit_events WHERE action = 'system.update.request'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(detail["to"], "v99.0.0");
    assert!(!detail.to_string().contains(PASSWORD));
}

/// Le plan de controle et `ciphera-updater.path` partagent le dossier de la
/// demande (`/var/lib/ciphera`) : avec un autre dossier de donnees, la
/// demande ne serait jamais lue, donc refus explicite.
#[tokio::test]
async fn update_request_refused_when_the_data_dir_is_not_watched() {
    let tmp = TempDir::new();
    let (state, _db) = common::state_in(&tmp.0, &tmp.0).await;
    assert_eq!(
        state.update_request_dir,
        std::path::PathBuf::from(shared::update::REQUEST_DIR)
    );
    let owner = login(&state, "owner").await;
    let r = send(
        &state,
        &owner,
        "POST",
        "/api/system/update",
        Some(json!({ "version": "v99.0.0", "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 503, "{}", r.raw);
    assert_eq!(r.code(), "updates_unavailable");
    assert!(
        r.raw
            .contains("mises à jour indisponibles avec ce répertoire de données"),
        "{}",
        r.raw
    );
    assert!(!tmp.0.join(REQUEST_FILE).exists());
}
