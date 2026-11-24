//! Matrice d'autorisation : chaque endpoint de `api_types::openapi()` (et
//! chaque route du routeur absente du document) est appele sans session et
//! avec chaque role, et le statut est compare a `authz::allowed`.
//!
//! Lecture des resultats :
//! - « sans session » : cookie de session forge et jeton CSRF qui lui
//!   correspond (la couche CSRF laisse passer : c'est bien l'extracteur de
//!   session qui doit repondre 401 `unauthenticated`) ;
//! - role refuse : 403 `forbidden` ;
//! - role autorise : ni 401 ni 403. Les identifiants des chemins n'existent
//!   pas et les corps sont vides : l'appel atteint le gestionnaire, qui
//!   repond 2xx, 400, 404, 409, 415 ou 422 selon le cas, sans effet de bord
//!   notable.
//!
//! Un endpoint ajoute a `openapi()` sans ligne dans `MATRIX` fait echouer
//! `matrix_covers_openapi_exactly`.

mod common;

use std::collections::BTreeSet;

use common::api::{Req, call};
use control_plane::auth::{csrf, password};
use control_plane::authz::{self, Level, Resource, Role};
use control_plane::state::AppState;
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const HOST: &str = "panel.example";
const ORIGIN: &str = "https://panel.example";

const ROLES: [Role; 5] = [
    Role::Owner,
    Role::Admin,
    Role::Ops,
    Role::Developer,
    Role::Viewer,
];

