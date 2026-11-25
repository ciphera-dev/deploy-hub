//! Etat local de l'agent (SQLite) : survit a un redemarrage.

mod common;

use ciphera_agent::docker::ContainerId;
use ciphera_agent::state::{DeploymentRecord, StateDb};
use common::TempPath;
use proto::{ImageRef, RunningDeployment};
use shared::ids::{DeploymentId, ServiceId};

fn deployment(image: &str, healthy: bool) -> RunningDeployment {
    RunningDeployment {
        deployment_id: DeploymentId::new(),
        service_id: ServiceId::new(),
        image: ImageRef(image.into()),
        healthy,
        serving: false,
        runs_as_root: false,
    }
}

#[test]
fn state_survives_restart() {
    let path = TempPath::new("ciphera-agent-state");
    let web = deployment("ghcr.io/acme/web:1", true);
    let worker = deployment("ghcr.io/acme/worker:3", false);
    {
        let db = StateDb::open(&path.0).unwrap();
        db.put_deployment(&web, &ContainerId("c-web".into()))
            .unwrap();
        db.put_deployment(&worker, &ContainerId("c-worker".into()))
            .unwrap();
        db.set_healthy(worker.deployment_id, true).unwrap();
        db.record_image(web.service_id, &ImageRef("ghcr.io/acme/web:0".into()))
            .unwrap();
        db.record_image(web.service_id, &web.image).unwrap();
    }

    // Nouveau processus : meme fichier.
    let db = StateDb::open(&path.0).unwrap();
    let mut running = db.running().unwrap();
    running.sort_by_key(|d| d.image.0.clone());
    let worker_now = RunningDeployment {
        healthy: true,
        ..worker.clone()
    };
    assert_eq!(running, vec![web.clone(), worker_now]);
    let records = db.deployments().unwrap();
    let web_record = records
        .iter()
        .find(|r| r.deployment.deployment_id == web.deployment_id)
        .unwrap();
    assert_eq!(web_record.container, ContainerId("c-web".into()));
    // Plus recente d'abord.
    assert_eq!(
        db.images(web.service_id).unwrap(),
        vec![web.image.clone(), ImageRef("ghcr.io/acme/web:0".into())]
    );
}

#[test]
fn removed_deployment_stays_removed_after_restart() {
    let path = TempPath::new("ciphera-agent-state");
    let web = deployment("ghcr.io/acme/web:1", true);
    {
        let db = StateDb::open(&path.0).unwrap();
        db.put_deployment(&web, &ContainerId("c-web".into()))
            .unwrap();
        assert!(db.remove_deployment(web.deployment_id).unwrap());
        assert!(!db.remove_deployment(web.deployment_id).unwrap());
    }
    let db = StateDb::open(&path.0).unwrap();
    assert!(db.running().unwrap().is_empty());
}

#[test]
fn put_replaces_the_same_deployment() {
    let path = TempPath::new("ciphera-agent-state");
    let db = StateDb::open(&path.0).unwrap();
    let web = deployment("ghcr.io/acme/web:1", false);
    db.put_deployment(&web, &ContainerId("old".into())).unwrap();
    db.put_deployment(&web, &ContainerId("new".into())).unwrap();
    let records = db.deployments().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].container, ContainerId("new".into()));
}

#[cfg(unix)]
#[test]
fn state_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let path = TempPath::new("ciphera-agent-state");
    let _db = StateDb::open(&path.0).unwrap();
    let mode = std::fs::metadata(&path.0).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "{mode:o}");
}

fn record(service: ServiceId, container: &str, port: Option<u16>) -> DeploymentRecord {
    DeploymentRecord {
        deployment: RunningDeployment {
            service_id: service,
            ..deployment("ciphera.invalid/web-production:x", true)
        },
        container: ContainerId(container.into()),
        port,
        project: None,
    }
}

#[test]
fn serving_is_exclusive_per_service_and_survives_restart() {
    let path = TempPath::new("ciphera-agent-state");
    let web = ServiceId::new();
    let other = ServiceId::new();
    let v1 = record(web, "c1", Some(8080));
    let v2 = record(web, "c2", Some(8080));
    let o1 = record(other, "c3", None);
    {
        let db = StateDb::open(&path.0).unwrap();
        for r in [&v1, &v2, &o1] {
            db.put_record(r).unwrap();
        }
        assert_eq!(db.set_serving(v1.deployment.deployment_id).unwrap(), None);
        assert_eq!(db.set_serving(o1.deployment.deployment_id).unwrap(), None);
        // Bascule : l'ancien deploiement du service est rendu.
        assert_eq!(
            db.set_serving(v2.deployment.deployment_id).unwrap(),
            Some(v1.deployment.deployment_id)
        );
        // Idempotent.
        assert_eq!(db.set_serving(v2.deployment.deployment_id).unwrap(), None);
        assert!(db.set_serving(DeploymentId::new()).is_err());
    }
    let db = StateDb::open(&path.0).unwrap();
    let mut serving: Vec<_> = db
        .serving()
        .unwrap()
        .into_iter()
        .map(|r| (r.deployment.deployment_id, r.container.0, r.port))
        .collect();
    serving.sort_by_key(|s| s.1.clone());
    assert_eq!(
        serving,
        vec![
            (v2.deployment.deployment_id, "c2".to_string(), Some(8080)),
            (o1.deployment.deployment_id, "c3".to_string(), None),
        ]
    );
    let r1 = db.record(v1.deployment.deployment_id).unwrap().unwrap();
    assert!(!r1.deployment.serving);
    assert!(
        db.record(v2.deployment.deployment_id)
            .unwrap()
            .unwrap()
            .deployment
            .serving
    );
    db.clear_serving(v2.deployment.deployment_id).unwrap();
    assert_eq!(db.serving().unwrap().len(), 1);
}

