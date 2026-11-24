//! Metriques du noeud mesurees toutes les 30 s dans un emplacement « derniere
//! valeur » (horloge tokio en pause : aucun temps reel ecoule).

mod common;

use std::sync::Arc;
use std::time::Duration;

use ciphera_agent::docker::{ContainerId, Health};
use ciphera_agent::metrics;
use ciphera_agent::state::StateDb;
use common::{FakeDocker, TempPath};
use proto::{ContainerMetrics, ImageRef, NodeMetrics, RunningDeployment};
use shared::ids::{DeploymentId, ServiceId};
use tokio::sync::watch;

/// Etat local vide (fichier temporaire supprime avec la garde).
fn db() -> (TempPath, StateDb) {
    let path = TempPath::new("ciphera-agent-metrics");
    let db = StateDb::open(&path.0).unwrap();
    (path, db)
}

/// Nouvelle mesure depuis la derniere lecture ?
fn fresh(rx: &mut watch::Receiver<Option<NodeMetrics>>) -> bool {
    if !rx.has_changed().unwrap() {
        return false;
    }
    assert_eq!(*rx.borrow_and_update(), Some(common::metrics()));
    true
}

#[tokio::test(start_paused = true)]
async fn metrics_emitted_every_30s() {
    assert_eq!(metrics::INTERVAL, Duration::from_secs(30));
    let (tx, mut rx) = metrics::slot();
    let (_path, db) = db();
    let task = tokio::spawn(metrics::run(Arc::new(FakeDocker::default()), db, tx));

    tokio::time::sleep(Duration::from_secs(29)).await;
    assert!(!fresh(&mut rx), "rien avant 30 s");
    tokio::time::sleep(Duration::from_secs(2)).await; // 31 s
    assert!(fresh(&mut rx));
    tokio::time::sleep(Duration::from_secs(28)).await; // 59 s
    assert!(!fresh(&mut rx));
    tokio::time::sleep(Duration::from_secs(2)).await; // 61 s
    assert!(fresh(&mut rx));
    for _ in 0..18 {
        tokio::time::sleep(Duration::from_secs(30)).await;
        assert!(fresh(&mut rx), "une mesure par periode");
    }
    task.abort();
}

/// Hors connexion, rien ne s'accumule : apres N periodes sans lecture, une
/// seule valeur attend (la plus recente).
#[tokio::test(start_paused = true)]
async fn missed_ticks_collapse_into_the_latest_sample() {
    let (tx, mut rx) = metrics::slot();
    let (_path, db) = db();
    let task = tokio::spawn(metrics::run(Arc::new(FakeDocker::default()), db, tx));
    tokio::time::sleep(Duration::from_secs(10 * 30 + 1)).await;
    assert!(fresh(&mut rx), "la derniere mesure");
    assert!(!fresh(&mut rx), "une seule, pas dix");
    task.abort();
}

#[tokio::test(start_paused = true)]
async fn sample_once_replaces_the_latest_value() {
    let (tx, mut rx) = metrics::slot();
    assert_eq!(*rx.borrow(), None, "aucune mesure au depart");
    let (_path, db) = db();
    metrics::sample_once(&FakeDocker::default(), &db, &tx).await;
    assert!(fresh(&mut rx));
}

/// Chaque mesure porte le conteneur servi de chaque service : CPU et
/// memoire s'il tourne, `running: false` s'il est arrete ou disparu hors du
/// panneau. Les deploiements non servis ne sont pas mesures.
#[tokio::test(start_paused = true)]
async fn metrics_include_each_serving_container() {
    let (_path, db) = db();
    let docker = FakeDocker::default();
    let mut serving = Vec::new();
    for (container, health) in [
        ("up", Some(Health::Healthy)),
        ("down", Some(Health::Stopped)),
        ("gone", None),
        ("old", Some(Health::Running)),
    ] {
        let d = RunningDeployment {
            deployment_id: DeploymentId::new(),
            service_id: ServiceId::new(),
            image: ImageRef("ghcr.io/acme/app:1".into()),
            healthy: true,
            serving: false,
            runs_as_root: false,
        };
        db.put_deployment(&d, &ContainerId(container.into()))
            .unwrap();
        if container != "old" {
            db.set_serving(d.deployment_id).unwrap();
            serving.push((container, d));
        }
        if let Some(h) = health {
            docker.set_health(container, h);
        }
    }
    docker.set_usage("up", 42.5, 128 << 20);

    let (tx, rx) = metrics::slot();
    metrics::sample_once(&docker, &db, &tx).await;
    let sample = rx.borrow().clone().expect("mesure");
    let mut got = sample.containers.clone();
    got.sort_by_key(|c| c.deployment_id.to_string());
    let mut want: Vec<ContainerMetrics> = serving
        .iter()
        .map(|(container, d)| ContainerMetrics {
            deployment_id: d.deployment_id,
            service_id: d.service_id,
            cpu_percent: if *container == "up" { 42.5 } else { 0.0 },
            mem_used_bytes: if *container == "up" { 128 << 20 } else { 0 },
            running: *container == "up",
        })
        .collect();
    want.sort_by_key(|c| c.deployment_id.to_string());
    assert_eq!(got, want);
    assert_eq!(
        NodeMetrics {
            containers: Vec::new(),
            ..sample
        },
        common::metrics()
    );
}

#[tokio::test(start_paused = true)]
async fn metrics_stop_when_nobody_reads_them() {
    let (tx, rx) = metrics::slot();
    let (_path, db) = db();
    let task = tokio::spawn(metrics::run(Arc::new(FakeDocker::default()), db, tx));
    drop(rx);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(task.is_finished());
}

/// Docker qui ne repond plus : la mesure abandonne apres
/// `metrics::DOCKER_TIMEOUT` (aucune valeur) au lieu d'attendre sans fin,
/// et la boucle de mesure continue.
#[tokio::test(start_paused = true)]
async fn hung_docker_does_not_block_sampling() {
    let (tx, rx) = metrics::slot();
    let (_path, db) = db();
    let docker = FakeDocker::default();
    docker.hang.store(true, std::sync::atomic::Ordering::SeqCst);
    let sampled = tokio::time::timeout(
        metrics::DOCKER_TIMEOUT * 4,
        metrics::sample_once(&docker, &db, &tx),
    )
    .await;
    assert!(sampled.is_ok(), "mesure abandonnee apres le delai");
    assert_eq!(*rx.borrow(), None, "aucune valeur inventee");
}
