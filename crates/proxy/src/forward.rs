//! Service HTTP Pingora : choix de la route, redirections, transfert vers
//! les conteneurs amont et journal d'acces.
//!
//! Pour chaque requete :
//! - en-tete `Host` unique et valide (sinon 400), coherent avec une cible
//!   absolue ; en-tetes de requete bornes (sinon 431) ; chemin sans segment
//!   `.`/`..` ni separateur encode (sinon 400) ;
//! - defi ACME HTTP-01 en cours (`/.well-known/acme-challenge/<jeton>`, en
//!   HTTP clair) : reponse du defi ;
//! - route trouvee : redirection 308 (`redirect_to`, chemin et requete
//!   conserves) ; en HTTP clair, route `TlsMode::Acme` dont l'hote a un
//!   certificat : redirection 308 vers `https://<hote>` (chemin et requete
//!   conserves), sauf `/.well-known/acme-challenge/` ; 503 sans amont sain,
//!   sinon transfert a un amont ;
//! - aucune route : un hote IP litteral (ou l'hote du panneau configure)
//!   recoit 301 vers `https://<hote>:<port du panneau>` + chemin et requete,
//!   corps vide (panneau avant domaine) ; tout autre hote recoit un 404 sobre.
//!
//! Vers l'amont, `X-Forwarded-For`, `Forwarded`, `X-Real-IP` et
//! `X-Forwarded-*` du client sont retires ; le proxy pose son propre
//! `X-Forwarded-For` (IP du socket client) et `X-Forwarded-Proto`. Les
//! en-tetes de saut (`Connection` et ceux qu'il nomme, `Keep-Alive`, `TE`,
//! `Proxy-Authorization`...) sont retires par la politique de requete amont
//! de Pingora, fixee a `standard` (seul le passage a WebSocket est transmis).
//!
//! Port 443 : le certificat est choisi par SNI parmi ceux emis par ACME
//! (`acme::CertResolver`). Sans certificat pour le nom, la poignee de main
//! echoue proprement ; une route `TlsMode::Acme` reste servie en HTTP tant
//! que son certificat n'existe pas. Une requete dont l'hote a une route
//! doit porter le meme nom que le SNI de la poignee de main (casse et point
//! final ignores) : sinon 421, jamais transmise (un certificat obtenu pour
//! un hote ne sert pas a joindre un autre hote sur la meme connexion).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use pingora::apps::HttpServerOptions;
use pingora::http::{RequestHeader, ResponseHeader};
use pingora::listeners::tls::TlsSettings;
use pingora::listeners::{TcpSocketOptions, TlsAccept};
use pingora::prelude::HttpPeer;
use pingora::protocols::ALPN;
use pingora::protocols::tls::TlsRef;
use pingora::proxy::{FailToProxy, HttpProxy, ProxyHttp, Session, http_proxy};
use pingora::server::configuration::ServerConf;
use pingora::services::listening::Service;
use pingora::upstreams::peer::HttpUpstreamRequestPolicy;
use pingora::{Error, ErrorSource, ErrorType, Result};

use proto::TlsMode;

use crate::access_log::log_path;
use crate::acme::{CHALLENGE_PREFIX, CertResolver, CertStore, Challenges};
use crate::conn_limit::{Capped, ConnLimiter, ConnLimits, HandshakeDeadline, Kind, LimitFilter};
use crate::routes::{RequestHost, RouteTable, parse_host, path_is_normal};

/// Taille cumulee maximale de la ligne de requete et des en-tetes.
pub const MAX_REQUEST_HEADER_BYTES: usize = 32 * 1024;
/// Nombre maximal d'en-tetes de requete.
pub const MAX_REQUEST_HEADERS: usize = 100;
/// Delai d'etablissement de la connexion amont.
pub const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Delai maximal entre deux lectures ou ecritures vers l'amont.
pub const UPSTREAM_IO_TIMEOUT: Duration = Duration::from_secs(60);
/// Duree de vie d'une connexion amont inactive reutilisable.
pub const UPSTREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Delai maximal entre deux lectures du corps de la requete du client.
/// Pose dans `early_request_filter`, qui s'execute une fois les en-tetes
/// lus : la lecture des en-tetes est bornee par `conn_limit` (premier octet,
/// delai total `HEADER_TIMEOUT`).
pub const DOWNSTREAM_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Requetes servies par une connexion client avant sa fermeture.
pub const KEEPALIVE_REQUEST_LIMIT: u32 = 1000;

