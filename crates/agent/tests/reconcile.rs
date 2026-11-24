//! Reconciliation : l'etat envoye a chaque (re)connexion vient de l'etat
//! local, avec la sante relue dans Docker.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ciphera_agent::docker::{ContainerId, Health};
use ciphera_agent::ops::Operations;
use ciphera_agent::reconcile;
use ciphera_agent::state::StateDb;
use common::{FakeDocker, TempPath};
use proto::{ImageRef, RunningDeployment};
use shared::ids::{DeploymentId, ServiceId};

fn deployment(image: &str, healthy: bool) -> RunningDeployment {
    RunningDeployment {
        deployment_id: DeploymentId::new(),
        service_id: ServiceId::new(),
        image: ImageRef(image.into()),
        healthy,
        serving: false,
    }
}

fn sorted(mut v: Vec<RunningDeployment>) -> Vec<RunningDeployment> {
    v.sort_by_key(|d| d.image.0.clone());
    v
}

#[tokio::test]
async fn reconcile_reports_running_deployments() {
    let path = TempPath::new("ciphera-agent-reconcile");
    let db = StateDb::open(&path.0).unwrap();
    let docker = Arc::new(FakeDocker::default());

    let healthy = deployment("a/healthy:1", false);
    let running = deployment("b/running:1", false);
    let unhealthy = deployment("c/unhealthy:1", true);
    let gone = deployment("d/gone:1", true);
    for (d, c, h) in [
        (&healthy, "c1", Some(Health::Healthy)),
        (&running, "c2", Some(Health::Running)),
        (&unhealthy, "c3", Some(Health::Unhealthy)),
        (&gone, "c4", None),
    ] {
        db.put_deployment(d, &ContainerId(c.into())).unwrap();
        if let Some(h) = h {
            docker.set_health(c, h);
        }
    }

    let ops = Operations::default();
    let state_fn = reconcile::state_fn(docker.clone(), db.clone(), ops.clone());
    let state = state_fn().await;
    assert!(state.in_progress.is_empty());
    let state = state.deployments;
    let expect = |d: &RunningDeployment, healthy| RunningDeployment {
        healthy,
        ..d.clone()
    };
    assert_eq!(
        sorted(state),
        vec![
            expect(&healthy, true),
            expect(&running, true),
            expect(&unhealthy, false),
        ]
    );
    // Sante enregistree, conteneur disparu retire de l'etat local.
    assert_eq!(
        sorted(db.running().unwrap()),
        vec![
            expect(&healthy, true),
            expect(&running, true),
            expect(&unhealthy, false),
        ]
    );

    // Appele a chaque reconnexion : relit Docker a chaque fois.
    docker.set_health("c3", Health::Healthy);
    let again = state_fn().await.deployments;
    assert!(again.iter().all(|d| d.healthy), "{again:?}");
}

#[tokio::test(start_paused = true)]
async fn unresponsive_docker_falls_back_to_the_local_state() {
    let path = TempPath::new("ciphera-agent-reconcile");
    let db = StateDb::open(&path.0).unwrap();
    let docker = Arc::new(FakeDocker::default());
    let web = deployment("a/web:1", true);
    db.put_deployment(&web, &ContainerId("c1".into())).unwrap();
    docker.hang.store(true, Ordering::SeqCst);

    let started = tokio::time::Instant::now();
    let state = reconcile::state_fn(docker, db.clone(), Operations::default())().await;
    assert_eq!(state.deployments, vec![web]);
    assert!(started.elapsed() <= reconcile::DOCKER_TIMEOUT + Duration::from_secs(1));
}
