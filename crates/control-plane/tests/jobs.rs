//! File de taches PostgreSQL : reprise des taches, bail, deduplication,
//! boucle de travail.

mod common;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use control_plane::jobs::worker::{self, HandlerError, Handlers, WorkerConfig};
use control_plane::jobs::{self, Failure, JobKind};
use control_plane::state::Clock;
use serde_json::json;
use shared::ids::JobId;
use sqlx::{Connection, PgPool};
use tokio::sync::watch;

/// Horloge reelle decalee d'un nombre de secondes reglable.
fn shifted_clock() -> (Clock, Arc<AtomicU64>) {
    let offset = Arc::new(AtomicU64::new(0));
    let o = offset.clone();
    let clock =
        Clock::from_fn(move || SystemTime::now() + Duration::from_secs(o.load(Ordering::SeqCst)));
    (clock, offset)
}

type Row = (String, i32, Option<String>, Option<String>);

async fn row(db: &PgPool, id: JobId) -> Row {
    sqlx::query_as("SELECT status, attempts, last_error, locked_by FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

async fn set(db: &PgPool, id: JobId, sql: &'static str) {
    sqlx::query(sql).bind(id).execute(db).await.unwrap();
}

#[tokio::test]
async fn two_workers_never_claim_same_job() {
    let (db, _guard) = common::database().await;
    const N: usize = 24;
    for i in 0..N {
        jobs::enqueue(&db, JobKind::Cleanup, json!({ "n": i }), None)
            .await
            .unwrap();
    }
    let clock = Clock::system();
    let mut tasks = Vec::new();
    for w in 0..8 {
        let (db, clock) = (db.clone(), clock.clone());
        tasks.push(tokio::spawn(async move {
            let mut got = Vec::new();
            while let Some(job) = jobs::claim(&db, &clock, &format!("w{w}")).await.unwrap() {
                got.push(job.id);
            }
            got
        }));
    }
    let mut all = Vec::new();
    for t in tasks {
        all.extend(t.await.unwrap());
    }
    let unique: HashSet<_> = all.iter().collect();
    assert_eq!(all.len(), N, "chaque tache est prise");
    assert_eq!(unique.len(), N, "aucune tache prise deux fois");
}

#[tokio::test]
async fn crashed_worker_job_reclaimed_after_lease_5min() {
    let (db, _guard) = common::database().await;
    let (clock, offset) = shifted_clock();
    let id = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();

    let first = jobs::claim(&db, &clock, "w1").await.unwrap().unwrap();
    assert_eq!(first.id, id);
    assert_eq!(first.attempts, 1);

    // Bail encore valide : personne d'autre ne la prend.
    offset.store(299, Ordering::SeqCst);
    assert!(jobs::claim(&db, &clock, "w2").await.unwrap().is_none());

    // Bail echu : reprise par un autre travailleur, tentative comptee.
    offset.store(301, Ordering::SeqCst);
    let second = jobs::claim(&db, &clock, "w2").await.unwrap().unwrap();
    assert_eq!(second.id, id);
    assert_eq!(second.attempts, 2);

    // Le travailleur tombe ne peut ni terminer, ni echouer, ni prolonger.
    assert!(!jobs::complete(&db, &clock, id, "w1").await.unwrap());
    assert!(!jobs::heartbeat(&db, &clock, id, "w1").await.unwrap());
    assert!(
        !jobs::fail(&db, &clock, id, "w1", "x", Failure::Terminal)
            .await
            .unwrap()
    );
    assert_eq!(row(&db, id).await.0, "running");
    assert!(jobs::complete(&db, &clock, id, "w2").await.unwrap());
    assert_eq!(row(&db, id).await.0, "done");
}

#[tokio::test]
async fn heartbeat_extends_the_lease() {
    let (db, _guard) = common::database().await;
    let (clock, offset) = shifted_clock();
    let id = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();
    jobs::claim(&db, &clock, "w1").await.unwrap().unwrap();
    offset.store(200, Ordering::SeqCst);
    assert!(jobs::heartbeat(&db, &clock, id, "w1").await.unwrap());
    // 400 s : au-dela de l'ancienne echeance (300 s), mais bail prolongee.
    offset.store(400, Ordering::SeqCst);
    assert!(jobs::claim(&db, &clock, "w2").await.unwrap().is_none());
    assert!(jobs::complete(&db, &clock, id, "w1").await.unwrap());
}

#[tokio::test]
async fn dedupe_key_prevents_duplicates() {
    let (db, _guard) = common::database().await;
    let clock = Clock::system();
    let key = Some("deploy:svc-1".to_string());
    let a = jobs::enqueue(&db, JobKind::Deploy, json!({"service": "1"}), key.clone())
        .await
        .unwrap();
    let b = jobs::enqueue(&db, JobKind::Deploy, json!({"service": "1"}), key.clone())
        .await
        .unwrap();
    assert_eq!(a, b, "la tache active existante est renvoyee");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // Encore active (en cours) : toujours dedupliquee.
    jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    assert_eq!(
        jobs::enqueue(&db, JobKind::Deploy, json!({}), key.clone())
            .await
            .unwrap(),
        a
    );

    // Terminale : la cle est libre.
    assert!(jobs::complete(&db, &clock, a, "w").await.unwrap());
    let c = jobs::enqueue(&db, JobKind::Deploy, json!({}), key.clone())
        .await
        .unwrap();
    assert_ne!(a, c);

    // Sans cle : jamais dedupliquee.
    let d = jobs::enqueue(&db, JobKind::Cleanup, json!({}), None)
        .await
        .unwrap();
    let e = jobs::enqueue(&db, JobKind::Cleanup, json!({}), None)
        .await
        .unwrap();
    assert_ne!(d, e);
}

#[tokio::test]
async fn concurrent_enqueue_with_same_key_yields_one_job() {
    let (db, _guard) = common::database().await;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let db = db.clone();
        tasks.push(tokio::spawn(async move {
            jobs::enqueue(&db, JobKind::Deploy, json!({}), Some("k".into()))
                .await
                .unwrap()
        }));
    }
    let mut ids = HashSet::new();
    for t in tasks {
        ids.insert(t.await.unwrap());
    }
    assert_eq!(ids.len(), 1);
}

#[tokio::test]
async fn fail_retries_with_backoff_then_goes_dead_at_max_attempts() {
    let (db, _guard) = common::database().await;
    let (clock, offset) = shifted_clock();
    let id = jobs::enqueue(&db, JobKind::Update, json!({}), None)
        .await
        .unwrap();
    set(&db, id, "UPDATE jobs SET max_attempts = 2 WHERE id = $1").await;

    jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    let retry = Failure::Retry(Duration::from_secs(60));
    assert!(
        jobs::fail(&db, &clock, id, "w", "boom 1", retry)
            .await
            .unwrap()
    );
    let (status, attempts, err, _) = row(&db, id).await;
    assert_eq!(
        (status.as_str(), attempts, err.as_deref()),
        ("queued", 1, Some("boom 1"))
    );

    // Pas avant l'echeance du delai.
    offset.store(30, Ordering::SeqCst);
    assert!(jobs::claim(&db, &clock, "w").await.unwrap().is_none());
    offset.store(61, Ordering::SeqCst);
    let again = jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    assert_eq!(again.attempts, 2);

    // Derniere tentative : meme avec reprise demandee, etat terminal `dead`.
    let retry = Failure::Retry(Duration::from_secs(1));
    assert!(
        jobs::fail(&db, &clock, id, "w", "boom 2", retry)
            .await
            .unwrap()
    );
    let (status, _, err, _) = row(&db, id).await;
    assert_eq!((status.as_str(), err.as_deref()), ("dead", Some("boom 2")));
    offset.store(10_000, Ordering::SeqCst);
    assert!(jobs::claim(&db, &clock, "w").await.unwrap().is_none());
}

#[tokio::test]
async fn lease_expiry_on_last_attempt_goes_dead() {
    let (db, _guard) = common::database().await;
    let (clock, offset) = shifted_clock();
    let id = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();
    set(&db, id, "UPDATE jobs SET max_attempts = 1 WHERE id = $1").await;
    jobs::claim(&db, &clock, "w1").await.unwrap().unwrap();
    offset.store(301, Ordering::SeqCst);
    assert!(jobs::claim(&db, &clock, "w2").await.unwrap().is_none());
    let (status, _, err, _) = row(&db, id).await;
    assert_eq!(status, "dead");
    assert!(err.unwrap().contains("bail"));
}

#[tokio::test]
async fn stored_error_is_truncated() {
    let (db, _guard) = common::database().await;
    let clock = Clock::system();
    let id = jobs::enqueue(&db, JobKind::Cleanup, json!({}), None)
        .await
        .unwrap();
    jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    let long = "e".repeat(10_000);
    assert!(
        jobs::fail(&db, &clock, id, "w", &long, Failure::Terminal)
            .await
            .unwrap()
    );
    let err = row(&db, id).await.2.unwrap();
    assert!(
        err.chars().count() <= jobs::MAX_ERROR_CHARS + 1,
        "len = {}",
        err.chars().count()
    );
}

#[tokio::test]
async fn priority_then_run_at_order() {
    let (db, _guard) = common::database().await;
    let clock = Clock::system();
    let low = jobs::enqueue(&db, JobKind::Cleanup, json!({}), None)
        .await
        .unwrap();
    let high = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();
    set(&db, high, "UPDATE jobs SET priority = 10 WHERE id = $1").await;
    assert_eq!(
        jobs::claim(&db, &clock, "w").await.unwrap().unwrap().id,
        high
    );
    assert_eq!(
        jobs::claim(&db, &clock, "w").await.unwrap().unwrap().id,
        low
    );
}

fn quick_config(id: &str) -> WorkerConfig {
    WorkerConfig {
        worker_id: id.to_string(),
        concurrency: 2,
        poll_interval: Duration::from_millis(50),
        shutdown_grace: Duration::from_secs(5),
        listen: true,
        heartbeat: jobs::LEASE / 3,
    }
}

async fn wait_status(db: &PgPool, id: JobId, want: &str) {
    for _ in 0..200 {
        if row(db, id).await.0 == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("la tache n'a pas atteint {want} : {:?}", row(db, id).await);
}

/// Attend qu'une tache ait ete tentee puis remise en file (erreur enregistree) :
/// une tache fraichement creee est deja "queued" sans avoir ete essayee.
async fn wait_requeued_after_attempt(db: &PgPool, id: JobId) {
    for _ in 0..200 {
        let r = row(db, id).await;
        if r.0 == "queued" && r.2.is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "la tache n'a pas ete remise en file : {:?}",
        row(db, id).await
    );
}

#[tokio::test]
async fn worker_runs_handlers_and_fails_unknown_kinds_terminally() {
    let (db, _guard) = common::database().await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = seen.clone();
    let handlers = Handlers::new()
        .register(JobKind::Cleanup, move |job| {
            let s = s.clone();
            async move {
                s.lock().unwrap().push(job.payload.clone());
                Ok(())
            }
        })
        .register(JobKind::Update, |_| async {
            Err(HandlerError::retry("pas encore"))
        });

    let (stop_tx, stop_rx) = watch::channel(false);
    let w = tokio::spawn(worker::run(
        db.clone(),
        Clock::system(),
        handlers,
        quick_config("t"),
        stop_rx,
    ));

    let ok = jobs::enqueue(&db, JobKind::Cleanup, json!({"n": 7}), None)
        .await
        .unwrap();
    let unknown = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();
    let retry = jobs::enqueue(&db, JobKind::Update, json!({}), None)
        .await
        .unwrap();
    wait_status(&db, ok, "done").await;
    wait_status(&db, unknown, "dead").await;
    assert!(row(&db, unknown).await.2.unwrap().contains("deploy"));
    assert_eq!(seen.lock().unwrap().as_slice(), &[json!({"n": 7})]);
    // Echec avec reprise : remise en file avec un delai.
    wait_requeued_after_attempt(&db, retry).await;
    assert_eq!(row(&db, retry).await.2.as_deref(), Some("pas encore"));

    stop_tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(10), w)
        .await
        .expect("arret propre")
        .unwrap();
}

#[tokio::test]
async fn worker_shutdown_waits_for_running_job_and_respects_concurrency() {
    let (db, _guard) = common::database().await;
    let running = Arc::new(AtomicU64::new(0));
    let peak = Arc::new(AtomicU64::new(0));
    let (r, p) = (running.clone(), peak.clone());
    let handlers = Handlers::new().register(JobKind::MetricsRollup, move |_| {
        let (r, p) = (r.clone(), p.clone());
        async move {
            let now = r.fetch_add(1, Ordering::SeqCst) + 1;
            p.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(300)).await;
            r.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    });
    for _ in 0..6 {
        jobs::enqueue(&db, JobKind::MetricsRollup, json!({}), None)
            .await
            .unwrap();
    }
    let (stop_tx, stop_rx) = watch::channel(false);
    let w = tokio::spawn(worker::run(
        db.clone(),
        Clock::system(),
        handlers,
        quick_config("t"),
        stop_rx,
    ));
    // Laisse demarrer, puis arret : les taches en cours finissent.
    tokio::time::sleep(Duration::from_millis(150)).await;
    stop_tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(10), w)
        .await
        .expect("arret propre")
        .unwrap();
    assert!(peak.load(Ordering::SeqCst) <= 2, "concurrence bornee a 2");
    assert_eq!(running.load(Ordering::SeqCst), 0, "plus rien en cours");
    let in_flight: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE status = 'running'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(in_flight, 0, "aucune tache laissee en cours");
    let done: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE status = 'done'")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(
        (1..6).contains(&done),
        "arret avant la fin de la file : {done}"
    );
}

