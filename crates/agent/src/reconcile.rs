//! Reconciliation : a chaque (re)connexion au plan de controle, l'agent
//! envoie `State` : la liste complete de ses deploiements (sante, deploiement
//! servi de chaque service) et de ses operations en cours (builds et
//! deploiements ni termines ni annules). C'est un
//! instantane complet, donc idempotent : le plan de controle remplace ce
//! qu'il savait de ce serveur. (`DeployProgress` et `LogLine` sont livres au
//! moins une fois et peuvent arriver en double apres une coupure : leurs
//! consommateurs doivent etre idempotents ; `Metrics` n'envoie que la
//! derniere mesure, voir `metrics`.)
//!
//! L'etat vient de l'etat local (`StateDb`), avec la sante relue dans
//! Docker ; un conteneur disparu est retire. Si Docker ne repond pas a
//! temps, le dernier etat connu est envoye tel quel.

use std::sync::Arc;
use std::time::Duration;

use proto::{AgentState, RunningDeployment};

use crate::docker::{DockerOps, Health};
use crate::ops::Operations;
use crate::state::StateDb;
use crate::transport::StateFn;

/// Delai maximal accorde a Docker pour relire la sante des conteneurs.
pub const DOCKER_TIMEOUT: Duration = Duration::from_secs(5);

/// Relit la sante de chaque deploiement enregistre et met l'etat local a
/// jour. Les erreurs de Docker laissent la valeur connue.
async fn refresh<D: DockerOps>(docker: &D, db: &StateDb) -> Vec<RunningDeployment> {
    let records = match db.deployments() {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("{e}");
            return Vec::new();
        }
    };
    let mut state = Vec::with_capacity(records.len());
    for record in records {
        let mut deployment = record.deployment;
        match docker.health(&record.container).await {
            Ok(Health::Missing) => {
                tracing::warn!(
                    "deploiement {} : conteneur disparu, retire de l'etat local",
                    deployment.deployment_id
                );
                if let Err(e) = db.remove_deployment(deployment.deployment_id) {
                    tracing::error!("{e}");
                }
                continue;
            }
            Ok(health) => {
                let healthy = health.is_healthy();
                if healthy != deployment.healthy {
                    deployment.healthy = healthy;
                    if let Err(e) = db.set_healthy(deployment.deployment_id, healthy) {
                        tracing::error!("{e}");
                    }
                }
            }
            Err(e) => tracing::warn!(
                "deploiement {} : sante illisible ({e}), derniere valeur conservee",
                deployment.deployment_id
            ),
        }
        state.push(deployment);
    }
    state
}

/// Etat courant : sante relue dans Docker (au plus `DOCKER_TIMEOUT`), sinon
/// etat local tel quel.
pub async fn snapshot<D: DockerOps>(docker: &D, db: &StateDb) -> Vec<RunningDeployment> {
    match tokio::time::timeout(DOCKER_TIMEOUT, refresh(docker, db)).await {
        Ok(state) => state,
        Err(_) => {
            tracing::warn!("Docker ne repond pas : dernier etat connu envoye");
            db.running().unwrap_or_else(|e| {
                tracing::error!("{e}");
                Vec::new()
            })
        }
    }
}

/// Fournisseur d'etat du lien, appele apres chaque `Hello` : deploiements
/// et operations en cours (`ops`).
pub fn state_fn<D: DockerOps>(docker: Arc<D>, db: StateDb, ops: Operations) -> StateFn {
    Arc::new(move || {
        let docker = docker.clone();
        let db = db.clone();
        let ops = ops.clone();
        Box::pin(async move {
            AgentState {
                deployments: snapshot(docker.as_ref(), &db).await,
                in_progress: ops.in_progress(),
                outcomes: db.outcomes().unwrap_or_else(|e| {
                    tracing::error!("issues illisibles : {e}");
                    Vec::new()
                }),
            }
        })
    })
}

/// Demandes d'etat complet (`RequestState`) fusionnees : une seule lecture
/// a la fois ; une demande recue pendant une lecture en relance une, une
/// seule, juste apres (rien n'est perdu). Une panique pendant la lecture ne
/// bloque pas les demandes suivantes.
#[derive(Clone, Default)]
pub struct StateRequests(Arc<Requests>);

#[derive(Default)]
struct Requests {
    running: std::sync::atomic::AtomicBool,
    again: std::sync::atomic::AtomicBool,
}

/// Remet `running` a faux en sortant, y compris sur panique.
struct Release<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for Release<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl StateRequests {
    /// Enregistre une demande ; `true` : aucune lecture en cours, l'appelant
    /// lance [`StateRequests::serve`].
    pub fn request(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        self.0.again.store(true, SeqCst);
        !self.0.running.swap(true, SeqCst)
    }

    /// Sert les demandes en attente (`read` puis `send`) jusqu'a ce qu'il
    /// n'en reste plus.
    pub async fn serve<T, R, RF, S, SF>(&self, read: R, send: S)
    where
        R: Fn() -> RF,
        RF: std::future::Future<Output = T>,
        S: Fn(T) -> SF,
        SF: std::future::Future<Output = ()>,
    {
        use std::sync::atomic::Ordering::SeqCst;
        loop {
            let release = Release(&self.0.running);
            while self.0.again.swap(false, SeqCst) {
                send(read().await).await;
            }
            drop(release);
            // Demande arrivee entre la derniere lecture et la liberation :
            // servie ici, sauf si une autre tache l'a deja prise.
            if !self.0.again.load(SeqCst) || self.0.running.swap(true, SeqCst) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod requests_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Demandes recues pendant une lecture : une seule lecture de plus,
    /// juste apres.
    #[tokio::test]
    async fn requests_during_a_read_are_merged_into_one_more_read() {
        let requests = StateRequests::default();
        let reads = Arc::new(AtomicUsize::new(0));
        assert!(requests.request());
        let (r, n) = (requests.clone(), reads.clone());
        let serving = tokio::spawn(async move {
            r.serve(
                || {
                    n.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(100))
                },
                |()| async {},
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        for _ in 0..3 {
            assert!(!requests.request(), "lecture deja en cours");
        }
        serving.await.unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        assert!(requests.request(), "plus rien en cours");
    }

    /// Panique pendant la lecture : la demande suivante est servie.
    #[tokio::test]
    async fn a_panicking_read_does_not_block_later_requests() {
        let requests = StateRequests::default();
        assert!(requests.request());
        let r = requests.clone();
        let failed = tokio::spawn(async move {
            r.serve(|| async { panic!("lecture de l'etat") }, |()| async {})
                .await;
        })
        .await;
        assert!(failed.is_err());
        assert!(requests.request(), "demande suivante servie");
    }
}
