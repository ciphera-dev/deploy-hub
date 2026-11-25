mod common;

use common::api::{Req, call};
use control_plane::auth::password;
use control_plane::state::AppState;
use serde_json::{Value, json};
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

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

/// Session ouverte : (cookie, jeton CSRF).
async fn login(state: &AppState, role: &str) -> (String, String) {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    user(state, &email, role).await;
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
) -> common::api::Reply {
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

fn git_project(name: &str) -> Value {
    json!({
        "name": name,
        "source": { "type": "git", "repository": "https://github.com/ciphera/web", "branch": "main" },
        "build": { "kind": "railpack" }
    })
}

#[tokio::test]
async fn create_project_creates_production_env() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;

    let r = send(
        &state,
        &dev,
        "POST",
        "/api/projects",
        Some(git_project("acme-web")),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let project = r.json();
    assert_eq!(project["name"], "acme-web");
    // Depot normalise en `proprietaire/nom`.
    assert_eq!(project["source"]["repository"], "ciphera/web");
    assert_eq!(project["source"]["branch"], "main");
    assert_eq!(project["build"]["kind"], "railpack");
    assert_eq!(project["status"], "none");
    let envs = project["environments"].as_array().unwrap();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0]["name"], "production");
    let services = project["services"].as_array().unwrap();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0]["name"], "acme-web");
    assert_eq!(services[0]["environment"], "production");

    let id = project["id"].as_str().unwrap();
    let got = send(&state, &dev, "GET", &format!("/api/projects/{id}"), None).await;
    assert_eq!(got.status, 200, "{}", got.raw);
    assert_eq!(got.json(), project);

    let list = send(&state, &dev, "GET", "/api/projects", None)
        .await
        .json();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], project["id"]);
    assert_eq!(list[0]["environment"], "production");
    assert_eq!(list[0]["services"], 1);
    assert_eq!(list[0]["source"]["repository"], "ciphera/web");

    // Meme nom : 409.
    let dup = send(
        &state,
        &dev,
        "POST",
        "/api/projects",
        Some(git_project("acme-web")),
    )
    .await;
    assert_eq!(dup.status, 409, "{}", dup.raw);
    assert_eq!(dup.code(), "name_taken");
}

#[tokio::test]
async fn create_project_for_each_build_and_image_source() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;

    let docker = json!({
        "name": "api",
        "source": { "type": "git", "repository": "ciphera/api", "branch": "feat/charts" },
        "build": { "kind": "dockerfile", "path": "docker/Dockerfile" }
    });
    let r = send(&state, &dev, "POST", "/api/projects", Some(docker)).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["build"]["path"], "docker/Dockerfile");

    let compose = json!({
        "name": "site",
        "source": { "type": "git", "repository": "ciphera/site", "branch": "main" },
        "build": { "kind": "compose", "path": "compose.yaml", "service": "web" }
    });
    let r = send(&state, &dev, "POST", "/api/projects", Some(compose)).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["build"]["service"], "web");
    assert_eq!(r.json()["services"][0]["name"], "web");

    let image = json!({
        "name": "cache",
        "source": { "type": "image", "image": "ghcr.io/org/app:1.2" }
    });
    let r = send(&state, &dev, "POST", "/api/projects", Some(image)).await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["source"]["image"], "ghcr.io/org/app:1.2");
    assert!(r.json()["build"].is_null());
}

#[tokio::test]
async fn image_project_internal_ref_refused() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;
    for image in [
        "ciphera.invalid/web-production:1",
        "ciphera.invalid/staging/x:build",
        "ciphera/web-production:1",
        "ciphera-staging/x:build",
        "docker.io/ciphera/web-production:1",
        "index.docker.io/ciphera-staging/x:build",
        "registry-1.docker.io/ciphera/web",
    ] {
        let body = json!({ "name": "cache", "source": { "type": "image", "image": image } });
        let r = send(&state, &dev, "POST", "/api/projects", Some(body)).await;
        assert_eq!(r.status, 422, "{image} : {}", r.raw);
        assert_eq!(r.code(), "validation_failed");
    }
    assert_eq!(
        send(&state, &dev, "GET", "/api/projects", None)
            .await
            .json(),
        json!([])
    );
}

#[tokio::test]
async fn project_name_rejects_injection() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;
    let too_long = "a".repeat(64);
    for name in [
        "a;rm -rf /",
        "../x",
        "",
        "-a",
        "A",
        "a b",
        "a\n",
        too_long.as_str(),
        // Prefixe reserve aux conteneurs et reseaux de Ciphera.
        "ciphera",
        "ciphera-prepare",
    ] {
        let r = send(
            &state,
            &dev,
            "POST",
            "/api/projects",
            Some(git_project(name)),
        )
        .await;
        assert_eq!(r.status, 422, "nom {name:?} : {}", r.raw);
        assert_eq!(r.code(), "validation_failed");
    }
    // 63 caracteres : accepte.
    let ok = send(
        &state,
        &dev,
        "POST",
        "/api/projects",
        Some(git_project(&"a".repeat(63))),
    )
    .await;
    assert_eq!(ok.status, 201, "{}", ok.raw);
}

