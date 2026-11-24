//! Un buildkitd par projet (`buildkit::BuildkitManager`), sans Docker :
//! conteneur propre a chaque projet, un seul en marche a la fois, arret apres
//! inactivite, suppression avec le projet ou quand l'etat est perime, nettoyage
//! au demarrage. Les dossiers du projet (conteneur jetable en production) sont
//! simules.

mod common;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use ciphera_agent::buildkit::{BuildkitError, BuildkitManager, ProjectFs};
use ciphera_agent::docker::{DockerOps, ROLE_LABEL};
use ciphera_agent::infra::{
    self, BUILDKIT_PROJECT_LABEL, BUILDKITD_ROLE, BuildkitAccount, InfraConfig, SubIds,
};
use common::FakeDocker;
use shared::ids::{DeploymentId, ProjectId};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ciphera-agent-bkm-{}-{}",
            std::process::id(),
            DeploymentId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cfg(root: &std::path::Path) -> InfraConfig {
    InfraConfig {
        postgres_secret_dir: root.join("secret"),
        postgres_socket_dir: root.join("pg"),
        postgres_user: (991, 991),
        buildkit_socket_dir: root.join("sock"),
        buildkit_apparmor_profile: None,
        buildkit_account: Some(BuildkitAccount {
            user: (995, 994),
            subuids: SubIds {
                start: 165536,
                count: 65536,
            },
            subgids: SubIds {
                start: 231072,
                count: 65536,
            },
        }),
        buildkit_state_dir: root.join("state"),
        buildkit_identity_dir: root.join("identity"),
        buildkit_usage_dir: root.join("usage"),
        buildkit_resources: infra::BuildkitResources::compute(16 << 30, 8, 200 << 30),
    }
}

/// Dossiers simules : les appels sont journalises, rien n'est cree sur le
/// disque (hors `remove`, qui ne touche a rien).
#[derive(Clone, Default)]
struct FakeFs {
    calls: Arc<Mutex<Vec<String>>>,
    fail_create: Arc<Mutex<bool>>,
}

impl FakeFs {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl ProjectFs for FakeFs {
    async fn create(&self, project: ProjectId) -> Result<(), BuildkitError> {
        if *self.fail_create.lock().unwrap() {
            return Err(BuildkitError::Fs("refuse".into()));
        }
        self.calls.lock().unwrap().push(format!("create:{project}"));
        Ok(())
    }

    async fn remove(&self, project: ProjectId) -> Result<(), BuildkitError> {
        self.calls.lock().unwrap().push(format!("remove:{project}"));
        Ok(())
    }

    async fn remove_legacy_state(&self) -> Result<(), BuildkitError> {
        self.calls.lock().unwrap().push("legacy-state".into());
        Ok(())
    }
}

struct Fixture {
    _scratch: Scratch,
    cfg: InfraConfig,
    docker: Arc<FakeDocker>,
    fs: FakeFs,
    manager: BuildkitManager<FakeDocker, FakeFs>,
}

fn fixture() -> Fixture {
    let scratch = Scratch::new();
    let cfg = cfg(&scratch.0);
    let docker = Arc::new(FakeDocker::default());
    let fs = FakeFs::default();
    let manager = BuildkitManager::new(docker.clone(), fs.clone(), cfg.clone())
        .with_readiness(Duration::from_secs(1), |_| true);
    Fixture {
        _scratch: scratch,
        cfg,
        docker,
        fs,
        manager,
    }
}

impl Fixture {
    fn running(&self, project: ProjectId) -> Option<bool> {
        let name = infra::buildkit_container_name(project);
        self.docker
            .containers
            .lock()
            .unwrap()
            .values()
            .find(|c| c.name == name)
            .map(|c| c.running)
    }
}

#[tokio::test]
async fn a_project_gets_its_own_buildkitd_and_paths() {
    let f = fixture();
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let lease_a = f.manager.ensure(a).await.unwrap();
    assert_eq!(
        lease_a.socket(),
        Some(f.cfg.buildkit_project_socket(a).as_path())
    );
    // Conteneur du projet : nom, etiquettes, etat et socket propres.
    let runs = f.docker.runs.lock().unwrap().clone();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].name(), infra::buildkit_container_name(a));
    assert!(
        runs[0]
            .labels()
            .contains(&(BUILDKIT_PROJECT_LABEL.to_string(), a.to_string()))
    );
    assert!(
        runs[0]
            .labels()
            .contains(&(ROLE_LABEL.to_string(), BUILDKITD_ROLE.to_string()))
    );
    drop(lease_a);
    let lease_b = f.manager.ensure(b).await.unwrap();
    assert_ne!(
        lease_b.socket().unwrap(),
        f.cfg.buildkit_project_socket(a).as_path()
    );
    let runs = f.docker.runs.lock().unwrap().clone();
    assert_ne!(runs[0].name(), runs[1].name());
    // Dossiers crees une fois par projet.
    assert_eq!(
        f.fs.calls()
            .iter()
            .filter(|c| c.starts_with("create:"))
            .count(),
        2
    );
    drop(lease_b);
    drop(f.manager.ensure(a).await.unwrap());
    assert_eq!(
        f.fs.calls()
            .iter()
            .filter(|c| c.starts_with("create:"))
            .count(),
        2,
        "dossiers deja crees"
    );
}

