//! Adresse IP du client, pour la limitation de debit, la liste d'IP
//! autorisees et l'audit.
//!
//! C'est l'adresse TCP du pair. `X-Forwarded-For` n'est lu que si le pair est
//! en boucle locale : le proxy du noeud (`ciphera-proxy`) joint le plan de
//! controle par 127.0.0.1 et **remplace** l'en-tete (il retire
//! `X-Forwarded-For`, `Forwarded` et `X-Real-IP` du client et pose l'IP de son
//! socket client) au lieu de l'allonger : un client direct qui envoie
//! `X-Forwarded-For` n'echappe donc pas a la limitation de debit. Le proxy ne
//! parle au plan de controle que par TCP en boucle locale : aucun autre pair
//! n'est cru.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::request::Parts;

const FORWARDED_FOR: &str = "x-forwarded-for";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

/// Adresse du client a partir du pair TCP et des en-tetes.
pub fn resolve(peer: Option<IpAddr>, headers: &HeaderMap) -> IpAddr {
    // Sans pair connu (ne devrait pas arriver), l'adresse « non specifiee »
    // regroupe ces requetes et l'en-tete n'est pas cru.
    let peer = peer.map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |ip| ip.to_canonical());
    if !peer.is_loopback() {
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
        Ok(ClientIp(resolve(peer, &parts.headers)))
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
        assert_eq!(resolve(Some(peer), &xff("192.0.2.1")), peer);
        assert_eq!(resolve(Some(peer), &HeaderMap::new()), peer);
    }

    #[test]
    fn loopback_peer_uses_forwarded_for() {
        let lo: IpAddr = "127.0.0.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();
        assert_eq!(
            resolve(Some(lo), &xff("192.0.2.1")),
            "192.0.2.1".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            resolve(Some(v6), &xff("10.0.0.1, 192.0.2.9")),
            "192.0.2.9".parse::<IpAddr>().unwrap()
        );
        assert_eq!(resolve(Some(lo), &xff("n'importe quoi")), lo);
        assert_eq!(resolve(Some(lo), &HeaderMap::new()), lo);
    }

    #[test]
    fn mapped_ipv4_is_canonical_and_missing_peer_is_untrusted() {
        let mapped: IpAddr = "::ffff:203.0.113.7".parse().unwrap();
        assert_eq!(
            resolve(Some(mapped), &xff("192.0.2.1")),
            "203.0.113.7".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            resolve(None, &xff("192.0.2.1")),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        );
    }
}
