//! Proxy Pingora de bout en bout (Linux) : un serveur partage par les tests
//! du fichier, chaque test utilisant ses propres noms d'hote.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use async_trait::async_trait;
use ciphera_proxy::conn_limit::ConnLimits;
use ciphera_proxy::forward::{ProxyConfig, TlsAssets};
use ciphera_proxy::routes::RouteTable;
use pingora::server::{RunArgs, ShutdownSignal, ShutdownSignalWatch};
use proto::{Route, TlsMode};
use shared::ids::ServiceId;

struct Harness {
    http: SocketAddr,
    https: SocketAddr,
    upstream: SocketAddr,
    table: Arc<RouteTable>,
    tls: TlsAssets,
}

/// Le serveur des tests ne s'arrete jamais (fin du processus de test).
struct Never;

#[async_trait]
impl ShutdownSignalWatch for Never {
    async fn recv(&self) -> ShutdownSignal {
        std::future::pending().await
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Amont de test : renvoie l'en-tete de la requete recue dans son corps.
fn start_upstream() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            thread::spawn(move || {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    match conn.read(&mut byte) {
                        Ok(1) => head.push(byte[0]),
                        _ => return,
                    }
                }
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    head.len()
                );
                let _ = conn.write_all(reply.as_bytes());
                let _ = conn.write_all(&head);
            });
        }
    });
    addr
}

/// Demarre un proxy sur des ports libres ; renvoie ses adresses HTTP et
/// HTTPS une fois a l'ecoute.
fn start_server(
    table: Arc<RouteTable>,
    tls: &TlsAssets,
    limits: ConnLimits,
) -> (SocketAddr, SocketAddr) {
    let http: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let https: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    let cfg = ProxyConfig {
        http: vec![http.to_string()],
        https: vec![https.to_string()],
        panel_host: Some("panel.example.com".into()),
        panel_port: 8443,
        threads: 2,
        limits,
    };
    let server = ciphera_proxy::build_server(&cfg, table, tls, None, None).unwrap();
    thread::spawn(move || {
        server.run(RunArgs {
            shutdown_signal: Box::new(Never),
        })
    });
    for _ in 0..100 {
        if TcpStream::connect(http).is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    (http, https)
}

fn harness() -> &'static Harness {
    static H: OnceLock<Harness> = OnceLock::new();
    H.get_or_init(|| {
        let upstream = start_upstream();
        let table = Arc::new(RouteTable::new());
        let tls = TlsAssets::memory();
        let (http, https) = start_server(table.clone(), &tls, ConnLimits::default());
        Harness {
            http,
            https,
            upstream,
            table,
            tls,
        }
    })
}

/// Ajoute des routes a la table partagee (chaque test a ses hotes).
fn add_routes(routes: Vec<Route>) {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap();
    let h = harness();
    let mut all: Vec<Route> = h
        .table
        .snapshot()
        .routes()
        .filter(|r| !routes.iter().any(|n| n.host == r.host))
        .cloned()
        .collect();
    all.extend(routes);
    h.table.apply(all).unwrap();
}

