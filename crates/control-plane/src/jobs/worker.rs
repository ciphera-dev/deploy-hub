//! Boucle de travail : prend les taches une par une (concurrence bornee),
//! les confie au gestionnaire du genre, prolonge le bail pendant l'execution
//! et conclut. Arret propre sur le signal d'arret.
//!
//! Livraison **au moins une fois** : un gestionnaire peut etre execute deux
//! fois pour la meme tache (travailleur tombe, bail echu, reprise). Il doit
//! etre idempotent et ne mettre dans ses erreurs aucun secret.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::{Semaphore, watch};
use tokio::task::{AbortHandle, JoinSet};

use super::{
    Failure, Job, JobKind, LEASE, NOTIFY_CHANNEL, claim_next, complete, fail, heartbeat,
    reap_expired,
};
use crate::state::Clock;

/// Echec d'un gestionnaire.
#[derive(Clone, Debug)]
pub struct HandlerError {
    /// Message conserve (tronque) : aucun secret.
    pub message: String,
    /// Reprise avec delai (tant qu'il reste des tentatives) ou terminal.
    pub retry: bool,
}

impl HandlerError {
    pub fn retry(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retry: true,
        }
    }

    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retry: false,
        }
    }
}

type BoxFuture = Pin<Box<dyn Future<Output = Result<(), HandlerError>> + Send>>;
type Handler = Arc<dyn Fn(Job) -> BoxFuture + Send + Sync>;
type DeadHandler =
    Arc<dyn Fn(Job, String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// Gestionnaires par genre de tache. Un genre sans gestionnaire echoue de
/// facon terminale.
#[derive(Clone, Default)]
pub struct Handlers {
    run: HashMap<JobKind, Handler>,
    dead: HashMap<JobKind, DeadHandler>,
}

impl Handlers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre le gestionnaire d'un genre (remplace le precedent).
    pub fn register<F, Fut>(mut self, kind: JobKind, handler: F) -> Self
    where
        F: Fn(Job) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), HandlerError>> + Send + 'static,
    {
        self.run
            .insert(kind, Arc::new(move |job| Box::pin(handler(job))));
        self
    }

    /// Enregistre le gestionnaire de fin d'un genre : appele au plus une
    /// fois avec la tache et sa derniere erreur quand elle passe `dead`
    /// (echec terminal, tentatives epuisees, bail echu a la derniere
    /// tentative). Un arret entre le passage `dead` et l'appel le perd : le
    /// gestionnaire ne doit etre qu'un raccourci, rattrape ailleurs (pour
    /// les deploiements : chien de garde `STALE_AFTER`, demande suivante).
    /// Il ne doit pas echouer : il journalise.
    pub fn on_dead<F, Fut>(mut self, kind: JobKind, handler: F) -> Self
    where
        F: Fn(Job, String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.dead.insert(
            kind,
            Arc::new(move |job, error| Box::pin(handler(job, error))),
        );
        self
    }

    /// Appelle le gestionnaire de fin du genre de `job`, s'il en a un.
    async fn dead(&self, job: Job, error: String) {
        if let Some(handler) = self.dead.get(&job.kind).cloned() {
            handler(job, error).await;
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkerConfig {
    /// Identifiant du travailleur (detenteur du bail) ; unique par processus.
    pub worker_id: String,
    /// Nombre maximal de taches executees en meme temps.
    pub concurrency: usize,
    /// Intervalle d'interrogation quand la file est vide.
    pub poll_interval: Duration,
    /// Attente maximale des taches en cours a l'arret ; au-dela elles sont
    /// abandonnees (reprises apres l'echeance de leur bail).
    pub shutdown_grace: Duration,
    /// Reveil par LISTEN/NOTIFY (l'interrogation reste le filet de securite).
    pub listen: bool,
    /// Intervalle des battements de coeur (`LEASE / 3` par defaut). Un bail
    /// perdu interrompt le gestionnaire.
    pub heartbeat: Duration,
}

impl WorkerConfig {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            concurrency: 4,
            poll_interval: Duration::from_secs(5),
            shutdown_grace: Duration::from_secs(30),
            listen: true,
            heartbeat: LEASE / 3,
        }
    }
}