/// Garde attendue d'un endpoint.
#[derive(Clone, Copy, Debug)]
enum Guard {
    /// Sans session, par conception (raison donnee).
    Public(&'static str),
    /// Session complete, quel que soit le role (compte connecte lui-meme).
    Session,
    /// `Require<R, L>` : `authz::allowed(role, R, L)`.
    Role(Resource, Level),
    /// `Require<Settings, Admin>` puis `owner_only` : proprietaire seulement.
    Owner,
}

use Guard::{Owner, Public, Role as R, Session};
use Level::{Admin as A, Read as Rd, Write as W};
use Resource::*;

const SETUP: Guard = Public("assistant d'installation : jeton X-Setup-Token, avant tout compte");
const AUTH: Guard = Public("connexion, MFA, inscription, invitation, mot de passe oublie");

/// Endpoints de `openapi()`.
const MATRIX: &[(&str, &str, Guard)] = &[
    ("GET", "/api/health", Public("sonde de sante, sans donnee")),
    ("GET", "/api/setup/status", SETUP),
    ("POST", "/api/setup/checks", SETUP),
    ("POST", "/api/setup/complete", SETUP),
    ("POST", "/api/auth/login", AUTH),
    ("POST", "/api/auth/logout", AUTH),
    ("GET", "/api/auth/mfa/methods", AUTH),
    ("POST", "/api/auth/mfa/verify", AUTH),
    ("POST", "/api/auth/recovery-code", AUTH),
    ("GET", "/api/auth/webauthn/status", AUTH),
    ("POST", "/api/auth/webauthn/assert/options", AUTH),
    ("POST", "/api/auth/webauthn/assert/verify", AUTH),
    ("GET", "/api/auth/invitations/{token}", AUTH),
    ("POST", "/api/auth/invitations/{token}/accept", AUTH),
    ("GET", "/api/auth/register", AUTH),
    ("POST", "/api/auth/register", AUTH),
    ("GET", "/api/auth/password/forgot", AUTH),
    ("POST", "/api/auth/password/forgot", AUTH),
    ("GET", "/api/auth/password/reset/{token}", AUTH),
    ("POST", "/api/auth/password/reset", AUTH),
    (
        "POST",
        "/v1/hooks/deploy/{hookId}",
        Public("deploy hook : jeton dh_ dans l'URL, compare a temps constant"),
    ),
    // Compte connecte.
    ("GET", "/api/me", Session),
    ("PATCH", "/api/me", Session),
    ("GET", "/api/me/avatar", Session),
    ("PUT", "/api/me/avatar", Session),
    ("DELETE", "/api/me/avatar", Session),
    ("POST", "/api/me/password", Session),
    ("GET", "/api/me/sessions", Session),
    ("DELETE", "/api/me/sessions/{id}", Session),
    ("GET", "/api/me/notification-prefs", Session),
    ("PUT", "/api/me/notification-prefs", Session),
    ("POST", "/api/me/mfa/totp/start", Session),
    ("POST", "/api/me/mfa/totp/confirm", Session),
    ("DELETE", "/api/me/mfa/totp", Session),
    ("GET", "/api/me/webauthn", Session),
    ("POST", "/api/me/webauthn/register/options", Session),
    ("POST", "/api/me/webauthn/register/verify", Session),
    ("DELETE", "/api/me/webauthn/{id}", Session),
    // Equipe.
    ("GET", "/api/team/members", R(Team, A)),
    ("PATCH", "/api/team/members/{id}", R(Team, A)),
    ("DELETE", "/api/team/members/{id}", R(Team, A)),
    ("GET", "/api/team/invitations", R(Team, A)),
    ("POST", "/api/team/invitations", R(Team, A)),
    ("DELETE", "/api/team/invitations/{id}", R(Team, A)),
    ("POST", "/api/team/members/{id}/reset-link", R(Team, A)),
    // Projets.
    ("GET", "/api/overview", R(Projects, Rd)),
    ("GET", "/api/projects", R(Projects, Rd)),
    ("POST", "/api/projects", R(Projects, W)),
    ("GET", "/api/projects/{id}", R(Projects, Rd)),
    ("PATCH", "/api/projects/{id}", R(Projects, W)),
    ("DELETE", "/api/projects/{id}", R(Projects, A)),
    ("GET", "/api/projects/{id}/git", R(Projects, Rd)),
    ("PATCH", "/api/projects/{id}/git", R(Projects, W)),
    ("GET", "/api/projects/{id}/deploy-hooks", R(Projects, A)),
    ("POST", "/api/projects/{id}/deploy-hooks", R(Projects, A)),
    (
        "DELETE",
        "/api/projects/{id}/deploy-hooks/{hookId}",
        R(Projects, A),
    ),
    ("GET", "/api/projects/{id}/overview", R(Projects, Rd)),
    ("GET", "/api/projects/{id}/services", R(Projects, Rd)),
    ("GET", "/api/git/providers/github/repos", R(Projects, W)),
    // Deploiements.
    ("POST", "/api/projects/{id}/deployments", R(Deployments, W)),
    ("POST", "/api/projects/{id}/restart", R(Deployments, W)),
    ("POST", "/api/projects/{id}/redeploy", R(Deployments, W)),
    (
        "POST",
        "/api/projects/{id}/services/{sid}/restart",
        R(Deployments, W),
    ),
    (
        "POST",
        "/api/projects/{id}/services/{sid}/stop",
        R(Deployments, W),
    ),
    ("GET", "/api/deployments", R(Deployments, Rd)),
    ("GET", "/api/deployments/{id}", R(Deployments, Rd)),
    ("POST", "/api/deployments/{id}/cancel", R(Deployments, W)),
    ("POST", "/api/deployments/{id}/rollback", R(Deployments, W)),
    // Variables.
    ("GET", "/api/projects/{id}/variables", R(Variables, Rd)),
    ("PUT", "/api/projects/{id}/variables", R(Variables, W)),
    ("PUT", "/api/projects/{id}/variables/bulk", R(Variables, W)),
    (
        "GET",
        "/api/projects/{id}/variables/status",
        R(Variables, Rd),
    ),
    (
        "DELETE",
        "/api/projects/{id}/variables/{key}",
        R(Variables, W),
    ),
    ("GET", "/api/variables", R(Variables, A)),
    ("POST", "/api/variables", R(Variables, A)),
    ("GET", "/api/variables/export", R(Variables, A)),
    ("PATCH", "/api/variables/{id}", R(Variables, A)),
    ("DELETE", "/api/variables/{id}", R(Variables, A)),
    // Domaines.
    ("GET", "/api/domains", R(Domains, Rd)),
    ("POST", "/api/domains", R(Domains, W)),
    ("PATCH", "/api/domains", R(Domains, W)),
    ("PATCH", "/api/domains/{id}", R(Domains, W)),
    ("DELETE", "/api/domains/{id}", R(Domains, W)),
    ("POST", "/api/domains/{id}/verify", R(Domains, W)),
    ("POST", "/api/domains/{id}/renew", R(Domains, W)),
    ("POST", "/api/domains/{id}/primary", R(Domains, W)),
    ("POST", "/api/domains/{id}/redirect", R(Domains, W)),
    // Parametres de l'instance.
    ("GET", "/api/settings/general", R(Settings, A)),
    ("PATCH", "/api/settings/general", R(Settings, A)),
    ("GET", "/api/settings/deployments", R(Settings, A)),
    ("PATCH", "/api/settings/deployments", R(Settings, A)),
    ("GET", "/api/settings/security", R(Settings, A)),
    ("PATCH", "/api/settings/security", R(Settings, A)),
    ("GET", "/api/settings/notifications", R(Settings, A)),
    ("PUT", "/api/settings/notifications", R(Settings, A)),
    ("GET", "/api/connectors/github", R(Settings, A)),
    ("DELETE", "/api/connectors/github", R(Settings, A)),
    ("POST", "/api/connectors/github/manifest", R(Settings, A)),
    ("GET", "/api/connectors/github/callback", R(Settings, A)),
    ("GET", "/api/system/version", R(Settings, A)),
    // Proprietaire seulement.
    ("GET", "/api/settings/danger", Owner),
    ("GET", "/api/settings/panel-domain", Owner),
    ("PUT", "/api/settings/panel-domain", Owner),
    ("POST", "/api/settings/panel-domain/confirm", Owner),
    ("DELETE", "/api/settings/panel-domain", Owner),
    ("POST", "/api/organization/export", Owner),
    ("DELETE", "/api/organization", Owner),
    ("POST", "/api/system/update", Owner),
];

/// Routes du routeur absentes de `openapi()` (WebSocket, telechargement,
/// webhook) : verifiees de la meme facon. Liste complete : controlee par
/// `every_router_route_is_in_the_matrix` (routes lues dans `src/`).
const OUTSIDE_OPENAPI: &[(&str, &str, Guard)] = &[
    ("GET", "/api/ws/logs", R(Logs, Rd)),
    ("GET", "/api/deployments/{id}/logs.txt", R(Logs, Rd)),
    (
        "POST",
        "/v1/webhooks/github",
        Public("webhook GitHub : signature HMAC-SHA256 X-Hub-Signature-256"),
    ),
];

/// Sans session par conception, mais hors du plan de controle : rien a
/// appeler ici, liste pour qu'aucune exemption ne reste implicite
/// (`control_plane_does_not_serve_acme_challenges` : 404 ici).
#[allow(dead_code)]
const EXEMPT_ELSEWHERE: &[(&str, &str)] = &[(
    "/.well-known/acme-challenge/{token}",
    "defi ACME http-01 servi par le proxy (crates/proxy), jeton public par nature",
)];

/// Seuls chemins `/api/*` admis sans session.
fn session_free_api_path(path: &str) -> bool {
    path == "/api/health" || path.starts_with("/api/setup/") || path.starts_with("/api/auth/")
}

fn documented() -> BTreeSet<(String, String)> {
    let doc = serde_json::to_value(api_types::openapi()).unwrap();
    let mut ops = BTreeSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for method in item.as_object().unwrap().keys() {
            if [
                "get", "put", "post", "delete", "patch", "head", "options", "trace",
            ]
            .contains(&method.as_str())
            {
                ops.insert((method.to_ascii_uppercase(), path.clone()));
            }
        }
    }
    ops
}

