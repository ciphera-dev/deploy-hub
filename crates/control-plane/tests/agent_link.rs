//! Lien QUIC + mTLS plan de controle <-> agent : certificat client
//! obligatoire et emis par la CA de l'instance, agent enregistre et non
//! revoque, `Hello` de meme version, `State` renvoye a chaque reconnexion.
//! PostgreSQL requis (voir `common::database`) ; QUIC en UDP local.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ciphera_agent::config::{AgentConfig, Identity};
use ciphera_agent::transport::{Agent, client_config, fixed_state};
use control_plane::agents::registry::{AgentRegistry, SendError};
use control_plane::agents::{servers, transport};
use pki::Ca;
use proto::{
    ALPN, AgentState, AgentToControl, ControlToAgent, DeploymentOutcome, ImageRef, NodeMetrics,
    PROTO_VERSION, RunningDeployment, Stage, StageStatus, codec, error_code,
};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{ConnectionError, Endpoint};
use shared::ids::{DeploymentId, ServerId, ServiceId};
use sqlx::PgPool;
use tokio::sync::{broadcast, mpsc, watch};

const WAIT: Duration = Duration::from_secs(10);

struct Link {
    addr: SocketAddr,
    registry: AgentRegistry,
    endpoint: Endpoint,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.endpoint.close(0u32.into(), b"fin du test");
    }
}

/// Lien du plan de controle sur 127.0.0.1 (port ephemere), certificat
/// serveur `localhost` emis par `ca`.
fn start(db: &PgPool, ca: &Ca) -> Link {
    let mut server = ca.issue_server_cert(&["localhost".into()]).unwrap();
    let tls = transport::LinkTls {
        ca_cert_pem: ca.cert_pem().to_string(),
        cert_pem: server.cert_pem.clone(),
        key_pem: server.key_pem.take().unwrap(),
    };
    let endpoint = transport::bind(
        "127.0.0.1:0".parse().unwrap(),
        transport::server_config(&tls).unwrap(),
    )
    .unwrap();
    let registry = AgentRegistry::new();
    tokio::spawn(transport::serve(
        endpoint.clone(),
        registry.clone(),
        db.clone(),
    ));
    Link {
        addr: endpoint.local_addr().unwrap(),
        registry,
        endpoint,
    }
}

/// Identite d'agent : cle generee cote agent, certificat signe par `issuer`,
/// confiance dans `trusted` (CA du plan de controle). Renvoie aussi le
/// certificat PEM (pour l'enregistrer en base).
fn identity(issuer: &Ca, trusted: &Ca, id: ServerId) -> (Identity, String) {
    let key = pki::AgentKey::generate().unwrap();
    let cert = issuer.issue_agent_cert(id, key.public_key_der()).unwrap();
    let identity = Identity::from_pem(
        cert.cert_pem.as_bytes(),
        key.key_pem().as_bytes(),
        trusted.cert_pem().as_bytes(),
    )
    .unwrap();
    (identity, cert.cert_pem)
}

fn agent_config(addr: SocketAddr) -> AgentConfig {
    AgentConfig {
        dir: std::env::temp_dir(),
        control_addr: addr,
        server_name: "localhost".into(),
        state_db: std::env::temp_dir().join("unused-state.db"),
        proxy_socket: std::env::temp_dir().join("unused-proxy.sock"),
        panel_upstream: SocketAddr::from(([127, 0, 0, 1], 8444)),
    }
}