/// En-tetes poses par le proxy, toujours retires de la requete du client.
const CLIENT_FORWARDING_HEADERS: [&str; 7] = [
    "x-forwarded-for",
    "forwarded",
    "x-real-ip",
    "x-forwarded-proto",
    "x-forwarded-host",
    "x-forwarded-port",
    "x-client-ip",
];

const NOT_FOUND_BODY: &[u8] = b"Not Found\n";

/// Ecoute et comportement du proxy.
#[derive(Clone, Debug)]
pub struct ProxyConfig {
    /// Adresses HTTP (`0.0.0.0:80`, `[::]:80`).
    pub http: Vec<String>,
    /// Adresses HTTPS (certificats ACME choisis par SNI).
    pub https: Vec<String>,
    /// Hote du panneau (sans port) redirige vers le panneau s'il n'a pas de
    /// route ; les hotes IP le sont toujours.
    pub panel_host: Option<String>,
    /// Port HTTPS du panneau.
    pub panel_port: u16,
    pub threads: usize,
    /// Plafonds de connexions et delai du premier octet.
    pub limits: ConnLimits,
}

/// Certificats et defis ACME partages par les services HTTP et HTTPS.
#[derive(Clone, Debug)]
pub struct TlsAssets {
    pub certs: Arc<CertStore>,
    pub challenges: Arc<Challenges>,
}

impl TlsAssets {
    /// Sans certificat ni defi, en memoire (tests).
    pub fn memory() -> Self {
        Self {
            certs: Arc::new(CertStore::memory()),
            challenges: Arc::new(Challenges::new()),
        }
    }
}

/// Logique de proxy (une instance par service).
pub struct Forwarder {
    table: Arc<RouteTable>,
    panel_host: Option<String>,
    panel_port: u16,
    tls: TlsAssets,
    /// Service HTTPS (sinon HTTP clair).
    secure: bool,
    /// Echeances de lecture des en-tetes (desarmees ici).
    limiter: Option<Arc<ConnLimiter>>,
}

/// SNI de la poignee de main, garde dans l'extension du resume TLS.
struct ClientSni(Option<String>);

/// Rappel de fin de poignee de main : garde le SNI du client.
struct KeepSni;

#[async_trait]
impl TlsAccept for KeepSni {
    async fn handshake_complete_callback(
        &self,
        tls: &TlsRef,
    ) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        Some(Arc::new(ClientSni(tls.server_name().map(str::to_string))))
    }
}

/// Le SNI de la connexion designe-t-il `host` (deja normalise) ?
fn sni_matches(session: &Session, host: &str) -> bool {
    session
        .digest()
        .and_then(|d| d.ssl_digest.as_ref())
        .and_then(|ssl| ssl.extension.get::<ClientSni>())
        .and_then(|sni| sni.0.as_deref())
        .is_some_and(|sni| sni.trim_end_matches('.').eq_ignore_ascii_case(host))
}

/// Etat d'une requete.
pub struct RequestCtx {
    start: Instant,
    host: Option<String>,
    upstream: Option<SocketAddr>,
}

impl Forwarder {
    pub fn new(
        table: Arc<RouteTable>,
        panel_host: Option<String>,
        panel_port: u16,
        tls: TlsAssets,
        secure: bool,
    ) -> Self {
        Self {
            table,
            panel_host,
            panel_port,
            tls,
            secure,
            limiter: None,
        }
    }

    fn is_panel_host(&self, host: &RequestHost) -> bool {
        host.ip.is_some() || self.panel_host.as_deref() == Some(host.name.as_str())
    }
}

