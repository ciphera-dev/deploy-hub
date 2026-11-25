//! Table de routes : validation, recherche, echange atomique ; socket de
//! pilotage (Unix) ; masquage des jetons dans les journaux d'acces.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use ciphera_proxy::access_log::log_path;
use ciphera_proxy::routes::{RouteError, RouteTable, parse_host, path_is_normal};
use proto::{Route, TlsMode};

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

fn route(host: &str, prefix: &str, upstream: &str) -> Route {
    Route {
        host: host.into(),
        path_prefix: prefix.into(),
        upstreams: vec![addr(upstream)],
        redirect_to: None,
        tls: TlsMode::None,
        service: None,
    }
}

/// Nature d'erreur attendue pour une table refusee.
type ErrorKind = fn(&RouteError) -> bool;

fn upstream_of(table: &RouteTable, host: &str, path: &str) -> Option<SocketAddr> {
    table.snapshot().lookup(host, path)?.pick_upstream()
}

#[test]
fn lookup_uses_the_longest_matching_prefix_on_segment_boundaries() {
    let table = RouteTable::new();
    table
        .apply(vec![
            route("app.example.com", "/", "10.0.0.1:80"),
            route("app.example.com", "/api", "10.0.0.2:80"),
            route("app.example.com", "/api/v2/", "10.0.0.3:80"),
            route("other.example.com", "/", "10.0.0.4:80"),
        ])
        .unwrap();
    let up = |path| upstream_of(&table, "app.example.com", path);
    assert_eq!(up("/"), Some(addr("10.0.0.1:80")));
    assert_eq!(up("/api"), Some(addr("10.0.0.2:80")));
    assert_eq!(up("/api/users"), Some(addr("10.0.0.2:80")));
    // "/api" ne couvre pas "/apix" (frontiere de segment).
    assert_eq!(up("/apix"), Some(addr("10.0.0.1:80")));
    assert_eq!(up("/api/v2/x"), Some(addr("10.0.0.3:80")));
    assert_eq!(
        upstream_of(&table, "other.example.com", "/x"),
        Some(addr("10.0.0.4:80"))
    );
    assert!(table.snapshot().lookup("nope.example.com", "/").is_none());
    // Sans route "/", un chemin hors prefixe ne correspond a rien.
    table
        .apply(vec![route("app.example.com", "/api", "10.0.0.2:80")])
        .unwrap();
    assert!(table.snapshot().lookup("app.example.com", "/").is_none());
}

#[test]
fn upstreams_are_used_in_turn() {
    let table = RouteTable::new();
    let mut r = route("app.example.com", "/", "10.0.0.1:80");
    r.upstreams.push(addr("10.0.0.2:80"));
    table.apply(vec![r]).unwrap();
    let snap = table.snapshot();
    let entry = snap.lookup("app.example.com", "/").unwrap();
    let picks: Vec<_> = (0..4).map(|_| entry.pick_upstream().unwrap()).collect();
    assert_eq!(
        picks,
        [
            addr("10.0.0.1:80"),
            addr("10.0.0.2:80"),
            addr("10.0.0.1:80"),
            addr("10.0.0.2:80")
        ]
    );
}