/// Connexion brute + `Hello` ; renvoie la premiere reponse, ou la cause de
/// la fermeture de la connexion.
async fn hello(
    client: quinn::ClientConfig,
    addr: SocketAddr,
    hello: AgentToControl,
) -> Result<(quinn::Connection, Option<ControlToAgent>), ConnectionError> {
    let endpoint = ciphera_agent::transport::client_endpoint(addr).unwrap();
    let connecting = endpoint.connect_with(client, addr, "localhost").unwrap();
    let conn = tokio::time::timeout(WAIT, connecting)
        .await
        .expect("delai de connexion depasse")?;
    let attempt = async {
        let (mut send, mut recv) = conn.open_bi().await.ok()?;
        codec::write_frame(&mut send, &hello).await.ok()?;
        codec::read_frame::<_, ControlToAgent>(&mut recv).await.ok()
    };
    match tokio::time::timeout(WAIT, attempt)
        .await
        .expect("delai depasse")
    {
        Some(reply) => Ok((conn, reply)),
        None => Err(tokio::time::timeout(WAIT, conn.closed())
            .await
            .expect("fermeture attendue")),
    }
}

/// Refus a la poignee de main TLS (alerte TLS = erreur QUIC « crypto »
/// 0x100-0x1ff), et non une fermeture applicative du plan de controle.
fn assert_tls_rejected(
    result: Result<(quinn::Connection, Option<ControlToAgent>), ConnectionError>,
) {
    let code = match result {
        Err(ConnectionError::ConnectionClosed(close)) => u64::from(close.error_code),
        Err(ConnectionError::TransportError(e)) => u64::from(e.code),
        Err(other) => panic!("refus TLS attendu, obtenu : {other:?}"),
        Ok((_, reply)) => panic!("connexion acceptee (reponse : {reply:?})"),
    };
    assert!(
        (0x100..0x200).contains(&code),
        "code {code:#x} : pas une alerte TLS"
    );
}

/// Fermeture applicative par le plan de controle avec ce code.
fn assert_app_closed(
    result: Result<(quinn::Connection, Option<ControlToAgent>), ConnectionError>,
    code: u32,
) {
    match result {
        Err(ConnectionError::ApplicationClosed(close)) => {
            assert_eq!(close.error_code, quinn::VarInt::from_u32(code), "{close:?}")
        }
        Err(other) => panic!("fermeture applicative {code} attendue, obtenu : {other:?}"),
        Ok((_, reply)) => panic!("connexion acceptee (reponse : {reply:?})"),
    }
}

/// Client brut qui fait confiance a `ca`, avec ou sans certificat client.
fn raw_client(ca: &Ca, client_cert: Option<(&str, &str)>) -> quinn::ClientConfig {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_slice(ca.cert_pem().as_bytes()).unwrap())
        .unwrap();
    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots);
    let mut tls = match client_cert {
        None => builder.with_no_client_auth(),
        Some((cert, key)) => builder
            .with_client_auth_cert(
                vec![CertificateDer::from_pem_slice(cert.as_bytes()).unwrap()],
                PrivateKeyDer::from_pem_slice(key.as_bytes()).unwrap(),
            )
            .unwrap(),
    };
    tls.alpn_protocols = vec![ALPN.to_vec()];
    quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls).unwrap()))
}

async fn next_state(
    events: &mut broadcast::Receiver<(ServerId, AgentToControl)>,
    id: ServerId,
) -> AgentState {
    tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await.unwrap() {
                (from, AgentToControl::State(state)) if from == id => return state,
                _ => {}
            }
        }
    })
    .await
    .expect("State attendu")
}

fn running() -> AgentState {
    AgentState {
        deployments: vec![RunningDeployment {
            deployment_id: DeploymentId::new(),
            service_id: ServiceId::new(),
            image: ImageRef("ghcr.io/acme/web:1".into()),
            healthy: true,
            serving: true,
        }],
        in_progress: vec![DeploymentId::new()],
        outcomes: vec![DeploymentOutcome {
            deployment_id: DeploymentId::new(),
            stage: Stage::Switch,
            status: StageStatus::Succeeded,
        }],
    }
}

fn hello_of(id: ServerId) -> AgentToControl {
    AgentToControl::Hello {
        version: PROTO_VERSION,
        agent_id: id,
    }
}

