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
//! - pour HTTP/2, une sonde (PING) apres une minute d'inactivite, la
//!   connexion etant fermee si le client ne repond pas dans la minute ;
//! - un plafond global de connexions et un plafond par adresse IP (IPv6 :
//!   par /64), refusees des l'acceptation (fermeture immediate).
//!
//! `ciphera.service` releve `LimitNOFILE` pour que ces plafonds tiennent.

use std::collections::HashMap;
use std::future::{Future, Ready, ready};
use std::io;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum_server::Server;
use axum_server::accept::Accept;
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
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
}

impl<T: AsyncRead + Unpin> AsyncRead for Limited<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
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
}

impl<S> Accept<TcpStream, S> for LimitAcceptor {
    type Stream = Limited<TcpStream>;
    type Service = S;
    type Future = Ready<io::Result<(Self::Stream, S)>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let guard = stream
            .peer_addr()
            .ok()
            .and_then(|peer| self.limits.try_acquire(peer.ip()));
        ready(match guard {
            Some(guard) => Ok((
                Limited {
                    inner: stream,
                    _guard: guard,
                },
                service,
            )),
            None => Err(io::Error::other("plafond de connexions atteint")),
        })
    }
}

fn harden<Acc>(mut server: Server<SocketAddr, Acc>, h: Hardening) -> Server<SocketAddr, Acc> {
    let builder = server.http_builder();
    builder.http1().header_read_timeout(Some(h.header_read));
    // HTTP/2 : un client muet repond a la sonde ou est coupe ; sans cela,
    // une connexion h2 sans flux resterait ouverte indefiniment.
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

/// Ecouteur HTTP borne (ecoute locale du proxy).
pub fn plain(
    addr: SocketAddr,
    h: Hardening,
    limits: ConnLimits,
) -> Server<SocketAddr, LimitAcceptor> {
    harden(
        axum_server::bind(addr).acceptor(LimitAcceptor { limits }),
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
        axum_server::bind(addr)
            .acceptor(RustlsAcceptor::new(config).acceptor(LimitAcceptor { limits })),
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
