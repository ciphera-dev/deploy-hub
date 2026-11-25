//! Ecouteur HTTP : redirige tout vers HTTPS (301, corps vide). Une fois le
//! domaine du panneau confirme (acces 8443 ferme), la cible est
//! `https://<domaine du panneau>` pour un hote nomme ; un hote IP recoit
//! alors un 404 sans `Location` (le domaine n'est jamais revele a qui
//! balaie les adresses IP).

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use std::net::Ipv6Addr;

use super::security_headers;
use crate::domain::domains::panel::PanelSwitch;

#[derive(Clone)]
struct Target {
    https_port: u16,
    panel: PanelSwitch,
}

/// Routeur de redirection vers `https://<hote>:<https_port>`, ou vers le
/// domaine confirme du panneau.
pub fn router(https_port: u16, panel: PanelSwitch) -> Router {
    security_headers::apply(
        Router::new().fallback(redirect).with_state(Target {
            https_port,
            panel: panel.clone(),
        }),
        panel,
    )
}

async fn redirect(State(target): State<Target>, headers: HeaderMap, uri: Uri) -> Response {
    let https_port = target.https_port;
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(clean_host);
    let Some(host) = host else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = uri.path_and_query().map_or("/", |p| p.as_str());
    let authority = match target.panel.current() {
        Some(panel) if panel.confirmed && is_ip(&host) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Some(panel) if panel.confirmed => panel.host,
        _ if https_port == 443 => host,
        _ => format!("{host}:{https_port}"),
    };
    let location = format!("https://{authority}{path}");
    match axum::http::HeaderValue::from_str(&location) {
        Ok(value) => (StatusCode::MOVED_PERMANENTLY, [(header::LOCATION, value)]).into_response(),
        Err(_) => StatusCode::BAD_REQUEST.into_response(),
    }
}

/// Hote IP, sous sa forme d'URL (IPv6 entre crochets).
fn is_ip(host: &str) -> bool {
    host.strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
        .parse::<std::net::IpAddr>()
        .is_ok()
}

/// Retire le port de l'en-tete Host et valide le reste (nom d'hote DNS,
/// IPv4 ou IPv6 entre crochets). Renvoie l'hote tel qu'il doit figurer dans une
/// URL, ou `None` s'il est absent ou douteux.
fn clean_host(raw: &str) -> Option<String> {
    if let Some(rest) = raw.strip_prefix('[') {
        let (addr, tail) = rest.split_once(']')?;
        if !(tail.is_empty() || valid_port(tail.strip_prefix(':')?)) {
            return None;
        }
        addr.parse::<Ipv6Addr>().ok()?;
        return Some(format!("[{addr}]"));
    }
    let host = match raw.split_once(':') {
        None => raw,
        Some((host, port)) if valid_port(port) => host,
        Some(_) => return None,
    };
    valid_hostname(host).then(|| host.to_string())
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

fn valid_hostname(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::clean_host;

    #[test]
    fn accepts_names_and_ip_literals_and_strips_port() {
        assert_eq!(
            clean_host("panel.example.com").unwrap(),
            "panel.example.com"
        );
        assert_eq!(
            clean_host("panel.example.com:80").unwrap(),
            "panel.example.com"
        );
        assert_eq!(clean_host("203.0.113.7:8080").unwrap(), "203.0.113.7");
        assert_eq!(clean_host("[2001:db8::1]:80").unwrap(), "[2001:db8::1]");
        assert_eq!(clean_host("[::1]").unwrap(), "[::1]");
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "evil.com/path",
            "a b",
            "evil.com@x.com",
            "x.com:80:90",
            "x.com:abc",
            "-x.com",
            "x..com",
            "[zz]:80",
            "[::1]x",
            "exa\\mple.com",
            "x.com#frag",
            "x.com?q",
        ] {
            assert!(clean_host(bad).is_none(), "{bad:?} devrait etre refuse");
        }
    }
}