#[tokio::test]
async fn agent_without_client_cert_rejected() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (_, cert_pem) = identity(&ca, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();

    // Client qui fait confiance a la CA mais ne presente aucun certificat.
    let result = hello(raw_client(&ca, None), link.addr, hello_of(id)).await;
    assert_tls_rejected(result);
    assert!(!link.registry.is_connected(id));
}

#[tokio::test]
async fn agent_with_cert_from_other_ca_rejected() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let other = Ca::generate("autre").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    // Meme enregistre en base (empreinte exacte), un certificat d'une autre
    // CA ne passe pas la verification de chaine.
    let (identity, cert_pem) = identity(&other, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();

    let result = hello(client_config(&identity).unwrap(), link.addr, hello_of(id)).await;
    assert_tls_rejected(result);
    assert!(!link.registry.is_connected(id));
}

#[tokio::test]
async fn server_cert_of_the_instance_ca_rejected_as_client_cert() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    // Certificat serveur (EKU ServerAuth, pas de SAN d'agent) de la bonne CA.
    let mut server = ca.issue_server_cert(&["agent.example".into()]).unwrap();
    let key = server.key_pem.take().unwrap();
    let client = raw_client(&ca, Some((&server.cert_pem, &key)));
    let result = hello(client, link.addr, hello_of(ServerId::new())).await;
    assert_tls_rejected(result);
}

#[tokio::test]
async fn agent_with_unknown_or_revoked_cert_rejected() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (identity, cert_pem) = identity(&ca, &ca, id);
    let client = client_config(&identity).unwrap();

    // Bonne CA mais aucune ligne `servers` : refuse avec UNAUTHORIZED.
    let unknown = hello(client.clone(), link.addr, hello_of(id)).await;
    assert_app_closed(unknown, error_code::UNAUTHORIZED);

    // Enregistre puis revoque : refuse.
    servers::register_local(&db, &cert_pem).await.unwrap();
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let revoked = hello(client.clone(), link.addr, hello_of(id)).await;
    assert_app_closed(revoked, error_code::UNAUTHORIZED);

    // Non revoque : accepte, Hello de meme version en reponse.
    sqlx::query("UPDATE servers SET revoked_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let (_conn, reply) = hello(client, link.addr, hello_of(id)).await.unwrap();
    assert_eq!(
        reply,
        Some(ControlToAgent::Hello {
            version: PROTO_VERSION
        })
    );
}

#[tokio::test]
async fn hello_with_another_agent_id_rejected() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (identity, cert_pem) = identity(&ca, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();
    let result = hello(
        client_config(&identity).unwrap(),
        link.addr,
        hello_of(ServerId::new()),
    )
    .await;
    assert_app_closed(result, error_code::UNAUTHORIZED);
    assert!(!link.registry.is_connected(id));
}

#[tokio::test]
async fn extra_streams_get_no_credit() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (identity, cert_pem) = identity(&ca, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();
    let endpoint = ciphera_agent::transport::client_endpoint(link.addr).unwrap();
    let conn = endpoint
        .connect_with(client_config(&identity).unwrap(), link.addr, "localhost")
        .unwrap()
        .await
        .unwrap();
    // Flux du protocole garde ouvert pendant tout le test.
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    codec::write_frame(&mut send, &hello_of(id)).await.unwrap();
    let reply = codec::read_frame::<_, ControlToAgent>(&mut recv)
        .await
        .unwrap();
    assert!(matches!(reply, Some(ControlToAgent::Hello { .. })));

    // Un seul flux bidirectionnel, aucun unidirectionnel : pas de credit.
    let short = Duration::from_millis(500);
    let uni = tokio::time::timeout(short, conn.open_uni()).await;
    assert!(
        uni.is_err(),
        "flux unidirectionnel accorde : {:?}",
        uni.map(|r| r.map(|_| ()))
    );
    assert!(
        tokio::time::timeout(short, conn.open_bi()).await.is_err(),
        "second flux bidirectionnel accorde"
    );
    assert!(link.registry.is_connected(id));
    assert!(conn.close_reason().is_none());
    drop((send, recv));
}

