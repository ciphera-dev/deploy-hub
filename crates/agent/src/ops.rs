//! Operations en cours sur l'agent (builds et deploiements), une au plus par
//! deploiement, annulables par `ControlToAgent::Stop`. La liste part dans
//! `AgentState::in_progress` a chaque connexion.
//!
//! Les decisions qui comptent sont prises sous le verrou du registre, donc
//! jamais a moitie : une annulation retire l'entree et interrompt la tache
//! d'un seul geste ; une operation entre en bascule (`Ticket::enter_switch`)
//! ou se termine (`Ticket::finish`) seulement si elle n'a pas ete annulee.
//! Une operation en bascule n'est plus interrompue : `cancel` attend qu'elle
//! rende la main.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use proto::Stage;
use shared::ids::{DeploymentId, ProjectId};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Nature d'une operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Build,
    Deploy,
}

impl OpKind {
    /// Etape rapportee quand l'operation est annulee.
    pub fn stage(self) -> Stage {
        match self {
            Self::Build => Stage::Build,
            Self::Deploy => Stage::Start,
        }
    }
}

struct Entry {
    kind: OpKind,
    /// Projet de l'operation, s'il est connu (retrait d'un projet).
    project: Option<ProjectId>,
    generation: u64,
    handle: Option<JoinHandle<()>>,
    /// Bascule commencee : l'operation n'est plus interrompue.
    switching: bool,
    /// Lache quand l'entree disparait : reveille ceux qui attendent.
    _released: watch::Sender<()>,
}

#[derive(Default)]
struct Registry {
    entries: HashMap<DeploymentId, Entry>,
    next: u64,
}

/// Registre partage des operations en cours.
#[derive(Clone, Default)]
pub struct Operations {
    inner: Arc<Mutex<Registry>>,
}

impl std::fmt::Debug for Operations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Operations")
            .field("in_progress", &self.in_progress())
            .finish()
    }
}

/// Issue de `Operations::cancel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancel<R = ()> {
    /// Aucune operation en cours pour ce deploiement.
    Idle,
    /// Operation interrompue (tache finie, processus tues) ; valeur rendue
    /// par le rappel d'annulation.
    Cancelled(OpKind, R),
    /// Bascule deja commencee : rien d'interrompu, l'operation a rendu la
    /// main (bascule reussie ou echouee : voir l'issue enregistree).
    TooLate,
}

/// Operation deja en cours : le travail n'a pas ete lance.
pub struct Busy {
    pub kind: OpKind,
    released: watch::Receiver<()>,
}

impl Busy {
    /// Attend la fin de l'operation en cours, `timeout` au plus ; `true` si
    /// elle a rendu la main.
    pub async fn wait(mut self, timeout: Duration) -> bool {
        tokio::time::timeout(timeout, async {
            // Erreur = emetteur lache = entree retiree.
            while self.released.changed().await.is_ok() {}
        })
        .await
        .is_ok()
    }
}

/// Poignee d'une operation sur son entree du registre.
#[derive(Clone)]
pub struct Ticket {
    ops: Operations,
    id: DeploymentId,
    generation: u64,
}

impl Ticket {
    fn with_entry<R>(&self, f: impl FnOnce(&mut Registry) -> R) -> Option<R> {
        let mut reg = self.ops.lock();
        match reg.entries.get(&self.id) {
            Some(e) if e.generation == self.generation => Some(f(&mut reg)),
            _ => None,
        }
    }

    /// Debut de la bascule : plus interruptible. `false` si l'operation a
    /// ete annulee entre-temps (elle doit s'arreter la).
    pub fn enter_switch(&self) -> bool {
        self.with_entry(|reg| {
            if let Some(e) = reg.entries.get_mut(&self.id) {
                e.switching = true;
            }
        })
        .is_some()
    }

    /// Fin de l'operation : `f` (enregistrements) puis retrait de l'entree,
    /// sous le verrou du registre ; `None` (rien fait) si elle a ete annulee.
    /// La tache peut continuer apres (drainage de l'ancien deploiement).
    pub fn finish<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        self.with_entry(|reg| {
            let r = f();
            reg.entries.remove(&self.id);
            r
        })
    }
}

impl Operations {
    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lance `work(ticket)` pour `id` ; `Err(Busy)` (rien n'est lance) si une
    /// operation de ce deploiement est deja en cours. L'entree disparait a
    /// la fin de `work` (sauf si une autre l'a remplacee entre-temps).
    pub fn spawn<W, F>(&self, id: DeploymentId, kind: OpKind, work: W) -> Result<(), Busy>
    where
        W: FnOnce(Ticket) -> F,
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_in(id, kind, None, work)
    }

    /// Comme `spawn`, pour une operation du projet `project`
    /// (`of_project`).
    pub fn spawn_in<W, F>(
        &self,
        id: DeploymentId,
        kind: OpKind,
        project: Option<ProjectId>,
        work: W,
    ) -> Result<(), Busy>
    where
        W: FnOnce(Ticket) -> F,
        F: Future<Output = ()> + Send + 'static,
    {
        // Verrou tenu pendant le lancement : la tache ne peut pas retirer
        // son entree avant qu'elle soit inseree.
        let mut reg = self.lock();
        if let Some(e) = reg.entries.get(&id) {
            return Err(Busy {
                kind: e.kind,
                released: e._released.subscribe(),
            });
        }
        reg.next += 1;
        let generation = reg.next;
        let ticket = Ticket {
            ops: self.clone(),
            id,
            generation,
        };
        let fut = work(ticket.clone());
        let handle = tokio::spawn(async move {
            fut.await;
            ticket.with_entry(|reg| reg.entries.remove(&ticket.id));
        });
        let (tx, _) = watch::channel(());
        reg.entries.insert(
            id,
            Entry {
                kind,
                project,
                generation,
                handle: Some(handle),
                switching: false,
                _released: tx,
            },
        );
        Ok(())
    }