/// Delai de reprise par defaut : 15 s doublees a chaque tentative, 1 h au plus.
pub fn backoff(attempts: i32) -> Duration {
    let exp = attempts.saturating_sub(1).clamp(0, 12) as u32;
    Duration::from_secs(15u64.saturating_mul(1u64 << exp)).min(Duration::from_secs(3600))
}

struct AbortOnDrop(AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Vrai quand l'arret est demande (ou l'emetteur disparu).
async fn stopped(rx: &mut watch::Receiver<bool>) {
    while !*rx.borrow() {
        if rx.changed().await.is_err() {
            return;
        }
    }
}

/// Boucle principale ; rend la main quand `stop` passe a vrai, apres l'arret
/// des taches en cours (dans la limite de `shutdown_grace`).
pub async fn run(
    db: PgPool,
    clock: Clock,
    handlers: Handlers,
    cfg: WorkerConfig,
    mut stop: watch::Receiver<bool>,
) {
    let slots = Arc::new(Semaphore::new(cfg.concurrency.max(1)));
    let mut running: JoinSet<()> = JoinSet::new();
    let mut listener = if cfg.listen {
        open_listener(&db).await
    } else {
        None
    };
    tracing::info!(worker = %cfg.worker_id, "travailleur de la file demarre");

    'main: loop {
        while running.try_join_next().is_some() {}
        if *stop.borrow() {
            break;
        }
        let permit = tokio::select! {
            p = slots.clone().acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => break,
            },
            () = stopped(&mut stop) => break,
        };
        // Bail echu a la derniere tentative : `dead`, gestionnaire de fin.
        match reap_expired(&db, &clock).await {
            Ok(dead) => {
                for job in dead {
                    tracing::warn!(job = %job.id, kind = %job.kind, "bail expire a la derniere tentative : tache morte");
                    let handlers = handlers.clone();
                    running.spawn(async move {
                        handlers
                            .dead(job, "bail expire a la derniere tentative".into())
                            .await;
                    });
                }
            }
            Err(e) => tracing::warn!("releve des baux expires impossible : {e}"),
        }
        match claim_next(&db, &clock, &cfg.worker_id).await {
            Ok(Some(job)) => {
                let (db, clock, handlers, worker) = (
                    db.clone(),
                    clock.clone(),
                    handlers.clone(),
                    cfg.worker_id.clone(),
                );
                let heartbeat = cfg.heartbeat;
                running.spawn(async move {
                    execute(&db, &clock, &handlers, &worker, heartbeat, job).await;
                    drop(permit);
                });
                continue;
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("prise d'une tache impossible : {e}"),
        }
        drop(permit);
        if cfg.listen && listener.is_none() {
            listener = open_listener(&db).await;
        }
        // File vide : attente d'un reveil, de l'echeance ou de l'arret. Sans
        // LISTEN, nouvel essai apres `LISTEN_RETRY` au plus.
        let listen_retry = cfg.poll_interval.min(LISTEN_RETRY);
        let wake = async {
            match listener.as_mut() {
                Some(l) => match l.recv().await {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::warn!("ecoute des nouvelles taches perdue : {e} (nouvel essai)");
                        tokio::time::sleep(listen_retry).await;
                        false
                    }
                },
                None if cfg.listen => {
                    tokio::time::sleep(listen_retry).await;
                    true
                }
                None => std::future::pending::<bool>().await,
            }
        };
        tokio::select! {
            ok = wake => if !ok {
                listener = None;
            },
            () = tokio::time::sleep(cfg.poll_interval) => {}
            () = stopped(&mut stop) => break 'main,
        }
    }

    tracing::info!("arret du travailleur : attente des taches en cours");
    let drained = tokio::time::timeout(cfg.shutdown_grace, async {
        while running.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tracing::warn!("taches encore en cours a la fin du delai : abandon (reprise apres bail)");
        running.abort_all();
    }
}

/// Delai maximal entre deux essais de LISTEN.
const LISTEN_RETRY: Duration = Duration::from_secs(5);

