//! Adresse IP du client, pour la limitation de debit, la liste d'IP
//! autorisees et l'audit.
//!
//! C'est l'adresse TCP du pair. `X-Forwarded-For` n'est lu que sur l'ecoute
//! locale du proxy, que [`ProxyListener`] marque (pose par
//! [`crate::http::panel_host::local`]), et seulement d'un pair en boucle
//! locale. Le proxy du noeud (`ciphera-proxy`) y joint le plan de controle
//! par 127.0.0.1 et **remplace** l'en-tete (il retire `X-Forwarded-For`,
//! `Forwarded` et `X-Real-IP` du client et pose l'IP de son socket client) au
//! lieu de l'allonger. Sur l'acces direct (8443), le pair fait toujours foi,
//! meme en boucle locale : un processus local ne choisit pas son adresse.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::request::Parts;

const FORWARDED_FOR: &str = "x-forwarded-for";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

/// Marqueur pose sur chaque requete de l'ecoute locale du proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProxyListener;

/// Adresse du client a partir du pair TCP et des en-tetes. `via_proxy` :
/// requete recue par l'ecoute locale du proxy ([`ProxyListener`]).
pub fn resolve(peer: Option<IpAddr>, via_proxy: bool, headers: &HeaderMap) -> IpAddr {
    // Sans pair connu (ne devrait pas arriver), l'adresse « non specifiee »
    // regroupe ces requetes et l'en-tete n'est pas cru.
    let peer = peer.map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |ip| ip.to_canonical());
    if !via_proxy || !peer.is_loopback() {
        return peer;
    }
    // Le proxy remplace l'en-tete : une seule valeur attendue. Par prudence,
    // la derniere (ajoutee par le saut le plus proche) fait foi.
    headers
        .get_all(FORWARDED_FOR)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .next_back()
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
        .map_or(peer, |ip| ip.to_canonical())
}

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip());
        let via_proxy = parts.extensions.get::<ProxyListener>().is_some();
        Ok(ClientIp(resolve(peer, via_proxy, &parts.headers)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn xff(value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(FORWARDED_FOR, HeaderValue::from_static(value));
        h
    }

    #[test]
    fn direct_peer_ignores_forwarded_for() {
        let peer: IpAddr = "203.0.113.7".parse().unwrap();
        assert_eq!(resolve(Some(peer), true, &xff("192.0.2.1")), peer);
        assert_eq!(resolve(Some(peer), true, &HeaderMap::new()), peer);
    }

    #[test]
    fn loopback_peer_uses_forwarded_for_only_via_the_proxy_listener() {
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(resolve(Some(lo), false, &xff("192.0.2.1")), lo);
    }

    #[test]
    fn loopback_peer_uses_forwarded_for() {
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();
        assert_eq!(
            resolve(Some(lo), true, &xff("192.0.2.1")),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            resolve(Some(v6), true, &xff("10.0.0.1, 192.0.2.9")),
            "192.0.2.9".parse::<IpAddr>().unwrap()
        );
        assert_eq!(resolve(Some(lo), true, &xff("n'importe quoi")), lo);
        assert_eq!(resolve(Some(lo), true, &HeaderMap::new()), lo);
    }

    #[test]
    fn mapped_ipv4_is_canonical_and_missing_peer_is_untrusted() {
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        assert_eq!(
            resolve(Some(mapped), true, &xff("192.0.2.1")),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            resolve(None, true, &xff("192.0.2.1")),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }

    /// IP vue par un handler, la requete venant de `peer` avec
    /// `X-Forwarded-For: 192.0.2.1`.
    async fn seen(app: axum::Router, peer: &str) -> String {
        use tower::ServiceExt;
        let mut req = axum::http::Request::builder()
            .uri("/")
            .header(FORWARDED_FOR, "192.0.2.1")
            .body(axum::body::Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
        let res = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    /// Routeur qui renvoie l'IP vue (repli : hors de la matrice des routes).
    fn echo() -> axum::Router {
        axum::Router::new().fallback(|ClientIp(ip): ClientIp| async move { ip.to_string() })
    }

    #[tokio::test]
    async fn xff_ignored_on_public_listener() {
        // Acces direct (8443) : meme un pair en boucle locale ne choisit pas
        // son adresse.
        assert_eq!(seen(echo(), "127.0.0.1:40000").await, "127.0.0.1");
        assert_eq!(seen(echo(), "[::1]:40000").await, "::1");
        assert_eq!(seen(echo(), "203.0.113.7:40000").await, "203.0.113.7");
    }

    #[tokio::test]
    async fn xff_trusted_on_local_listener() {
        let local = crate::http::panel_host::local(echo());
        assert_eq!(seen(local.clone(), "127.0.0.1:40000").await, "192.0.2.1");
        // L'ecoute locale n'est joignable qu'en boucle locale ; un autre pair
        // n'y est jamais cru.
        assert_eq!(seen(local, "203.0.113.7:40000").await, "203.0.113.7");
    }
}