/// Autorite de la cible : celle de l'URI (HTTP/2) ou, en HTTP/1, celle de
/// la cible brute en forme absolue (`GET http://hote/x`) que Pingora retire
/// de `uri` (il n'y garde que le chemin).
fn target_authority(req: &RequestHeader) -> Option<&[u8]> {
    if let Some(a) = req.uri.authority() {
        return Some(a.as_str().as_bytes());
    }
    let raw = req.raw_path();
    let scheme_len = [&b"http://"[..], &b"https://"[..]]
        .iter()
        .find(|s| raw.len() >= s.len() && raw[..s.len()].eq_ignore_ascii_case(s))?
        .len();
    let rest = &raw[scheme_len..];
    let end = rest
        .iter()
        .position(|b| matches!(b, b'/' | b'?' | b'#'))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Hote de la requete : un seul en-tete `Host` valide ; avec une cible
/// absolue (`GET http://hote/`), les deux doivent concorder.
fn request_host(req: &RequestHeader) -> Option<RequestHost> {
    let mut values = req.headers.get_all(http::header::HOST).iter();
    let header = values.next();
    if values.next().is_some() {
        return None;
    }
    let authority = target_authority(req);
    let raw = match (header, authority) {
        (Some(h), Some(a)) if h.as_bytes().eq_ignore_ascii_case(a) => h.as_bytes(),
        (Some(_), Some(_)) => return None,
        (Some(h), None) => h.as_bytes(),
        // HTTP/2 seulement : l'autorite tient lieu de `Host`. En HTTP/1.1,
        // `Host` reste obligatoire meme avec une cible absolue.
        (None, Some(a)) if req.version == http::Version::HTTP_2 => a,
        (None, Some(_)) => return None,
        (None, None) => return None,
    };
    parse_host(raw).ok()
}

/// Route du domaine du panneau : ni service ni redirection (convention du
/// plan de controle, que l'agent applique aussi : son seul amont admis est
/// l'ecoute locale du panneau).
fn is_panel_route(route: &proto::Route) -> bool {
    route.service.is_none() && route.redirect_to.is_none()
}

fn header_bytes(req: &RequestHeader) -> usize {
    let line = req.method.as_str().len() + req.uri.to_string().len() + 12;
    req.headers
        .iter()
        .fold(line, |n, (k, v)| n + k.as_str().len() + v.len() + 4)
}

/// Chemin et requete a reprendre dans une redirection ; toujours `/...`
/// (`OPTIONS *` ou cible sans chemin : `/`), pour que l'hote de la
/// redirection ne puisse pas etre prolonge.
fn path_and_query(req: &RequestHeader) -> &str {
    req.uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .filter(|pq| pq.starts_with('/'))
        .unwrap_or("/")
}

/// Reponse produite par le proxy lui-meme : sans `Server`, jamais mise en
/// cache, connexion fermee ensuite.
async fn respond(
    session: &mut Session,
    status: u16,
    location: Option<&str>,
    body: Bytes,
) -> Result<()> {
    let mut resp = ResponseHeader::build(status, Some(4))?;
    resp.insert_header(http::header::CONTENT_LENGTH, body.len().to_string())?;
    resp.insert_header(http::header::CACHE_CONTROL, "no-store")?;
    if !body.is_empty() {
        resp.insert_header(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")?;
    }
    if let Some(location) = location {
        resp.insert_header(http::header::LOCATION, location)?;
    }
    session.set_keepalive(None);
    session
        .write_response_header(Box::new(resp), body.is_empty())
        .await?;
    if !body.is_empty() {
        session.write_response_body(Some(body), true).await?;
    }
    Ok(())
}

#[async_trait]
impl ProxyHttp for Forwarder {
    type CTX = RequestCtx;

    fn new_ctx(&self) -> Self::CTX {
        RequestCtx {
            start: Instant::now(),
            host: None,
            upstream: None,
        }
    }

    async fn early_request_filter(
        &self,
        session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        // HTTP/1 : flux de la connexion. HTTP/2 n'en a pas, mais `Capped`
        // ne lui pose pas d'echeance d'en-tetes.
        if let (Some(limiter), Some(stream)) = (&self.limiter, session.stream()) {
            limiter.headers_read(stream.id());
        }
        session.set_read_timeout(Some(DOWNSTREAM_READ_TIMEOUT));
        Ok(())
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let req = session.req_header();
        if req.headers.len() > MAX_REQUEST_HEADERS || header_bytes(req) > MAX_REQUEST_HEADER_BYTES {
            respond(session, 431, None, Bytes::new()).await?;
            return Ok(true);
        }
        let Some(host) = request_host(req) else {
            respond(session, 400, None, Bytes::new()).await?;
            return Ok(true);
        };
        ctx.host = Some(host.name.clone());
        if !path_is_normal(req.uri.path()) {
            respond(session, 400, None, Bytes::new()).await?;
            return Ok(true);
        }
        let pq = path_and_query(req).to_string();
        let challenge = req.uri.path().strip_prefix(CHALLENGE_PREFIX);
        if !self.secure
            && let Some(answer) = challenge.and_then(|token| self.tls.challenges.get(token))
        {
            respond(session, 200, None, Bytes::from(answer)).await?;
            return Ok(true);
        }
        let table = self.table.snapshot();
        match table.lookup(&host.name, req.uri.path()) {
            Some(_) if self.secure && !sni_matches(session, &host.name) => {
                respond(session, 421, None, Bytes::new()).await?;
                Ok(true)
            }
            Some(entry) => {
                if let Some(target) = &entry.route().redirect_to {
                    let location = format!("{}{pq}", target.trim_end_matches('/'));
                    respond(session, 308, Some(&location), Bytes::new()).await?;
                    return Ok(true);
                }
                // 80 -> 443 des que le certificat existe (defis exclus).
                if !self.secure
                    && challenge.is_none()
                    && entry.route().tls == TlsMode::Acme
                    && self.tls.certs.has(&host.name)
                {
                    let location = format!("https://{}{pq}", host.name);
                    respond(session, 308, Some(&location), Bytes::new()).await?;
                    return Ok(true);
                }
                // Le panneau n'est jamais servi en HTTP clair (route sans
                // amont avant son certificat, ou certificat absent ici).
                if !self.secure && is_panel_route(entry.route()) {
                    respond(session, 503, None, Bytes::new()).await?;
                    return Ok(true);
                }
                match entry.pick_upstream() {
                    Some(up) => {
                        ctx.upstream = Some(up);
                        Ok(false)
                    }
                    None => {
                        respond(session, 503, None, Bytes::new()).await?;
                        Ok(true)
                    }
                }
            }
            None if self.is_panel_host(&host) => {
                // Panneau servi sur son domaine (route avec amont) : l'acces
                // direct du port du panneau peut etre ferme, on y renvoie.
                let location = match table
                    .routes()
                    .find(|r| is_panel_route(r) && !r.upstreams.is_empty())
                {
                    Some(panel) => format!("https://{}{pq}", panel.host),
                    None => format!("https://{}:{}{pq}", host.url_authority(), self.panel_port),
                };
                respond(session, 301, Some(&location), Bytes::new()).await?;
                Ok(true)
            }
            None => {
                respond(session, 404, None, Bytes::from_static(NOT_FOUND_BODY)).await?;
                Ok(true)
            }
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let Some(addr) = ctx.upstream else {
            return Error::e_explain(ErrorType::InternalError, "aucun amont choisi");
        };
        let mut peer = HttpPeer::new(addr, false, String::new());
        peer.options.connection_timeout = Some(UPSTREAM_CONNECT_TIMEOUT);
        peer.options.total_connection_timeout = Some(UPSTREAM_CONNECT_TIMEOUT);
        peer.options.read_timeout = Some(UPSTREAM_IO_TIMEOUT);
        peer.options.write_timeout = Some(UPSTREAM_IO_TIMEOUT);
        peer.options.idle_timeout = Some(UPSTREAM_IDLE_TIMEOUT);
        peer.options.alpn = ALPN::H1;
        // Fixee explicitement : en-tetes de saut et ceux nommes par
        // `Connection` retires, seul le passage a WebSocket transmis
        // (normalise). Toute autre valeur doit faire echouer les tests.
        peer.options.http_upstream_request_policy = HttpUpstreamRequestPolicy::standard();
        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> Result<()> {
        for name in CLIENT_FORWARDING_HEADERS {
            upstream_request.remove_header(name);
        }
        let client = session
            .client_addr()
            .and_then(|a| a.as_inet())
            .map(|a| a.ip().to_canonical());
        if let Some(ip) = client {
            upstream_request.insert_header("x-forwarded-for", ip.to_string())?;
        }
        let tls = session.digest().is_some_and(|d| d.ssl_digest.is_some());
        upstream_request.insert_header("x-forwarded-proto", if tls { "https" } else { "http" })?;
        Ok(())
    }

    async fn fail_to_proxy(
        &self,
        session: &mut Session,
        e: &Error,
        _ctx: &mut Self::CTX,
    ) -> FailToProxy {
        let code = match e.etype() {
            ErrorType::HTTPStatus(code) => *code,
            _ => match e.esource() {
                ErrorSource::Upstream => 502,
                ErrorSource::Downstream => match e.etype() {
                    ErrorType::WriteError | ErrorType::ReadError | ErrorType::ConnectionClosed => 0,
                    _ => 400,
                },
                ErrorSource::Internal | ErrorSource::Unset => 500,
            },
        };
        if code > 0 && session.response_written().is_none() {
            let _ = respond(session, code, None, Bytes::new()).await;
        }
        FailToProxy {
            error_code: code,
            can_reuse_downstream: false,
        }
    }

    /// Resume de la requete dans les journaux d'erreur de Pingora : chemin
    /// sans requete, jetons masques.
    fn request_summary(&self, session: &Session, ctx: &Self::CTX) -> String {
        let req = session.req_header();
        format!(
            "{} {}, Host: {}",
            req.method,
            log_path(req.uri.path()),
            ctx.host.as_deref().unwrap_or("-")
        )
    }

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, ctx: &mut Self::CTX) {
        let req = session.req_header();
        let status = session.response_written().map_or(0, |r| r.status.as_u16());
        tracing::info!(
            target: "access",
            method = %req.method,
            host = ctx.host.as_deref().unwrap_or("-"),
            path = %log_path(req.uri.path()),
            status,
            duration_ms = ctx.start.elapsed().as_millis() as u64,
        );
    }
}

fn socket_options(addr: &str) -> TcpSocketOptions {
    let mut opts = TcpSocketOptions::default();
    // `[::]:80` n'occupe que l'IPv6 : `0.0.0.0:80` peut ecouter a cote.
    if addr.starts_with('[') {
        opts.ipv6_only = Some(true);
    }
    opts
}

/// Service Pingora d'une sorte d'ecoute, enveloppe par les plafonds de
/// connexions (compteurs partages par `limiter`).
fn capped_service(
    conf: &Arc<ServerConf>,
    cfg: &ProxyConfig,
    table: Arc<RouteTable>,
    tls: &TlsAssets,
    limiter: &Arc<ConnLimiter>,
    kind: Kind,
) -> Service<Capped<HttpProxy<Forwarder>>> {
    let mut forwarder = Forwarder::new(
        table,
        cfg.panel_host.clone(),
        cfg.panel_port,
        tls.clone(),
        kind == Kind::Tls,
    );
    forwarder.limiter = Some(limiter.clone());
    let mut proxy = http_proxy(conf, forwarder);
    let mut options = HttpServerOptions::default();
    options.keepalive_request_limit = Some(KEEPALIVE_REQUEST_LIMIT);
    proxy.server_options = Some(options);
    let name = match kind {
        Kind::Plain => "ciphera-proxy-http",
        Kind::Tls => "ciphera-proxy-https",
    };
    let mut service = Service::new(name.to_string(), Capped::new(proxy, limiter.clone()));
    service.set_connection_filter(Arc::new(LimitFilter {
        limiter: limiter.clone(),
        kind,
    }));
    if kind == Kind::Tls {
        service
            .endpoints()
            .set_pre_tls_callback(Arc::new(HandshakeDeadline {
                limiter: limiter.clone(),
            }));
    }
    service.threads = Some(cfg.threads.max(1));
    service
}

/// Services HTTP et HTTPS du proxy : un service par sorte d'ecoute, pour
/// que le filtre de connexion sache si une admission attend une poignee de
/// main TLS (plafond a part, voir `conn_limit`). Echoue si un ecouteur TLS
/// ne peut pas etre prepare.
pub fn proxy_services(
    conf: &Arc<ServerConf>,
    cfg: &ProxyConfig,
    table: Arc<RouteTable>,
    tls: &TlsAssets,
) -> std::result::Result<Vec<Service<Capped<HttpProxy<Forwarder>>>>, String> {
    let limiter = Arc::new(ConnLimiter::new(cfg.limits));
    let mut services = Vec::new();
    if !cfg.http.is_empty() {
        let mut http = capped_service(conf, cfg, table.clone(), tls, &limiter, Kind::Plain);
        for addr in &cfg.http {
            http.add_tcp_with_settings(addr, socket_options(addr));
        }
        services.push(http);
    }
    if !cfg.https.is_empty() {
        let mut https = capped_service(conf, cfg, table, tls, &limiter, Kind::Tls);
        for addr in &cfg.https {
            // Chemins vides : ignores des qu'un resolveur est pose. ALPN : HTTP/1
            // seulement (pas d'`enable_h2`) ; en h2, `session.stream()` est
            // vide et `Capped` ne pose pas d'echeance d'en-tetes (`is_h2`).
            let mut settings = TlsSettings::with_callbacks(Box::new(KeepSni))
                .map_err(|e| format!("ecouteur TLS {addr} : {e}"))?;
            settings.set_cert_resolver(Arc::new(CertResolver::new(tls.certs.clone())));
            https.add_tls_with_settings(addr, Some(socket_options(addr)), settings);
        }
        services.push(https);
    }
    Ok(services)
}
