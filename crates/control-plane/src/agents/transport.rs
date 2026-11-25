//! Ecoute QUIC des agents. Un agent n'est accepte que si :
//! 1. son certificat client chaine vers la CA de l'instance (seule racine ;
//!    certificat client obligatoire, verifie par rustls) ;
//! 2. ce certificat porte un SAN `urn:ciphera:agent:<id>` ;
//! 3. son empreinte correspond a une ligne `servers` non revoquee de cet id ;
//! 4. son `Hello` annonce la meme version du protocole et le meme id ;
//! 5. l'autorisation (3) est relue juste apres l'inscription dans le
//!    registre : une revocation qui tombe entre le premier controle et
//!    l'inscription est vue ici (voir `AgentRegistry::disconnect`).
//!
//! Sinon la connexion est fermee avec un code de `proto::error_code`.
//!
//! Livraison : `State` est un instantane complet de l'agent, renvoye a
//! chaque connexion (idempotent) ; `DeployProgress` et `LogLine` sont livres
//! au moins une fois (doublon possible apres une coupure) : les abonnes du
//! registre doivent les traiter de facon idempotente. `Metrics` : derniere
//! mesure seulement (une apres `State` a chaque connexion, puis une par
//! mesure) ; une mesure peut manquer ou, rarement, etre repetee.

use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use proto::{ALPN, AgentToControl, ControlToAgent, PROTO_VERSION, codec, error_code};
use quinn::crypto::rustls::QuicServerConfig;
use quinn::{Connection, Endpoint, Incoming, VarInt};
use rustls::RootCertStore;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use shared::ids::ServerId;
use sqlx::PgPool;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

use super::certs;
use super::registry::AgentRegistry;
use super::servers;

/// Certificat CA de l'instance, relatif au dossier de configuration.
pub const CA_CRT: &str = "ca/ca.crt";
/// Certificat serveur du lien QUIC (emis par la CA de l'instance).
pub const LINK_CRT: &str = "tls/link.crt";
pub const LINK_KEY: &str = "tls/link.key";

/// Delai pour ouvrir le flux et recevoir le `Hello` de l'agent.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Materiel TLS du lien (PEM).
pub struct LinkTls {
    pub ca_cert_pem: String,
    pub cert_pem: String,
    pub key_pem: Zeroizing<String>,
}

impl fmt::Debug for LinkTls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkTls").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct TlsError(&'static str);

impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TLS du lien agents : {}", self.0)
    }
}

impl std::error::Error for TlsError {}

impl LinkTls {
    /// Lit `ca/ca.crt`, `tls/link.crt` et `tls/link.key` (ecrits par `ciphera init`).
    pub fn load(config_dir: &Path) -> std::io::Result<Self> {
        let read = |rel: &str| std::fs::read_to_string(config_dir.join(rel));
        Ok(Self {
            ca_cert_pem: read(CA_CRT)?,
            cert_pem: read(LINK_CRT)?,
            key_pem: Zeroizing::new(read(LINK_KEY)?),
        })
    }
}

/// Configuration serveur : TLS 1.3, ALPN `ciphera/1`, certificat client
/// obligatoire et verifie contre la seule CA de l'instance.
pub fn server_config(tls: &LinkTls) -> Result<quinn::ServerConfig, TlsError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let ca = CertificateDer::from_pem_slice(tls.ca_cert_pem.as_bytes())
        .map_err(|_| TlsError("certificat de la CA illisible"))?;
    let mut roots = RootCertStore::empty();
    roots
        .add(ca)
        .map_err(|_| TlsError("certificat de la CA refuse"))?;
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|_| TlsError("verificateur des certificats clients"))?;
    let cert = CertificateDer::from_pem_slice(tls.cert_pem.as_bytes())
        .map_err(|_| TlsError("certificat du lien illisible"))?;
    let key = PrivateKeyDer::from_pem_slice(tls.key_pem.as_bytes())
        .map_err(|_| TlsError("cle du lien illisible"))?;
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| TlsError("TLS 1.3 indisponible"))?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![cert], key)
        .map_err(|_| TlsError("certificat ou cle du lien refuses"))?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    let quic = QuicServerConfig::try_from(config).map_err(|_| TlsError("suite TLS 1.3 absente"))?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(quic));
    server.transport_config(Arc::new(transport_config()));
    Ok(server)
}

/// Limites QUIC du lien : un seul flux bidirectionnel ouvert par l'agent,
/// aucun flux unidirectionnel, fenetres de reception bornees (une trame
/// maximale tient dans la fenetre d'un flux), deconnexion apres 30 s sans
/// trafic (l'agent envoie un keep-alive toutes les 10 s).
pub fn transport_config() -> quinn::TransportConfig {
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(VarInt::from_u32(1))
        .max_concurrent_uni_streams(VarInt::from_u32(0))
        .stream_receive_window(VarInt::from_u32(STREAM_RECEIVE_WINDOW))
        .receive_window(VarInt::from_u32(RECEIVE_WINDOW))
        .max_idle_timeout(Some(VarInt::from_u32(IDLE_TIMEOUT_MS).into()));
    transport
}