#[tokio::test]
async fn version_mismatch_closes_with_error() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (identity, cert_pem) = identity(&ca, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();

    let endpoint = ciphera_agent::transport::client_endpoint(link.addr).unwrap();
    let conn = endpoint
        .connect_with(client_config(&identity).unwrap(), link.addr, "localhost")
        .unwrap()
        .await
        .unwrap();
    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    codec::write_frame(
        &mut send,
        &AgentToControl::Hello {
            version: PROTO_VERSION + 1,
            agent_id: id,
        },
    )
    .await
    .unwrap();
    let reply = tokio::time::timeout(WAIT, codec::read_frame::<_, ControlToAgent>(&mut recv))
        .await
        .unwrap();
    assert!(!matches!(reply, Ok(Some(ControlToAgent::Hello { .. }))));
    match tokio::time::timeout(WAIT, conn.closed()).await.unwrap() {
        ConnectionError::ApplicationClosed(close) => {
            assert_eq!(
                close.error_code,
                quinn::VarInt::from_u32(error_code::VERSION_MISMATCH)
            );
            let reason = String::from_utf8_lossy(&close.reason);
            assert!(reason.contains("version"), "raison : {reason}");
        }
        other => panic!("fermeture inattendue : {other:?}"),
    }
    // Refus note comme etat du serveur (Vue d'ensemble), efface par une
    // connexion reussie.
    let status = link.registry.link_error(id).expect("etat du serveur");
    assert!(
        status.contains(&format!("agent en version {}", PROTO_VERSION + 1))
            && status.contains(&format!("plan de contrôle en version {PROTO_VERSION}")),
        "{status}"
    );
    assert!(!link.registry.is_connected(id));

    let (_conn, reply) = hello(
        client_config(&identity).unwrap(),
        link.addr,
        AgentToControl::Hello {
            version: PROTO_VERSION,
            agent_id: id,
        },
    )
    .await
    .unwrap();
    assert!(matches!(reply, Some(ControlToAgent::Hello { .. })));
    wait_until("connexion", || link.registry.is_connected(id)).await;
    assert_eq!(link.registry.link_error(id), None);
}

#[tokio::test]
async fn reconnect_after_drop_resends_state() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (identity, cert_pem) = identity(&ca, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();

    let state = running();
    let mut agent = Agent::new(
        &agent_config(link.addr),
        &identity,
        fixed_state(state.clone()),
    )
    .unwrap();
    agent.retry = Duration::from_millis(50);
    let mut events = link.registry.subscribe();
    let (inbox_tx, mut inbox) = mpsc::channel(8);
    let (_outbox_tx, outbox) = mpsc::channel(8);
    let running_agent = tokio::spawn(async move { agent.run(inbox_tx, outbox).await });

    assert_eq!(next_state(&mut events, id).await, state);
    assert!(link.registry.is_connected(id));

    // Ordre du plan de controle recu par l'agent.
    link.registry
        .send(id, ControlToAgent::RequestMetrics)
        .unwrap();
    let received = tokio::time::timeout(WAIT, inbox.recv()).await.unwrap();
    assert_eq!(received, Some(ControlToAgent::RequestMetrics));

    // Coupure : l'agent se reconnecte et renvoie son etat.
    link.registry.disconnect(id);
    assert_eq!(next_state(&mut events, id).await, state);
    assert!(link.registry.is_connected(id));

    running_agent.abort();
}

fn sample(n: u32) -> NodeMetrics {
    NodeMetrics {
        cpu_percent: n as f32,
        mem_used_bytes: 1,
        mem_total_bytes: 2,
        disk_used_bytes: 3,
        disk_total_bytes: 4,
        net_rx_bytes: 5,
        net_tx_bytes: 6,
        containers: Vec::new(),
    }
}