#[tokio::test]
async fn source_and_build_inputs_are_validated() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;
    let git = |repository: &str, branch: &str, build: Value| json!({"name":"p","source":{"type":"git","repository":repository,"branch":branch},"build":build});
    let rp = || json!({"kind":"railpack"});
    let bad_bodies = [
        // depot : pas GitHub, pas https, caracteres interdits
        git("https://evil.example/o/r", "main", rp()),
        git("http://github.com/o/r", "main", rp()),
        git("o/r;id", "main", rp()),
        git("https://github.com/o/r/../x", "main", rp()),
        git("https://github.com/o/r?x=1", "main", rp()),
        // branche
        git("o/r", "main;id", rp()),
        git("o/r", "--upload-pack=x", rp()),
        git("o/r", "a..b", rp()),
        git("o/r", "a b", rp()),
        git("o/r", "", rp()),
        // chemins
        git(
            "o/r",
            "main",
            json!({"kind":"dockerfile","path":"../Dockerfile"}),
        ),
        git(
            "o/r",
            "main",
            json!({"kind":"dockerfile","path":"/etc/passwd"}),
        ),
        git("o/r", "main", json!({"kind":"dockerfile","path":"a b"})),
        git(
            "o/r",
            "main",
            json!({"kind":"compose","path":"c.yaml","service":"a;b"}),
        ),
        // image
        json!({"name":"p","source":{"type":"image","image":"nginx; rm -rf /"}}),
        json!({"name":"p","source":{"type":"image","image":"nginx $(id)"}}),
        json!({"name":"p","source":{"type":"image","image":"ng`id`inx"}}),
        json!({"name":"p","source":{"type":"image","image":""}}),
        // incoherences source / build
        json!({"name":"p","source":{"type":"git","repository":"o/r","branch":"main"}}),
        json!({"name":"p","source":{"type":"image","image":"nginx"},"build":{"kind":"railpack"}}),
        // champ inconnu
        json!({"name":"p","extra":1,"source":{"type":"image","image":"nginx"}}),
    ];
    for body in bad_bodies {
        let r = send(&state, &dev, "POST", "/api/projects", Some(body.clone())).await;
        assert_eq!(r.status, 422, "{body} : {}", r.raw);
    }
    assert_eq!(
        send(&state, &dev, "GET", "/api/projects", None)
            .await
            .json(),
        json!([])
    );
}

#[tokio::test]
async fn viewer_cannot_create_403() {
    let (state, _pg) = common::state().await;
    let viewer = login(&state, "viewer").await;
    let r = send(
        &state,
        &viewer,
        "POST",
        "/api/projects",
        Some(git_project("p")),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "forbidden");
    // Lecture permise.
    assert_eq!(
        send(&state, &viewer, "GET", "/api/projects", None)
            .await
            .status,
        200
    );
    // Sans session : 401.
    let r = call(&state, "GET", "/api/projects", Req::default()).await;
    assert_eq!(r.status, 401);
}

#[tokio::test]
async fn writes_require_csrf_token() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;
    let r = call(
        &state,
        "POST",
        "/api/projects",
        Req {
            cookie: Some(&dev.0),
            body: Some(git_project("p")),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
}

#[tokio::test]
async fn delete_requires_admin() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;
    let admin = login(&state, "admin").await;
    let created = send(
        &state,
        &dev,
        "POST",
        "/api/projects",
        Some(git_project("p")),
    )
    .await;
    let id = created.json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/projects/{id}");

    let r = send(&state, &dev, "DELETE", &path, None).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(send(&state, &dev, "GET", &path, None).await.status, 200);

    let r = send(&state, &admin, "DELETE", &path, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(send(&state, &admin, "GET", &path, None).await.status, 404);
    // Cascade : plus d'environnement, de service ni de source Git.
    for table in ["environments", "services", "git_sources"] {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    assert_eq!(
        send(&state, &admin, "DELETE", &path, None).await.status,
        404
    );
}

#[tokio::test]
async fn patch_updates_name_branch_and_build() {
    let (state, _pg) = common::state().await;
    let dev = login(&state, "developer").await;
    let viewer = login(&state, "viewer").await;
    let created = send(
        &state,
        &dev,
        "POST",
        "/api/projects",
        Some(git_project("p")),
    )
    .await;
    let id = created.json()["id"].as_str().unwrap().to_string();
    let path = format!("/api/projects/{id}");

    let r = send(
        &state,
        &dev,
        "PATCH",
        &path,
        Some(json!({"name":"renamed","branch":"develop","build":{"kind":"dockerfile","path":"Dockerfile"}})),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let p = r.json();
    assert_eq!(p["name"], "renamed");
    assert_eq!(p["source"]["branch"], "develop");
    assert_eq!(p["build"]["kind"], "dockerfile");
    assert_eq!(p["services"][0]["name"], "renamed");

    for bad in [
        json!({"name":"a;b"}),
        json!({"branch":"x y"}),
        json!({"build":{"kind":"dockerfile","path":"../x"}}),
        json!({"role":"owner"}),
        json!({}),
    ] {
        let r = send(&state, &dev, "PATCH", &path, Some(bad.clone())).await;
        assert_eq!(r.status, 422, "{bad} : {}", r.raw);
    }
    assert_eq!(
        send(&state, &viewer, "PATCH", &path, Some(json!({"name":"x"})))
            .await
            .status,
        403
    );
    let missing = format!("/api/projects/{}", Uuid::new_v4());
    assert_eq!(
        send(&state, &dev, "PATCH", &missing, Some(json!({"name":"x"})))
            .await
            .status,
        404
    );
    assert_eq!(
        send(&state, &dev, "GET", "/api/projects/not-a-uuid", None)
            .await
            .status,
        404
    );
}
