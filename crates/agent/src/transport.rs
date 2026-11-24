//! Lien QUIC + mTLS vers le plan de controle. Le certificat du plan de
//! controle est verifie contre la seule CA de l'instance (aucune racine du
//! systeme, aucune option pour desactiver la verification). Apres chaque
//! (re)connexion : `Hello`, puis l'etat courant (`State`).
//!
//! Garanties de livraison : `State` est un instantane complet (idempotent,
//! renvoye a chaque connexion) ; `DeployProgress` et `LogLine` passent par la
//! file des messages, livres au moins une fois, et peuvent arriver en double
//! apres une coupure : les consommateurs du plan de controle doivent etre
//! idempotents. `Metrics` ne passe pas par cette file : derniere valeur
//! seulement (voir `metrics`), envoyee une fois apres `State` a chaque
//! connexion puis a chaque nouvelle mesure ; rien n'est garde hors connexion.

use std::fmt;
use std::future::Future;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use proto::error_code;
use proto::{ALPN, AgentState, AgentToControl, ControlToAgent, NodeMetrics, PROTO_VERSION, codec};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, ConnectionError, Endpoint, VarInt};
use rustls::RootCertStore;
use shared::ids::ServerId;
use tokio::sync::{mpsc, watch};

use crate::config::{AgentConfig, Identity};
use crate::metrics::Latest;