async fn wait_until(what: &str, cond: impl Fn() -> bool) {
    tokio::time::timeout(WAIT, async {
        while !cond() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what} : delai depasse"));
}

/// Metriques hors de la file des messages : connecte, chaque mesure part
/// aussitot ; apres une coupure pendant laquelle N mesures ont ete prises, la
/// reconnexion n'en livre qu'une, la derniere, juste apres `State`.
#[tokio::test]
async fn reconnect_after_missed_ticks_delivers_at_most_one_sample() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let (identity, cert_pem) = identity(&ca, &ca, id);
    servers::register_local(&db, &cert_pem).await.unwrap();

    let (metrics_tx, metrics_rx) = watch::channel(None);
    let mut agent = Agent::new(
        &agent_config(link.addr),
        &identity,
        fixed_state(AgentState::default()),
    )
    .unwrap()
    .with_metrics(metrics_rx);
    agent.retry = Duration::from_millis(50);
    let mut events = link.registry.subscribe();
    let (inbox_tx, _inbox) = mpsc::channel(8);
    let (_outbox_tx, outbox) = mpsc::channel(8);
    let running_agent = tokio::spawn(async move { agent.run(inbox_tx, outbox).await });
    assert_eq!(next_state(&mut events, id).await, AgentState::default());

    // Connecte : la mesure part aussitot.
    metrics_tx.send_replace(Some(sample(1)));
    let first = tokio::time::timeout(WAIT, async {
        loop {
            if let (from, AgentToControl::Metrics(m)) = events.recv().await.unwrap()
                && from == id
            {
                return m;
            }
        }
    })
    .await
    .expect("Metrics attendu");
    assert_eq!(first, sample(1));

    // Coupure longue : agent refuse a chaque essai, dix periodes manquees.
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    link.registry.disconnect(id);
    wait_until("deconnexion", || !link.registry.is_connected(id)).await;
    for n in 2..=11 {
        metrics_tx.send_replace(Some(sample(n)));
    }
    sqlx::query("UPDATE servers SET revoked_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();

    // Reconnexion : State, puis une seule mesure (la derniere).
    assert_eq!(next_state(&mut events, id).await, AgentState::default());
    let mut received = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(1500), async {
        loop {
            if let (from, AgentToControl::Metrics(m)) = events.recv().await.unwrap()
                && from == id
            {
                received.push(m);
            }
        }
    })
    .await;
    assert_eq!(received, vec![sample(11)]);

    running_agent.abort();
}

/// Revocation (UPDATE puis `disconnect`) tombee entre le controle de
/// l'agent et son inscription dans le registre : la relecture faite apres
/// l'inscription la voit, l'agent n'est jamais laisse connecte.
#[tokio::test]
async fn revocation_between_check_and_attach_is_caught() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let link = start(&db, &ca);
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &cert.cert_pem).await.unwrap();
    let key_pem = key.key_pem();
    let client = raw_client(&ca, Some((cert.cert_pem.as_str(), key_pem)));

    let endpoint = ciphera_agent::transport::client_endpoint(link.addr).unwrap();
    let conn = tokio::time::timeout(
        WAIT,
        endpoint
            .connect_with(client, link.addr, "localhost")
            .unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    // Premier controle (a la fin de la poignee de main) passe.
    tokio::time::sleep(Duration::from_millis(500)).await;
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    link.registry.disconnect(id); // rien a fermer : pas encore inscrit

    let (mut send, mut recv) = conn.open_bi().await.unwrap();
    codec::write_frame(
        &mut send,
        &AgentToControl::Hello {
            version: PROTO_VERSION,
            agent_id: id,
        },
    )
    .await
    .unwrap();
    let _ = tokio::time::timeout(WAIT, codec::read_frame::<_, ControlToAgent>(&mut recv)).await;
    match tokio::time::timeout(WAIT, conn.closed()).await {
        Ok(ConnectionError::ApplicationClosed(close)) => assert_eq!(
            close.error_code,
            quinn::VarInt::from_u32(error_code::UNAUTHORIZED),
            "{close:?}"
        ),
        Ok(other) => panic!("fermeture UNAUTHORIZED attendue : {other:?}"),
        Err(_) => panic!("agent revoque toujours connecte"),
    }
    assert!(!link.registry.is_connected(id));
}

