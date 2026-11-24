//! Client du socket de pilotage du proxy : transmet les tables, se reconnecte
//! et renvoie la derniere table quand le proxy redemarre.

#![cfg(unix)]

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use ciphera_agent::proxy_client;
use proto::{ControlToAgent, ProxyReply, Route, TlsMode, codec};
use tokio::net::UnixListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

fn table(host: &str) -> Vec<Route> {
    vec![Route {
        host: host.into(),
        path_prefix: "/".into(),
        upstreams: vec!["172.18.0.5:8080".parse::<SocketAddr>().unwrap()],
        redirect_to: None,
        tls: TlsMode::Acme,
        service: None,
    }]
}

/// Faux proxy : chaque table recue part dans `seen` et recoit `Applied`.
/// Une connexion a la fois, dans la tache elle-meme : l'interrompre coupe
/// aussi la connexion (comme un proxy qui s'arrete).
fn fake_proxy(path: &Path, seen: mpsc::UnboundedSender<Vec<Route>>) -> JoinHandle<()> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path).unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            while let Ok(Some(msg)) = codec::read_frame::<_, ControlToAgent>(&mut stream).await {
                let ControlToAgent::ApplyRoutes(routes) = msg else {
                    panic!("seul ApplyRoutes est envoye au proxy");
                };
                let n = routes.len() as u32;
                seen.send(routes).unwrap();
                codec::write_frame(&mut stream, &ProxyReply::Applied { routes: n })
                    .await
                    .unwrap();
            }
        }
    })
}

async fn next(rx: &mut mpsc::UnboundedReceiver<Vec<Route>>) -> Vec<Route> {
    tokio::time::timeout(Duration::from_secs(15), rx.recv())
        .await
        .expect("table recue a temps")
        .expect("faux proxy en marche")
}

#[tokio::test]
async fn forwards_tables_and_reapplies_the_last_one_after_a_proxy_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proxy.sock");
    let proxy = proxy_client::spawn(path.clone());

    // Table recue avant que le proxy n'ecoute : transmise des la connexion.
    proxy.apply(table("a.example.com"));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let server = fake_proxy(&path, tx.clone());
    assert_eq!(next(&mut rx).await, table("a.example.com"));

    proxy.apply(table("b.example.com"));
    assert_eq!(next(&mut rx).await, table("b.example.com"));

    // Redemarrage du proxy : sa table est vide, l'agent renvoie la derniere.
    server.abort();
    let _ = server.await;
    let (tx2, mut rx2) = mpsc::unbounded_channel();
    let _server = fake_proxy(&path, tx2);
    assert_eq!(next(&mut rx2).await, table("b.example.com"));
    assert!(rx.try_recv().is_err(), "rien de plus sur l'ancien proxy");

    proxy.apply(table("c.example.com"));
    assert_eq!(next(&mut rx2).await, table("c.example.com"));
}

#[tokio::test]
async fn certificate_requests_use_their_own_connection_and_return_the_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proxy.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let msg = codec::read_frame::<_, ControlToAgent>(&mut stream)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            msg,
            ControlToAgent::IssueCertificate {
                host: "app.example.com".into()
            }
        );
        let reply = ProxyReply::Certificate(proto::CertificateOutcome::Issued {
            not_after: 1_900_000_000,
            issuer: "R11".into(),
        });
        codec::write_frame(&mut stream, &reply).await.unwrap();
    });
    let outcome = proxy_client::issue_certificate(&path, "app.example.com").await;
    assert_eq!(
        outcome,
        proto::CertificateOutcome::Issued {
            not_after: 1_900_000_000,
            issuer: "R11".into()
        }
    );
    server.await.unwrap();

    // Proxy absent : echec explicite, sans attente.
    let outcome = proxy_client::issue_certificate(&dir.path().join("absent.sock"), "a.b").await;
    assert!(matches!(outcome, proto::CertificateOutcome::Failed { .. }));
}

#[tokio::test]
async fn check_and_remove_orders_are_relayed_to_the_proxy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("proxy.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        for (expected, reply) in [
            (
                ControlToAgent::CheckCertificate {
                    host: "app.example.com".into(),
                },
                proto::CertificateOutcome::Missing,
            ),
            (
                ControlToAgent::RemoveCertificate {
                    host: "app.example.com".into(),
                },
                proto::CertificateOutcome::Removed,
            ),
        ] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let msg = codec::read_frame::<_, ControlToAgent>(&mut stream)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(msg, expected);
            codec::write_frame(&mut stream, &ProxyReply::Certificate(reply))
                .await
                .unwrap();
        }
    });
    let check = ControlToAgent::CheckCertificate {
        host: "app.example.com".into(),
    };
    assert_eq!(
        proxy_client::certificate_request(&path, check).await,
        proto::CertificateOutcome::Missing
    );
    let remove = ControlToAgent::RemoveCertificate {
        host: "app.example.com".into(),
    };
    assert_eq!(
        proxy_client::certificate_request(&path, remove).await,
        proto::CertificateOutcome::Removed
    );
    server.await.unwrap();

    // Un ordre qui n'est pas un ordre de certificat n'est pas relaye.
    let other = proxy_client::certificate_request(&path, ControlToAgent::RequestMetrics).await;
    assert!(matches!(other, proto::CertificateOutcome::Failed { .. }));
}
