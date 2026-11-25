//! Ecouteurs TCP du panneau : acces direct HTTPS (8443, ouvert sur Internet
//! jusqu'a la confirmation d'un domaine) et ecoute locale du proxy.
//!
//! Sans bornes, un seul hote epuise les descripteurs du plan de controle en
//! ouvrant des connexions muettes (slowloris) : plus d'API, de webhooks, de
//! connexions a la base. Chaque ecouteur pose donc :
//!
//! - un delai pour la premiere requete (connexion muette, poignee TLS
//!   comprise dans le delai propre de rustls) ;
//! - un delai de lecture des en-tetes de chaque requete HTTP/1, qui ferme
//!   aussi une connexion keep-alive restee inactive ;
//! - un delai total de lecture du corps de chaque requete ;
//! - pour HTTP/2, une sonde (PING) qui ferme les pairs muets, et un delai
//!   d'inactivite (aucune requete en cours depuis `h2_idle`) qui ferme aussi
//!   ceux qui repondent aux PING sans rien demander ([`with_idle_tracking`]) ;
//! - un plafond global de connexions et un plafond par adresse IP (IPv6 :
//!   par /64), refusees des l'acceptation (fermeture immediate).
//!
//! `ciphera.service` releve `LimitNOFILE` pour que ces plafonds tiennent.

use std::collections::HashMap;
use std::future::{Future, Ready, ready};
use std::io;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tower_service::Service;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::Version;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum_server::accept::Accept;
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use axum_server::{ConnectionShutdown, Server};
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::Sleep;

/// Bornes d'un ecouteur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hardening {
    /// Delai de la premiere requete d'une connexion.
    pub first_request: Duration,
    /// Delai de lecture des en-tetes d'une requete (et d'inactivite entre
    /// deux requetes d'une connexion keep-alive).
    pub header_read: Duration,
    /// Delai total de lecture du corps d'une requete (voir
    /// [`with_body_deadline`]).
    pub body_read: Duration,
    /// Inactivite HTTP/2 toleree avant la sonde, puis delai de sa reponse.
    pub h2_idle: Duration,
    /// Connexions ouvertes au plus sur l'ecouteur.
    pub max_connections: usize,
    /// Connexions ouvertes au plus par adresse IP (IPv6 : par /64).
    pub max_per_ip: usize,
}

impl Hardening {
    /// Acces direct (Internet).
    pub const PUBLIC: Self = Self {
        first_request: Duration::from_secs(10),
        header_read: Duration::from_secs(20),
        body_read: Duration::from_secs(30),
        h2_idle: Duration::from_secs(60),
        max_connections: 4096,
        max_per_ip: 128,
    };
    /// Ecoute locale : le proxy y concentre tout le trafic du domaine du
    /// panneau (une seule adresse, bornee par client cote proxy).
    pub const LOCAL: Self = Self {
        first_request: Duration::from_secs(10),
        header_read: Duration::from_secs(20),
        body_read: Duration::from_secs(30),
        h2_idle: Duration::from_secs(60),
        max_connections: 4096,
        max_per_ip: 4096,
    };
}

#[derive(Default)]
struct Counts {
    total: usize,
    by_ip: HashMap<IpAddr, usize>,
}

/// Compteur de connexions d'un ecouteur.
#[derive(Clone)]
pub struct ConnLimits {
    max_total: usize,
    max_per_ip: usize,
    counts: Arc<Mutex<Counts>>,
}

/// Delai laisse a la fermeture ordonnee (GOAWAY) avant la coupure du flux.
const GRACE: Duration = Duration::from_secs(5);

/// Etat d'une connexion ouverte, partage entre son flux ([`Limited`]) et les
/// requetes qu'elle porte (extension posee par [`WithConn`], lue par
/// [`with_idle_tracking`]). Propre a la connexion : aucune cle partagee.
struct ConnState {
    /// Requetes dont la reponse n'est pas terminee.
    in_flight: AtomicUsize,
    /// Une requete HTTP/2 a ete vue : le delai d'inactivite s'applique.
    h2: AtomicBool,
    /// Fin de la derniere reponse (ou ouverture de la connexion).
    idle_since: Mutex<tokio::time::Instant>,
    /// Reveil de la lecture du flux, pour armer le delai a la fin d'une requete.
    waker: Mutex<Option<Waker>>,
    /// Poignee de fermeture ordonnee de la connexion (vue sur une requete).
    shutdown: Mutex<Option<ConnectionShutdown>>,
    /// La fermeture ordonnee a ete demandee.
    closing: AtomicBool,
}

impl ConnState {
    fn new() -> Self {
        Self {
            in_flight: AtomicUsize::new(0),
            h2: AtomicBool::new(false),
            idle_since: Mutex::new(tokio::time::Instant::now()),
            waker: Mutex::new(None),
            shutdown: Mutex::new(None),
            closing: AtomicBool::new(false),
        }
    }
}

/// Une requete en cours : rendue a la fin de sa reponse (ou si elle est
/// abandonnee).
struct InFlight(Arc<ConnState>);

