mod common;

use common::{Resp, plain_get, self_signed, serve_https, serve_redirect, state, tls_get};

fn assert_security_headers(r: &Resp, what: &str) {
    assert_eq!(
        r.header("content-security-policy"),
        Some(
            "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'"
        ),
        "{what}"
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
