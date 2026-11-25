//! Registre des agents connectes : envoi d'un ordre a un serveur, diffusion
//! des messages recus de tous les agents.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use proto::{AgentToControl, ControlToAgent, error_code};
use quinn::{Connection, VarInt};
use shared::ids::ServerId;
use tokio::sync::{broadcast, mpsc};

use crate::domain::logs::cap_received;
use crate::domain::logs::rate::LineBucket;

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
    /// Debit des lignes de journal par agent (applique avant la diffusion).
    log_rates: Mutex<HashMap<ServerId, LineBucket>>,
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
                log_rates: Mutex::new(HashMap::new()),
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
    /// de journal sur leur propre canal, le reste sur `subscribe`. Une ligne
    /// de journal est limitee avant d'entrer dans le canal : debit de
    /// `AGENT_LINES_PER_SEC` lignes par seconde et par agent (les autres
    /// sont ignorees, comptees pour `take_ignored`), ligne brute plafonnee a
    /// `MAX_RECEIVED_LINE_BYTES` (la coupe a 16 Kio vient apres le masquage).
    pub fn publish(&self, server: ServerId, msg: AgentToControl) {
        self.publish_at(server, msg, Instant::now());
    }

    fn publish_at(&self, server: ServerId, msg: AgentToControl, now: Instant) {
        let msg = match msg {
            AgentToControl::LogLine(mut line) => {
                let admitted = self
                    .log_rates()
                    .entry(server)
                    .or_insert_with(|| LineBucket::new(now))
                    .admit(line.target, now);
                if !admitted {
                    return;
                }
                line.line = cap_received(line.line);
                let _ = self
                    .inner
                    .logs
                    .send((server, AgentToControl::LogLine(line)));
                return;
            }
            msg => msg,
        };
        // Sans abonne, le message est simplement ignore.
        let _ = self.inner.events.send((server, msg));
    }

    fn log_rates(&self) -> MutexGuard<'_, HashMap<ServerId, LineBucket>> {
        self.inner
            .log_rates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Lignes de journal ignorees par le debit, par agent, depuis le dernier
    /// appel : (agent, nombre, derniere cible touchee).
    pub fn take_ignored(&self) -> Vec<(ServerId, u64, proto::LogTarget)> {
        self.log_rates()
            .iter_mut()
            .filter_map(|(server, bucket)| {
                bucket
                    .take_ignored()
                    .map(|(n, target)| (*server, n, target))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::logs::MAX_RECEIVED_LINE_BYTES;
    use crate::domain::logs::rate::AGENT_LINES_PER_SEC;
    use proto::{LogLine, LogStream, LogTarget};
    use shared::ids::DeploymentId;

    fn line(target: LogTarget, text: String) -> AgentToControl {
        AgentToControl::LogLine(LogLine {
            target,
            stream: LogStream::Stdout,
            timestamp_ms: 1,
            line: text,
        })
    }

    #[test]
    fn oversized_line_truncated_on_receive() {
        let registry = AgentRegistry::new();
        let mut logs = registry.subscribe_logs();
        let target = LogTarget::Deployment(DeploymentId::new());
        registry.publish(
            ServerId(uuid::Uuid::new_v4()),
            line(target, "é".repeat(1024 * 1024)),
        );
        let Ok((_, AgentToControl::LogLine(got))) = logs.try_recv() else {
            panic!("ligne attendue");
        };
        assert!(got.line.len() <= MAX_RECEIVED_LINE_BYTES);
        assert!(got.line.len() > MAX_RECEIVED_LINE_BYTES - 4);
    }

    #[test]
    fn agent_line_rate_limited() {
        let registry = AgentRegistry::new();
        let mut logs = registry.subscribe_logs();
        let target = LogTarget::Deployment(DeploymentId::new());
        let (flood, quiet) = (
            ServerId(uuid::Uuid::new_v4()),
            ServerId(uuid::Uuid::new_v4()),
        );
        let now = Instant::now();
        let mut from_flood = 0;
        let mut quiet_seen = false;
        let total = 3 * AGENT_LINES_PER_SEC as usize;
        for n in 0..total {
            registry.publish_at(flood, line(target, format!("l{n}")), now);
            while let Ok((server, _)) = logs.try_recv() {
                if server == flood {
                    from_flood += 1;
                }
            }
        }
        // Un autre agent n'est pas touche par l'inondation.
        registry.publish_at(quiet, line(target, "calme".into()), now);
        while let Ok((server, _)) = logs.try_recv() {
            quiet_seen |= server == quiet;
        }
        assert_eq!(from_flood, AGENT_LINES_PER_SEC as usize);
        assert!(quiet_seen);
        assert_eq!(
            registry.take_ignored(),
            vec![(flood, (total - from_flood) as u64, target)]
        );
        assert!(registry.take_ignored().is_empty());
    }
}
