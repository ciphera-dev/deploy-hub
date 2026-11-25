//! Journaux en direct (`Logs` / `StopLogs`) : un seul suivi par cible.
//!
//! Le conteneur suivi vient de l'etat local : celui du deploiement
//! (`LogTarget::Deployment`) ou celui qui sert le service
//! (`LogTarget::Service`), puis celui du deploiement suivant apres une
//! bascule. Une cible sans conteneur connu est ignoree. Les
//! lignes passent par la file des messages de l'agent sans jamais l'attendre :
//! file pleine, la ligne est abandonnee (les journaux ne retardent ni les
//! progressions de deploiement ni les autres messages).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::StreamExt;
use proto::{AgentToControl, LogTarget};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::docker::{ContainerId, DockerOps, LogStart};
use crate::state::StateDb;

struct Follow {
    generation: u64,
    task: AbortHandle,
}

struct Inner<D> {
    docker: Arc<D>,
    db: StateDb,
    active: Mutex<HashMap<LogTarget, Follow>>,
    next_generation: AtomicU64,
}

/// Suivis de journaux en cours, par cible.
pub struct LogFollows<D> {
    inner: Arc<Inner<D>>,
}

impl<D> Clone for LogFollows<D> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<D: DockerOps> LogFollows<D> {
    pub fn new(docker: Arc<D>, db: StateDb) -> Self {
        Self {
            inner: Arc::new(Inner {
                docker,
                db,
                active: Mutex::new(HashMap::new()),
                next_generation: AtomicU64::new(1),
            }),
        }
    }

    fn active_map(&self) -> MutexGuard<'_, HashMap<LogTarget, Follow>> {
        // Aucune operation ne panique sous ce verrou.
        self.inner
            .active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Nombre de suivis en cours.
    pub fn active(&self) -> usize {
        self.active_map().len()
    }

    /// Conteneur de la cible dans l'etat local.
    fn container(&self, target: LogTarget) -> Option<ContainerId> {
        let found = match target {
            LogTarget::Deployment(id) => self.inner.db.record(id).map(|r| r.map(|r| r.container)),
            LogTarget::Service(id) => self.inner.db.serving().map(|records| {
                records
                    .into_iter()
                    .find(|r| r.deployment.service_id == id)
                    .map(|r| r.container)
            }),
        };
        found.unwrap_or_else(|e| {
            tracing::error!("journaux : etat local illisible ({e})");
            None
        })
    }

    /// Ordre `Logs` : envoie les `tail` dernieres lignes, puis suit la cible
    /// si `follow` (une demande sur une cible deja suivie est ignoree).
    pub fn start(
        &self,
        target: LogTarget,
        tail: u32,
        follow: bool,
        outbox: mpsc::Sender<AgentToControl>,
    ) {
        let mut active = self.active_map();
        if follow && active.contains_key(&target) {
            return;
        }
        let Some(container) = self.container(target) else {
            return;
        };
        let generation = self.inner.next_generation.fetch_add(1, Ordering::Relaxed);
        let this = self.clone();
        let task = tokio::spawn(async move {
            let mut container = container;
            let mut start = LogStart::Tail(tail);
            let mut dropped = 0u64;
            loop {
                let mut lines = this.inner.docker.logs(&container, start, follow).await;
                while let Some(line) = lines.next().await {
                    match outbox.try_send(AgentToControl::LogLine(line.into_line(target))) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => dropped += 1,
                        Err(mpsc::error::TrySendError::Closed(_)) => break,
                    }
                }
                // Fin du flux d'un service suivi : un nouveau deploiement le
                // sert (bascule) ? Le suivi continue sur son conteneur, neuf,
                // lu depuis le debut de son journal (seule sa premiere ligne
                // par flux est remplacee, voir `LogWindow`).
                let next = match target {
                    LogTarget::Service(_) if follow && !outbox.is_closed() => {
                        this.container(target)
                    }
                    _ => None,
                };
                match next {
                    Some(next) if next != container => {
                        container = next;
                        start = LogStart::Beginning;
                    }
                    _ => break,
                }
            }
            if dropped > 0 {
                tracing::warn!("journaux : {dropped} ligne(s) abandonnee(s), file pleine");
            }
            if follow {
                let mut active = this.active_map();
                if active
                    .get(&target)
                    .is_some_and(|f| f.generation == generation)
                {
                    active.remove(&target);
                }
            }
        });
        if follow {
            active.insert(
                target,
                Follow {
                    generation,
                    task: task.abort_handle(),
                },
            );
        }
    }

    /// Ordre `StopLogs` : arrete le suivi de la cible, s'il y en a un.
    pub fn stop(&self, target: LogTarget) {
        if let Some(follow) = self.active_map().remove(&target) {
            follow.task.abort();
        }
    }
}