#[test]
fn invalid_tables_are_refused_whole_and_the_previous_one_stays() {
    let table = RouteTable::new();
    table
        .apply(vec![route("app.example.com", "/", "10.0.0.1:80")])
        .unwrap();
    let redirect = |to: &str| Route {
        redirect_to: Some(to.into()),
        upstreams: Vec::new(),
        ..route("r.example.com", "/", "10.0.0.9:80")
    };
    let cases: Vec<(Route, ErrorKind)> = vec![
        (route("App.Example.com", "/", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::Host(_))
        }),
        (route("bad host", "/", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::Host(_))
        }),
        (route("a..b", "/", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::Host(_))
        }),
        (route("app.example.com:80", "/", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::Host(_))
        }),
        (route("app.example.com", "api", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::PathPrefix(_))
        }),
        (route("app.example.com", "/a?b", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::PathPrefix(_))
        }),
        (route("app.example.com", "/", "0.0.0.0:80"), |e| {
            matches!(e, RouteError::Upstream(_))
        }),
        (route("app.example.com", "/", "10.0.0.1:0"), |e| {
            matches!(e, RouteError::Upstream(_))
        }),
        (route("app.example.com", "/", "169.254.169.254:80"), |e| {
            matches!(e, RouteError::Upstream(_))
        }),
        (route("app.example.com", "/", "[fe80::1]:80"), |e| {
            matches!(e, RouteError::Upstream(_))
        }),
        (route("app.example.com", "/", "[febf::1]:80"), |e| {
            matches!(e, RouteError::Upstream(_))
        }),
        (
            route("app.example.com", "/", "[::ffff:169.254.1.1]:80"),
            |e| matches!(e, RouteError::Upstream(_)),
        ),
        (route("app.example.com", "/a/../b", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::PathPrefix(_))
        }),
        (route("app.example.com", "/a%2Fb", "10.0.0.1:80"), |e| {
            matches!(e, RouteError::PathPrefix(_))
        }),
        (redirect("javascript:alert(1)"), |e| {
            matches!(e, RouteError::Redirect(_))
        }),
        (redirect("https://evil.example.com/path"), |e| {
            matches!(e, RouteError::Redirect(_))
        }),
        (redirect("https://evil.example.com?x"), |e| {
            matches!(e, RouteError::Redirect(_))
        }),
        (redirect("https://user@evil.example.com"), |e| {
            matches!(e, RouteError::Redirect(_))
        }),
        (redirect("https://a b"), |e| {
            matches!(e, RouteError::Redirect(_))
        }),
    ];
    for (bad, expected) in cases {
        let err = table
            .apply(vec![
                route("ok.example.com", "/", "10.0.0.5:80"),
                bad.clone(),
            ])
            .unwrap_err();
        assert!(expected(&err), "{bad:?} -> {err:?}");
        assert_eq!(
            upstream_of(&table, "app.example.com", "/"),
            Some(addr("10.0.0.1:80")),
            "table precedente conservee"
        );
        assert!(table.snapshot().lookup("ok.example.com", "/").is_none());
    }
    let dup = table
        .apply(vec![
            route("app.example.com", "/", "10.0.0.1:80"),
            route("app.example.com", "/", "10.0.0.2:80"),
        ])
        .unwrap_err();
    assert!(matches!(dup, RouteError::Duplicate(_)), "{dup:?}");
    // Redirections valides : origine seule, port facultatif.
    table
        .apply(vec![
            redirect("https://www.example.com"),
            Route {
                host: "s.example.com".into(),
                ..redirect("http://www.example.com:8080/")
            },
        ])
        .unwrap();
    assert_eq!(table.snapshot().len(), 2);
}

