//! Metriques du noeud, mesurees toutes les 30 s.
//!
//! Elles ne passent pas par la file des messages vers le plan de controle :
//! chaque mesure remplace la precedente dans un emplacement « derniere
//! valeur » (`tokio::sync::watch`). Le lien (voir `transport`) envoie la
//! derniere mesure une fois juste apres `State` a chaque (re)connexion, puis
//! chaque nouvelle mesure tant qu'il est connecte. Hors connexion rien ne
//! s'accumule : apres une longue coupure, une seule mesure part. Une mesure
//! peut donc etre perdue ou, rarement, renvoyee : seule la plus recente
//! compte.
//!
//! Chaque mesure porte aussi le conteneur servi de chaque service (etat
//! local) : CPU et memoire s'il tourne, `running: false` s'il est arrete,
//! en echec ou disparu (y compris hors du panneau).

use std::sync::Arc;
use std::time::Duration;

use proto::{ContainerMetrics, NodeMetrics};
use tokio::sync::watch;
use tokio::time::{Instant, MissedTickBehavior};

use crate::docker::DockerOps;
use crate::state::StateDb;

/// Periode de collecte (fixee par le spec).
pub const INTERVAL: Duration = Duration::from_secs(30);
/// Delai maximal de chaque appel a Docker pendant une mesure : un Docker qui
/// ne repond plus ne bloque pas la boucle de mesure.
pub const DOCKER_TIMEOUT: Duration = Duration::from_secs(10);

/// Emplacement de la derniere mesure (`None` avant la premiere).
pub type Slot = watch::Sender<Option<NodeMetrics>>;
/// Lecture de la derniere mesure (donnee au lien, `Agent::with_metrics`).
pub type Latest = watch::Receiver<Option<NodeMetrics>>;

/// Emplacement vide et son lecteur.
pub fn slot() -> (Slot, Latest) {
    watch::channel(None)
}

/// Mesure une fois et remplace la valeur precedente (meme identique : le
/// lien l'envoie).
pub async fn sample_once<D: DockerOps>(docker: &D, db: &StateDb, slot: &Slot) {
    match tokio::time::timeout(DOCKER_TIMEOUT, docker.stats()).await {
        Ok(Ok(mut m)) => {
            m.containers = containers(docker, db).await;
            slot.send_replace(Some(m));
        }
        Ok(Err(e)) => tracing::warn!("metriques indisponibles : {e}"),
        Err(_) => tracing::warn!("metriques indisponibles : Docker ne repond pas"),
    }
}

/// Conteneur servi de chaque service. Sante illisible : conteneur omis
/// (aucune conclusion) ; consommation illisible : zeros.
async fn containers<D: DockerOps>(docker: &D, db: &StateDb) -> Vec<ContainerMetrics> {
    let serving = match db.serving() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("{e}");
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(serving.len());
    for record in serving {
        let running =
            match tokio::time::timeout(DOCKER_TIMEOUT, docker.health(&record.container)).await {
                Ok(Ok(h)) => h.is_healthy(),
                Ok(Err(e)) => {
                    tracing::warn!(
                        "deploiement {} : sante illisible ({e})",
                        record.deployment.deployment_id
                    );
                    continue;
                }
                Err(_) => {
                    tracing::warn!(
                        "deploiement {} : sante illisible (Docker ne repond pas)",
                        record.deployment.deployment_id
                    );
                    continue;
                }
            };
        let (cpu_percent, mem_used_bytes) = if running {
            tokio::time::timeout(DOCKER_TIMEOUT, docker.container_usage(&record.container))
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .unwrap_or((0.0, 0))
        } else {
            (0.0, 0)
        };
        out.push(ContainerMetrics {
            deployment_id: record.deployment.deployment_id,
            service_id: record.deployment.service_id,
            cpu_percent,
            mem_used_bytes,
            running,
        });
    }
    out
}

/// Mesure toutes les `INTERVAL` (premiere mesure apres 30 s) tant que
/// quelqu'un lit l'emplacement.
pub async fn run<D: DockerOps>(docker: Arc<D>, db: StateDb, slot: Slot) {
    let mut tick = tokio::time::interval_at(Instant::now() + INTERVAL, INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            () = slot.closed() => return,
        }
        sample_once(docker.as_ref(), &db, &slot).await;
    }
}