impl InFlight {
    fn start(state: Arc<ConnState>) -> Self {
        state.in_flight.fetch_add(1, Ordering::SeqCst);
        Self(state)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if self.0.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            *self.0.idle_since.lock().unwrap_or_else(|e| e.into_inner()) =
                tokio::time::Instant::now();
            let waker = self
                .0
                .waker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}

/// Cle du plafond par adresse : IPv4 (ou IPv4 dans IPv6) telle quelle,
/// IPv6 reduite a son /64 (un hote en dispose d'ordinaire en entier).
fn ip_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
    }
}

impl ConnLimits {
    pub fn new(max_total: usize, max_per_ip: usize) -> Self {
        Self {
            max_total,
            max_per_ip,
            counts: Arc::default(),
        }
    }

    /// Place pour une connexion de `ip`, rendue quand le jeton est detruit.
    pub fn try_acquire(&self, ip: IpAddr) -> Option<ConnGuard> {
        let key = ip_key(ip);
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        let per_ip = counts.by_ip.get(&key).copied().unwrap_or(0);
        if counts.total >= self.max_total || per_ip >= self.max_per_ip {
            return None;
        }
        counts.total += 1;
        counts.by_ip.insert(key, per_ip + 1);
        Some(ConnGuard {
            counts: self.counts.clone(),
            key,
        })
    }

    /// Connexions ouvertes.
    pub fn open(&self) -> usize {
        self.counts.lock().unwrap_or_else(|e| e.into_inner()).total
    }
}

/// Place occupee par une connexion.
pub struct ConnGuard {
    counts: Arc<Mutex<Counts>>,
    key: IpAddr,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        let mut counts = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        counts.total = counts.total.saturating_sub(1);
        if let Some(n) = counts.by_ip.get_mut(&self.key) {
            *n -= 1;
            if *n == 0 {
                counts.by_ip.remove(&self.key);
            }
        }
    }
}

/// Flux qui garde sa place jusqu'a sa fermeture.
pub struct Limited<T> {
    inner: T,
    _guard: ConnGuard,
    state: Arc<ConnState>,
    idle: Duration,
    sleep: Pin<Box<Sleep>>,
}

impl<T: AsyncRead + Unpin> AsyncRead for Limited<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        // HTTP/2 sans requete en cours depuis `idle` : fermeture ordonnee
        // (GOAWAY), meme si le client repond aux PING ; fin de flux si elle
        // n'aboutit pas dans `GRACE`.
        if this.state.h2.load(Ordering::SeqCst) && this.state.in_flight.load(Ordering::SeqCst) == 0
        {
            let since = *this
                .state
                .idle_since
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let closing = this.state.closing.load(Ordering::SeqCst);
            let limit = if closing {
                this.idle + GRACE
            } else {
                this.idle
            };
            this.sleep.as_mut().reset(since + limit);
            if this.sleep.as_mut().poll(cx).is_ready() {
                let shutdown = this
                    .state
                    .shutdown
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                match shutdown {
                    Some(shutdown) if !closing => {
                        this.state.closing.store(true, Ordering::SeqCst);
                        shutdown.graceful_shutdown();
                        this.sleep.as_mut().reset(since + this.idle + GRACE);
                        let _ = this.sleep.as_mut().poll(cx);
                    }
                    _ => return Poll::Ready(Ok(())),
                }
            }
        }
        *this.state.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Limited<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

/// Acceptation sous plafond : au-dela, la connexion est fermee aussitot.
#[derive(Clone)]
pub struct LimitAcceptor {
    limits: ConnLimits,
    idle: Duration,
}

/// Service d'une connexion : pose l'etat de la connexion en extension de
/// chaque requete.
#[derive(Clone)]
pub struct WithConn<S> {
    inner: S,
    state: Arc<ConnState>,
}

impl<S, R> Service<Request<R>> for WithConn<S>
where
    S: Service<Request<R>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request<R>) -> Self::Future {
        request.extensions_mut().insert(self.state.clone());
        self.inner.call(request)
    }
}

impl<S> Accept<TcpStream, S> for LimitAcceptor {
    type Stream = Limited<TcpStream>;
    type Service = WithConn<S>;
    type Future = Ready<io::Result<(Self::Stream, WithConn<S>)>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let guard = stream
            .peer_addr()
            .ok()
            .and_then(|peer| self.limits.try_acquire(peer.ip()));
        ready(match guard {
            Some(guard) => {
                let state = Arc::new(ConnState::new());
                Ok((
                    Limited {
                        inner: stream,
                        _guard: guard,
                        state: state.clone(),
                        idle: self.idle,
                        sleep: Box::pin(tokio::time::sleep(self.idle)),
                    },
                    WithConn {
                        inner: service,
                        state,
                    },
                ))
            }
            None => Err(io::Error::other("plafond de connexions atteint")),
        })
    }
}

