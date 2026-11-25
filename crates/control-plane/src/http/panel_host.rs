//! Hotes reconnus par les ecouteurs du panneau.
//!
//! - Acces direct (8443, [`direct`]) : le cookie de session ne tient pas
//!   compte du port. Un nom DNS qui joint le panneau sur 8443 sans etre
//!   declare pourrait aussi etre attache a un projet et recevoir ce cookie.
//!   L'acces direct ne sert donc que les adresses IP litterales, `localhost`
//!   et les noms reserves au panneau (hote de `CIPHERA_PUBLIC_URL`, RP ID
//!   WebAuthn, `CIPHERA_PANEL_ALIASES`, domaine du panneau confirme) ; tout
//!   autre `Host` recoit 421 avec une page breve qui cite
//!   `CIPHERA_PANEL_ALIASES`.
//! - Ecoute locale du proxy ([`local`]) : marque chaque requete
//!   ([`client_ip::ProxyListener`]) ; seul ce marqueur rend
//!   `X-Forwarded-For` digne de foi.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Router};
use shared::config::Config;

use crate::auth::webauthn::request_host;
use crate::domain::domains::panel::PanelSwitch;
use crate::http::client_ip::ProxyListener;

/// Page de la reponse 421 : ne reprend jamais l'hote recu.
const MISDIRECTED: &str = "<!doctype html>\n<html lang=\"fr\"><head><meta charset=\"utf-8\">\
<title>Nom non déclaré</title></head><body>\
<h1>421 : nom non déclaré pour le panneau</h1>\
<p>Ce nom n'est pas déclaré pour l'accès direct au panneau Ciphera (port 8443). \
Joignez le panneau par l'adresse IP du serveur, ou déclarez ce nom dans \
<code>CIPHERA_PANEL_ALIASES</code> (fichier <code>/etc/ciphera/ciphera.env</code>) \
puis redémarrez le service <code>ciphera</code>.</p>\
<p>Un nom déclaré ne peut plus servir de domaine à un projet.</p></body></html>\n";

/// Nom d'hote sans port, en minuscules et sans point final ; adresse IPv6
/// sans crochets. `None` si la valeur est vide ou mal formee.
pub fn host_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    // Adresse IPv6 sans crochets (donc sans port) : prise telle quelle.
    if let Ok(ip) = raw.parse::<IpAddr>() {
        return Some(ip.to_string());
    }
    let name = if let Some(rest) = raw.strip_prefix('[') {
        let (v6, after) = rest.split_once(']')?;
        if !(after.is_empty() || after.strip_prefix(':').is_some_and(is_port)) {
            return None;
        }
        v6
    } else {
        match raw.rsplit_once(':') {
            Some((name, port)) if is_port(port) => name,
            Some(_) => return None,
            None => raw,
        }
    };
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    (!name.is_empty()).then_some(name)
}

fn is_port(p: &str) -> bool {
    !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit())
}

/// Nom d'hote de la requete (en-tete `Host` ou autorite de l'URI), normalise
/// par [`host_name`].
pub fn request_host_name(headers: &HeaderMap, uri: &Uri) -> Option<String> {
    request_host(headers, uri).and_then(host_name)
}

/// L'acces direct sert-il ce nom (deja normalise par [`host_name`]) ?
pub fn served_directly(cfg: &Config, panel: &PanelSwitch, name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    if name.parse::<IpAddr>().is_ok() || name == "localhost" {
        return true;
    }
    if cfg.reserved_hosts().iter().any(|h| h == name) {
        return true;
    }
    panel
        .current()
        .is_some_and(|p| p.confirmed && p.host.eq_ignore_ascii_case(name))
}

#[derive(Clone)]
struct DirectHosts {
    cfg: Arc<Config>,
    panel: PanelSwitch,
}

async fn guard(State(hosts): State<DirectHosts>, request: Request, next: Next) -> Response {
    let name = request_host_name(request.headers(), request.uri());
    if served_directly(&hosts.cfg, &hosts.panel, name.as_deref()) {
        return next.run(request).await;
    }
    misdirected()
}

fn misdirected() -> Response {
    (
        StatusCode::MISDIRECTED_REQUEST,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("default-src 'none'"),
            ),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
            (
                header::REFERRER_POLICY,
                HeaderValue::from_static("no-referrer"),
            ),
        ],
        MISDIRECTED,
    )
        .into_response()
}

/// Routeur de l'acces direct (8443) : 421 pour un nom non declare.
pub fn direct(router: Router, cfg: Arc<Config>, panel: PanelSwitch) -> Router {
    router.layer(middleware::from_fn_with_state(
        DirectHosts { cfg, panel },
        guard,
    ))
}

/// Routeur de l'ecoute locale du proxy : requetes marquees
/// [`ProxyListener`].
pub fn local(router: Router) -> Router {
    router.layer(Extension(ProxyListener))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_name_strips_port_brackets_and_trailing_dot() {
        for (raw, want) in [
            ("Panel.Example.com:8443", Some("panel.example.com")),
            ("panel.example.com.", Some("panel.example.com")),
            ("203.0.113.7:8443", Some("203.0.113.7")),
            ("[2001:DB8::1]:8443", Some("2001:db8::1")),
            ("[::1]", Some("::1")),
            ("::1", Some("::1")),
            ("2001:DB8::1", Some("2001:db8::1")),
            ("localhost", Some("localhost")),
            ("", None),
            (":8443", None),
            ("host:port", None),
            ("[::1]x", None),
            ("[::1", None),
        ] {
            assert_eq!(host_name(raw).as_deref(), want, "{raw}");
        }
    }
}
