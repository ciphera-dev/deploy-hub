//! Emission ACME HTTP-01 de bout en bout contre Pebble (serveur ACME de
//! test de Let's Encrypt), par le socket de pilotage comme en production.
//!
//! `#[ignore]` : Pebble et son DNS de test doivent tourner sur la meme pile
//! reseau (job CI `acme-pebble`, ou localement, voir le rapport de la
//! Task 30) :
//! - `CIPHERA_TEST_ACME_DIRECTORY` : `https://localhost:14000/dir` ;
//! - `CIPHERA_TEST_ACME_CA_FILE` : `pebble.minica.pem` (copie de l'image) ;
//! - `CIPHERA_TEST_ACME_HTTP_PORT` : port ou Pebble valide HTTP-01
//!   (`httpPort` de sa configuration, 5002 par defaut).
//!
//! Le DNS de test (pebble-challtestsrv) resout tout nom vers 127.0.0.1 : le
//! proxy du test ecoute sur 127.0.0.1:<port HTTP-01>.

#![cfg(unix)]

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ciphera_proxy::acme::{self, AcmeConfig, AcmeIssuer, CertStore, Challenges};
use ciphera_proxy::conn_limit::ConnLimits;
use ciphera_proxy::control;
use ciphera_proxy::forward::{ProxyConfig, TlsAssets};
use ciphera_proxy::routes::RouteTable;
use pingora::server::{RunArgs, ShutdownSignal, ShutdownSignalWatch};
use proto::{CertificateOutcome, ControlToAgent, ProxyReply, Route, TlsMode, codec};

struct Never;

#[async_trait]
impl ShutdownSignalWatch for Never {
    async fn recv(&self) -> ShutdownSignal {
        std::future::pending().await
    }
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("{name} manquant : lancer Pebble (job CI acme-pebble, voir l'en-tete du fichier)")
    })
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn ask(stream: &mut tokio::net::UnixStream, msg: ControlToAgent) -> ProxyReply {
    codec::write_frame(stream, &msg).await.unwrap();
    tokio::time::timeout(Duration::from_secs(200), codec::read_frame(stream))
        .await
        .expect("reponse du proxy")
        .unwrap()
        .expect("trame")
}

#[test]
#[ignore = "Pebble requis (CIPHERA_TEST_ACME_DIRECTORY, CIPHERA_TEST_ACME_CA_FILE) : job CI acme-pebble"]
fn issues_cert_via_pebble_http01() {
    issue_and_redirect("app.ciphera-pebble.org");
}

/// Domaine genere `<projet>.<ip>.sslip.io` : meme route `TlsMode::Acme` que
/// les domaines personnalises ; certificat emis, puis 308 vers HTTPS.
/// (Le serveur du test reste lie au port HTTP-01 : un processus par test,
/// voir le job CI `acme-pebble`.)
#[test]
#[ignore = "Pebble requis (CIPHERA_TEST_ACME_DIRECTORY, CIPHERA_TEST_ACME_CA_FILE) : job CI acme-pebble"]
fn generated_domain_https_via_pebble() {
    issue_and_redirect("shop.203.0.113.7.sslip.io");
}

fn issue_and_redirect(host: &str) {
    let directory = var("CIPHERA_TEST_ACME_DIRECTORY");
    let ca_file = var("CIPHERA_TEST_ACME_CA_FILE");
    let http_port: u16 = std::env::var("CIPHERA_TEST_ACME_HTTP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5002);

    // Dossier d'etat cree par le proxy (0700), comme StateDirectoryMode=0700 :
    // celui de `tempdir()` suit le umask (0755 sur les runners) et serait
    // refuse.
    let state_root = tempfile::tempdir().unwrap();
    let state_dir = state_root.path().join("state");
    let sealer = Arc::new(acme::load_or_create_key(&state_dir).unwrap());
    let certs = Arc::new(CertStore::open(&state_dir, sealer.clone()).unwrap());
    let tls = TlsAssets {
        certs: certs.clone(),
        challenges: Arc::new(Challenges::new()),
    };
    let issuer = Arc::new(AcmeIssuer::new(
        AcmeConfig {
            directory,
            ca_file: Some(ca_file.into()),
            contact: None,
        },
        state_dir.clone(),
        sealer.clone(),
        certs.clone(),
        tls.challenges.clone(),
    ));

    let http: SocketAddr = format!("127.0.0.1:{http_port}").parse().unwrap();
    let cfg = ProxyConfig {
        http: vec![http.to_string()],
        https: vec![format!("127.0.0.1:{}", free_port())],
        panel_host: None,
        panel_port: 8443,
        threads: 2,
        limits: ConnLimits::default(),
        io: Default::default(),
    };
    let socket_dir = tempfile::tempdir().unwrap();
    let socket = socket_dir.path().join("proxy.sock");
    let listener = control::bind(&socket).unwrap();
    let table = Arc::new(RouteTable::new());
    let server = ciphera_proxy::build_server(
        &cfg,
        table,
        &tls,
        Some(listener),
        Some(issuer),
        control::ControlAccess::new([rustix::process::getuid().as_raw()]),
    )
    .unwrap();
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

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let mut stream = tokio::net::UnixStream::connect(&socket).await.unwrap();
        // Sans route ACME pour l'hote : refus, rien n'est demande a Pebble.
        let reply = ask(
            &mut stream,
            ControlToAgent::IssueCertificate { host: host.into() },
        )
        .await;
        assert!(
            matches!(
                reply,
                ProxyReply::Certificate(CertificateOutcome::Failed { .. })
            ),
            "{reply:?}"
        );

        let route = Route {
            host: host.into(),
            path_prefix: "/".into(),
            upstreams: Vec::new(),
            redirect_to: None,
            tls: TlsMode::Acme,
            service: None,
            panel_confirmed: false,
        };
        let reply = ask(&mut stream, ControlToAgent::ApplyRoutes(vec![route])).await;
        assert_eq!(reply, ProxyReply::Applied { routes: 1 });

        let reply = ask(
            &mut stream,
            ControlToAgent::IssueCertificate { host: host.into() },
        )
        .await;
        let ProxyReply::Certificate(CertificateOutcome::Issued { not_after, issuer }) = reply
        else {
            panic!("certificat non emis : {reply:?}");
        };
        assert!(not_after > now(), "{not_after}");
        assert!(issuer.to_lowercase().contains("pebble"), "{issuer}");
    });

    // Installe pour SNI, et chiffre au repos (aucune cle en clair).
    assert!(certs.has(host));
    let sealed = std::fs::read(state_dir.join("certs").join(format!("{host}.age"))).unwrap();
    assert!(!String::from_utf8_lossy(&sealed).contains("PRIVATE KEY"));
    let account_files: Vec<_> = std::fs::read_dir(state_dir.join("acme"))
        .unwrap()
        .map(|e| std::fs::read(e.unwrap().path()).unwrap())
        .collect();
    assert_eq!(account_files.len(), 1, "un compte ACME par annuaire");
    assert!(!String::from_utf8_lossy(&account_files[0]).contains("key_pkcs8"));
    // Un redemarrage relit le certificat avec la meme identite.
    let reloaded = CertStore::open(&state_dir, sealer).unwrap();
    assert!(reloaded.has(host));

    // Port 80 : 308 vers HTTPS maintenant que le certificat existe.
    use std::io::{Read, Write};
    let mut s = TcpStream::connect(http).unwrap();
    s.write_all(
        format!("GET /p?q=1 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 308"), "{out}");
    assert!(
        out.to_lowercase()
            .contains(&format!("location: https://{host}/p?q=1")),
        "{out}"
    );
}
