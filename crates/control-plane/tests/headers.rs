mod common;

use common::api::{Reply, Req, call};
use common::{Resp, plain_get, self_signed, serve_https, serve_redirect, state, tls_get};
use control_plane::auth::password;
use control_plane::domain::domains::panel::{Panel, PanelSwitch};
use control_plane::state::AppState;
use serde_json::json;
use uuid::Uuid;

const QUOTE: char = 39u8 as char;
const CSP_START: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'";
const CSP_END: &str = "; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self' https://github.com; object-src 'none'";

/// Nonce de `style-src` d'une page HTML (la politique entiere est verifiee).
fn style_nonce(r: &Resp) -> String {
    let csp = r.header("content-security-policy").unwrap();
    let mid = csp
        .strip_prefix(CSP_START)
        .and_then(|rest| rest.strip_suffix(CSP_END))
        .unwrap_or_else(|| panic!("CSP inattendue : {csp}"));
    let nonce = mid
        .strip_prefix(" 'nonce-")
        .and_then(|n| n.strip_suffix(QUOTE))
        .unwrap_or_else(|| panic!("nonce absent : {csp}"));
    assert!(!csp.contains("unsafe-inline"));
    nonce.to_string()
}

fn assert_security_headers(r: &Resp, what: &str) {
    let csp = r.header("content-security-policy").unwrap_or_default();
    assert!(
        csp == format!("{CSP_START}{CSP_END}") || csp.starts_with(&format!("{CSP_START} 'nonce-")),
        "{what} : {csp}"
    );
    assert_eq!(
        r.header("strict-transport-security"),
        Some("max-age=31536000"),
        "{what}"
    );
    assert_eq!(
        r.header("x-content-type-options"),
        Some("nosniff"),
        "{what}"
    );
    assert_eq!(r.header("referrer-policy"), Some("no-referrer"), "{what}");
}

#[tokio::test]
async fn security_headers_present() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    for path in [
        "/api/health",
        "/api/nope",
        "/",
        "/projects/abc",
        "/assets/missing.js",
    ] {
        let r = tls_get(addr, &cert, path).await;
        assert_security_headers(&r, path);
    }
    let (redir, _h2) = serve_redirect(8443).await;
    let r = plain_get(redir, "localhost", "/").await;
    assert_security_headers(&r, "redirection");
    let r = plain_get(redir, "bad host", "/").await;
    assert_eq!(r.status, 400);
    assert_security_headers(&r, "redirection 400");
}

#[tokio::test]
async fn html_nonce_is_present_and_differs_per_response() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    let a = tls_get(addr, &cert, "/").await;
    let b = tls_get(addr, &cert, "/projects/abc").await;
    let (na, nb) = (style_nonce(&a), style_nonce(&b));
    assert_ne!(na, nb);
    assert_eq!(na.len(), 43);
    // Le meme nonce est dans la page, pour la lecture au demarrage.
    let html = String::from_utf8(a.body.clone()).unwrap();
    assert!(html.contains(&format!("content=\"{na}\"")), "{html}");
    // L'API et les fichiers statiques gardent la politique sans nonce.
    let api = tls_get(addr, &cert, "/api/health").await;
    assert_eq!(
        api.header("content-security-policy"),
        Some(format!("{CSP_START}{CSP_END}").as_str())
    );
}

#[tokio::test]
async fn csp_has_base_uri_form_action_object_src() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    for path in ["/api/health", "/", "/api/nope"] {
        let r = tls_get(addr, &cert, path).await;
        let csp = r.header("content-security-policy").unwrap();
        for directive in [
            "base-uri 'none'",
            "form-action 'self' https://github.com;",
            "object-src 'none'",
        ] {
            assert!(
                csp.contains(directive),
                "{path} : {directive} absent de {csp}"
            );
        }
        let form_action = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("form-action"))
            .unwrap();
        assert_eq!(form_action, "form-action 'self' https://github.com");
        assert!(
            !form_action.contains('*'),
            "{path} : joker dans {form_action}"
        );
    }
}

const HSTS: &str = "max-age=31536000";
const HSTS_SUBDOMAINS: &str = "max-age=31536000; includeSubDomains";
const PASSWORD: &str = "correct horse battery staple";

/// Domaine du panneau en memoire (confirme), sans passer par l'emission.
fn panel(state: &AppState, host: &str) {
    state.panel.set(Some(Panel {
        id: Uuid::new_v4(),
        host: host.into(),
        confirmed: true,
    }));
}

/// `Strict-Transport-Security` d'une reponse recue sous l'hote `host`.
async fn hsts_for(state: &AppState, path: &str, host: &str) -> String {
    let r = call(
        state,
        "GET",
        path,
        Req {
            host: Some(host),
            ..Req::default()
        },
    )
    .await;
    r.headers
        .get("strict-transport-security")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_else(|| panic!("{path} sous {host} : HSTS absent"))
        .to_string()
}