/// Delai maximal du `Hello` de reponse.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Premier delai avant de se reconnecter ; double a chaque echec.
pub const DEFAULT_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);
const KEEP_ALIVE: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT_MS: u32 = 30_000;
const STREAM_RECEIVE_WINDOW: u32 = (codec::MAX_FRAME_LEN + 64 * 1024) as u32;
const RECEIVE_WINDOW: u32 = 8 * 1024 * 1024;
/// Intervalle de verification du certificat de l'agent : passe les deux
/// tiers de sa validite, l'agent se reconnecte et le plan de controle lui
/// en envoie un nouveau (`ControlToAgent::RenewedCertificate`).
pub const RENEW_CHECK: Duration = Duration::from_secs(12 * 3600);

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Etat courant des deploiements, calcule a chaque connexion (voir
/// `reconcile::state_fn`).
pub type StateFn =
    Arc<dyn Fn() -> std::pin::Pin<Box<dyn Future<Output = AgentState> + Send>> + Send + Sync>;

/// Fournisseur d'un etat fixe (tests, agent sans etat local).
pub fn fixed_state(state: AgentState) -> StateFn {
    Arc::new(move || {
        let state = state.clone();
        Box::pin(async move { state })
    })
}

#[derive(Debug)]
pub enum LinkError {
    Tls(&'static str),
    Io(std::io::Error),
    Connect(String),
    /// Connexion fermee par le plan de controle (code, raison).
    Closed(u32, String),
    /// Connexion perdue sans fermeture applicative.
    Lost(String),
    /// Le plan de controle parle une autre version du protocole.
    VersionMismatch(u16),
    Protocol(&'static str),
    /// Reconnexion pour le certificat de l'agent (renouvele, ou a
    /// renouveler).
    Renewal(&'static str),
}

impl LinkError {
    /// Erreur qu'une reconnexion ne corrigera pas.
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Self::Tls(_) | Self::VersionMismatch(_) | Self::Closed(error_code::VERSION_MISMATCH, _)
        )
    }
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tls(m) => write!(f, "configuration TLS : {m}"),
            Self::Io(e) => write!(f, "socket UDP : {e}"),
            Self::Connect(m) => write!(f, "connexion impossible : {m}"),
            Self::Closed(code, reason) => {
                write!(f, "fermee par le plan de controle (code {code}) : {reason}")
            }
            Self::Lost(m) => write!(f, "connexion perdue : {m}"),
            Self::VersionMismatch(v) => write!(
                f,
                "le plan de controle parle la version {v} du protocole (agent : {PROTO_VERSION})"
            ),
            Self::Protocol(m) => write!(f, "protocole : {m}"),
            Self::Renewal(m) => write!(f, "certificat de l'agent : {m}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// Configuration client : CA de l'instance seule, certificat de l'agent,
/// TLS 1.3, ALPN `ciphera/1`.
pub fn client_config(identity: &Identity) -> Result<quinn::ClientConfig, LinkError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots
        .add(identity.ca.clone())
        .map_err(|_| LinkError::Tls("certificat de la CA refuse"))?;
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| LinkError::Tls("TLS 1.3 indisponible"))?
        .with_root_certificates(roots)
        .with_client_auth_cert(vec![identity.cert.clone()], identity.key.clone_key())
        .map_err(|_| LinkError::Tls("certificat ou cle de l'agent refuses"))?;
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let quic =
        QuicClientConfig::try_from(tls).map_err(|_| LinkError::Tls("suite TLS 1.3 absente"))?;
    // Le plan de controle n'ouvre aucun flux vers l'agent ; fenetres bornees
    // (une trame maximale tient dans celle d'un flux) ; keep-alive bien en
    // dessous du delai d'inactivite des deux cotes (30 s).
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(VarInt::from_u32(0))
        .max_concurrent_uni_streams(VarInt::from_u32(0))
        .stream_receive_window(VarInt::from_u32(STREAM_RECEIVE_WINDOW))
        .receive_window(VarInt::from_u32(RECEIVE_WINDOW))
        .max_idle_timeout(Some(VarInt::from_u32(IDLE_TIMEOUT_MS).into()))
        .keep_alive_interval(Some(KEEP_ALIVE));
    let mut config = quinn::ClientConfig::new(Arc::new(quic));
    config.transport_config(Arc::new(transport));
    Ok(config)
}

/// Socket UDP locale (port ephemere) de la meme famille que `remote`.
pub fn client_endpoint(remote: SocketAddr) -> std::io::Result<Endpoint> {
    let local: SocketAddr = if remote.is_ipv4() {
        (Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (Ipv6Addr::UNSPECIFIED, 0).into()
    };
    Endpoint::client(local)
}

/// Agent connecte (ou cherchant a se connecter) au plan de controle.
pub struct Agent {
    pub endpoint: Endpoint,
    /// Reconstruite apres un renouvellement du certificat.
    client: Mutex<quinn::ClientConfig>,
    identity: Mutex<Identity>,
    /// Dossier de l'identite (`agent.crt` renouvele y est ecrit).
    dir: PathBuf,
    control_addr: SocketAddr,
    server_name: String,
    agent_id: ServerId,
    state: StateFn,
    /// Derniere mesure des metriques (voir `metrics`), si fournie.
    metrics: Option<Latest>,
    /// Premier delai de reconnexion.
    pub retry: Duration,
}

impl Agent {
    pub fn new(cfg: &AgentConfig, identity: &Identity, state: StateFn) -> Result<Self, LinkError> {
        Ok(Self {
            endpoint: client_endpoint(cfg.control_addr).map_err(LinkError::Io)?,
            client: Mutex::new(client_config(identity)?),
            identity: Mutex::new(identity.duplicate()),
            dir: cfg.dir.clone(),
            control_addr: cfg.control_addr,
            server_name: cfg.server_name.clone(),
            agent_id: identity.agent_id,
            state,
            metrics: None,
            retry: DEFAULT_RETRY,
        })
    }

    /// Envoie aussi les metriques : la derniere mesure apres `State` a chaque
    /// connexion, puis chaque nouvelle mesure tant que le lien tient.
    pub fn with_metrics(mut self, latest: Latest) -> Self {
        self.metrics = Some(latest);
        self
    }

    /// Se connecte et se reconnecte jusqu'a une erreur fatale, ou jusqu'a la
    /// fermeture de `inbox` (arret demande, `Ok`). Les messages de `outbox`
    /// attendent la connexion suivante en cas de coupure ; un message dont
    /// l'ecriture a echoue est renvoye en premier a la connexion suivante
    /// (livraison au moins une fois).
    pub async fn run(
        &self,
        inbox: mpsc::Sender<ControlToAgent>,
        mut outbox: mpsc::Receiver<AgentToControl>,
    ) -> Result<(), LinkError> {
        let mut delay = self.retry;
        let mut pending = None;
        loop {
            let mut established = false;
            match self
                .session(&inbox, &mut outbox, &mut pending, &mut established)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) if e.is_fatal() => return Err(e),
                Err(e) => tracing::warn!("lien avec le plan de controle : {e}"),
            }
            if established {
                delay = self.retry;
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(MAX_RETRY.max(self.retry));
        }
    }

    async fn session(
        &self,
        inbox: &mpsc::Sender<ControlToAgent>,
        outbox: &mut mpsc::Receiver<AgentToControl>,
        pending: &mut Option<AgentToControl>,
        established: &mut bool,
    ) -> Result<(), LinkError> {
        let client = self
            .client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let conn = self
            .endpoint
            .connect_with(client, self.control_addr, &self.server_name)
            .map_err(|e| LinkError::Connect(e.to_string()))?
            .await
            .map_err(|e| connection_error(&e))?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| connection_error(&e))?;
        codec::write_frame(
            &mut send,
            &AgentToControl::Hello {
                version: PROTO_VERSION,
                agent_id: self.agent_id,
            },
        )
        .await
        .map_err(|_| closed(&conn))?;

        let reply = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            codec::read_frame::<_, ControlToAgent>(&mut recv),
        )
        .await;
        match reply {
            Ok(Ok(Some(ControlToAgent::Hello { version }))) if version == PROTO_VERSION => {}
            Ok(Ok(Some(ControlToAgent::Hello { version }))) => {
                let _ = codec::write_frame(
                    &mut send,
                    &AgentToControl::Error {
                        code: error_code::VERSION_MISMATCH,
                        message: format!("version {version} non prise en charge"),
                    },
                )
                .await;
                close(&conn, error_code::VERSION_MISMATCH, "version du protocole");
                return Err(LinkError::VersionMismatch(version));
            }
            Ok(Ok(Some(_))) => {
                close(&conn, error_code::PROTOCOL, "Hello attendu");
                return Err(LinkError::Protocol("Hello attendu"));
            }
            Ok(Ok(None) | Err(_)) => return Err(closed(&conn)),
            Err(_) => {
                close(&conn, error_code::PROTOCOL, "Hello attendu");
                return Err(LinkError::Protocol("pas de Hello du plan de controle"));
            }
        }
        *established = true;
        tracing::info!("connecte au plan de controle ({})", self.control_addr);

        let state = (self.state)().await;
        codec::write_frame(&mut send, &AgentToControl::State(state))
            .await
            .map_err(|_| closed(&conn))?;
        if let Some(msg) = pending.take()
            && codec::write_frame(&mut send, &msg).await.is_err()
        {
            *pending = Some(msg);
            return Err(closed(&conn));
        }
        // Derniere mesure, une seule fois, meme apres une longue coupure.
        // Une ecriture ratee ne la remet pas en attente : la prochaine
        // connexion enverra la plus recente.
        let mut metrics = self.metrics.clone();
        let latest = metrics
            .as_mut()
            .and_then(|rx| rx.borrow_and_update().clone());
        if let Some(m) = latest {
            codec::write_frame(&mut send, &AgentToControl::Metrics(m))
                .await
                .map_err(|_| closed(&conn))?;
        }

        // Lecture dans une tache a part : read_frame n'est pas annulable sans
        // perte, il ne doit pas etre mis en concurrence dans un select.
        let forward = inbox.clone();
        // Certificat renouvele : traite ici (fichier, configuration TLS).
        let (renewed_tx, mut renewed_rx) = mpsc::channel::<String>(1);
        let mut reader = tokio::spawn(async move {
            loop {
                match codec::read_frame::<_, ControlToAgent>(&mut recv).await {
                    Ok(Some(ControlToAgent::RenewedCertificate { cert_pem })) => {
                        let _ = renewed_tx.try_send(cert_pem);
                    }
                    Ok(Some(msg)) => {
                        if forward.send(msg).await.is_err() {
                            return true; // arret demande
                        }
                    }
                    Ok(None) | Err(_) => return false,
                }
            }
        });
        let check = tokio::time::sleep(RENEW_CHECK);
        tokio::pin!(check);
        let mut outbox_open = true;
        let result = loop {
            tokio::select! {
                Some(cert_pem) = renewed_rx.recv() => match self.install_certificate(&cert_pem) {
                    Ok(()) => {
                        tracing::info!("certificat de l'agent renouvele : reconnexion");
                        close(&conn, error_code::NONE, "certificat renouvele");
                        break Err(LinkError::Renewal("renouvele, reconnexion"));
                    }
                    Err(e) => tracing::error!("certificat renouvele refuse : {e}"),
                },
                () = &mut check => {
                    if self.renewal_due() {
                        // Le plan de controle renouvelle a la connexion. Un
                        // renouvellement reussi remplace le certificat : ce
                        // message ne revient que si le renouvellement echoue.
                        let left = self
                            .identity
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .days_left(now_unix());
                        tracing::warn!(
                            "certificat de l'agent a renouveler (expire dans {} jour(s)) : \
                             reconnexion pour le renouvellement ; s'il expire, \
                             `ciphera admin renew-certs` sur le serveur",
                            left.map_or_else(|| "?".to_string(), |d| d.to_string())
                        );
                        close(&conn, error_code::NONE, "certificat a renouveler");
                        break Err(LinkError::Renewal("a renouveler, reconnexion"));
                    }
                    check.as_mut().reset(tokio::time::Instant::now() + RENEW_CHECK);
                }
                stopped = &mut reader => {
                    break match stopped {
                        Ok(true) => {
                            close(&conn, error_code::NONE, "arret de l'agent");
                            Ok(())
                        }
                        _ => Err(closed(&conn)),
                    };
                }
                out = outbox.recv(), if outbox_open => match out {
                    Some(msg) => {
                        if codec::write_frame(&mut send, &msg).await.is_err() {
                            *pending = Some(msg);
                            break Err(closed(&conn));
                        }
                    }
                    None => outbox_open = false,
                },
                m = next_metrics(&mut metrics) => {
                    if codec::write_frame(&mut send, &AgentToControl::Metrics(m)).await.is_err() {
                        break Err(closed(&conn));
                    }
                }
            }
        };
        reader.abort();
        result
    }

    fn renewal_due(&self) -> bool {
        self.identity
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .renewal_due(now_unix())
    }

    /// Verifie le certificat renouvele, le garde (`agent.crt` remplace
    /// atomiquement) puis reconstruit la configuration TLS : la connexion
    /// suivante le presente.
    fn install_certificate(&self, cert_pem: &str) -> Result<(), String> {
        let mut identity = self.identity.lock().unwrap_or_else(PoisonError::into_inner);
        let cert = identity.check_renewed(cert_pem, now_unix())?;
        crate::config::replace_cert_file(&self.dir, cert_pem.as_bytes())
            .map_err(|e| format!("ecriture de {} : {e}", crate::config::CERT_FILE))?;
        identity.set_cert(cert);
        let client = client_config(&identity).map_err(|e| e.to_string())?;
        *self.client.lock().unwrap_or_else(PoisonError::into_inner) = client;
        Ok(())
    }
}

/// Prochaine mesure (en attente indefinie sans metriques). Annulable sans
/// perte : `watch::Receiver::changed` l'est.
async fn next_metrics(rx: &mut Option<watch::Receiver<Option<NodeMetrics>>>) -> NodeMetrics {
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    loop {
        if rx.changed().await.is_err() {
            return std::future::pending().await;
        }
        if let Some(m) = rx.borrow_and_update().clone() {
            return m;
        }
    }
}

fn close(conn: &Connection, code: u32, reason: &str) {
    conn.close(VarInt::from_u32(code), reason.as_bytes());
}

fn closed(conn: &Connection) -> LinkError {
    match conn.close_reason() {
        Some(e) => connection_error(&e),
        None => LinkError::Lost("flux interrompu".into()),
    }
}

fn connection_error(e: &ConnectionError) -> LinkError {
    match e {
        ConnectionError::ApplicationClosed(close) => {
            let code = u32::try_from(close.error_code.into_inner()).unwrap_or(u32::MAX);
            LinkError::Closed(code, String::from_utf8_lossy(&close.reason).into_owned())
        }
        other => LinkError::Lost(other.to_string()),
    }
}