fn upstream_route(host: &str) -> Route {
    Route {
        host: host.into(),
        path_prefix: "/".into(),
        upstreams: vec![harness().upstream],
        redirect_to: None,
        tls: TlsMode::Acme,
        service: Some(ServiceId::new()),
    }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Envoie une requete brute (deja terminee par une ligne vide) et lit la
/// reponse jusqu'a la fermeture.
fn send_raw(raw: &[u8]) -> Response {
    send_raw_within(raw, Duration::from_secs(10))
}

/// Comme `send_raw`, en lisant au plus pendant `wait` (connexion que le
/// proxy garde ouverte, par exemple apres une demande de WebSocket).
fn send_raw_within(raw: &[u8], wait: Duration) -> Response {
    send_to(harness().http, raw, wait)
}

/// Requete brute vers le proxy ecoutant sur `addr`.
fn send_to(addr: SocketAddr, raw: &[u8], wait: Duration) -> Response {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(wait)).unwrap();
    s.write_all(raw).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let split = out
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("reponse sans en-tete : {:?}", String::from_utf8_lossy(&out)));
    let head = String::from_utf8_lossy(&out[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Response {
        status,
        headers,
        body: out[split + 4..].to_vec(),
    }
}

fn get(host: &str, target: &str, extra: &str) -> Response {
    send_raw(
        format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n")
            .as_bytes(),
    )
}

#[test]
fn unknown_host_404() {
    let r = get("unknown.example.net", "/x?y=1", "");
    assert_eq!(r.status, 404);
    assert!(r.body.len() < 64, "reponse sobre : {}", r.body_text());
    assert!(!r.body_text().to_lowercase().contains("pingora"));
    assert!(
        r.header("server")
            .is_none_or(|s| !s.to_lowercase().contains("pingora"))
    );
}

#[test]
fn redirect_308_preserves_path_and_query() {
    add_routes(vec![Route {
        redirect_to: Some("https://www.redirect.example".into()),
        upstreams: Vec::new(),
        ..upstream_route("redirect.example")
    }]);
    let r = get("redirect.example", "/a/b%20c?x=1&y=two", "");
    assert_eq!(r.status, 308);
    assert_eq!(
        r.header("location"),
        Some("https://www.redirect.example/a/b%20c?x=1&y=two")
    );
    let r = get("REDIRECT.example:80", "/", "");
    assert_eq!(r.status, 308);
    assert_eq!(r.header("location"), Some("https://www.redirect.example/"));
}

#[test]
fn host_header_validated() {
    for host in [
        "bad host",
        "exa_mple.com",
        "a..b",
        "[::1",
        "x.example.com:99999",
        "x.example.com/evil",
    ] {
        assert_eq!(get(host, "/", "").status, 400, "Host: {host}");
    }
    // Host absent (HTTP/1.1), double, ou contredit par la cible absolue.
    let r = send_raw(b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 400);
    let r = send_raw(
        b"GET / HTTP/1.1\r\nHost: a.example.com\r\nHost: b.example.com\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 400);
    let r = send_raw(
        b"GET http://b.example.com/ HTTP/1.1\r\nHost: a.example.com\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 400);
}

#[test]
fn ip_or_panel_host_without_route_redirects_301_to_the_panel() {
    for (host, want) in [
        ("127.0.0.1", "https://127.0.0.1:8443/setup?step=2"),
        ("203.0.113.7:80", "https://203.0.113.7:8443/setup?step=2"),
        ("[2001:db8::1]", "https://[2001:db8::1]:8443/setup?step=2"),
        (
            "panel.example.com",
            "https://panel.example.com:8443/setup?step=2",
        ),
    ] {
        let r = get(host, "/setup?step=2", "");
        assert_eq!(r.status, 301, "{host}");
        assert_eq!(r.header("location"), Some(want), "{host}");
        assert!(r.body.is_empty(), "{host} : corps vide attendu");
    }
    // Cible sans chemin : redirection vers la racine, l'hote ne peut pas
    // etre prolonge.
    let r = send_raw(b"OPTIONS * HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 301);
    assert_eq!(r.header("location"), Some("https://127.0.0.1:8443/"));
}

#[test]
fn forwards_to_upstream_with_its_own_forwarded_headers() {
    add_routes(vec![upstream_route("app.forward.example")]);
    let r = get(
        "app.forward.example",
        "/hello?q=1",
        "X-Forwarded-For: 192.0.2.1\r\nForwarded: for=192.0.2.2\r\nX-Real-IP: 192.0.2.3\r\nX-Forwarded-Proto: https\r\n",
    );
    assert_eq!(r.status, 200, "{}", r.body_text());
    let seen = r.body_text().to_lowercase();
    assert!(seen.starts_with("get /hello?q=1 http/1.1\r\n"), "{seen}");
    assert!(seen.contains("host: app.forward.example\r\n"), "{seen}");
    assert!(seen.contains("x-forwarded-for: 127.0.0.1\r\n"), "{seen}");
    assert!(seen.contains("x-forwarded-proto: http\r\n"), "{seen}");
    assert!(
        !seen.contains("192.0.2."),
        "en-tetes du client retires : {seen}"
    );
    assert!(!seen.contains("forwarded: for="), "{seen}");
    assert!(!seen.contains("x-real-ip"), "{seen}");
    assert_eq!(seen.matches("x-forwarded-for").count(), 1, "{seen}");
}

#[test]
fn route_without_healthy_upstream_is_503_and_dead_upstream_502() {
    let dead: SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
    add_routes(vec![
        Route {
            upstreams: Vec::new(),
            ..upstream_route("empty.forward.example")
        },
        Route {
            upstreams: vec![dead],
            ..upstream_route("dead.forward.example")
        },
    ]);
    assert_eq!(get("empty.forward.example", "/", "").status, 503);
    assert_eq!(get("dead.forward.example", "/", "").status, 502);
}

#[test]
fn oversized_request_headers_are_refused() {
    add_routes(vec![upstream_route("big.forward.example")]);
    let big = format!("X-Big: {}\r\n", "a".repeat(40 * 1024));
    assert_eq!(get("big.forward.example", "/", &big).status, 431);
}

#[test]
fn tls_listener_without_certificates_fails_handshake_cleanly() {
    use pingora::tls::{ClientConfig, RootCertStore, ServerName, TlsConnector};
    let h = harness();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        pingora::tls::install_default_crypto_provider();
        let config = ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(h.https).await.unwrap();
        let name = ServerName::try_from("app.forward.example").unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            TlsConnector::from(Arc::new(config)).connect(name, tcp),
        )
        .await
        .expect("echec rapide, sans attente");
        assert!(result.is_err(), "aucun certificat pour ce nom");
    });
    // Le proxy reste disponible.
    assert_eq!(get("unknown.example.net", "/", "").status, 404);
}

#[test]
fn hop_by_hop_headers_are_not_forwarded() {
    add_routes(vec![upstream_route("hop.forward.example")]);
    let r = send_raw(
        b"GET / HTTP/1.1\r\nHost: hop.forward.example\r\nConnection: X-Hop, close\r\nX-Hop: secret\r\nKeep-Alive: timeout=5\r\nProxy-Connection: keep-alive\r\nTE: trailers\r\nTrailer: X-T\r\nProxy-Authorization: Basic Zm9vOmJhcg==\r\nX-Kept: yes\r\n\r\n",
    );
    assert_eq!(r.status, 200, "{}", r.body_text());
    let seen = r.body_text().to_lowercase();
    assert!(seen.contains("x-kept: yes\r\n"), "{seen}");
    for gone in [
        "x-hop",
        "keep-alive: timeout",
        "proxy-connection",
        "te: trailers",
        "trailer:",
        "proxy-authorization",
        "zm9vomjhcg",
    ] {
        assert!(!seen.contains(gone), "{gone} transmis : {seen}");
    }
}

#[test]
fn websocket_upgrade_headers_are_kept() {
    add_routes(vec![upstream_route("ws.forward.example")]);
    let r = send_raw_within(
        b"GET /ws HTTP/1.1\r\nHost: ws.forward.example\r\nConnection: Upgrade, X-Hop\r\nUpgrade: websocket\r\nX-Hop: 1\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        Duration::from_secs(2),
    );
    assert_eq!(r.status, 200, "{}", r.body_text());
    let seen = r.body_text().to_lowercase();
    assert!(seen.contains("upgrade: websocket\r\n"), "{seen}");
    assert!(seen.contains("connection: upgrade\r\n"), "{seen}");
    assert!(seen.contains("sec-websocket-key: "), "{seen}");
    assert!(!seen.contains("x-hop"), "{seen}");
}

#[test]
fn dot_segments_and_encoded_separators_are_refused_before_routing() {
    add_routes(vec![
        upstream_route("dots.forward.example"),
        Route {
            path_prefix: "/public".into(),
            ..upstream_route("dots2.forward.example")
        },
    ]);
    for target in [
        "/a/../b",
        "/./x",
        "/public/%2e%2e/admin",
        "/public/%2E%2E/admin",
        "/public%2fadmin",
        "/public/..%2Fadmin",
        "/public/%5c..%5cadmin",
    ] {
        assert_eq!(
            get("dots.forward.example", target, "").status,
            400,
            "{target}"
        );
        assert_eq!(
            get("dots2.forward.example", target, "").status,
            400,
            "{target}"
        );
    }
    assert_eq!(
        get("dots.forward.example", "/a.b/c..d/x%20y", "").status,
        200
    );
}

#[test]
fn absolute_form_needs_a_matching_host_in_http_1_1() {
    add_routes(vec![upstream_route("abs.forward.example")]);
    // Sans Host : refuse en HTTP/1.1.
    let r = send_raw(b"GET http://abs.forward.example/x HTTP/1.1\r\nConnection: close\r\n\r\n");
    assert_eq!(r.status, 400);
    // Avec un Host concordant : transmis.
    let r = send_raw(
        b"GET http://abs.forward.example/x HTTP/1.1\r\nHost: abs.forward.example\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 200, "{}", r.body_text());
}

#[test]
fn trailing_dot_host_uses_the_route() {
    add_routes(vec![upstream_route("dot.forward.example")]);
    let r = get("dot.forward.example.", "/", "");
    assert_eq!(r.status, 200, "{}", r.body_text());
}

#[test]
fn content_length_with_transfer_encoding_is_not_smuggled() {
    add_routes(vec![upstream_route("smuggle.forward.example")]);
    let r = send_raw(
        b"POST / HTTP/1.1\r\nHost: smuggle.forward.example\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n0\r\n\r\nGET /admin HTTP/1.1\r\nHost: smuggle.forward.example\r\n\r\n",
    );
    // Pingora suit RFC 9112 6.3 : Transfer-Encoding fait foi, Content-Length
    // est retire ; l'amont voit le meme decoupage que le proxy, et la
    // requete cachee apres le corps n'est pas transmise dans celui-ci.
    assert_eq!(r.status, 200, "{}", r.body_text());
    let seen = r.body_text().to_lowercase();
    assert!(!seen.contains("content-length"), "{seen}");
    assert!(seen.contains("transfer-encoding: chunked\r\n"), "{seen}");
    assert!(!seen.contains("/admin"), "{seen}");
}

#[test]
fn connect_is_405() {
    let r = send_raw(
        b"CONNECT app.forward.example:443 HTTP/1.1\r\nHost: app.forward.example:443\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.status, 405);
}

#[test]
fn more_than_100_headers_is_431() {
    add_routes(vec![upstream_route("many.forward.example")]);
    let extra: String = (0..101).map(|i| format!("X-H{i}: 1\r\n")).collect();
    assert_eq!(get("many.forward.example", "/", &extra).status, 431);
    let extra: String = (0..90).map(|i| format!("X-H{i}: 1\r\n")).collect();
    assert_eq!(get("many.forward.example", "/", &extra).status, 200);
}

/// Requete simple sur `addr` : `Some(statut)`, ou `None` si la connexion est
/// fermee sans reponse (connexion refusee par le filtre).
fn try_status(addr: SocketAddr) -> Option<u16> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: unknown.example.net\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let line = String::from_utf8_lossy(&out).into_owned();
    line.split_whitespace().nth(1)?.parse().ok()
}

#[test]
fn per_ip_connections_above_the_cap_are_refused() {
    let limits = ConnLimits {
        max_connections: 1000,
        max_connections_per_ip: 4,
        max_pending_tls: 100,
        first_byte_timeout: Duration::from_secs(30),
        ..ConnLimits::default()
    };
    let (http, _) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    // La sonde de demarrage de start_server est deja liberee.
    let mut held: Vec<TcpStream> = Vec::new();
    for _ in 0..50 {
        if try_status(http) == Some(404) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    for _ in 0..4 {
        held.push(TcpStream::connect(http).unwrap());
    }
    thread::sleep(Duration::from_millis(200));
    assert_eq!(try_status(http), None, "cinquieme connexion de la meme IP");
    assert_eq!(try_status(http), None, "toujours refusee");
    // Une connexion fermee libere sa place.
    drop(held.pop());
    let mut freed = None;
    for _ in 0..50 {
        freed = try_status(http);
        if freed.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(freed, Some(404));
    drop(held);
}

#[test]
fn silent_connection_is_closed_after_the_first_byte_timeout() {
    let limits = ConnLimits {
        first_byte_timeout: Duration::from_secs(1),
        ..ConnLimits::default()
    };
    let (http, _) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    let mut s = TcpStream::connect(http).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let start = std::time::Instant::now();
    let mut buf = [0u8; 16];
    let n = s.read(&mut buf).unwrap_or(0);
    assert_eq!(n, 0, "fermee sans reponse");
    let waited = start.elapsed();
    assert!(
        waited >= Duration::from_millis(800) && waited < Duration::from_secs(5),
        "{waited:?}"
    );
}

#[test]
fn failed_tls_handshakes_do_not_lock_out_port_80() {
    // Plafond global volontairement bas : 30 poignees de main ratees (non
    // liberees avant 11 s), au-dela du plafond des attentes TLS (16, plus
    // haut que celui des actives), ne doivent pas occuper le port 80.
    let limits = ConnLimits {
        max_connections: 8,
        max_connections_per_ip: 1000,
        max_pending_tls: 16,
        max_pending_tls_per_ip: 1000,
        first_byte_timeout: Duration::from_secs(30),
        ..ConnLimits::default()
    };
    let (http, https) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    for _ in 0..30 {
        if let Ok(s) = TcpStream::connect(https) {
            drop(s);
        }
    }
    thread::sleep(Duration::from_millis(300));
    for i in 0..10 {
        assert_eq!(try_status(http), Some(404), "requete {i} sur le port 80");
    }
}

/// Poignee de main TLS (SNI `host`) en ne faisant confiance qu'a `root` ;
/// `GET /` puis la reponse brute, ou l'erreur de la poignee de main.
fn tls_get(host: &str, root: &[u8]) -> Result<String, String> {
    tls_request(host, host, root)
}

/// Comme `tls_get`, avec un SNI (`sni`) et un en-tete `Host` distincts.
fn tls_request(sni: &str, host: &str, root: &[u8]) -> Result<String, String> {
    use pingora::tls::{ClientConfig, RootCertStore, ServerName, TlsConnector};
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        pingora::tls::install_default_crypto_provider();
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_slice(root).unwrap())
            .unwrap();
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(harness().https)
            .await
            .unwrap();
        let name = ServerName::try_from(sni.to_string()).unwrap();
        let mut tls = tokio::time::timeout(
            Duration::from_secs(5),
            TlsConnector::from(Arc::new(config)).connect(name, tcp),
        )
        .await
        .expect("poignee de main rapide")
        .map_err(|e| e.to_string())?;
        tls.write_all(
            format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();
        let mut out = Vec::new();
        let _ = tls.read_to_end(&mut out).await;
        Ok(String::from_utf8_lossy(&out).into_owned())
    })
}

#[test]
fn acme_route_redirects_to_https_with_308_once_its_certificate_exists() {
    let host = "secure.forward.example";
    add_routes(vec![upstream_route(host)]);
    // Sans certificat : servie en HTTP.
    assert_eq!(get(host, "/x?y=1", "").status, 200);

    let ck = rcgen::generate_simple_self_signed(vec![host.to_string()]).unwrap();
    let cert_pem = ck.cert.pem();
    harness()
        .tls
        .certs
        .install(host, &cert_pem, &ck.signing_key.serialize_pem())
        .unwrap();

    let r = get(host, "/a/b?x=1&y=two", "");
    assert_eq!(r.status, 308);
    assert_eq!(
        r.header("location"),
        Some("https://secure.forward.example/a/b?x=1&y=two")
    );
    assert!(r.body.is_empty());
    // Les defis restent en HTTP clair (jeton inconnu : transmis a l'amont).
    let r = get(host, "/.well-known/acme-challenge/unknown-token", "");
    assert_eq!(r.status, 200, "{}", r.body_text());
    // Route TlsMode::None : jamais redirigee.
    add_routes(vec![Route {
        tls: TlsMode::None,
        ..upstream_route("plain.forward.example")
    }]);
    assert_eq!(get("plain.forward.example", "/", "").status, 200);

    // 443 : certificat choisi par SNI, requete transmise.
    let reply = tls_get(host, cert_pem.as_bytes()).expect("poignee de main TLS");
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(
        reply.to_lowercase().contains("x-forwarded-proto: https"),
        "{reply}"
    );
    // Autre nom : aucun certificat, refus propre.
    assert!(tls_get("other.forward.example", cert_pem.as_bytes()).is_err());
}

#[test]
fn acme_challenge_is_answered_on_plain_http_only() {
    let h = harness();
    h.tls
        .challenges
        .insert("tok-EN_123", "tok-EN_123.thumbprint".into())
        .unwrap();
    // Tout hote (la validation precede la route).
    let r = get(
        "chal.forward.example",
        "/.well-known/acme-challenge/tok-EN_123",
        "",
    );
    assert_eq!(r.status, 200);
    assert_eq!(r.body_text(), "tok-EN_123.thumbprint");
    let r = get(
        "chal.forward.example",
        "/.well-known/acme-challenge/other",
        "",
    );
    assert_eq!(r.status, 404);
    h.tls.challenges.remove("tok-EN_123");
    let r = get(
        "chal.forward.example",
        "/.well-known/acme-challenge/tok-EN_123",
        "",
    );
    assert_eq!(r.status, 404);
}

/// Route du domaine du panneau, telle que l'agent la transmet (ni service ni
/// redirection ; amont : l'ecoute locale du panneau, ou aucun).
fn panel_route(host: &str, upstreams: Vec<SocketAddr>) -> Route {
    Route {
        host: host.into(),
        path_prefix: "/".into(),
        upstreams,
        redirect_to: None,
        tls: TlsMode::Acme,
        service: None,
    }
}

#[test]
fn panel_route_is_never_served_in_clear_and_takes_over_the_ip_fallback() {
    // Serveur a part : la route du panneau change le repli des hotes IP.
    let table = Arc::new(RouteTable::new());
    let tls = TlsAssets::memory();
    let (http, _) = start_server(table.clone(), &tls, ConnLimits::default());
    let get_at = |host: &str, target: &str| {
        send_to(
            http,
            format!("GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
            Duration::from_secs(10),
        )
    };
    let host = "panel.switch.example";

    // Avant le certificat : route sans amont (pour l'emission ACME). Le
    // port 80 ne sert aucun panneau ; les hotes IP vont encore en 8443.
    table.apply(vec![panel_route(host, Vec::new())]).unwrap();
    let r = get_at(host, "/login");
    assert_eq!(r.status, 503, "{}", r.body_text());
    assert!(!r.body_text().to_lowercase().contains("get /login"));
    let r = get_at("203.0.113.7", "/setup?step=2");
    assert_eq!(r.status, 301);
    assert_eq!(
        r.header("location"),
        Some("https://203.0.113.7:8443/setup?step=2")
    );

    // Route servie mais sans certificat dans ce proxy : jamais transmise en
    // HTTP clair.
    table
        .apply(vec![panel_route(host, vec![harness().upstream])])
        .unwrap();
    let r = get_at(host, "/login");
    assert_eq!(r.status, 503, "{}", r.body_text());
    assert!(!r.body_text().to_lowercase().contains("get /login"));

    // Panneau servi sur son domaine : les hotes IP et l'ancien nom y sont
    // renvoyes, plus jamais vers 8443.
    for (h, want) in [
        (
            "203.0.113.7:80",
            "https://panel.switch.example/setup?step=2",
        ),
        ("[2001:db8::1]", "https://panel.switch.example/setup?step=2"),
        (
            "panel.example.com",
            "https://panel.switch.example/setup?step=2",
        ),
    ] {
        let r = get_at(h, "/setup?step=2");
        assert_eq!(r.status, 301, "{h}");
        assert_eq!(r.header("location"), Some(want), "{h}");
        assert!(r.body.is_empty(), "{h}");
    }

    // Certificat installe : 308 vers HTTPS (defis exclus).
    let ck = rcgen::generate_simple_self_signed(vec![host.to_string()]).unwrap();
    tls.certs
        .install(host, &ck.cert.pem(), &ck.signing_key.serialize_pem())
        .unwrap();
    let r = get_at(host, "/login?x=1");
    assert_eq!(r.status, 308);
    assert_eq!(
        r.header("location"),
        Some("https://panel.switch.example/login?x=1")
    );
}

/// Le SNI de la poignee de main et l'en-tete `Host` doivent designer le
/// meme hote route : sinon 421 (requete mal dirigee), jamais transmise.
#[test]
fn https_host_must_match_the_sni_of_the_handshake() {
    let a = "sni-a.forward.example";
    let b = "sni-b.forward.example";
    add_routes(vec![upstream_route(a), upstream_route(b)]);
    // Un certificat qui couvre les deux noms : seule la verification du
    // proxy peut refuser.
    let ck = rcgen::generate_simple_self_signed(vec![a.to_string(), b.to_string()]).unwrap();
    let cert_pem = ck.cert.pem();
    let key_pem = ck.signing_key.serialize_pem();
    for host in [a, b] {
        harness()
            .tls
            .certs
            .install(host, &cert_pem, &key_pem)
            .unwrap();
    }
    let ok = tls_request(a, a, cert_pem.as_bytes()).unwrap();
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    let ok = tls_request(a, "SNI-A.forward.example.", cert_pem.as_bytes()).unwrap();
    assert!(
        ok.starts_with("HTTP/1.1 200"),
        "casse et point final : {ok}"
    );
    let crossed = tls_request(a, b, cert_pem.as_bytes()).unwrap();
    assert!(crossed.starts_with("HTTP/1.1 421"), "{crossed}");
    assert!(!crossed.contains("x-forwarded-proto"), "jamais transmise");
}

/// Lit jusqu'a la fermeture (ou `wait`) ; renvoie les octets recus et le
/// temps ecoule.
fn read_until_closed(s: &mut TcpStream, wait: Duration) -> (Vec<u8>, Duration) {
    s.set_read_timeout(Some(wait)).unwrap();
    let start = std::time::Instant::now();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    (out, start.elapsed())
}

/// Delai total de lecture des en-tetes : un client qui envoie ses en-tetes
/// octet par octet (chaque lecture sous le delai de Pingora) est coupe a
/// l'echeance, sans reponse ; des en-tetes lus a temps desarment le delai
/// (corps lent accepte).
#[test]
fn slow_headers_are_cut_at_the_total_header_deadline() {
    let limits = ConnLimits {
        header_timeout: Duration::from_secs(1),
        ..ConnLimits::default()
    };
    let (http, _) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    let mut s = TcpStream::connect(http).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: unknown.example.net\r\n")
        .unwrap();
    let start = std::time::Instant::now();
    let mut cut = false;
    for _ in 0..40 {
        thread::sleep(Duration::from_millis(100));
        if s.write_all(b"X").is_err() {
            cut = true;
            break;
        }
    }
    let (out, _) = read_until_closed(&mut s, Duration::from_secs(5));
    let waited = start.elapsed();
    assert!(cut || out.is_empty(), "{:?}", String::from_utf8_lossy(&out));
    assert!(
        !String::from_utf8_lossy(&out).contains("HTTP/1.1 4"),
        "coupee sans reponse : {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(waited < Duration::from_secs(4), "{waited:?}");

    // En-tetes complets a temps, corps envoye apres l'echeance : servi.
    add_routes(vec![upstream_route("slow-body.forward.example")]);
    let table = harness().table.clone();
    let (http, _) = start_server(table, &TlsAssets::memory(), limits);
    let mut s = TcpStream::connect(http).unwrap();
    s.write_all(
        b"POST / HTTP/1.1\r\nHost: slow-body.forward.example\r\nContent-Length: 5\r\nConnection: close\r\n\r\n",
    )
    .unwrap();
    thread::sleep(Duration::from_millis(1500));
    s.write_all(b"hello").unwrap();
    let (out, _) = read_until_closed(&mut s, Duration::from_secs(10));
    let text = String::from_utf8_lossy(&out);
    assert!(text.starts_with("HTTP/1.1 200"), "{text}");
}

/// Poignee de main TLS bornee : une connexion qui n'envoie rien (ou un
/// ClientHello incomplet) est fermee au bout du delai de poignee de main,
/// pas apres les 60 s de Pingora.
#[test]
fn tls_handshake_time_is_bounded() {
    let limits = ConnLimits {
        tls_handshake_timeout: Duration::from_secs(1),
        ..ConnLimits::default()
    };
    let (_, https) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    let start = std::time::Instant::now();
    let mut silent = TcpStream::connect(https).unwrap();
    let mut partial = TcpStream::connect(https).unwrap();
    // Debut d'un enregistrement TLS handshake annoncant 512 octets.
    partial
        .write_all(&[0x16, 0x03, 0x01, 0x02, 0x00, 0x01])
        .unwrap();
    for s in [&mut silent, &mut partial] {
        read_until_closed(s, Duration::from_secs(10));
        let waited = start.elapsed();
        assert!(
            waited >= Duration::from_millis(700) && waited < Duration::from_secs(4),
            "{waited:?}"
        );
    }
}

/// Une meme IP (un NAT) ne peut tenir qu'un nombre borne de poignees de
/// main en cours : au-dela, refus immediat, sans toucher au port 80 ; la
/// place revient apres le delai de poignee de main.
#[test]
fn pending_tls_handshakes_are_capped_per_ip_and_expire() {
    let limits = ConnLimits {
        max_pending_tls_per_ip: 3,
        tls_handshake_timeout: Duration::from_secs(1),
        first_byte_timeout: Duration::from_secs(30),
        ..ConnLimits::default()
    };
    let (http, https) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    let held: Vec<TcpStream> = (0..3).map(|_| TcpStream::connect(https).unwrap()).collect();
    thread::sleep(Duration::from_millis(200));
    let mut extra = TcpStream::connect(https).unwrap();
    let (_, waited) = read_until_closed(&mut extra, Duration::from_secs(5));
    assert!(
        waited < Duration::from_millis(700),
        "refus immediat : {waited:?}"
    );
    assert_eq!(try_status(http), Some(404), "port 80 libre");
    drop(held);
    // Delai de poignee de main (1 s) + marge de l'admission (1 s).
    thread::sleep(Duration::from_millis(2500));
    let mut again = TcpStream::connect(https).unwrap();
    let (_, waited) = read_until_closed(&mut again, Duration::from_secs(5));
    assert!(waited >= Duration::from_millis(700), "admise : {waited:?}");
}

/// Lit une reponse dont le corps a une longueur annoncee
/// (`Content-Length`), sans attendre la fermeture : renvoie le statut.
fn read_one_response(s: &mut TcpStream) -> u16 {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        assert_eq!(s.read(&mut byte).unwrap(), 1, "reponse tronquee");
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let len: usize = head
        .split("\r\n")
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| v.trim().parse().unwrap())
        .expect("Content-Length");
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).unwrap();
    head.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// Connexion persistante : l'echeance totale de lecture des en-tetes
/// (20 s) s'applique aussi a la requete suivante, pas seulement a la
/// premiere. Une requete complete est servie, puis les en-tetes de la
/// suivante arrivent octet par octet (chaque octet bien sous le delai de
/// lecture de Pingora) : la connexion est coupee vers 20 s, sans reponse.
#[test]
fn keepalive_second_request_headers_deadline() {
    add_routes(vec![upstream_route("keepalive.forward.example")]);
    let mut s = TcpStream::connect(harness().http).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: keepalive.forward.example\r\n\r\n")
        .unwrap();
    assert_eq!(read_one_response(&mut s), 200);
    let start = std::time::Instant::now();
    s.write_all(b"GET / HTTP/1.1\r\nHost: keepalive.forward.example\r\n")
        .unwrap();
    // Un octet toutes les 500 ms ; la lecture (bornee a 500 ms) sert aussi
    // d'attente et voit la fermeture.
    s.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let mut out = Vec::new();
    let mut buf = [0u8; 256];
    let waited = loop {
        match s.read(&mut buf) {
            Ok(0) => break start.elapsed(),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break start.elapsed(),
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "connexion jamais coupee"
        );
        if s.write_all(b"X").is_err() {
            break start.elapsed();
        }
    };
    assert!(
        out.is_empty(),
        "coupee sans reponse : {:?}",
        String::from_utf8_lossy(&out)
    );
    assert!(
        waited >= Duration::from_secs(19) && waited <= Duration::from_secs(22),
        "{waited:?}"
    );
}

/// Plafond par IP par defaut : 64 connexions simultanees.
#[test]
fn per_ip_cap_64() {
    let limits = ConnLimits {
        first_byte_timeout: Duration::from_secs(30),
        ..ConnLimits::default()
    };
    assert_eq!(limits.max_connections_per_ip, 64);
    let (http, _) = start_server(Arc::new(RouteTable::new()), &TlsAssets::memory(), limits);
    // La sonde de demarrage de start_server est deja liberee.
    for _ in 0..50 {
        if try_status(http) == Some(404) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let mut held: Vec<TcpStream> = (0..63).map(|_| TcpStream::connect(http).unwrap()).collect();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(try_status(http), Some(404), "64e connexion admise");
    thread::sleep(Duration::from_millis(200));
    held.push(TcpStream::connect(http).unwrap());
    thread::sleep(Duration::from_millis(200));
    assert_eq!(try_status(http), None, "65e connexion refusee");
    drop(held);
}