#[test]
fn matrix_covers_openapi_exactly() {
    let documented = documented();
    let mut listed = BTreeSet::new();
    for (method, path, _) in MATRIX {
        assert!(
            listed.insert((method.to_string(), path.to_string())),
            "ligne en double : {method} {path}"
        );
    }
    let missing: Vec<_> = documented.difference(&listed).collect();
    assert!(
        missing.is_empty(),
        "endpoints de openapi() sans ligne dans MATRIX : {missing:?}"
    );
    let stale: Vec<_> = listed.difference(&documented).collect();
    assert!(
        stale.is_empty(),
        "lignes de MATRIX hors openapi() : {stale:?}"
    );
    for (method, path, _) in OUTSIDE_OPENAPI {
        assert!(
            !documented.contains(&(method.to_string(), path.to_string())),
            "{method} {path} est documente : le deplacer dans MATRIX"
        );
    }
}

#[test]
fn only_health_setup_and_auth_are_session_free_under_api() {
    for (method, path, guard) in MATRIX.iter().chain(OUTSIDE_OPENAPI) {
        if let Public(reason) = guard {
            assert!(!reason.is_empty(), "{method} {path} : raison manquante");
            assert!(
                session_free_api_path(path) || !path.starts_with("/api/"),
                "{method} {path} ne peut pas etre sans session"
            );
        }
    }
}

