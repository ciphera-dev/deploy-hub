//! Interface web embarquee (`web/dist`).

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use std::borrow::Cow;

use crate::http::security_headers::CspNonce;

/// Marqueur de `web/index.html` remplace a chaque reponse par le nonce.
pub const NONCE_PLACEHOLDER: &str = "__CSP_NONCE__";

pub struct Asset {
    pub data: Cow<'static, [u8]>,
}

#[cfg(feature = "embed")]
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Dist;

#[cfg(feature = "embed")]
fn embedded(path: &str) -> Option<Asset> {
    Dist::get(path).map(|f| Asset { data: f.data })
}

#[cfg(not(feature = "embed"))]
fn embedded(_path: &str) -> Option<Asset> {
    None
}

/// Reponse pour un chemin hors `/api` : fichier statique, sinon `index.html`
/// (routes du navigateur), sinon 503 si l'interface n'est pas compilee.
pub fn serve(path: &str) -> Response {
    respond(path, embedded)
}

pub(crate) fn respond(path: &str, lookup: impl Fn(&str) -> Option<Asset>) -> Response {
    let rel = path.trim_start_matches('/');
    // Un fichier existant (JS, CSS, favicon...) est servi tel quel.
    if !rel.is_empty()
        && let Some(asset) = lookup(rel)
    {
        let cache = if rel.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return (
            [
                (header::CONTENT_TYPE, mime_for(rel)),
                (header::CACHE_CONTROL, cache),
            ],
            asset.data.into_owned(),
        )
            .into_response();
    }
    // Un chemin qui ressemble a un fichier (extension) mais absent : 404 net.
    if rel
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    match lookup("index.html") {
        Some(index) => html_with_nonce(&index.data),

        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "interface non compilée",
        )
            .into_response(),
    }
}

/// Page HTML avec un nonce neuf (32 octets aleatoires) : il remplace le
/// marqueur de la balise `csp-nonce` et part dans `style-src` (voir
/// `security_headers`). Sans source d'aleas, la page est refusee.
fn html_with_nonce(index: &[u8]) -> Response {
    let Some(nonce) = new_nonce() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let html = String::from_utf8_lossy(index).replace(NONCE_PLACEHOLDER, &nonce);
    let mut response = (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response();
    response.extensions_mut().insert(CspNonce(nonce));
    response
}

fn new_nonce() -> Option<String> {
    let mut bytes = [0u8; 32];
    SysRng.try_fill_bytes(&mut bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}

fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" | "map" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(path: &str) -> Option<Asset> {
        match path {
            "index.html" => Some(Asset {
                data: Cow::Borrowed(b"<html>"),
            }),
            "assets/app.js" => Some(Asset {
                data: Cow::Borrowed(b"x"),
            }),
            _ => None,
        }
    }

    fn nonce_of(r: &Response) -> String {
        r.extensions().get::<CspNonce>().unwrap().0.clone()
    }

    async fn body_of(r: Response) -> String {
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn with_marker(path: &str) -> Option<Asset> {
        (path == "index.html").then_some(Asset {
            data: Cow::Borrowed(br#"<meta name="csp-nonce" content="__CSP_NONCE__">"#),
        })
    }

    #[tokio::test]
    async fn html_gets_a_fresh_nonce_each_time() {
        let a = respond("/", with_marker);
        let b = respond("/", with_marker);
        let (na, nb) = (nonce_of(&a), nonce_of(&b));
        assert_ne!(na, nb);
        // 32 octets en base64 sans bourrage.
        assert_eq!(na.len(), 43);
        let body = body_of(a).await;
        assert!(body.contains(&format!(r#"content="{na}""#)));
        assert!(!body.contains(NONCE_PLACEHOLDER));
    }

    #[test]
    fn assets_carry_no_nonce() {
        let r = respond("/assets/app.js", fake);
        assert!(r.extensions().get::<CspNonce>().is_none());
    }

    fn none(_: &str) -> Option<Asset> {
        None
    }

    fn content_type(r: &Response) -> &str {
        r.headers()[header::CONTENT_TYPE].to_str().unwrap()
    }

    #[test]
    fn spa_route_gets_index() {
        let r = respond("/projects/abc", fake);
        assert_eq!(r.status(), StatusCode::OK);
        assert!(content_type(&r).starts_with("text/html"));
    }

    #[test]
    fn asset_gets_its_content_type() {
        let r = respond("/assets/app.js", fake);
        assert_eq!(r.status(), StatusCode::OK);
        assert!(content_type(&r).starts_with("text/javascript"));
    }

    #[test]
    fn missing_file_is_404_not_index() {
        assert_eq!(
            respond("/assets/missing.js", fake).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn empty_dist_is_503() {
        assert_eq!(respond("/", none).status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            respond("/projects", none).status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