#[test]
fn compose_bundles_are_kept_per_deployment() {
    let path = TempPath::new("ciphera-agent-state");
    let db = StateDb::open(&path.0).unwrap();
    let id = DeploymentId::new();
    assert_eq!(db.bundle(id).unwrap(), None);
    db.put_bundle(id, "{\"services\":{}}").unwrap();
    assert_eq!(db.bundle(id).unwrap().as_deref(), Some("{\"services\":{}}"));
    assert!(db.remove_bundle(id).unwrap());
    assert_eq!(db.bundle(id).unwrap(), None);
}

/// Base creee par l'agent de la Task 19 (schema 1) : migree sans perte.
#[test]
fn version_1_database_is_migrated() {
    let path = TempPath::new("ciphera-agent-state");
    let web = deployment("ghcr.io/acme/web:1", true);
    {
        let conn = rusqlite::Connection::open(&path.0).unwrap();
        conn.execute_batch(
            "CREATE TABLE deployments (
                deployment_id TEXT PRIMARY KEY, service_id TEXT NOT NULL,
                image TEXT NOT NULL, container_id TEXT NOT NULL,
                healthy INTEGER NOT NULL DEFAULT 0 CHECK (healthy IN (0, 1)),
                updated_at INTEGER NOT NULL);
             CREATE TABLE images (service_id TEXT NOT NULL, image TEXT NOT NULL,
                pulled_at INTEGER NOT NULL, PRIMARY KEY (service_id, image));
             PRAGMA user_version = 1;",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO deployments VALUES (?1, ?2, ?3, 'c-web', 1, 0)",
            rusqlite::params![
                web.deployment_id.to_string(),
                web.service_id.to_string(),
                web.image.0
            ],
        )
        .unwrap();
    }
    let db = StateDb::open(&path.0).unwrap();
    assert_eq!(db.running().unwrap(), vec![web.clone()]);
    assert_eq!(db.set_serving(web.deployment_id).unwrap(), None);
    drop(db);
    let db = StateDb::open(&path.0).unwrap();
    assert_eq!(db.serving().unwrap().len(), 1);
}

#[test]
fn terminal_outcomes_survive_restart_with_bounded_retention() {
    use proto::{AgentState, Stage, StageStatus};
    let path = TempPath::new("ciphera-agent-outcomes");
    let (ok, failed, old) = (
        DeploymentId::new(),
        DeploymentId::new(),
        DeploymentId::new(),
    );
    {
        let db = StateDb::open(&path.0).unwrap();
        db.put_outcome(ok, Stage::Switch, &StageStatus::Succeeded)
            .unwrap();
        let long = "e".repeat(AgentState::MAX_OUTCOME_MESSAGE * 4);
        db.put_outcome(
            failed,
            Stage::Healthcheck,
            &StageStatus::Failed { message: long },
        )
        .unwrap();
        // Plus vieux que la garde : oublie.
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - AgentState::OUTCOME_RETENTION_SECS as i64
            - 60;
        db.put_outcome_at(old, Stage::Switch, &StageStatus::Succeeded, at)
            .unwrap();
    }
    let db = StateDb::open(&path.0).unwrap();
    assert_eq!(
        db.outcome(ok).unwrap().unwrap().status,
        StageStatus::Succeeded
    );
    match db.outcome(failed).unwrap().unwrap().status {
        StageStatus::Failed { message } => {
            assert_eq!(message.len(), AgentState::MAX_OUTCOME_MESSAGE)
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(db.outcome(old).unwrap(), None);
    // Au plus MAX_OUTCOMES issues, les plus recentes gardees.
    for _ in 0..AgentState::MAX_OUTCOMES {
        db.put_outcome(DeploymentId::new(), Stage::Image, &StageStatus::Succeeded)
            .unwrap();
    }
    let all = db.outcomes().unwrap();
    assert_eq!(all.len(), AgentState::MAX_OUTCOMES);
    assert!(all.iter().all(|o| o.deployment_id != ok));
    db.remove_outcome(all[0].deployment_id).unwrap();
    assert_eq!(db.outcome(all[0].deployment_id).unwrap(), None);
}

/// Base de la version 6 (services Compose nouveaux notes) : migree en v7
/// sans perte, table des services mis a jour vide.
#[test]
fn version_6_database_is_migrated() {
    use shared::ids::ProjectId;
    let path = TempPath::new("ciphera-agent-state-v6");
    let (id, project) = (DeploymentId::new(), ProjectId::new());
    {
        let db = StateDb::open(&path.0).unwrap();
        db.put_compose_created(id, project, "shop-production", "mailer")
            .unwrap();
    }
    {
        // Retour au schema 6 : sans la table de la version 7.
        let conn = rusqlite::Connection::open(&path.0).unwrap();
        conn.execute_batch("DROP TABLE compose_updated; PRAGMA user_version = 6;")
            .unwrap();
    }
    let db = StateDb::open(&path.0).unwrap();
    assert_eq!(
        db.compose_created().unwrap(),
        vec![(
            id,
            project,
            "shop-production".to_string(),
            "mailer".to_string()
        )]
    );
    assert!(db.compose_updated().unwrap().is_empty());
    db.put_compose_updated(id, "shop-production", "cache")
        .unwrap();
    assert_eq!(db.compose_updated().unwrap().len(), 1);
}