fn harden<Acc>(mut server: Server<SocketAddr, Acc>, h: Hardening) -> Server<SocketAddr, Acc> {
    let builder = server.http_builder();
    builder.http1().header_read_timeout(Some(h.header_read));
    // HTTP/2 : un client muet repond a la sonde ou est coupe. Le client qui
    // repond aux PING sans rien demander est coupe par `with_idle_tracking`
    // (hyper n'a pas de delai d'inactivite).
    builder
        .http2()
        .keep_alive_interval(Some(h.h2_idle))
        .keep_alive_timeout(h.h2_idle);
    server.first_request_timeout(h.first_request)
}

/// Corps de requete dont la lecture totale est bornee : un client qui le
/// distille octet par octet (chaque lecture arrivant a temps) est coupe.
struct Deadline {
    inner: Body,
    sleep: Pin<Box<Sleep>>,
}

impl HttpBody for Deadline {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        if self.sleep.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Some(Err(axum::Error::new(io::Error::new(
                io::ErrorKind::TimedOut,
                "delai de lecture du corps depasse",
            )))));
        }
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

async fn deadline(limit: Duration, request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let body = Body::new(Deadline {
        inner: body,
        sleep: Box::pin(tokio::time::sleep(limit)),
    });
    next.run(Request::from_parts(parts, body)).await
}

/// Borne a `limit` la lecture totale du corps de chaque requete du routeur.
pub fn with_body_deadline(router: Router, limit: Duration) -> Router {
    router.layer(middleware::from_fn(move |request, next| {
        deadline(limit, request, next)
    }))
}

/// Reponse dont la fin (dernier frame ou abandon) rend la requete en cours.
struct Tracked {
    inner: Body,
    guard: Option<InFlight>,
}

impl HttpBody for Tracked {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        if matches!(polled, Poll::Ready(None | Some(Err(_)))) || self.inner.is_end_stream() {
            self.guard = None;
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

async fn track(request: Request, next: Next) -> Response {
    let Some(state) = request.extensions().get::<Arc<ConnState>>().cloned() else {
        return next.run(request).await;
    };
    if request.version() == Version::HTTP_2 {
        if let Some(shutdown) = request.extensions().get::<ConnectionShutdown>() {
            *state.shutdown.lock().unwrap_or_else(|e| e.into_inner()) = Some(shutdown.clone());
        }
        state.h2.store(true, Ordering::SeqCst);
    }
    // Tenu pendant le traitement : une requete abandonnee rend sa place.
    let guard = InFlight::start(state);
    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    let body = Body::new(Tracked {
        inner: body,
        guard: Some(guard),
    });
    Response::from_parts(parts, body)
}

/// Suit les requetes en cours de chaque connexion des ecouteurs bornes
/// ([`plain`], [`tls`]). Une connexion HTTP/2 sans requete en cours depuis
/// `h2_idle` est fermee (GOAWAY, puis coupure apres `GRACE`) ; un flux long
/// (logs en direct) compte comme en cours jusqu'a sa fin et n'est jamais coupe.
pub fn with_idle_tracking(router: Router) -> Router {
    router.layer(middleware::from_fn(track))
}

/// Ecouteur HTTP borne (ecoute locale du proxy).
pub fn plain(
    addr: SocketAddr,
    h: Hardening,
    limits: ConnLimits,
) -> Server<SocketAddr, LimitAcceptor> {
    harden(
        axum_server::bind(addr).acceptor(LimitAcceptor {
            limits,
            idle: h.h2_idle,
        }),
        h,
    )
}

/// Ecouteur HTTPS borne (acces direct) ; poignee TLS bornee par rustls.
pub fn tls(
    addr: SocketAddr,
    config: RustlsConfig,
    h: Hardening,
    limits: ConnLimits,
) -> Server<SocketAddr, RustlsAcceptor<LimitAcceptor>> {
    harden(
        axum_server::bind(addr).acceptor(RustlsAcceptor::new(config).acceptor(LimitAcceptor {
            limits,
            idle: h.h2_idle,
        })),
        h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_are_global_and_per_ip_and_released_on_drop() {
        let limits = ConnLimits::new(3, 2);
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();
        let a1 = limits.try_acquire(a).unwrap();
        let _a2 = limits.try_acquire(a).unwrap();
        assert!(limits.try_acquire(a).is_none(), "plafond par IP");
        let _b1 = limits.try_acquire(b).unwrap();
        assert!(limits.try_acquire(b).is_none(), "plafond global");
        drop(a1);
        assert_eq!(limits.open(), 2);
        assert!(limits.try_acquire(b).is_some());
    }

    #[test]
    fn ipv6_is_counted_per_64_and_mapped_ipv4_as_ipv4() {
        let limits = ConnLimits::new(10, 1);
        let _x = limits
            .try_acquire("2001:db8:1:2::1".parse().unwrap())
            .unwrap();
        assert!(
            limits
                .try_acquire("2001:db8:1:2::ffff".parse().unwrap())
                .is_none()
        );
        assert!(
            limits
                .try_acquire("2001:db8:1:3::1".parse().unwrap())
                .is_some()
        );
        let _v4 = limits.try_acquire("192.0.2.7".parse().unwrap()).unwrap();
        assert!(
            limits
                .try_acquire("::ffff:192.0.2.7".parse().unwrap())
                .is_none()
        );
    }
}