/// Fenetre d'un flux : une trame maximale (prefixe compris) et un peu de marge.
const STREAM_RECEIVE_WINDOW: u32 = (codec::MAX_FRAME_LEN + 64 * 1024) as u32;
/// Fenetre de la connexion entiere.
const RECEIVE_WINDOW: u32 = 8 * 1024 * 1024;
const IDLE_TIMEOUT_MS: u32 = 30_000;

/// Connexions simultanees (poignees de main comprises) ; au-dela, refus.
pub const MAX_CONNECTIONS: usize = 64;

/// Socket UDP d'ecoute du lien.
pub fn bind(addr: SocketAddr, config: quinn::ServerConfig) -> std::io::Result<Endpoint> {
    Endpoint::server(config, addr)
}

/// Accepte les agents jusqu'a la fermeture de `endpoint`. Au-dela de
/// `MAX_CONNECTIONS` connexions en cours, les nouvelles sont refusees avant
/// la poignee de main.
pub async fn serve(endpoint: Endpoint, registry: AgentRegistry, db: PgPool) {
    serve_with_renewal(endpoint, registry, db, None).await;
}

/// Comme [`serve`] ; avec la CA de l'instance (`ca`), le certificat d'un
/// agent qui a passe les deux tiers de sa validite est renouvele a sa
/// connexion (voir `agents::certs`).
pub async fn serve_with_renewal(
    endpoint: Endpoint,
    registry: AgentRegistry,
    db: PgPool,
    ca: Option<Arc<pki::Ca>>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    while let Some(incoming) = endpoint.accept().await {
        let Ok(permit) = slots.clone().try_acquire_owned() else {
            tracing::warn!(
                "lien agents : {} refuse, {MAX_CONNECTIONS} connexions en cours",
                incoming.remote_address()
            );
            incoming.refuse();
            continue;
        };
        let registry = registry.clone();
        let db = db.clone();
        let ca = ca.clone();
        tokio::spawn(async move {
            handle(incoming, &registry, &db, ca.as_deref()).await;
            drop(permit);
        });
    }
}

fn close(conn: &Connection, code: u32, reason: &str) {
    conn.close(VarInt::from_u32(code), reason.as_bytes());
}

fn peer_leaf(conn: &Connection) -> Option<CertificateDer<'static>> {
    conn.peer_identity()?
        .downcast::<Vec<CertificateDer<'static>>>()
        .ok()?
        .into_iter()
        .next()
}

/// Identifiant et empreinte du certificat de l'agent s'il est enregistre et
/// non revoque ; sinon la connexion est fermee.
async fn authenticate(
    conn: &Connection,
    db: &PgPool,
) -> Option<(ServerId, String, CertificateDer<'static>)> {
    let remote = conn.remote_address();
    let Some((id, fingerprint, der)) = peer_leaf(conn).and_then(|der| {
        pki::agent_id_from_cert(&der).map(|id| (id, pki::fingerprint_sha256(&der), der))
    }) else {
        tracing::warn!("lien agents : certificat sans identifiant d'agent ({remote})");
        close(
            conn,
            error_code::UNAUTHORIZED,
            "certificat sans identifiant d'agent",
        );
        return None;
    };
    match servers::is_authorized(db, id, &fingerprint).await {
        Ok(true) => Some((id, fingerprint, der)),
        Ok(false) => {
            tracing::warn!(
                "lien agents : agent {id} refuse, certificat {fingerprint} inconnu ou revoque ({remote})"
            );
            close(conn, error_code::UNAUTHORIZED, "agent inconnu ou revoque");
            None
        }
        Err(_) => {
            tracing::error!("lien agents : base indisponible, agent {id} refuse");
            close(conn, error_code::INTERNAL, "erreur interne");
            None
        }
    }
}