#[tokio::test]
async fn send_to_unconnected_server_fails() {
    let registry = AgentRegistry::new();
    assert!(matches!(
        registry.send(ServerId::new(), ControlToAgent::RequestMetrics),
        Err(SendError::NotConnected)
    ));
}

#[tokio::test]
async fn agent_enrolled_by_init_connects() {
    let (db, _guard) = common::database().await;
    let tmp = common::TempDir::new();
    let agent_dir = tmp.0.join("agent");
    control_plane::init::run(&control_plane::init::InitOptions {
        config_dir: tmp.0.clone(),
        public_ip: "203.0.113.7".parse().unwrap(),
        panel_port: 8443,
        owner: None,
        agent_dir: agent_dir.clone(),
        agent_owner: None,
        postgres_owner: None,
    })
    .unwrap();

    // Demarrage du serveur : enregistrement du serveur local (idempotent).
    let id = servers::register_local_from_dir(&db, &tmp.0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        servers::register_local_from_dir(&db, &tmp.0).await.unwrap(),
        Some(id)
    );
    let kind: String = sqlx::query_scalar("SELECT kind FROM servers WHERE id = $1")
        .bind(id)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(kind, "local");

    let tls = transport::LinkTls::load(&tmp.0).unwrap();
    let endpoint = transport::bind(
        "127.0.0.1:0".parse().unwrap(),
        transport::server_config(&tls).unwrap(),
    )
    .unwrap();
    let registry = AgentRegistry::new();
    tokio::spawn(transport::serve(
        endpoint.clone(),
        registry.clone(),
        db.clone(),
    ));
    let link = Link {
        addr: endpoint.local_addr().unwrap(),
        registry,
        endpoint,
    };

    let identity = Identity::load(&agent_dir).unwrap();
    assert_eq!(identity.agent_id, id);
    let agent = Agent::new(
        &agent_config(link.addr),
        &identity,
        fixed_state(AgentState::default()),
    )
    .unwrap();
    let mut events = link.registry.subscribe();
    let (inbox_tx, _inbox) = mpsc::channel(8);
    let (_outbox_tx, outbox) = mpsc::channel(8);
    let running_agent = tokio::spawn(async move { agent.run(inbox_tx, outbox).await });
    assert_eq!(next_state(&mut events, id).await, AgentState::default());
    running_agent.abort();
}

