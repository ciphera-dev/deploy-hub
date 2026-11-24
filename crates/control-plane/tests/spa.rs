mod common;

use common::{self_signed, serve_https, state, tls_get};

fn dist_file(rel: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/dist")
        .join(rel);
    std::fs::read(&p).unwrap_or_else(|_| {
        panic!(
            "{} manquant : lancer `npm --prefix web run build` avant les tests",
            p.display()
        )
    })
}

#[tokio::test]
async fn spa_fallback() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    let r = tls_get(addr, &cert, "/projects/abc").await;
    assert_eq!(r.status, 200);
    assert!(r.header("content-type").unwrap().starts_with("text/html"));
    assert_eq!(r.body, dist_file("index.html"));
    let root = tls_get(addr, &cert, "/").await;
    assert_eq!(root.body, dist_file("index.html"));
}

#[tokio::test]
async fn static_assets_have_correct_content_type() {
    let (state, _pg) = state().await;
    let cert = self_signed();
    let (addr, _h) = serve_https(state, &cert).await;
    let index = String::from_utf8(dist_file("index.html")).unwrap();
    let find = |ext: &str| {
        index
            .split('"')
            .find(|s| s.starts_with("/assets/") && s.ends_with(ext))
            .unwrap_or_else(|| panic!("aucun asset {ext} dans index.html"))
            .to_string()
    };
    let js = tls_get(addr, &cert, &find(".js")).await;
    assert_eq!(js.status, 200);
    assert!(
        js.header("content-type")
            .unwrap()
            .starts_with("text/javascript")
    );
    let css = tls_get(addr, &cert, &find(".css")).await;
    assert!(css.header("content-type").unwrap().starts_with("text/css"));
    let svg = tls_get(addr, &cert, "/favicon.svg").await;
    assert_eq!(svg.header("content-type"), Some("image/svg+xml"));
}