async fn handle(incoming: Incoming, registry: &AgentRegistry, db: &PgPool, ca: Option<&pki::Ca>) {
    let remote = incoming.remote_address();
    let conn = match incoming.await {
        Ok(conn) => conn,
        Err(e) => {
            tracing::warn!("lien agents : poignee de main refusee ({remote}) : {e}");
            return;
        }
    };
    let Some((id, fingerprint, leaf)) = authenticate(&conn, db).await else {
        return;
    };

    let Ok(Ok((mut send, mut recv))) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, conn.accept_bi()).await
    else {
        close(&conn, error_code::PROTOCOL, "Hello attendu");
        return;
    };
    let hello = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        codec::read_frame::<_, AgentToControl>(&mut recv),
    )
    .await;
    match hello {
        Ok(Ok(Some(AgentToControl::Hello { version, .. }))) if version != PROTO_VERSION => {
            // Plan de controle et agent sont livres ensemble : une autre
            // version signale une mise a jour incomplete.
            tracing::error!(
                "lien agents : agent {id} en version {version} du protocole (plan de controle : {PROTO_VERSION}), refuse : mettre a jour Ciphera (binaires livres ensemble)"
            );
            registry.set_link_error(
                id,
                format!(
                    "Version du protocole incompatible : agent en version {version}, plan de contrôle en version {PROTO_VERSION}. Mettez à jour Ciphera sur ce serveur."
                ),
            );
            let reason = format!(
                "version {version} du protocole non prise en charge (attendue {PROTO_VERSION})"
            );
            close(&conn, error_code::VERSION_MISMATCH, &reason);
            return;
        }
        Ok(Ok(Some(AgentToControl::Hello { agent_id, .. }))) if agent_id != id => {
            tracing::warn!("lien agents : Hello de {agent_id} avec le certificat de {id}, refuse");
            close(
                &conn,
                error_code::UNAUTHORIZED,
                "identifiant different du certificat",
            );
            return;
        }
        Ok(Ok(Some(AgentToControl::Hello { .. }))) => {}
        _ => {
            close(&conn, error_code::PROTOCOL, "Hello attendu");
            return;
        }
    }
    if codec::write_frame(
        &mut send,
        &ControlToAgent::Hello {
            version: PROTO_VERSION,
        },
    )
    .await
    .is_err()
    {
        return;
    }

    let (generation, mut outbox) = registry.attach(id, conn.clone());
    // Relecture apres l'inscription : ferme la course controle -> inscription
    // avec une revocation (UPDATE puis `disconnect`) arrivee entre les deux.
    match servers::is_authorized(db, id, &fingerprint).await {
        Ok(true) => {}
        Ok(false) => {
            registry.detach(id, generation);
            tracing::warn!("lien agents : agent {id} revoque pendant la connexion, refuse");
            close(&conn, error_code::UNAUTHORIZED, "agent inconnu ou revoque");
            return;
        }
        Err(_) => {
            registry.detach(id, generation);
            tracing::error!("lien agents : base indisponible, agent {id} refuse");
            close(&conn, error_code::INTERNAL, "erreur interne");
            return;
        }
    }
    tracing::info!("lien agents : agent {id} connecte ({remote})");
    // Certificat renouvele presente pour la premiere fois : il remplace
    // l'ancien. Sinon, certificat a renouveler : le nouveau part en premier.
    match servers::promote_next_cert(db, id, &fingerprint).await {
        Ok(true) => tracing::info!("lien agents : agent {id}, certificat renouvele en service"),
        Ok(false) => {
            if let Some(ca) = ca {
                match certs::renew_agent(db, ca, id, &fingerprint, &leaf, certs::now_unix()).await {
                    Ok(Some(cert_pem)) => {
                        tracing::info!("lien agents : agent {id}, certificat renouvele envoye");
                        if let Err(e) =
                            registry.send(id, ControlToAgent::RenewedCertificate { cert_pem })
                        {
                            tracing::warn!(
                                "lien agents : certificat renouvele non envoye a {id} : {e}"
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(e) => tracing::error!(
                        "lien agents : renouvellement du certificat de {id} impossible : {e}"
                    ),
                }
            }
        }
        Err(e) => {
            tracing::error!("lien agents : certificat renouvele de {id} non enregistre : {e}")
        }
    }
    let writer = tokio::spawn(async move {
        while let Some(msg) = outbox.recv().await {
            if codec::write_frame(&mut send, &msg).await.is_err() {
                break;
            }
        }
    });
    loop {
        match codec::read_frame::<_, AgentToControl>(&mut recv).await {
            Ok(Some(AgentToControl::Hello { .. })) => {
                close(&conn, error_code::PROTOCOL, "Hello en double");
                break;
            }
            Ok(Some(msg)) => {
                // Accuses de suppression, journalises ici ; `DeploymentRemoved`
                // (pose `cleaned_at`, `retention::removed`) et `ProjectRemoved`
                // sont traites par le suivi des deploiements.
                match &msg {
                    AgentToControl::DeploymentRemoved { deployment_id } => {
                        tracing::info!(
                            "lien agents : {id} a supprime le deploiement {deployment_id}"
                        )
                    }
                    AgentToControl::ProjectRemoved { project_id } => {
                        tracing::info!("lien agents : {id} a supprime le projet {project_id}")
                    }
                    _ => {}
                }
                registry.publish(id, msg)
            }
            Ok(None) | Err(codec::Error::Io(_)) => break,
            Err(_) => {
                close(&conn, error_code::PROTOCOL, "message illisible");
                break;
            }
        }
    }
    writer.abort();
    registry.detach(id, generation);
    close(&conn, error_code::NONE, "fin");
    tracing::info!("lien agents : agent {id} deconnecte");
}
