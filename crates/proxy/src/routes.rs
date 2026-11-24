//! Table de routes du proxy, pilotee par l'agent (`ApplyRoutes`).
//!
//! La table complete est validee puis remplace la precedente d'un seul
//! echange atomique (`ArcSwap`) : chaque requete lit un instantane
//! (`snapshot`) et voit donc entierement l'ancienne table ou entierement la
//! nouvelle, jamais un melange. Une table invalide est refusee en bloc et la
//! precedente reste en place.

use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use arc_swap::ArcSwap;
use proto::Route;

/// Nombre maximal de routes d'une table.
pub const MAX_ROUTES: usize = 10_000;
/// Nombre maximal de conteneurs amont d'une route.
pub const MAX_UPSTREAMS: usize = 64;
const MAX_HOST_LEN: usize = 253;
const MAX_LABEL_LEN: usize = 63;
const MAX_PREFIX_LEN: usize = 1024;

/// Raison du refus d'une table (la valeur fautive, jamais un secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    Host(String),
    PathPrefix(String),
    Upstream(String),
    Redirect(String),
    Duplicate(String),
    TooMany(usize),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(h) => write!(f, "hote invalide : {h:?}"),
            Self::PathPrefix(p) => write!(f, "prefixe de chemin invalide : {p:?}"),
            Self::Upstream(u) => write!(f, "amont invalide : {u}"),
            Self::Redirect(r) => write!(f, "redirection invalide : {r:?}"),
            Self::Duplicate(r) => write!(f, "route en double : {r}"),
            Self::TooMany(n) => write!(f, "{n} routes ou amonts, au-dela du maximum"),
        }
    }
}

impl std::error::Error for RouteError {}

/// Route validee, avec son tourniquet d'amonts.
#[derive(Debug)]
pub struct Entry {
    route: Route,
    next: AtomicUsize,
}

impl Entry {
    pub fn route(&self) -> &Route {
        &self.route
    }

    /// Amont suivant (tourniquet), `None` si la route n'en a aucun.
    pub fn pick_upstream(&self) -> Option<SocketAddr> {
        let ups = &self.route.upstreams;
        if ups.is_empty() {
            return None;
        }
        let i = self.next.fetch_add(1, Ordering::Relaxed);
        Some(ups[i % ups.len()])
    }

    fn matches(&self, path: &str) -> bool {
        let prefix = self.route.path_prefix.as_str();
        match path.strip_prefix(prefix) {
            None => false,
            Some(rest) => prefix.ends_with('/') || rest.is_empty() || rest.starts_with('/'),
        }
    }
}

/// Instantane immuable de la table.
#[derive(Debug, Default)]
pub struct Table {
    /// Par hote : routes triees du prefixe le plus long au plus court.
    hosts: HashMap<String, Vec<Entry>>,
    len: usize,
}

impl Table {
    /// Route de `host` (minuscules, sans port) dont le prefixe le plus long
    /// couvre `path` (sans requete).
    pub fn lookup(&self, host: &str, path: &str) -> Option<&Entry> {
        self.hosts.get(host)?.iter().find(|e| e.matches(path))
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Routes de la table (ordre non specifie).
    pub fn routes(&self) -> impl Iterator<Item = &Route> {
        self.hosts.values().flatten().map(Entry::route)
    }

    fn build(routes: Vec<Route>) -> Result<Self, RouteError> {
        if routes.len() > MAX_ROUTES {
            return Err(RouteError::TooMany(routes.len()));
        }
        let len = routes.len();
        let mut hosts: HashMap<String, Vec<Entry>> = HashMap::new();
        for route in routes {
            validate(&route)?;
            let entries = hosts.entry(route.host.clone()).or_default();
            if entries
                .iter()
                .any(|e| e.route.path_prefix == route.path_prefix)
            {
                return Err(RouteError::Duplicate(format!(
                    "{}{}",
                    route.host, route.path_prefix
                )));
            }
            entries.push(Entry {
                route,
                next: AtomicUsize::new(0),
            });
        }
        for entries in hosts.values_mut() {
            entries.sort_by_key(|e| std::cmp::Reverse(e.route.path_prefix.len()));
        }
        Ok(Self { hosts, len })
    }
}

/// Table partagee entre le socket de pilotage (ecrivain) et les requetes.
#[derive(Debug, Default)]
pub struct RouteTable {
    current: ArcSwap<Table>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Valide `routes` puis remplace la table d'un bloc. En cas d'erreur, rien
    /// ne change. Renvoie le nombre de routes appliquees.
    pub fn apply(&self, routes: Vec<Route>) -> Result<usize, RouteError> {
        let table = Table::build(routes)?;
        let len = table.len;
        self.current.store(Arc::new(table));
        Ok(len)
    }