    /// `Stop` : interrompt l'operation de `id` et attend sa fin (processus
    /// tues, dossiers liberes). En bascule : rien n'est interrompu, attente
    /// qu'elle rende la main (`TooLate`).
    ///
    /// `on_cancel(kind)` s'execute sous le verrou du registre, avant que le
    /// creneau soit libere : l'annulation y est rendue durable (issue
    /// gardee), si bien qu'un ordre qui attendait ce creneau, ou un ordre
    /// rejoue, la trouve deja en place a son reveil.
    pub async fn cancel<R>(
        &self,
        id: DeploymentId,
        on_cancel: impl FnOnce(OpKind) -> R,
    ) -> Cancel<R> {
        enum Decision<R> {
            Wait(watch::Receiver<()>),
            Abort(Entry, R),
        }
        let decision = {
            let mut reg = self.lock();
            let switching = match reg.entries.get(&id) {
                None => return Cancel::Idle,
                Some(e) => e.switching,
            };
            if switching {
                match reg.entries.get(&id) {
                    Some(e) => Decision::Wait(e._released.subscribe()),
                    None => return Cancel::Idle,
                }
            } else {
                let Some(kind) = reg.entries.get(&id).map(|e| e.kind) else {
                    return Cancel::Idle;
                };
                let r = on_cancel(kind);
                match reg.entries.remove(&id) {
                    Some(e) => Decision::Abort(e, r),
                    None => return Cancel::Idle,
                }
            }
        };
        match decision {
            Decision::Wait(mut released) => {
                while released.changed().await.is_ok() {}
                Cancel::TooLate
            }
            Decision::Abort(mut e, r) => {
                if let Some(h) = e.handle.take() {
                    h.abort();
                    let _ = h.await;
                }
                Cancel::Cancelled(e.kind, r)
            }
        }
    }

    pub fn is_running(&self, id: DeploymentId) -> bool {
        self.lock().entries.contains_key(&id)
    }

    /// Deploiements du projet `project` dont une operation est en cours.
    pub fn of_project(&self, project: ProjectId) -> Vec<DeploymentId> {
        let mut ids: Vec<_> = self
            .lock()
            .entries
            .iter()
            .filter(|(_, e)| e.project == Some(project))
            .map(|(id, _)| *id)
            .collect();
        ids.sort();
        ids
    }

    /// Deploiements dont une operation est en cours.
    pub fn in_progress(&self) -> Vec<DeploymentId> {
        let mut ids: Vec<_> = self.lock().entries.keys().copied().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn finished_ticket_does_not_remove_a_newer_operation() {
        let ops = Operations::default();
        let id = DeploymentId::new();
        let (go, wait) = tokio::sync::oneshot::channel::<()>();
        ops.spawn(id, OpKind::Build, |t| async move {
            assert_eq!(t.finish(|| 1), Some(1));
            let _ = wait.await;
        })
        .unwrap_or_else(|_| panic!("libre"));
        while ops.is_running(id) {
            tokio::task::yield_now().await;
        }
        let (hold, held) = tokio::sync::oneshot::channel::<()>();
        ops.spawn(id, OpKind::Deploy, |_| async move {
            let _ = held.await;
        })
        .unwrap_or_else(|_| panic!("libre apres finish"));
        // Fin de la premiere tache : la seconde reste enregistree.
        go.send(()).unwrap();
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(ops.is_running(id));
        drop(hold);
    }

    #[tokio::test]
    async fn cancelled_operation_cannot_enter_switch_or_finish() {
        let ops = Operations::default();
        let id = DeploymentId::new();
        let (tx, rx) = tokio::sync::oneshot::channel();
        ops.spawn(id, OpKind::Deploy, |t| async move {
            let _ = tx.send(t);
            std::future::pending::<()>().await;
        })
        .unwrap_or_else(|_| panic!("libre"));
        let ticket = rx.await.unwrap();
        assert_eq!(
            ops.cancel(id, |_| ()).await,
            Cancel::Cancelled(OpKind::Deploy, ())
        );
        assert!(!ticket.enter_switch());
        assert_eq!(ticket.finish(|| ()), None);
        assert_eq!(ops.cancel(id, |_| ()).await, Cancel::Idle);
    }

    #[tokio::test]
    async fn busy_waits_for_the_running_operation() {
        let ops = Operations::default();
        let id = DeploymentId::new();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        ops.spawn(id, OpKind::Build, |_| async move {
            let _ = rx.await;
        })
        .unwrap_or_else(|_| panic!("libre"));
        let busy = ops
            .spawn(id, OpKind::Deploy, |_| async {})
            .expect_err("occupe");
        assert_eq!(busy.kind, OpKind::Build);
        let waiting = tokio::spawn(busy.wait(Duration::from_secs(60)));
        tx.send(()).unwrap();
        assert!(waiting.await.unwrap());
        assert!(!ops.is_running(id));
    }
}
