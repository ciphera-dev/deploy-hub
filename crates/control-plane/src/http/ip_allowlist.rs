//! Liste d'IP autorisees (Parametres > Securite de l'organisation).
//!
//! Le middleware [`layer`] s'applique a `/api/*` et au panneau (SPA) : une
//! adresse hors liste recoit 403 `ip_not_allowed`. L'adresse est celle de
//! [`client_ip`] (pair TCP ; `X-Forwarded-For` seulement depuis le proxy du
//! noeud en boucle locale). Liste vide : aucune restriction.
//!
//! Exemptes, car appelees par des machines et non par l'equipe : les deploy
//! hooks (`/v1/hooks/deploy/{id}`), le webhook GitHub, les defis ACME et
//! `/api/health`. Les bloquer casserait les deploiements et HTTPS.
//!
//! La liste est gardee en memoire et relue en base toutes les
//! [`REFRESH_SECS`] secondes : `ciphera admin clear-ip-allowlist` (autre
//! processus) rouvre l'acces sans redemarrer le service.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use shared::error::ApiError;
use sqlx::PgPool;

use crate::http::client_ip;
use crate::state::AppState;

/// Entrees au plus.
pub const MAX_ENTRIES: usize = 100;
/// Intervalle de relecture de la liste en base.
pub const REFRESH_SECS: f64 = 5.0;

/// Adresse ou bloc CIDR, ramene a son reseau (`203.0.113.7/24` devient
/// `203.0.113.0/24`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpNet {
    network: IpAddr,
    prefix: u8,
}

fn max_prefix(ip: IpAddr) -> u8 {
    if ip.is_ipv4() { 32 } else { 128 }
}

fn masked(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => {
            let mask = u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0);
            IpAddr::V4((u32::from(v4) & mask).into())
        }
        IpAddr::V6(v6) => {
            let mask = u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0);
            IpAddr::V6((u128::from(v6) & mask).into())
        }
    }
}

impl IpNet {
    /// `adresse` ou `adresse/prefixe` ; rien d'autre (pas d'espace, pas de
    /// nom d'hote).
    pub fn parse(value: &str) -> Option<Self> {
        let (addr, prefix) = match value.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (value, None),
        };
        let ip = addr.parse::<IpAddr>().ok()?.to_canonical();
        let max = max_prefix(ip);
        let prefix = match prefix {
            None => max,
            Some(p) if !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) => {
                p.parse::<u8>().ok().filter(|p| *p <= max)?
            }
            Some(_) => return None,
        };
        Some(Self {
            network: masked(ip, prefix),
            prefix,
        })
    }

    /// Vrai si `ip` est dans ce bloc.
    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        ip.is_ipv4() == self.network.is_ipv4() && masked(ip, self.prefix) == self.network
    }
}

impl fmt::Display for IpNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.prefix == max_prefix(self.network) {
            write!(f, "{}", self.network)
        } else {
            write!(f, "{}/{}", self.network, self.prefix)
        }
    }
}

/// Liste lue en base (entrees illisibles ignorees : la contrainte et la
/// validation de l'API n'en laissent pas passer).
pub async fn load(db: &PgPool) -> sqlx::Result<Vec<IpNet>> {
    let list: Option<Vec<String>> = sqlx::query_scalar("SELECT ip_allowlist FROM instance LIMIT 1")
        .fetch_optional(db)
        .await?;
    Ok(list
        .unwrap_or_default()
        .iter()
        .filter_map(|v| IpNet::parse(v))
        .collect())
}

/// Liste lue et instant de la lecture (secondes Unix).
type Snapshot = (f64, Arc<Vec<IpNet>>);

/// Liste en memoire.
#[derive(Clone, Default)]
pub struct IpAllowlist(Arc<RwLock<Option<Snapshot>>>);

impl fmt::Debug for IpAllowlist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IpAllowlist")
    }
}

impl IpAllowlist {
    /// Remplace la liste (apres une ecriture validee en base).
    pub fn set(&self, now: f64, list: Vec<IpNet>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Some((now, Arc::new(list)));
    }

    fn cached(&self) -> Option<Snapshot> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Liste courante, relue si elle date de plus de [`REFRESH_SECS`]. Base
    /// injoignable : derniere liste connue, sinon erreur (rien n'est ouvert
    /// par defaut).
    async fn current(&self, state: &AppState) -> Result<Arc<Vec<IpNet>>, sqlx::Error> {
        let now = state.clock.unix_secs();
        let cached = self.cached();
        if let Some((at, list)) = &cached
            && (now - at).abs() < REFRESH_SECS
        {
            return Ok(list.clone());
        }
        match load(&state.db).await {
            Ok(list) => {
                self.set(now, list);
                Ok(self.cached().map(|(_, l)| l).unwrap_or_default())
            }
            Err(e) => match cached {
                Some((_, list)) => {
                    tracing::warn!(error = %e, "liste d'IP autorisees non relue : derniere liste gardee");
                    Ok(list)
                }
                None => Err(e),
            },
        }
    }
}

