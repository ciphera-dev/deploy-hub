//! Ecouteurs du panneau (A-I1) : connexions muettes, en-tetes lents et
//! plafonds de connexions, sur de vrais sockets.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::Router;
use axum::routing::get;
use axum_server::Handle;
use control_plane::http::listener::{self, ConnLimits, Hardening};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const FAST: Hardening = Hardening {
    first_request: Duration::from_millis(400),
    header_read: Duration::from_millis(400),
    max_connections: 4,
    max_per_ip: 2,
};

async fn serve(h: Hardening) -> (SocketAddr, ConnLimits) {
    let limits = ConnLimits::new(h.max_connections, h.max_per_ip);
    let handle: Handle<SocketAddr> = Handle::new();
    let app = Router::new().route("/", get(|| async { "ok" }));
    let server = listener::plain("127.0.0.1:0".parse().unwrap(), h, limits.clone())
        .handle(handle.clone())
        .serve(app.into_make_service_with_connect_info::<SocketAddr>());
    tokio::spawn(server);
    (handle.listening().await.unwrap(), limits)
}

/// La connexion est fermee par le serveur avant `within`.
async fn closed_within(stream: &mut TcpStream, within: Duration) -> bool {
    let mut buf = [0u8; 256];
    let start = Instant::now();
    loop {
        match tokio::time::timeout(
            within.saturating_sub(start.elapsed()),
            stream.read(&mut buf),
        )
        .await
        {
            Err(_) => return false,
            Ok(Ok(0)) | Ok(Err(_)) => return true,
            Ok(Ok(_)) => continue,
        }
    }
}

async fn get_ok(addr: SocketAddr) -> bool {
    let Ok(mut s) = TcpStream::connect(addr).await else {
        return false;
    };
    if s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out)).await;
    out.starts_with(b"HTTP/1.1 200")
}

#[tokio::test]
async fn silent_connection_is_closed_after_the_first_request_delay() {
    let (addr, limits) = serve(FAST).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    assert!(closed_within(&mut s, Duration::from_secs(3)).await);
    // Place rendue a la fermeture.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(limits.open(), 0);
}

#[tokio::test]
async fn slow_headers_are_cut() {
    let (addr, _) = serve(FAST).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").await.unwrap();
    assert!(closed_within(&mut s, Duration::from_secs(3)).await);
}

#[tokio::test]
async fn idle_keep_alive_connection_is_closed() {
    let (addr, _) = serve(FAST).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).await.unwrap();
    assert!(buf[..n].starts_with(b"HTTP/1.1 200"));
    assert!(closed_within(&mut s, Duration::from_secs(3)).await);
}

#[tokio::test]
async fn connections_beyond_the_per_ip_cap_are_closed_at_once() {
    let (addr, limits) = serve(Hardening {
        first_request: Duration::from_secs(30),
        header_read: Duration::from_secs(30),
        ..FAST
    })
    .await;
    let _a = TcpStream::connect(addr).await.unwrap();
    let _b = TcpStream::connect(addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(limits.open(), 2);
    let mut c = TcpStream::connect(addr).await.unwrap();
    assert!(closed_within(&mut c, Duration::from_secs(2)).await);
    assert!(!get_ok(addr).await, "plafond atteint : refus");
    drop(_a);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(get_ok(addr).await, "place rendue");
}

#[tokio::test]
async fn normal_requests_are_served() {
    let (addr, _) = serve(FAST).await;
    for _ in 0..5 {
        assert!(get_ok(addr).await);
    }
}

#[test]
fn public_and_local_bounds() {
    let p = Hardening::PUBLIC;
    assert!(p.first_request <= Duration::from_secs(10));
    assert!(p.header_read <= Duration::from_secs(30));
    assert!(p.max_per_ip < p.max_connections);
    // Le plafond tient sous LimitNOFILE de ciphera.service (65536).
    assert!(p.max_connections + Hardening::LOCAL.max_connections < 65536 / 2);
}