#[tokio::test]
async fn only_one_buildkitd_runs_at_a_time_and_a_busy_one_is_kept() {
    let f = fixture();
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let lease_a = f.manager.ensure(a).await.unwrap();
    assert_eq!(f.running(a), Some(true));
    // A sert un build : B ne le remplace pas.
    assert!(matches!(
        f.manager.ensure(b).await,
        Err(BuildkitError::Busy(_))
    ));
    assert_eq!(f.running(a), Some(true));
    assert_eq!(f.running(b), None);
    drop(lease_a);
    // A libere : le buildkitd de A est arrete avant celui de B.
    let _lease_b = f.manager.ensure(b).await.unwrap();
    assert_eq!(f.running(a), Some(false));
    assert_eq!(f.running(b), Some(true));
    let calls = f.docker.calls();
    let stop_a = calls
        .iter()
        .position(|c| c == &format!("stop:id-{}", infra::buildkit_container_name(a)))
        .expect("arret de A");
    let run_b = calls
        .iter()
        .position(|c| c == &format!("run:{}", infra::buildkit_container_name(b)))
        .expect("demarrage de B");
    assert!(stop_a < run_b, "{calls:?}");
}

#[tokio::test]
async fn without_a_build_account_no_buildkitd_starts() {
    let scratch = Scratch::new();
    let mut c = cfg(&scratch.0);
    c.buildkit_account = None;
    let docker = Arc::new(FakeDocker::default());
    let manager = BuildkitManager::new(docker.clone(), FakeFs::default(), c)
        .with_readiness(Duration::from_secs(1), |_| true);
    assert!(!manager.available());
    assert!(manager.ensure(ProjectId::new()).await.is_err());
    assert!(docker.runs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_idle_buildkitd_is_stopped_but_a_busy_one_is_not() {
    let f = fixture();
    let p = ProjectId::new();
    let lease = f.manager.ensure(p).await.unwrap();
    assert_eq!(f.manager.stop_idle(Duration::ZERO).await, 0);
    assert_eq!(f.running(p), Some(true));
    drop(lease);
    // Pas assez ancien.
    assert_eq!(f.manager.stop_idle(Duration::from_secs(600)).await, 0);
    assert_eq!(f.running(p), Some(true));
    assert_eq!(f.manager.stop_idle(Duration::ZERO).await, 1);
    assert_eq!(f.running(p), Some(false));
    // Etat conserve : ni conteneur supprime, ni dossiers supprimes.
    assert!(!f.fs.calls().iter().any(|c| c.starts_with("remove:")));
    assert!(f.docker.calls().iter().all(|c| !c.starts_with("remove:")));
    // Deja arrete : rien de plus.
    assert_eq!(f.manager.stop_idle(Duration::ZERO).await, 0);
    // Redemarre a la demande.
    let _again = f.manager.ensure(p).await.unwrap();
    assert_eq!(f.running(p), Some(true));
}

#[tokio::test]
async fn remove_project_deletes_container_state_and_usage_file() {
    let f = fixture();
    let (gone, kept) = (ProjectId::new(), ProjectId::new());
    drop(f.manager.ensure(gone).await.unwrap());
    drop(f.manager.ensure(kept).await.unwrap());
    let marker = |p: ProjectId| f.cfg.buildkit_usage_dir.join(p.to_string());
    assert!(marker(gone).exists());
    assert!(f.cfg.buildkit_project_config(gone).exists());

    f.manager.remove_project(gone).await.unwrap();
    assert_eq!(f.running(gone), None);
    assert_eq!(f.running(kept), Some(true));
    assert!(f.fs.calls().contains(&format!("remove:{gone}")));
    assert!(!f.fs.calls().contains(&format!("remove:{kept}")));
    assert!(!marker(gone).exists());
    assert!(!f.cfg.buildkit_project_config(gone).exists());
    assert!(marker(kept).exists());
    // Un projet inconnu : sans erreur.
    f.manager.remove_project(ProjectId::new()).await.unwrap();
    // Recree a la demande (dossiers a nouveau crees).
    drop(f.manager.ensure(gone).await.unwrap());
    assert_eq!(
        f.fs.calls()
            .iter()
            .filter(|c| **c == format!("create:{gone}"))
            .count(),
        2
    );
}

#[tokio::test]
async fn stale_state_is_swept_after_thirty_days_only() {
    let f = fixture();
    let (old, fresh, busy) = (ProjectId::new(), ProjectId::new(), ProjectId::new());
    drop(f.manager.ensure(old).await.unwrap());
    drop(f.manager.ensure(fresh).await.unwrap());
    // Un build en cours : son etat n'est jamais balaye.
    let busy_lease = f.manager.ensure(busy).await.unwrap();
    let age = |p: ProjectId, days: u64| {
        let path = f.cfg.buildkit_usage_dir.join(p.to_string());
        let when = SystemTime::now() - Duration::from_secs(days * 24 * 3600);
        std::fs::File::options()
            .append(true)
            .create(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    };
    age(old, 31);
    age(fresh, 29);
    age(busy, 40);
    assert_eq!(
        f.manager
            .sweep_stale(ciphera_agent::buildkit::STALE_AFTER)
            .await,
        1
    );
    assert!(f.fs.calls().contains(&format!("remove:{old}")));
    assert!(!f.fs.calls().contains(&format!("remove:{fresh}")));
    assert!(f.cfg.buildkit_usage_dir.join(fresh.to_string()).exists());
    assert!(!f.cfg.buildkit_usage_dir.join(old.to_string()).exists());
    assert!(!f.fs.calls().contains(&format!("remove:{busy}")));
    drop(busy_lease);
}

#[tokio::test]
async fn startup_cleans_leftovers_once() {
    let f = fixture();
    // Reste d'un arret de l'agent : un buildkitd de projet en marche, un
    // conteneur jetable abandonne, le buildkitd partage d'avant.
    let orphan = ProjectId::new();
    f.docker.add_container(
        "orphan",
        &infra::buildkit_container_name(orphan),
        &[
            (ROLE_LABEL, BUILDKITD_ROLE),
            (BUILDKIT_PROJECT_LABEL, &orphan.to_string()),
        ],
        None,
    );
    f.docker
        .add_container("helper", "x", &[(ROLE_LABEL, "buildkit-helper")], None);
    f.docker
        .add_container("legacy", "ciphera-buildkitd", &[], None);
    let p = ProjectId::new();
    drop(f.manager.ensure(p).await.unwrap());
    assert!(!f.docker.container("orphan").unwrap().running);
    assert!(f.docker.container("helper").is_none());
    assert!(f.docker.container("legacy").is_none());
    // Etat du buildkitd partage supprime une seule fois.
    drop(f.manager.ensure(ProjectId::new()).await.unwrap());
    assert_eq!(
        f.fs.calls().iter().filter(|c| *c == "legacy-state").count(),
        1
    );
    // Apres un redemarrage de l'agent : marque presente, pas de nouvelle suppression.
    let again = BuildkitManager::new(f.docker.clone(), f.fs.clone(), f.cfg.clone())
        .with_readiness(Duration::from_secs(1), |_| true);
    drop(again.ensure(p).await.unwrap());
    assert_eq!(
        f.fs.calls().iter().filter(|c| *c == "legacy-state").count(),
        1
    );
}

#[tokio::test]
async fn garbage_collector_budget_is_shared_between_known_projects() {
    let f = fixture();
    let (a, b) = (ProjectId::new(), ProjectId::new());
    let reserved = |p: ProjectId| {
        std::fs::read_to_string(f.cfg.buildkit_project_config(p)).unwrap_or_default()
    };
    drop(f.manager.ensure(a).await.unwrap());
    assert!(
        reserved(a).contains("reservedSpace = \"20GB\""),
        "{}",
        reserved(a)
    );
    drop(f.manager.ensure(b).await.unwrap());
    // Deux projets : la moitie chacun.
    assert!(
        reserved(b).contains("reservedSpace = \"10GB\""),
        "{}",
        reserved(b)
    );
    // Le buildkitd de A est recree avec sa nouvelle part (empreinte).
    drop(f.manager.ensure(a).await.unwrap());
    let runs = f.docker.runs.lock().unwrap().clone();
    let first = runs.first().unwrap();
    let last = runs.last().unwrap();
    assert_eq!(first.name(), last.name());
    assert_ne!(first.fingerprint(), last.fingerprint());
}

#[tokio::test]
async fn a_failed_directory_creation_starts_nothing() {
    let f = fixture();
    *f.fs.fail_create.lock().unwrap() = true;
    assert!(f.manager.ensure(ProjectId::new()).await.is_err());
    assert!(f.docker.runs.lock().unwrap().is_empty());
    assert!(
        f.docker
            .list_managed(&[(ROLE_LABEL, BUILDKITD_ROLE)])
            .await
            .unwrap()
            .is_empty()
    );
}