    /// Instantane courant : une requete n'en lit qu'un.
    pub fn snapshot(&self) -> Arc<Table> {
        self.current.load_full()
    }
}

fn validate(route: &Route) -> Result<(), RouteError> {
    // Hote deja normalise par l'emetteur : minuscules, sans point final ni port.
    if !valid_dns_name(&route.host) || route.host.ends_with('.') {
        return Err(RouteError::Host(route.host.clone()));
    }
    let p = &route.path_prefix;
    if !p.starts_with('/')
        || p.len() > MAX_PREFIX_LEN
        || !p
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'?' && b != b'#')
        || !path_is_normal(p)
    {
        return Err(RouteError::PathPrefix(p.clone()));
    }
    if route.upstreams.len() > MAX_UPSTREAMS {
        return Err(RouteError::TooMany(route.upstreams.len()));
    }
    for up in &route.upstreams {
        let ip = up.ip();
        if up.port() == 0
            || ip.is_unspecified()
            || ip.is_multicast()
            || is_broadcast(ip)
            || is_link_local(ip)
        {
            return Err(RouteError::Upstream(up.to_string()));
        }
    }
    if let Some(target) = &route.redirect_to
        && !valid_redirect_origin(target)
    {
        return Err(RouteError::Redirect(target.clone()));
    }
    Ok(())
}

fn is_broadcast(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if v4.is_broadcast())
}

/// 169.254.0.0/16 et fe80::/10 (metadonnees de nuage, voisins du lien) :
/// jamais un conteneur de projet.
fn is_link_local(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => v6.segments()[0] & 0xffc0 == 0xfe80,
    }
}

/// Chemin de requete sans ambiguite, verifie avant la recherche de route :
/// ni segment `.` ou `..`, ni barre oblique inverse, ni `/`, `\` ou `.`
/// encodes (`%2F`, `%5C`, `%2E`), qu'un amont pourrait normaliser autrement
/// que la table (route `/api` contournee par `/public/%2e%2e/api`).
pub fn path_is_normal(path: &str) -> bool {
    if path.contains('\\') {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    if ["%2f", "%5c", "%2e"].iter().any(|e| lower.contains(e)) {
        return false;
    }
    !path.split('/').any(|seg| seg == "." || seg == "..")
}

/// `http(s)://hote[:port]` (`/` final tolere) : ni chemin, ni requete, ni
/// identifiants. Le chemin de la requete y est ajoute tel quel.
fn valid_redirect_origin(target: &str) -> bool {
    let Some(rest) = target
        .strip_prefix("https://")
        .or_else(|| target.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    !authority.is_empty()
        && !authority.contains(['/', '?', '#', '@'])
        && parse_host(authority.as_bytes()).is_ok()
}

/// Nom d'hote ASCII en minuscules : etiquettes de 1 a 63 caracteres
/// `[a-z0-9-]`, sans tiret en bordure, 253 caracteres au plus (point final
/// tolere).
fn valid_dns_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    !name.is_empty()
        && name.len() <= MAX_HOST_LEN
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= MAX_LABEL_LEN
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Hote d'une requete (en-tete `Host` ou autorite de la cible).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHost {
    /// En minuscules, sans port ni point final ; adresse IPv6 sans crochets.
    pub name: String,
    pub port: Option<u16>,
    /// Adresse IP litterale (v4 ou v6).
    pub ip: Option<IpAddr>,
}

impl RequestHost {
    /// Forme a placer dans une URL (`[v6]` entre crochets).
    pub fn url_authority(&self) -> String {
        match self.ip {
            Some(IpAddr::V6(v6)) => format!("[{v6}]"),
            _ => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidHost;

/// Valide un en-tete `Host` : nom DNS, IPv4 ou `[IPv6]`, port facultatif
/// (1 a 65535). Tout le reste (espaces, `_`, `/`, `@`, octets non ASCII,
/// crochets incomplets) est refuse.
pub fn parse_host(value: &[u8]) -> Result<RequestHost, InvalidHost> {
    let text = std::str::from_utf8(value).map_err(|_| InvalidHost)?;
    if !text.is_ascii() {
        return Err(InvalidHost);
    }
    let lower = text.to_ascii_lowercase();
    let (host, port) = if let Some(rest) = lower.strip_prefix('[') {
        let (inner, after) = rest.split_once(']').ok_or(InvalidHost)?;
        let ip: std::net::Ipv6Addr = inner.parse().map_err(|_| InvalidHost)?;
        let port = match after {
            "" => None,
            p => Some(parse_port(p.strip_prefix(':').ok_or(InvalidHost)?)?),
        };
        return Ok(RequestHost {
            name: ip.to_string(),
            port,
            ip: Some(IpAddr::V6(ip)),
        });
    } else {
        match lower.split_once(':') {
            Some((h, p)) => (h, Some(parse_port(p)?)),
            None => (lower.as_str(), None),
        }
    };
    if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        return Ok(RequestHost {
            name: v4.to_string(),
            port,
            ip: Some(IpAddr::V4(v4)),
        });
    }
    if !valid_dns_name(host) {
        return Err(InvalidHost);
    }
    Ok(RequestHost {
        name: host.strip_suffix('.').unwrap_or(host).to_string(),
        port,
        ip: None,
    })
}

fn parse_port(p: &str) -> Result<u16, InvalidHost> {
    if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
        return Err(InvalidHost);
    }
    match p.parse::<u16>() {
        Ok(0) | Err(_) => Err(InvalidHost),
        Ok(n) => Ok(n),
    }
}
