//! Journaux en direct cote agent : un seul suivi par cible, arrete par
//! `StopLogs`, lignes envoyees au plan de controle avec leur cible.

mod common;

use std::sync::Arc;
use std::time::Duration;

use ciphera_agent::docker::{ContainerId, ContainerLog};
use ciphera_agent::logs::LogFollows;
use ciphera_agent::state::StateDb;
use common::{FakeDocker, TempPath};
use proto::{AgentToControl, ImageRef, LogStream, LogTarget, RunningDeployment};
use shared::ids::{DeploymentId, ServiceId};
use tokio::sync::mpsc;

fn line(text: &str) -> ContainerLog {
    ContainerLog {
        stream: LogStream::Stdout,
        timestamp_ms: 1_700_000_000_000,
        line: text.to_string(),
    }
}

struct Env {
    _path: TempPath,
    docker: Arc<FakeDocker>,
    db: StateDb,
    follows: LogFollows<FakeDocker>,
    service: ServiceId,
    deployment: DeploymentId,
}

impl Env {
    fn new() -> Self {
        let path = TempPath::new("ciphera-agent-logs");
        let db = StateDb::open(&path.0).unwrap();
        let docker = Arc::new(FakeDocker::default());
        let service = ServiceId::new();
        let deployment = DeploymentId::new();
        db.put_deployment(
            &RunningDeployment {
                deployment_id: deployment,
                service_id: service,
                image: ImageRef("ciphera.invalid/web-production:1".into()),
                healthy: true,
                serving: false,
                runs_as_root: false,
            },
            &ContainerId("c-web".into()),
        )
        .unwrap();
        db.set_serving(deployment).unwrap();
        let follows = LogFollows::new(docker.clone(), db.clone());
        Self {
            _path: path,
            docker,
            db,
            follows,
            service,
            deployment,
        }
    }

    fn log_calls(&self) -> Vec<String> {
        self.docker
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("logs:"))
            .collect()
    }
}

async fn next_line(rx: &mut mpsc::Receiver<AgentToControl>) -> proto::LogLine {
    match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        Ok(Some(AgentToControl::LogLine(l))) => l,
        other => panic!("ligne attendue, recu {other:?}"),
    }
}

#[tokio::test]
async fn follows_serving_container_once_per_target_until_stop() {
    let env = Env::new();
    let feed = env.docker.feed_logs("c-web");
    let (tx, mut rx) = mpsc::channel(16);
    let target = LogTarget::Service(env.service);

    env.follows.start(target, 500, true, tx.clone());
    env.follows.start(target, 500, true, tx.clone());
    feed.send(line("GET / 200")).unwrap();
    let got = next_line(&mut rx).await;
    assert_eq!(got.target, target);
    assert_eq!(got.line, "GET / 200");
    assert_eq!(got.stream, LogStream::Stdout);
    // Un seul suivi Docker pour deux demandes sur la meme cible.
    assert_eq!(env.log_calls(), vec!["logs:c-web:500:true"]);
    assert_eq!(env.follows.active(), 1);

    env.follows.stop(target);
    assert_eq!(env.follows.active(), 0);
    tokio::task::yield_now().await;
    let _ = feed.send(line("apres l'arret"));
    let late = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(late.is_err(), "aucune ligne apres StopLogs : {late:?}");
}

#[tokio::test]
async fn deployment_target_follows_its_container_and_unknown_target_is_ignored() {
    let env = Env::new();
    let feed = env.docker.feed_logs("c-web");
    let (tx, mut rx) = mpsc::channel(16);

    env.follows
        .start(LogTarget::Service(ServiceId::new()), 10, true, tx.clone());
    assert!(
        env.log_calls().is_empty(),
        "service inconnu : rien a suivre"
    );
    assert_eq!(env.follows.active(), 0);

    let target = LogTarget::Deployment(env.deployment);
    env.follows.start(target, 0, true, tx);
    feed.send(line("pret")).unwrap();
    assert_eq!(next_line(&mut rx).await.target, target);
    // Fin du flux (conteneur arrete) : le suivi est retire.
    drop(feed);
    for _ in 0..50 {
        if env.follows.active() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(env.follows.active(), 0);
}

#[tokio::test]
async fn full_outbox_drops_lines_without_blocking() {
    let env = Env::new();
    let feed = env.docker.feed_logs("c-web");
    // File pleine : les lignes sont abandonnees, le suivi ne bloque pas les
    // autres messages de l'agent.
    let (tx, mut rx) = mpsc::channel(1);
    let target = LogTarget::Service(env.service);
    env.follows.start(target, 0, true, tx);
    for i in 0..20 {
        feed.send(line(&format!("ligne {i}"))).unwrap();
    }
    drop(feed);
    for _ in 0..50 {
        if env.follows.active() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        env.follows.active(),
        0,
        "le flux s'est termine sans attendre"
    );
    assert_eq!(next_line(&mut rx).await.line, "ligne 0");
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn service_follow_moves_to_the_new_serving_container() {
    let env = Env::new();
    let old = env.docker.feed_logs("c-web");
    let (tx, mut rx) = mpsc::channel(16);
    let target = LogTarget::Service(env.service);
    env.follows.start(target, 0, true, tx);
    old.send(line("ancien")).unwrap();
    assert_eq!(next_line(&mut rx).await.line, "ancien");

    // Nouveau deploiement servi, puis arret de l'ancien conteneur : le suivi
    // continue sur le nouveau, sans relire son passe.
    let next = DeploymentId::new();
    env.db
        .put_deployment(
            &RunningDeployment {
                deployment_id: next,
                service_id: env.service,
                image: ImageRef("ciphera.invalid/web-production:2".into()),
                healthy: true,
                serving: false,
                runs_as_root: false,
            },
            &ContainerId("c-web-2".into()),
        )
        .unwrap();
    env.db.set_serving(next).unwrap();
    let new = env.docker.feed_logs("c-web-2");
    drop(old);
    new.send(line("nouveau")).unwrap();
    let got = next_line(&mut rx).await;
    assert_eq!((got.target, got.line.as_str()), (target, "nouveau"));
    assert_eq!(
        env.log_calls(),
        vec!["logs:c-web:0:true", "logs:c-web-2:0:true"]
    );
    assert_eq!(env.follows.active(), 1);
    env.follows.stop(target);
}