impl IpAllowlist {
    /// `ip` est autorisee (liste vide : aucune restriction). Liste
    /// illisible : refus.
    pub async fn allows(&self, state: &AppState, ip: IpAddr) -> bool {
        self.current(state)
            .await
            .is_ok_and(|list| list.is_empty() || list.iter().any(|net| net.contains(ip)))
    }
}

/// `rest` est un seul segment de chemin (ni vide, ni `.`/`..`).
fn one_segment(rest: Option<&str>) -> bool {
    rest.is_some_and(|s| !s.is_empty() && !s.contains('/') && s != "." && s != "..")
}

/// Routes appelees par des machines, jamais filtrees.
fn exempt(path: &str) -> bool {
    path == "/api/health"
        || path == "/v1/webhooks/github"
        || one_segment(path.strip_prefix("/v1/hooks/deploy/"))
        || one_segment(path.strip_prefix("/.well-known/acme-challenge/"))
}

/// Middleware : 403 `ip_not_allowed` hors liste.
pub async fn layer(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if exempt(request.uri().path()) {
        return next.run(request).await;
    }
    let list = match state.ip_allowlist.current(&state).await {
        Ok(list) => list,
        Err(e) => {
            tracing::error!(error = %e, "liste d'IP autorisees illisible");
            return ApiError::new(503, "unavailable", "Service momentanément indisponible.")
                .into_response();
        }
    };
    if list.is_empty() {
        return next.run(request).await;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    let ip = client_ip::resolve(peer, request.headers());
    if list.iter().any(|net| net.contains(ip)) {
        return next.run(request).await;
    }
    tracing::warn!(%ip, "acces refuse : adresse hors de la liste d'IP autorisees");
    ApiError::new(
        403,
        "ip_not_allowed",
        "Accès refusé depuis cette adresse IP.",
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(v: &str) -> IpAddr {
        v.parse().unwrap()
    }

    #[test]
    fn parse_canonicalizes_and_rejects_garbage() {
        assert_eq!(
            IpNet::parse("203.0.113.7").unwrap().to_string(),
            "203.0.113.7"
        );
        assert_eq!(
            IpNet::parse("203.0.113.7/24").unwrap().to_string(),
            "203.0.113.0/24"
        );
        assert_eq!(IpNet::parse("0.0.0.0/0").unwrap().to_string(), "0.0.0.0/0");
        assert_eq!(
            IpNet::parse("2001:db8::1/32").unwrap().to_string(),
            "2001:db8::/32"
        );
        assert_eq!(
            IpNet::parse("::ffff:203.0.113.7").unwrap().to_string(),
            "203.0.113.7"
        );
        for bad in [
            "",
            "x",
            "203.0.113.7/",
            "203.0.113.7/33",
            "203.0.113.7/+8",
            "203.0.113.7/24/8",
            " 203.0.113.7",
            "2001:db8::/129",
            "203.0.113.7/0024",
            "example.com",
        ] {
            assert!(IpNet::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn contains_matches_family_and_prefix() {
        let net = IpNet::parse("203.0.113.0/24").unwrap();
        assert!(net.contains(ip("203.0.113.200")));
        assert!(net.contains(ip("::ffff:203.0.113.200")));
        assert!(!net.contains(ip("203.0.114.1")));
        assert!(!net.contains(ip("2001:db8::1")));
        let all = IpNet::parse("0.0.0.0/0").unwrap();
        assert!(all.contains(ip("198.51.100.9")));
        assert!(!all.contains(ip("2001:db8::1")));
        let v6 = IpNet::parse("2001:db8::/32").unwrap();
        assert!(v6.contains(ip("2001:db8:ffff::1")));
        assert!(!v6.contains(ip("2001:db9::1")));
    }

    #[test]
    fn machine_routes_are_exempt_only_on_their_exact_shape() {
        for path in [
            "/api/health",
            "/v1/webhooks/github",
            "/v1/hooks/deploy/abc",
            "/.well-known/acme-challenge/tok",
        ] {
            assert!(exempt(path), "{path}");
        }
        for path in [
            "/api/me",
            "/",
            "/v1/hooks/deploy/",
            "/v1/hooks/deploy/../../api/me",
            "/v1/hooks/deploy/a/b",
            "/.well-known/acme-challenge/..",
            "/v1/webhooks/github/x",
            "/api/health/x",
        ] {
            assert!(!exempt(path), "{path}");
        }
    }
}
