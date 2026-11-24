//! Interface web embarquee (`web/dist`).

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::borrow::Cow;

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
        Some(index) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            index.data.into_owned(),
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "interface non compilée",
        )
            .into_response(),
    }
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