/// Pendant qu'une table remplace l'autre, aucune requete ne voit un melange :
/// chaque requete lit un instantane, et chaque instantane est entierement
/// l'ancienne table ou entierement la nouvelle.
#[test]
fn apply_is_atomic_no_request_sees_partial_table() {
    const HOSTS: usize = 200;
    let generation = |g: u16| -> Vec<Route> {
        (0..HOSTS)
            .map(|i| {
                route(
                    &format!("h{i}.example.com"),
                    "/",
                    &format!("10.0.{}.{}:{}", i / 250, i % 250 + 1, 1000 + g),
                )
            })
            .collect()
    };
    let table = Arc::new(RouteTable::new());
    table.apply(generation(0)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let checked = Arc::new(AtomicUsize::new(0));
    let readers: Vec<_> = (0..8)
        .map(|_| {
            let (table, stop, checked) = (table.clone(), stop.clone(), checked.clone());
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let snap = table.snapshot();
                    let ports: Vec<u16> = (0..HOSTS)
                        .map(|i| {
                            snap.lookup(&format!("h{i}.example.com"), "/")
                                .expect("tous les hotes de chaque generation")
                                .pick_upstream()
                                .unwrap()
                                .port()
                        })
                        .collect();
                    let mixed = ports.iter().filter(|p| **p != ports[0]).count();
                    assert_eq!(mixed, 0, "table partielle vue (generations melangees)");
                    assert_eq!(snap.len(), HOSTS);
                    checked.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();
    for g in 1..=300u16 {
        table.apply(generation(g % 2)).unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    for r in readers {
        r.join().expect("lecteur sans panique");
    }
    assert!(checked.load(Ordering::Relaxed) > 0);
}

#[test]
fn host_header_is_parsed_and_validated() {
    let ok = |v: &str| parse_host(v.as_bytes()).unwrap_or_else(|e| panic!("{v}: {e:?}"));
    let h = ok("App.Example.COM");
    assert_eq!(
        (h.name.as_str(), h.port, h.ip),
        ("app.example.com", None, None)
    );
    let h = ok("app.example.com.:8080");
    assert_eq!((h.name.as_str(), h.port), ("app.example.com", Some(8080)));
    let h = ok("203.0.113.7:80");
    assert_eq!(h.ip, Some("203.0.113.7".parse().unwrap()));
    assert_eq!(h.url_authority(), "203.0.113.7");
    let h = ok("[2001:DB8::1]:80");
    assert_eq!(h.ip, Some("2001:db8::1".parse().unwrap()));
    assert_eq!(h.url_authority(), "[2001:db8::1]");
    assert_eq!(ok("localhost").name, "localhost");
    for bad in [
        "",
        " ",
        "bad host",
        "a..b",
        ".example.com",
        "exa_mple.com",
        "-a.example.com",
        "a-.example.com",
        "example.com:",
        "example.com:0",
        "example.com:65536",
        "example.com:8x",
        "[::1",
        "::1",
        "[::1]x",
        "[not-ip]",
        "a.example.com/x",
        "user@a.example.com",
        "a.example.com\r\nX: y",
        "é.example.com",
    ] {
        assert!(parse_host(bad.as_bytes()).is_err(), "{bad:?} accepte");
    }
    let long = format!("{}.com", "a".repeat(64));
    assert!(parse_host(long.as_bytes()).is_err(), "label de 64");
    assert!(parse_host(&[b'a', 0xff]).is_err());
}

#[test]
fn request_paths_with_dot_segments_or_encoded_separators_are_not_normal() {
    for ok in [
        "/",
        "/api/projects",
        "/a.b/c..d/.well-known/x",
        "/files/My%20Doc",
        "/a//b",
    ] {
        assert!(path_is_normal(ok), "{ok}");
    }
    for bad in [
        "/..",
        "/a/../b",
        "/a/./b",
        "/a/.",
        "/public/%2e%2e/api",
        "/public/%2E./api",
        "/a%2fb",
        "/a%2Fb",
        "/a%5cb",
        "/a\\b",
    ] {
        assert!(!path_is_normal(bad), "{bad}");
    }
}

#[test]
fn upstreams_in_other_private_ranges_are_accepted() {
    let table = RouteTable::new();
    table
        .apply(vec![
            route("a.example.com", "/", "172.18.0.5:8080"),
            route("b.example.com", "/", "[fd00::5]:8080"),
            route("c.example.com", "/", "169.253.0.1:80"),
            route("d.example.com", "/", "[fec0::1]:80"),
        ])
        .unwrap();
}

fn app_route(host: &str, upstream: &str) -> Route {
    Route {
        service: Some(shared::ids::ServiceId::new()),
        ..route(host, "/", upstream)
    }
}

#[test]
fn loopback_upstream_refused_for_app_route() {
    let table = RouteTable::new();
    for up in [
        "127.0.0.1:8080",
        "127.0.0.2:8080",
        "[::1]:8080",
        "[::ffff:127.0.0.1]:8080",
    ] {
        let err = table
            .apply(vec![app_route("a.example.com", up)])
            .unwrap_err();
        assert!(matches!(err, RouteError::Upstream(_)), "{up} : {err}");
    }
    // Les autres adresses interdites le restent (non specifiee, lien-local,
    // multicast), et une adresse de conteneur passe.
    for up in [
        "0.0.0.0:80",
        "169.254.169.254:80",
        "224.0.0.1:80",
        "[ff02::1]:80",
    ] {
        let err = table
            .apply(vec![app_route("a.example.com", up)])
            .unwrap_err();
        assert!(matches!(err, RouteError::Upstream(_)), "{up} : {err}");
    }
    table
        .apply(vec![app_route("a.example.com", "10.89.0.5:8080")])
        .unwrap();
    // Seul le panneau (ni service ni redirection) joint l'ecoute locale.
    table
        .apply(vec![
            route("panel.example.com", "/", "127.0.0.1:8444"),
            route("panel6.example.com", "/", "[::1]:8444"),
        ])
        .unwrap();
    // Un refus laisse la table precedente en place.
    assert!(
        table
            .apply(vec![
                route("panel.example.com", "/", "127.0.0.1:8444"),
                app_route("b.example.com", "127.0.0.1:9"),
            ])
            .is_err()
    );
    assert!(table.snapshot().lookup("panel.example.com", "/").is_some());
}

#[test]
fn loopback_upstream_allowed_only_by_the_explicit_test_table() {
    let table = RouteTable::with_loopback_upstreams();
    table
        .apply(vec![app_route("a.example.com", "127.0.0.1:8080")])
        .unwrap();
    assert!(
        table
            .apply(vec![app_route("a.example.com", "169.254.169.254:80")])
            .is_err()
    );
}

#[test]
fn access_log_path_drops_the_query_and_redacts_tokens() {
    let cases = [
        ("/", "/"),
        ("/api/projects?secret=1", "/api/projects"),
        (
            "/api/auth/password/reset/AbCdEf_123-xyz",
            "/api/auth/password/reset/[redacted]",
        ),
        (
            "/api/auth/password/reset/AbCdEf?x=1",
            "/api/auth/password/reset/[redacted]",
        ),
        (
            "/api/auth/invitations/tok123/accept",
            "/api/auth/invitations/[redacted]/accept",
        ),
        ("/invite/tok123", "/invite/[redacted]"),
        ("/reset/tok123", "/reset/[redacted]"),
        ("/reset/", "/reset/"),
        ("/api/auth/password/reset", "/api/auth/password/reset"),
        ("/resetx/abc", "/resetx/abc"),
        ("/static/invite/abc", "/static/invite/abc"),
        // Formes qu'un amont tolerant lirait comme un chemin a jeton.
        ("/invite//tok123", "/invite/[redacted]"),
        ("//invite/tok123", "/invite/[redacted]"),
        ("/%69nvite/tok123", "/invite/[redacted]"),
        ("/%2569nvite/tok123", "/invite/[redacted]"),
        ("/INVITE/tok123", "/invite/[redacted]"),
        ("/static/../invite/tok123", "/invite/[redacted]"),
        ("/invite/./tok123", "/invite/[redacted]"),
        ("/invite%2ftok123", "/invite/[redacted]"),
        ("/invite\\tok123", "/invite/[redacted]"),
        (
            "/api/auth//invitations/tok123/accept",
            "/api/auth/invitations/[redacted]/[redacted]",
        ),
        (
            "/api/auth/invitations/tok123/%61ccept",
            "/api/auth/invitations/[redacted]/[redacted]",
        ),
        (
            "/api/auth/invitations/tok123/accept/",
            "/api/auth/invitations/[redacted]/accept",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(log_path(input), want, "{input}");
    }
}

#[cfg(unix)]
mod control_socket {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use ciphera_proxy::acme::{self, AcmeConfig, AcmeIssuer, CertStore, Challenges};
    use ciphera_proxy::control;
    use proto::codec;
    use proto::{CertificateOutcome, ControlToAgent, ProxyReply};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;
    use tokio::sync::watch;

    use super::*;

    async fn start() -> (tempfile::TempDir, std::path::PathBuf, Arc<RouteTable>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy.sock");
        let listener = control::bind(&path).unwrap();
        let table = Arc::new(RouteTable::new());
        let (_tx, rx) = watch::channel(false);
        let t = table.clone();
        tokio::spawn(async move {
            let _keep = _tx;
            control::serve(listener, t, None, rx).await;
        });
        (dir, path, table)
    }

    /// Lit jusqu'a la fermeture par le proxy (avec delai maximal).
    async fn read_to_close(s: &mut UnixStream) -> Vec<u8> {
        let mut out = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut out))
            .await
            .expect("le proxy ferme la connexion")
            .ok();
        out
    }

    /// Refus explicite : une trame `Rejected` puis la fermeture. Une
    /// connexion coupee sans reponse (proxy tombe) ne compte pas.
    fn rejected(bytes: &[u8]) -> bool {
        matches!(
            codec::decode::<ProxyReply>(bytes),
            Ok(ProxyReply::Rejected { .. })
        )
    }

    /// Le service repond encore a une table valide (vide).
    async fn still_serving(path: &std::path::Path) {
        let mut s = UnixStream::connect(path).await.unwrap();
        codec::write_frame(&mut s, &ControlToAgent::ApplyRoutes(Vec::new()))
            .await
            .unwrap();
        let reply: ProxyReply =
            tokio::time::timeout(Duration::from_secs(5), codec::read_frame(&mut s))
                .await
                .expect("reponse du proxy")
                .unwrap()
                .unwrap();
        assert_eq!(reply, ProxyReply::Applied { routes: 0 });
    }

    #[tokio::test]
    async fn control_socket_rejects_non_proto_payload() {
        let (_dir, path, table) = start().await;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o660);

        let mut huge = (codec::MAX_FRAME_LEN as u32 + 1).to_be_bytes().to_vec();
        huge.extend_from_slice(b"xxxx");
        let other_message = codec::encode(&ControlToAgent::RequestMetrics).unwrap();
        let mut garbage_frame = 5u32.to_be_bytes().to_vec();
        garbage_frame.extend_from_slice(&[0xff; 5]);
        // Longueur valide, charge illisible : ApplyRoutes suivi d'un octet
        // de trop, puis ApplyRoutes annoncant 3 routes sans les fournir.
        let valid = codec::encode(&ControlToAgent::ApplyRoutes(Vec::new())).unwrap();
        let mut trailing = valid[4..].to_vec();
        trailing.push(0);
        let mut trailing_frame = (trailing.len() as u32).to_be_bytes().to_vec();
        trailing_frame.extend_from_slice(&trailing);
        let mut truncated = valid[4..].to_vec();
        *truncated.last_mut().unwrap() = 3;
        let mut truncated_frame = (truncated.len() as u32).to_be_bytes().to_vec();
        truncated_frame.extend_from_slice(&truncated);
        let payloads: Vec<(&str, Vec<u8>)> = vec![
            ("octet en trop", trailing_frame),
            ("routes annoncees absentes", truncated_frame),
            ("http", b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec()),
            ("json", br#"{"ApplyRoutes":[]}"#.to_vec()),
            ("trop grande", huge),
            ("postcard illisible", garbage_frame),
            ("autre message proto", other_message.to_vec()),
        ];
        for (what, payload) in payloads {
            let mut s = UnixStream::connect(&path).await.unwrap();
            s.write_all(&payload).await.unwrap();
            let reply = read_to_close(&mut s).await;
            assert!(rejected(&reply), "{what} : {reply:?}");
            assert!(table.snapshot().is_empty(), "{what} : table modifiee");
            still_serving(&path).await;
        }

        // Une vraie table passe, sur une connexion qui reste ouverte.
        let mut s = UnixStream::connect(&path).await.unwrap();
        for n in [1usize, 2] {
            let routes: Vec<Route> = (0..n)
                .map(|i| route(&format!("h{i}.example.com"), "/", "10.0.0.1:80"))
                .collect();
            codec::write_frame(&mut s, &ControlToAgent::ApplyRoutes(routes))
                .await
                .unwrap();
            let reply: ProxyReply = codec::read_frame(&mut s).await.unwrap().unwrap();
            assert_eq!(reply, ProxyReply::Applied { routes: n as u32 });
            assert_eq!(table.snapshot().len(), n);
        }
        // Table invalide : refusee, connexion conservee, table inchangee.
        codec::write_frame(
            &mut s,
            &ControlToAgent::ApplyRoutes(vec![route("bad host", "/", "10.0.0.1:80")]),
        )
        .await
        .unwrap();
        let reply: ProxyReply = codec::read_frame(&mut s).await.unwrap().unwrap();
        assert!(matches!(reply, ProxyReply::Rejected { .. }), "{reply:?}");
        assert_eq!(table.snapshot().len(), 2);
    }

    #[tokio::test]
    async fn stale_socket_file_is_replaced_but_not_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy.sock");
        drop(control::bind(&path).unwrap());
        assert!(control::bind(&path).is_ok(), "socket orphelin remplace");
        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(control::bind(&file).is_err(), "fichier ordinaire conserve");
        assert_eq!(std::fs::read(&file).unwrap(), b"x");
    }

    /// Proxy avec un emetteur ACME vers un annuaire injoignable (aucun
    /// appel reseau ne peut aboutir) et un magasin sur disque.
    async fn start_with_issuer() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        Arc<RouteTable>,
        Arc<CertStore>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy.sock");
        let state = dir.path().join("state");
        let sealer = Arc::new(acme::load_or_create_key(&state).unwrap());
        let store = Arc::new(CertStore::open(&state, sealer.clone()).unwrap());
        let issuer = Arc::new(AcmeIssuer::new(
            AcmeConfig {
                directory: "https://127.0.0.1:9/dir".into(),
                ca_file: None,
                contact: None,
            },
            state.clone(),
            sealer,
            store.clone(),
            Arc::new(Challenges::new()),
        ));
        let listener = control::bind(&path).unwrap();
        let table = Arc::new(RouteTable::new());
        let (tx, rx) = watch::channel(false);
        let t = table.clone();
        tokio::spawn(async move {
            let _keep = tx;
            control::serve(listener, t, Some(issuer), rx).await;
        });
        (dir, path, table, store)
    }

    fn acme_route(host: &str) -> Route {
        Route {
            host: host.into(),
            path_prefix: "/".into(),
            upstreams: vec![addr("10.0.0.1:80")],
            redirect_to: None,
            tls: TlsMode::Acme,
            service: None,
        }
    }

    async fn ask(path: &std::path::Path, msg: ControlToAgent) -> ProxyReply {
        let mut s = UnixStream::connect(path).await.unwrap();
        codec::write_frame(&mut s, &msg).await.unwrap();
        tokio::time::timeout(Duration::from_secs(30), codec::read_frame(&mut s))
            .await
            .expect("reponse du proxy")
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn issuance_waits_briefly_for_a_route_sent_just_after() {
        let (_dir, path, _table, _store) = start_with_issuer().await;
        let p = path.clone();
        let issue = tokio::spawn(async move {
            ask(
                &p,
                ControlToAgent::IssueCertificate {
                    host: "late.example.com".into(),
                },
            )
            .await
        });
        // La table arrive par une autre connexion, un peu apres la demande.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let reply = ask(
            &path,
            ControlToAgent::ApplyRoutes(vec![acme_route("late.example.com")]),
        )
        .await;
        assert_eq!(reply, ProxyReply::Applied { routes: 1 });
        let ProxyReply::Certificate(CertificateOutcome::Failed { reason }) = issue.await.unwrap()
        else {
            panic!("echec attendu (annuaire injoignable)");
        };
        // La route a ete vue : l'echec vient d'ACME, pas de la table.
        assert!(!reason.contains("aucune route"), "{reason}");
    }

    #[tokio::test]
    async fn a_dropped_route_keeps_its_key_and_only_removal_deletes_it() {
        let (dir, path, _table, store) = start_with_issuer().await;
        for host in ["keep.example.com", "gone.example.com"] {
            let ck = rcgen::generate_simple_self_signed(vec![host.to_string()]).unwrap();
            store
                .install(host, &ck.cert.pem(), &ck.signing_key.serialize_pem())
                .unwrap();
        }
        let certs = dir.path().join("state").join("certs");
        // Route retiree (DNS en erreur, autre serveur) : certificat garde.
        let reply = ask(
            &path,
            ControlToAgent::ApplyRoutes(vec![acme_route("keep.example.com")]),
        )
        .await;
        assert_eq!(reply, ProxyReply::Applied { routes: 1 });
        assert!(store.has("gone.example.com"), "route retiree : cle gardee");
        assert!(certs.join("gone.example.com.age").exists());

        // Etat detenu : `Issued` ou `Missing`, sans rien emettre.
        let ProxyReply::Certificate(CertificateOutcome::Issued { not_after, .. }) = ask(
            &path,
            ControlToAgent::CheckCertificate {
                host: "gone.example.com".into(),
            },
        )
        .await
        else {
            panic!("certificat detenu attendu");
        };
        assert!(not_after > 0);
        assert_eq!(
            ask(
                &path,
                ControlToAgent::CheckCertificate {
                    host: "none.example.com".into(),
                },
            )
            .await,
            ProxyReply::Certificate(CertificateOutcome::Missing)
        );

        // Domaine supprime : certificat et cle effaces, les autres gardes.
        assert_eq!(
            ask(
                &path,
                ControlToAgent::RemoveCertificate {
                    host: "gone.example.com".into(),
                },
            )
            .await,
            ProxyReply::Certificate(CertificateOutcome::Removed)
        );
        assert!(!store.has("gone.example.com"));
        assert!(
            !certs.join("gone.example.com.age").exists(),
            "cle supprimee du disque"
        );
        assert!(store.has("keep.example.com"));
        assert!(certs.join("keep.example.com.age").exists());
    }
}