/// Simulation du temps : certificat d'agent emis il y a 300 jours (65 jours
/// restants). A la connexion, le plan de controle en envoie un nouveau
/// (meme cle) ; l'agent le verifie, remplace `agent.crt`, se reconnecte
/// avec lui, et l'empreinte en base est remplacee. L'ancien certificat
/// n'est plus accepte ensuite.
#[tokio::test]
async fn expiring_agent_cert_is_renewed_over_the_link() {
    let (db, _guard) = common::database().await;
    let ca = Arc::new(Ca::generate("instance").unwrap());
    let mut server = ca.issue_server_cert(&["localhost".into()]).unwrap();
    let tls = transport::LinkTls {
        ca_cert_pem: ca.cert_pem().to_string(),
        cert_pem: server.cert_pem.clone(),
        key_pem: server.key_pem.take().unwrap(),
    };
    let endpoint = transport::bind(
        "127.0.0.1:0".parse().unwrap(),
        transport::server_config(&tls).unwrap(),
    )
    .unwrap();
    let registry = AgentRegistry::new();
    tokio::spawn(transport::serve_with_renewal(
        endpoint.clone(),
        registry.clone(),
        db.clone(),
        Some(ca.clone()),
    ));
    let link = Link {
        addr: endpoint.local_addr().unwrap(),
        registry,
        endpoint,
    };

    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let now = control_plane::agents::certs::now_unix();
    let day = 86_400;
    let old = ca
        .issue_agent_cert_between(id, key.public_key_der(), now - 300 * day, now + 65 * day)
        .unwrap();
    servers::register_local(&db, &old.cert_pem).await.unwrap();
    let tmp = common::TempDir::new();
    std::fs::write(tmp.0.join("agent.crt"), &old.cert_pem).unwrap();
    let identity = Identity::from_pem(
        old.cert_pem.as_bytes(),
        key.key_pem().as_bytes(),
        ca.cert_pem().as_bytes(),
    )
    .unwrap();
    let mut cfg = agent_config(link.addr);
    cfg.dir = tmp.0.clone();
    let mut agent = Agent::new(&cfg, &identity, fixed_state(running())).unwrap();
    agent.retry = Duration::from_millis(50);
    let mut events = link.registry.subscribe();
    let (inbox_tx, _inbox) = mpsc::channel(8);
    let (_outbox_tx, outbox) = mpsc::channel(8);
    let running_agent = tokio::spawn(async move { agent.run(inbox_tx, outbox).await });

    // Premiere connexion (ancien certificat), puis reconnexion (nouveau).
    next_state(&mut events, id).await;
    next_state(&mut events, id).await;
    let current: (String, Option<String>) =
        sqlx::query_as("SELECT cert_fingerprint, next_cert_fingerprint FROM servers WHERE id = $1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    let written = std::fs::read_to_string(tmp.0.join("agent.crt")).unwrap();
    assert_ne!(written, old.cert_pem, "agent.crt remplace");
    let new_der = pki::der_from_pem(&written).unwrap();
    assert_eq!(pki::agent_id_from_cert(&new_der), Some(id));
    assert_eq!(
        pki::public_key_der(&new_der).unwrap(),
        key.public_key_der(),
        "meme cle"
    );
    assert_eq!(pki::renewal_due(&new_der, now), Some(false));
    assert_eq!(
        current.0,
        pki::fingerprint_sha256(&new_der),
        "empreinte remplacee"
    );
    assert_eq!(current.1, None);
    assert!(link.registry.is_connected(id));

    // L'ancien certificat n'ouvre plus le lien.
    let old_identity = Identity::from_pem(
        old.cert_pem.as_bytes(),
        key.key_pem().as_bytes(),
        ca.cert_pem().as_bytes(),
    )
    .unwrap();
    running_agent.abort();
    let result = hello(
        client_config(&old_identity).unwrap(),
        link.addr,
        hello_of(id),
    )
    .await;
    assert_app_closed(result, error_code::UNAUTHORIZED);
}

/// Certificat renouvele refuse par l'agent (autre cle) : rien n'est ecrit
/// et l'ancien reste accepte.
#[tokio::test]
async fn renewed_cert_with_another_key_is_refused_by_the_agent() {
    let ca = Ca::generate("instance").unwrap();
    let id = ServerId::new();
    let (identity, _) = identity(&ca, &ca, id);
    let other = pki::AgentKey::generate().unwrap();
    let foreign = ca.issue_agent_cert(id, other.public_key_der()).unwrap();
    let now = control_plane::agents::certs::now_unix();
    assert_eq!(
        identity.check_renewed(&foreign.cert_pem, now).unwrap_err(),
        "cle publique differente"
    );
    let other_id = ca
        .issue_agent_cert(ServerId::new(), other.public_key_der())
        .unwrap();
    assert!(identity.check_renewed(&other_id.cert_pem, now).is_err());
}
