mod common;

use common::{Resp, plain_get, self_signed, serve_https, serve_redirect, state, tls_get};

const QUOTE: char = 39u8 as char;
const CSP_START: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'";
const CSP_END: &str = "; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'; object-src 'none'";

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
        for directive in ["base-uri 'none'", "form-action 'self'", "object-src 'none'"] {
            assert!(
                csp.contains(directive),
                "{path} : {directive} absent de {csp}"
            );
        }
    }
}
