//! Ecouteurs du panneau (A-I1) : connexions muettes, en-tetes lents et
//! plafonds de connexions, sur de vrais sockets ; hotes reconnus sur l'acces
//! direct (421 pour un nom non declare).

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, header};
use axum::routing::{get, post};
use axum_server::Handle;
use control_plane::domain::domains::panel::{Panel, PanelSwitch};
use control_plane::http::listener::{self, ConnLimits, Hardening};
use control_plane::http::panel_host;
use shared::config::Config;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower::ServiceExt;

const FAST: Hardening = Hardening {
    first_request: Duration::from_millis(400),
    header_read: Duration::from_millis(400),
    body_read: Duration::from_millis(500),
    h2_idle: Duration::from_millis(300),
    max_connections: 4,
    max_per_ip: 2,
};

async fn serve(h: Hardening) -> (SocketAddr, ConnLimits) {
    let limits = ConnLimits::new(h.max_connections, h.max_per_ip);
    let handle: Handle<SocketAddr> = Handle::new();
    let app = Router::new().route("/", get(|| async { "ok" })).route(
        "/",
        post(|body: axum::body::Bytes| async move { format!("lu {}", body.len()) }),
    );
    let app = listener::with_body_deadline(app, h.body_read);
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

/// Corps goutte a goutte : chaque lecture arrive a temps, le total non.
#[tokio::test]
async fn body_read_timeout() {
    let (addr, _) = serve(Hardening {
        header_read: Duration::from_secs(30),
        ..FAST
    })
    .await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 20\r\n\r\n")
        .await
        .unwrap();
    let start = Instant::now();
    for _ in 0..20 {
        if s.write_all(b"a").await.is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), s.read_to_end(&mut out)).await;
    assert!(
        !out.starts_with(b"HTTP/1.1 200"),
        "corps lent accepte : {}",
        String::from_utf8_lossy(&out)
    );
    assert!(start.elapsed() < Duration::from_secs(5));
    // Un corps qui arrive vite passe.
    let mut ok = TcpStream::connect(addr).await.unwrap();
    ok.write_all(
        b"POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
    )
    .await
    .unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(3), ok.read_to_end(&mut out)).await;
    assert!(out.starts_with(b"HTTP/1.1 200"));
}

/// HTTP/2 sans aucun flux : le serveur sonde la connexion (PING) et la ferme
/// si le client ne repond pas.
#[tokio::test]
async fn h2_idle_timeout() {
    let (addr, limits) = serve(FAST).await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    // Preface HTTP/2 puis SETTINGS vide ; le client ne repond ensuite a rien.
    s.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();
    s.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).await.unwrap();
    assert!(closed_within(&mut s, Duration::from_secs(5)).await);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(limits.open(), 0);
}

#[test]
fn public_and_local_bounds() {
    let p = Hardening::PUBLIC;
    assert!(p.first_request <= Duration::from_secs(10));
    assert!(p.header_read <= Duration::from_secs(30));
    assert!(p.max_per_ip < p.max_connections);
    for h in [p, Hardening::LOCAL] {
        assert_eq!(h.body_read, Duration::from_secs(30));
        assert_eq!(h.h2_idle, Duration::from_secs(60));
    }
    // Le plafond tient sous LimitNOFILE de ciphera.service (65536).
    assert!(p.max_connections + Hardening::LOCAL.max_connections < 65536 / 2);
}

// --- Hotes reconnus sur l'acces direct (8443) -------------------------------

fn direct_config(pairs: &'static [(&'static str, &'static str)]) -> std::sync::Arc<Config> {
    std::sync::Arc::new(
        Config::from_lookup(|name| {
            if name == "CIPHERA_DATABASE_URL" {
                return Some("postgres://u:p@127.0.0.1/c".to_string());
            }
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        })
        .unwrap(),
    )
}

/// Statut et corps d'une requete sur l'acces direct sous l'hote `host`.
async fn direct_get(
    cfg: std::sync::Arc<Config>,
    panel: PanelSwitch,
    host: Option<&str>,
) -> (u16, String) {
    let app = panel_host::direct(Router::new().route("/", get(|| async { "ok" })), cfg, panel);
    let mut req = Request::builder().uri("/");
    if let Some(h) = host {
        req = req.header(header::HOST, h);
    }
    let res = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 16)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn unknown_host_on_8443_gets_421() {
    let cfg = direct_config(&[
        ("CIPHERA_PUBLIC_URL", "https://panel.example.com:8443"),
        ("CIPHERA_PANEL_ALIASES", "admin.example.com"),
    ]);
    // Domaine du panneau enregistre mais pas encore confirme : non reconnu.
    let panel = PanelSwitch::default();
    panel.set(Some(Panel {
        id: uuid::Uuid::new_v4(),
        host: "pending.example.net".into(),
        confirmed: false,
    }));
    for host in [
        Some("app.example.org"),
        Some("app.example.org:8443"),
        Some("evil.panel.example.com"),
        Some("pending.example.net:8443"),
        Some("localhost.example.org"),
        None,
    ] {
        let (status, body) = direct_get(cfg.clone(), panel.clone(), host).await;
        assert_eq!(status, 421, "{host:?}");
        assert!(body.contains("CIPHERA_PANEL_ALIASES"), "{body}");
        assert!(
            !body.contains("app.example.org"),
            "hote non repris : {body}"
        );
    }
}

#[tokio::test]
async fn ip_and_alias_hosts_served() {
    let cfg = direct_config(&[
        ("CIPHERA_PUBLIC_URL", "https://Panel.Example.com:8443"),
        ("CIPHERA_WEBAUTHN_RP_ID", "rp.example.com"),
        (
            "CIPHERA_PANEL_ALIASES",
            "admin.example.com, ops.example.com",
        ),
    ]);
    let panel = PanelSwitch::default();
    panel.set(Some(Panel {
        id: uuid::Uuid::new_v4(),
        host: "confirmed.example.net".into(),
        confirmed: true,
    }));
    for host in [
        "203.0.113.7",
        "203.0.113.7:8443",
        "[2001:db8::1]:8443",
        "[::1]",
        "127.0.0.1:8443",
        "localhost",
        "LOCALHOST:8443",
        "panel.example.com:8443",
        "rp.example.com",
        "admin.example.com:8443",
        "OPS.example.com.",
        "confirmed.example.net:8443",
    ] {
        let (status, body) = direct_get(cfg.clone(), panel.clone(), Some(host)).await;
        assert_eq!((status, body.as_str()), (200, "ok"), "{host}");
    }
    // Sans aucun nom declare : IP et localhost seulement.
    let bare = direct_config(&[]);
    assert_eq!(
        direct_get(
            bare.clone(),
            PanelSwitch::default(),
            Some("198.51.100.4:8443")
        )
        .await
        .0,
        200
    );
    assert_eq!(
        direct_get(bare, PanelSwitch::default(), Some("panel.example.com"))
            .await
            .0,
        421
    );
}