fn expected_allowed(guard: Guard, role: Role) -> bool {
    match guard {
        Public(_) | Session => true,
        R(resource, level) => authz::allowed(role, resource, level),
        Owner => authz::allowed(role, Settings, A) && role == Role::Owner,
    }
}

/// Chemin concret : identifiants inexistants.
fn concrete(path: &str) -> String {
    path.split('/')
        .map(|segment| match segment {
            "{token}" => "inconnu".to_string(),
            "{key}" => "INCONNUE".to_string(),
            s if s.starts_with('{') => Uuid::new_v4().to_string(),
            s => s.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

async fn session(state: &AppState, role: Role, n: usize) -> (String, String) {
    let email = format!("{}@example.com", role.as_str());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, $4, 'Test')",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role.as_str())
    .execute(&state.db)
    .await
    .unwrap();
    // Un pair par connexion : aucune limite de debit partagee.
    let peer = format!("203.0.113.{}:50000", 10 + n);
    let r = call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            peer: Some(&peer),
            body: Some(json!({ "email": email, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "connexion {role:?} : {}", r.raw);
    (r.token(), r.csrf())
}

fn headers<'a>() -> Vec<(&'a str, String)> {
    vec![("host", HOST.to_string()), ("origin", ORIGIN.to_string())]
}

#[tokio::test]
async fn every_endpoint_enforces_its_guard() {
    let (state, _pg) = common::state().await;
    let mut sessions = Vec::new();
    for (n, role) in ROLES.into_iter().enumerate() {
        sessions.push((role, session(&state, role, n).await));
    }
    let forged = "sans-session-valide";
    let forged_csrf = csrf::token_for(forged);

    let mut failures = Vec::new();
    for &(method, template, guard) in MATRIX.iter().chain(OUTSIDE_OPENAPI) {
        let path = concrete(template);

        // Sans session.
        let r = match guard {
            // Appele tel quel, sans cookie : il ne doit pas echouer en 5xx.
            Public(_) => {
                call(
                    &state,
                    method,
                    &path,
                    Req {
                        headers: headers(),
                        ..Req::default()
                    },
                )
                .await
            }
            _ => {
                call(
                    &state,
                    method,
                    &path,
                    Req {
                        cookie: Some(forged),
                        csrf: Some(&forged_csrf),
                        headers: headers(),
                        ..Req::default()
                    },
                )
                .await
            }
        };
        match guard {
            Public(_) if r.status >= 500 => {
                failures.push(format!("{method} {template} sans session : {}", r.status))
            }
            Public(_) => {}
            _ if r.status != 401 || r.code() != "unauthenticated" => failures.push(format!(
                "{method} {template} sans session : {} {} (401 attendu)",
                r.status, r.raw
            )),
            _ => {}
        }
        if let Public(_) = guard {
            continue;
        }

        // Chaque role.
        for (role, (token, csrf_token)) in &sessions {
            let r = call(
                &state,
                method,
                &path,
                Req {
                    cookie: Some(token),
                    csrf: Some(csrf_token),
                    headers: headers(),
                    // Ajouter un second facteur redemande le mot de passe.
                    body: matches!(
                        template,
                        "/api/me/mfa/totp/start" | "/api/me/webauthn/register/options"
                    )
                    .then(|| json!({ "current_password": PASSWORD })),
                    ..Req::default()
                },
            )
            .await;
            let allowed = expected_allowed(guard, *role);
            let ok = if allowed {
                r.status != 401 && r.status != 403 && r.status < 500
            } else {
                r.status == 403 && r.code() == "forbidden"
            };
            if !ok {
                failures.push(format!(
                    "{method} {template} {role:?} : {} {} ({})",
                    r.status,
                    r.raw,
                    if allowed { "autorise" } else { "403 attendu" }
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Routes declarees dans le code (`.route("chemin", get(..).post(..))`),
/// lues dans `src/` : le routeur axum n'expose pas sa table. Parametres
/// normalises (`{}`), comme pour la comparaison avec `openapi()`.
fn declared_routes() -> BTreeSet<(String, String)> {
    fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut routes = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (start, _) in text.match_indices(".route(") {
            let args = &text[start + ".route(".len()..];
            // Fin de l'appel : parenthese fermante au meme niveau.
            let mut depth = 1;
            let end = args
                .char_indices()
                .find_map(|(i, c)| {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    (depth == 0).then_some(i)
                })
                .unwrap();
            let args = &args[..end];
            let Some(open) = args.find('"') else { continue };
            let rest = &args[open + 1..];
            let close = rest.find('"').unwrap();
            let path = normalize(&rest[..close]);
            let handlers = &rest[close + 1..];
            let mut found = false;
            for method in ["get", "post", "put", "patch", "delete"] {
                for (i, _) in handlers.match_indices(&format!("{method}(")) {
                    let before = handlers[..i].chars().last();
                    if before.is_none_or(|c| !c.is_alphanumeric() && c != '_') {
                        routes.insert((method.to_ascii_uppercase(), path.clone()));
                        found = true;
                    }
                }
            }
            assert!(
                found,
                "{} : route sans methode lue ({path})",
                file.display()
            );
        }
    }
    routes
}

fn normalize(path: &str) -> String {
    path.split('/')
        .map(|s| if s.starts_with('{') { "{}" } else { s })
        .collect::<Vec<_>>()
        .join("/")
}

/// Toute route du routeur est soit dans `openapi()` (donc `MATRIX`), soit
/// dans `OUTSIDE_OPENAPI` : aucune route ne peut echapper a la matrice.
#[test]
fn every_router_route_is_in_the_matrix() {
    let mut known: BTreeSet<(String, String)> = documented()
        .into_iter()
        .map(|(m, p)| (m, normalize(&p)))
        .collect();
    for (method, path, _) in OUTSIDE_OPENAPI {
        known.insert((method.to_string(), normalize(path)));
    }
    let declared = declared_routes();
    assert!(declared.len() > 50, "lecture des routes : {declared:?}");
    let missing: Vec<_> = declared.difference(&known).collect();
    assert!(
        missing.is_empty(),
        "routes du routeur hors openapi() et hors OUTSIDE_OPENAPI : {missing:?}"
    );
    let unrouted: Vec<_> = known.difference(&declared).collect();
    assert!(unrouted.is_empty(), "lignes sans route : {unrouted:?}");
}

/// Le defi ACME http-01 est servi par le proxy du noeud, jamais par le plan
/// de controle : ni jeton, ni repli de l'interface sur ce chemin.
#[tokio::test]
async fn control_plane_does_not_serve_acme_challenges() {
    let (state, _pg) = common::state().await;
    for (method, path) in [
        ("GET", "/.well-known/acme-challenge/inconnu"),
        ("GET", "/.well-known/acme-challenge/"),
    ] {
        let r = call(
            &state,
            method,
            path,
            Req {
                headers: headers(),
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 404, "{method} {path} : {} {}", r.raw, r.status);
    }
}

/// Sans aucun cookie ni jeton CSRF : jamais de reponse 2xx d'une route
/// protegee (401 `unauthenticated`, ou 403 CSRF pour une ecriture).
#[tokio::test]
async fn protected_routes_refuse_a_request_without_cookie() {
    let (state, _pg) = common::state().await;
    let mut failures = Vec::new();
    for &(method, template, guard) in MATRIX.iter().chain(OUTSIDE_OPENAPI) {
        if let Public(_) = guard {
            continue;
        }
        let r = call(
            &state,
            method,
            &concrete(template),
            Req {
                headers: headers(),
                ..Req::default()
            },
        )
        .await;
        let refused = (r.status == 401 && r.code() == "unauthenticated")
            || (method != "GET"
                && r.status == 403
                && r.code().as_str().is_some_and(|c| c.starts_with("csrf")));
        if !refused {
            failures.push(format!("{method} {template} : {} {}", r.status, r.raw));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