/// Ecoute des nouvelles taches ; un echec est journalise (l'appelant
/// reessaie, l'interrogation reste le filet de securite).
async fn open_listener(db: &PgPool) -> Option<PgListener> {
    let opened = async {
        let mut l = PgListener::connect_with(db).await?;
        l.listen(NOTIFY_CHANNEL).await?;
        Ok::<_, sqlx::Error>(l)
    };
    match opened.await {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::warn!("ecoute des nouvelles taches impossible : {e} (nouvel essai)");
            None
        }
    }
}

/// Execute une tache : gestionnaire (avec battements de coeur), puis
/// conclusion si le bail est encore le notre. Bail perdu (tache reprise par
/// un autre travailleur) : le gestionnaire est interrompu aussitot et rien
/// n'est conclu, pour qu'une meme tache ne s'execute jamais deux fois en
/// meme temps.
async fn execute(
    db: &PgPool,
    clock: &Clock,
    handlers: &Handlers,
    worker: &str,
    heartbeat_every: Duration,
    job: Job,
) {
    let id = job.id;
    let kind = job.kind;
    let attempts = job.attempts;
    let last_attempt = job.attempts >= job.max_attempts;
    // Copie pour le gestionnaire de fin (la tache part au gestionnaire).
    let dead_job = job.clone();
    let Some(handler) = handlers.run.get(&kind).cloned() else {
        let msg = format!("aucun gestionnaire pour le genre {kind}");
        tracing::error!(job = %id, "{msg}");
        if record_fail(db, clock, id, worker, &msg, Failure::Terminal).await {
            handlers.dead(dead_job, msg).await;
        }
        return;
    };

    // Tache dediee : un panic du gestionnaire devient un echec, pas un arret.
    let mut task = tokio::spawn(handler(job));
    let _guard = AbortOnDrop(task.abort_handle());
    let mut beat = tokio::time::interval(heartbeat_every);
    beat.tick().await;
    let outcome = loop {
        tokio::select! {
            res = &mut task => break res,
            _ = beat.tick() => match heartbeat(db, clock, id, worker).await {
                Ok(true) => {}
                Ok(false) => {
                    task.abort();
                    let _ = (&mut task).await;
                    tracing::warn!(job = %id, "bail perdu : la tache a ete reprise, gestionnaire interrompu");
                    return;
                }
                Err(e) => tracing::warn!(job = %id, "battement de coeur impossible : {e}"),
            },
        }
    };

    match outcome {
        Ok(Ok(())) => match complete(db, clock, id, worker).await {
            Ok(true) => tracing::info!(job = %id, %kind, "tache terminee"),
            Ok(false) => tracing::warn!(job = %id, "bail perdu : conclusion ignoree"),
            Err(e) => tracing::error!(job = %id, "conclusion impossible : {e}"),
        },
        Ok(Err(e)) => {
            let failure = if e.retry {
                Failure::Retry(backoff(attempts))
            } else {
                Failure::Terminal
            };
            tracing::warn!(job = %id, %kind, attempts, "tache en echec");
            if record_fail(db, clock, id, worker, &e.message, failure).await
                && (!e.retry || last_attempt)
            {
                handlers.dead(dead_job, e.message).await;
            }
        }
        Err(join) => {
            // Cause non journalisee en clair dans la base (aucun secret).
            let msg = if join.is_panic() {
                "le gestionnaire a plante"
            } else {
                "le gestionnaire a ete interrompu"
            };
            tracing::error!(job = %id, %kind, "{msg}");
            if record_fail(
                db,
                clock,
                id,
                worker,
                msg,
                Failure::Retry(backoff(attempts)),
            )
            .await
                && last_attempt
            {
                handlers.dead(dead_job, msg.to_string()).await;
            }
        }
    }
}

/// Enregistre l'echec ; vrai s'il l'a ete (bail encore detenu).
async fn record_fail(
    db: &PgPool,
    clock: &Clock,
    id: shared::ids::JobId,
    worker: &str,
    msg: &str,
    failure: Failure,
) -> bool {
    match fail(db, clock, id, worker, msg, failure).await {
        Ok(true) => true,
        Ok(false) => {
            tracing::warn!(job = %id, "bail perdu : echec ignore");
            false
        }
        Err(e) => {
            tracing::error!(job = %id, "enregistrement de l'echec impossible : {e}");
            false
        }
    }
}