/// Remet a faux un indicateur quand le gestionnaire est abandonne.
struct Alive(Arc<std::sync::atomic::AtomicBool>);

impl Drop for Alive {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn lost_lease_aborts_the_handler_and_concludes_nothing() {
    let (db, _guard) = common::database().await;
    let alive = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (a, f) = (alive.clone(), finished.clone());
    let handlers = Handlers::new().register(JobKind::Deploy, move |_| {
        let (a, f) = (a.clone(), f.clone());
        async move {
            a.store(true, Ordering::SeqCst);
            let _guard = Alive(a);
            tokio::time::sleep(Duration::from_secs(5)).await;
            f.store(true, Ordering::SeqCst);
            Ok(())
        }
    });
    let id = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();
    let mut cfg = quick_config("t");
    cfg.heartbeat = Duration::from_millis(100);
    let (stop_tx, stop_rx) = watch::channel(false);
    let w = tokio::spawn(worker::run(
        db.clone(),
        Clock::system(),
        handlers,
        cfg,
        stop_rx,
    ));
    for _ in 0..100 {
        if alive.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(alive.load(Ordering::SeqCst), "gestionnaire demarre");
    // Un autre travailleur a repris la tache (bail echu puis repris).
    set(&db, id, "UPDATE jobs SET locked_by = 'autre' WHERE id = $1").await;
    for _ in 0..100 {
        if !alive.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!alive.load(Ordering::SeqCst), "gestionnaire interrompu");
    assert!(!finished.load(Ordering::SeqCst), "jamais arrive au bout");
    let (status, _, _, locked_by) = row(&db, id).await;
    assert_eq!(
        status, "running",
        "rien n'est conclu par l'ancien detenteur"
    );
    assert_eq!(locked_by.as_deref(), Some("autre"));
    stop_tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(10), w)
        .await
        .expect("arret propre")
        .unwrap();
}

#[tokio::test]
async fn acme_jobs_have_their_own_concurrency_limit() {
    let (db, _guard) = common::database().await;
    // Emissions en tete de file : une seule a la fois, les deploiements
    // passent quand meme.
    let a = jobs::enqueue(&db, JobKind::AcmeIssue, json!({ "n": 1 }), None)
        .await
        .unwrap();
    let b = jobs::enqueue(&db, JobKind::AcmeIssue, json!({ "n": 2 }), None)
        .await
        .unwrap();
    let deploy = jobs::enqueue(&db, JobKind::Deploy, json!({}), None)
        .await
        .unwrap();
    let clock = Clock::system();
    let first = jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    assert_eq!(first.id, a);
    let second = jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    assert_eq!(second.id, deploy, "la seconde emission attend");
    assert!(jobs::claim(&db, &clock, "w").await.unwrap().is_none());
    assert!(jobs::complete(&db, &clock, a, "w").await.unwrap());
    let third = jobs::claim(&db, &clock, "w").await.unwrap().unwrap();
    assert_eq!(third.id, b);
}

/// LISTEN impossible au demarrage (base qui refuse les nouvelles
/// connexions) : le travailleur le signale et reessaie, sans attendre son
/// intervalle d'interrogation (ici une minute).
#[tokio::test]
async fn listen_failure_at_startup_is_retried() {
    let (db, _guard) = common::database().await;
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&db)
        .await
        .unwrap();
    // Base fermee puis rouverte depuis une autre base du serveur.
    let mut admin = sqlx::postgres::PgConnection::connect_with(
        &(*db.connect_options()).clone().database("template1"),
    )
    .await
    .unwrap();
    let toggle = |allow: bool| {
        sqlx::AssertSqlSafe(format!(
            "ALTER DATABASE \"{name}\" ALLOW_CONNECTIONS {allow}"
        ))
    };
    sqlx::query(toggle(false))
        .execute(&mut admin)
        .await
        .unwrap();
    let handlers = Handlers::new().register(JobKind::Cleanup, |_| async { Ok(()) });
    let mut cfg = quick_config("t");
    cfg.poll_interval = Duration::from_secs(60);
    let worker_db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_lazy_with((*db.connect_options()).clone());
    let (stop_tx, stop_rx) = watch::channel(false);
    let w = tokio::spawn(worker::run(
        worker_db,
        Clock::system(),
        handlers,
        cfg,
        stop_rx,
    ));
    // Premier LISTEN (et premiere prise) refuse.
    tokio::time::sleep(Duration::from_millis(500)).await;
    sqlx::query(toggle(true)).execute(&mut admin).await.unwrap();
    drop(admin);
    // Le LISTEN est bien rouvert (pas seulement l'interrogation de repli) :
    // une connexion de cette base l'a pour derniere requete.
    let mut listening = false;
    for _ in 0..100 {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE datname = current_database() AND query ILIKE 'LISTEN%'",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        if n > 0 {
            listening = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(listening, "LISTEN jamais rouvert");
    let id = jobs::enqueue(&db, JobKind::Cleanup, json!({}), None)
        .await
        .unwrap();
    let mut done = false;
    for _ in 0..150 {
        if row(&db, id).await.0 == "done" {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stop_tx.send(true).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), w).await;
    assert!(
        done,
        "tache prise apres le retour de la base : {:?}",
        row(&db, id).await
    );
}
