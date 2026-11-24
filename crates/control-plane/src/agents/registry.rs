//! Registre des agents connectes : envoi d'un ordre a un serveur, diffusion
//! des messages recus de tous les agents.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use proto::{AgentToControl, ControlToAgent, error_code};
use quinn::{Connection, VarInt};
use shared::ids::ServerId;
use tokio::sync::{broadcast, mpsc};

/// Ordres en attente d'ecriture par agent ; au-dela, `send` echoue.
pub const OUTBOX_CAPACITY: usize = 256;
const EVENTS_CAPACITY: usize = 1024;
/// Lignes de journal en attente par abonne (canal a part : un flot de
/// lignes ne fait jamais perdre un message de deploiement).
const LOGS_CAPACITY: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub enum SendError {
    /// Aucun agent connecte pour ce serveur.
    NotConnected,
    /// File d'envoi de l'agent pleine.
    Busy,
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => f.write_str("agent non connecte"),
            Self::Busy => f.write_str("file d'envoi de l'agent pleine"),
        }
    }
}

impl std::error::Error for SendError {}

struct Slot {
    generation: u64,
    outbox: mpsc::Sender<ControlToAgent>,
    /// `None` : agent dans le meme processus (`attach_channel`).
    conn: Option<Connection>,
}

impl Slot {
    fn close(&self, reason: &[u8]) {
        if let Some(conn) = &self.conn {
            conn.close(VarInt::from_u32(error_code::NONE), reason);
        }
    }
}

struct Inner {
    agents: Mutex<HashMap<ServerId, Slot>>,
    /// Derniere connexion refusee par serveur (version du protocole
    /// differente) : affichee comme etat du serveur jusqu'a une connexion
    /// reussie.
    link_errors: Mutex<HashMap<ServerId, String>>,
    events: broadcast::Sender<(ServerId, AgentToControl)>,
    logs: broadcast::Sender<(ServerId, AgentToControl)>,
    next_generation: AtomicU64,
}

#[derive(Clone)]
pub struct AgentRegistry {
    inner: Arc<Inner>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for AgentRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRegistry").finish_non_exhaustive()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                agents: Mutex::new(HashMap::new()),
                link_errors: Mutex::new(HashMap::new()),
                events: broadcast::channel(EVENTS_CAPACITY).0,
                logs: broadcast::channel(LOGS_CAPACITY).0,
                next_generation: AtomicU64::new(1),
            }),
        }
    }

    fn agents(&self) -> MutexGuard<'_, HashMap<ServerId, Slot>> {
        // Aucune operation ne panique sous ce verrou : un poison est sans
        // consequence sur la table.
        self.inner
            .agents
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Met `msg` dans la file d'envoi de l'agent `server`.
    pub fn send(&self, server: ServerId, msg: ControlToAgent) -> Result<(), SendError> {
        let agents = self.agents();
        let slot = agents.get(&server).ok_or(SendError::NotConnected)?;
        slot.outbox.try_send(msg).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => SendError::Busy,
            mpsc::error::TrySendError::Closed(_) => SendError::NotConnected,
        })
    }

    /// Messages recus de tous les agents, a partir de maintenant, hors
    /// lignes de journal (`subscribe_logs`).
    pub fn subscribe(&self) -> broadcast::Receiver<(ServerId, AgentToControl)> {
        self.inner.events.subscribe()
    }

    /// Lignes de journal (`LogLine`) recues de tous les agents, a partir de
    /// maintenant.
    pub fn subscribe_logs(&self) -> broadcast::Receiver<(ServerId, AgentToControl)> {
        self.inner.logs.subscribe()
    }

    /// Serveurs dont un agent est connecte.
    pub fn connected(&self) -> Vec<ServerId> {
        self.agents().keys().copied().collect()
    }

    pub fn is_connected(&self, server: ServerId) -> bool {
        self.agents().contains_key(&server)
    }

    fn link_errors(&self) -> MutexGuard<'_, HashMap<ServerId, String>> {
        self.inner
            .link_errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Note le refus de la connexion de `server` (affiche comme son etat).
    pub fn set_link_error(&self, server: ServerId, message: String) {
        self.link_errors().insert(server, message);
    }

    /// Cause du dernier refus de la connexion de `server`, effacee par une
    /// connexion reussie.
    pub fn link_error(&self, server: ServerId) -> Option<String> {
        self.link_errors().get(&server).cloned()
    }

    /// Ferme la connexion de l'agent `server` (il se reconnectera).
    ///
    /// Revocation d'un serveur : poser `revoked_at` en base, PUIS appeler
    /// `disconnect`. Une connexion en cours d'acceptation relit
    /// l'autorisation apres s'etre inscrite (`transport`) : soit elle est
    /// deja inscrite et `disconnect` la ferme, soit sa relecture voit la
    /// revocation.
    pub fn disconnect(&self, server: ServerId) {
        if let Some(slot) = self.agents().remove(&server) {
            slot.close(b"deconnexion");
        }
    }

    /// Inscrit la connexion authentifiee de `server` ; une connexion
    /// precedente du meme serveur est fermee. Renvoie la generation (pour
    /// `detach`) et la file des ordres a ecrire.
    pub(crate) fn attach(
        &self,
        server: ServerId,
        conn: Connection,
    ) -> (u64, mpsc::Receiver<ControlToAgent>) {
        self.attach_slot(server, Some(conn))
    }

    /// Inscrit un agent sans connexion QUIC (agent dans le meme processus,
    /// tests) : les ordres pour `server` arrivent sur la file renvoyee. Une
    /// connexion precedente du meme serveur est fermee.
    pub fn attach_channel(&self, server: ServerId) -> mpsc::Receiver<ControlToAgent> {
        self.attach_slot(server, None).1
    }

    fn attach_slot(
        &self,
        server: ServerId,
        conn: Option<Connection>,
    ) -> (u64, mpsc::Receiver<ControlToAgent>) {
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        self.link_errors().remove(&server);
        let (outbox, rx) = mpsc::channel(OUTBOX_CAPACITY);
        let previous = self.agents().insert(
            server,
            Slot {
                generation,
                outbox,
                conn,
            },
        );
        if let Some(old) = previous {
            old.close(b"remplacee");
        }
        (generation, rx)
    }

    /// Retire la connexion `generation` de `server` si c'est toujours elle.
    pub(crate) fn detach(&self, server: ServerId, generation: u64) {
        let mut agents = self.agents();
        if agents
            .get(&server)
            .is_some_and(|s| s.generation == generation)
        {
            agents.remove(&server);
        }
    }

    /// Diffuse un message recu de l'agent `server` aux abonnes : les lignes
    /// de journal sur leur propre canal, le reste sur `subscribe`.
    pub fn publish(&self, server: ServerId, msg: AgentToControl) {
        let channel = match msg {
            AgentToControl::LogLine(_) => &self.inner.logs,
            _ => &self.inner.events,
        };
        // Sans abonne, le message est simplement ignore.
        let _ = channel.send((server, msg));
    }
}
