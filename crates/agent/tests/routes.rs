//! Table des routes calculee par l'agent : la route du panneau (sans
//! service ni redirection) ne garde que l'ecoute locale du panneau fixee
//! dans la configuration de l'agent, jamais une autre adresse envoyee par le
//! plan de controle.

mod common;

use std::net::SocketAddr;

use ciphera_agent::routes;
use common::FakeDocker;
use proto::{Route, TlsMode};

fn panel_route(upstreams: &[&str]) -> Route {
    Route {
        host: "panel.example.com".into(),
        path_prefix: "/".into(),
        upstreams: upstreams.iter().map(|u| u.parse().unwrap()).collect(),
        redirect_to: None,
        tls: TlsMode::Acme,
        service: None,
    }
}

#[tokio::test]
async fn panel_route_keeps_only_the_configured_local_upstream() {
    let docker = FakeDocker::default();
    let panel: SocketAddr = "127.0.0.1:8444".parse().unwrap();
    let desired = vec![panel_route(&["127.0.0.1:8444"])];
    let table = routes::render(&desired, &[], &docker, Some(panel))
        .await
        .unwrap();
    assert_eq!(table[0].upstreams, vec![panel]);

    // Toute autre adresse (base de donnees publiee en local, IP du reseau)
    // est ignoree : la route reste sans amont (503).
    let desired = vec![panel_route(&["127.0.0.1:5432", "10.0.0.5:8444"])];
    let table = routes::render(&desired, &[], &docker, Some(panel))
        .await
        .unwrap();
    assert!(table[0].upstreams.is_empty());

    // Sans ecoute du panneau configuree : aucun amont.
    let desired = vec![panel_route(&["127.0.0.1:8444"])];
    let table = routes::render(&desired, &[], &docker, None).await.unwrap();
    assert!(table[0].upstreams.is_empty());
}

#[tokio::test]
async fn service_routes_never_take_upstreams_from_the_control_plane() {
    let docker = FakeDocker::default();
    let panel: SocketAddr = "127.0.0.1:8444".parse().unwrap();
    let mut route = panel_route(&["127.0.0.1:8444"]);
    route.service = Some(shared::ids::ServiceId::new());
    let table = routes::render(&[route], &[], &docker, Some(panel))
        .await
        .unwrap();
    assert!(table[0].upstreams.is_empty());
}