/// Session d'un utilisateur du role donne : (cookie, jeton CSRF).
async fn login(state: &AppState, role: &str) -> (String, String) {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, 'Test', now())",
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

async fn set_hsts_option(state: &AppState, s: &(String, String), on: bool) -> Reply {
    call(
        state,
        "PUT",
        "/api/settings/panel-domain/hsts",
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            body: Some(json!({ "include_subdomains": on })),
            ..Req::default()
        },
    )
    .await
}

async fn with_instance(state: &AppState) {
    sqlx::query("INSERT INTO instance (id, name) VALUES ($1, 'Org')")
        .bind(Uuid::new_v4())
        .execute(&state.db)
        .await
        .unwrap();
}

/// Hote du panneau strictement sous son domaine enregistrable : les
/// sous-domaines du panneau sont couverts d'office. Tout autre hote (acces
/// direct par l'IP, autre nom) garde la politique de base.
#[tokio::test]
async fn hsts_subdomains_for_sub_panel() {
    let (state, _pg) = state().await;
    panel(&state, "panel.example.org");
    for path in ["/api/health", "/", "/api/nope"] {
        assert_eq!(
            hsts_for(&state, path, "panel.example.org").await,
            HSTS_SUBDOMAINS,
            "{path}"
        );
        assert_eq!(
            hsts_for(&state, path, "Panel.Example.org.:443").await,
            HSTS_SUBDOMAINS,
            "{path} : casse, point final et port"
        );
    }
    for other in ["203.0.113.7:8443", "localhost", "other.example.org"] {
        assert_eq!(
            hsts_for(&state, "/api/health", other).await,
            HSTS,
            "{other}"
        );
    }
    // Suffixe public a plusieurs niveaux : `panel.example.co.uk` est sous
    // `example.co.uk`.
    panel(&state, "panel.example.co.uk");
    assert_eq!(
        hsts_for(&state, "/api/health", "panel.example.co.uk").await,
        HSTS_SUBDOMAINS
    );
    // Domaine enregistrable lui-meme (`example.co.uk`) : rien sans l'option.
    panel(&state, "example.co.uk");
    assert_eq!(hsts_for(&state, "/api/health", "example.co.uk").await, HSTS);
}

/// Panneau sur un domaine enregistrable (`example.org`) : includeSubDomains
/// seulement apres l'option du proprietaire (desactivee par defaut).
#[tokio::test]
async fn hsts_apex_requires_opt_in() {
    let (state, _pg) = state().await;
    with_instance(&state).await;
    panel(&state, "example.org");
    assert_eq!(hsts_for(&state, "/", "example.org").await, HSTS);

    let owner = login(&state, "owner").await;
    let r = call(
        &state,
        "GET",
        "/api/settings/panel-domain",
        Req {
            cookie: Some(&owner.0),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.json()["hsts_include_subdomains"], false, "{}", r.raw);

    // Proprietaire seulement.
    let admin = login(&state, "admin").await;
    let r = set_hsts_option(&state, &admin, true).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(hsts_for(&state, "/", "example.org").await, HSTS);

    let r = set_hsts_option(&state, &owner, true).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["hsts_include_subdomains"], true, "{}", r.raw);
    assert_eq!(hsts_for(&state, "/", "example.org").await, HSTS_SUBDOMAINS);
    // L'option ne couvre que le domaine du panneau.
    assert_eq!(hsts_for(&state, "/", "203.0.113.7").await, HSTS);
    // Gardee en base : un redemarrage la reprend.
    let reloaded = PanelSwitch::load(&state.db).await.unwrap();
    assert!(reloaded.hsts_include_subdomains());

    let r = set_hsts_option(&state, &owner, false).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(hsts_for(&state, "/", "example.org").await, HSTS);
}

/// Jamais `preload` (inscription irreversible dans les navigateurs), quels
/// que soient l'hote et l'option.
#[tokio::test]
async fn hsts_never_preload() {
    let (state, _pg) = state().await;
    with_instance(&state).await;
    let owner = login(&state, "owner").await;
    assert_eq!(set_hsts_option(&state, &owner, true).await.status, 200);
    for host in ["example.org", "panel.example.org"] {
        panel(&state, host);
        for target in [host, "203.0.113.7", "localhost"] {
            for path in ["/api/health", "/", "/api/nope"] {
                let v = hsts_for(&state, path, target).await;
                assert!(
                    !v.to_ascii_lowercase().contains("preload"),
                    "{host} {target} {path} : {v}"
                );
            }
        }
    }
    let (redir, _h) = serve_redirect(8443).await;
    let r = plain_get(redir, "example.org", "/").await;
    let v = r.header("strict-transport-security").unwrap();
    assert!(!v.contains("preload"), "{v}");
}
