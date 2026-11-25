#![allow(dead_code)]
//! `DockerOps` simule : enregistre les conteneurs et reseaux demandes, sante
//! fixee par le test, metriques constantes.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use ciphera_agent::docker::{
    COMPOSE_PROJECT_KEY, ContainerId, ContainerLog, ContainerSpec, DockerError, DockerOps, Health,
    MANAGED_LABEL, ManagedContainer, PROJECT_LABEL, compose_project_prefix, is_project_volume,
    network_project,
};
use futures_util::stream::{self, BoxStream, StreamExt};
use proto::NodeMetrics;
use shared::ids::{DeploymentId, ProjectId};
use tokio::sync::mpsc;

/// Conteneur connu du faux Docker.
#[derive(Clone, Debug)]
pub struct FakeContainer {
    pub name: String,
    pub image: String,
    pub labels: HashMap<String, String>,
    /// Adresse sur le reseau de projet (`project_address`).
    pub ip: Option<IpAddr>,
    pub running: bool,
}

#[derive(Default)]
pub struct FakeDocker {
    /// Ordre des appels : `network:<nom>`, `run:<nom>` ou `stop:<id>`.
    pub calls: Mutex<Vec<String>>,
    pub runs: Mutex<Vec<ContainerSpec>>,
    /// Sante par identifiant de conteneur (absent : `Missing`).
    pub health: Mutex<HashMap<String, Health>>,
    /// Docker ne repond plus (sante et metriques en attente infinie).
    pub hang: AtomicBool,
    /// `stop` echoue (erreur d'API) tant que vrai.
    pub fail_stop: AtomicBool,
    /// `runs_as_root` echoue (erreur d'API) tant que vrai.
    pub fail_inspect: AtomicBool,
    /// `run` echoue avec cette erreur d'API (refus au demarrage).
    pub fail_run: Mutex<Option<String>>,
    /// Conteneurs existants par identifiant.
    pub containers: Mutex<HashMap<String, FakeContainer>>,
    /// Duree de la suppression des images (nettoyage lent).
    pub rmi_delay: Mutex<std::time::Duration>,
    /// Journaux par conteneur (`feed_logs`) : le flux suit le canal jusqu'a
    /// sa fermeture ; sans canal, flux vide.
    pub log_feeds: Mutex<HashMap<String, mpsc::UnboundedReceiver<ContainerLog>>>,
    /// CPU (%) et memoire (octets) par conteneur (`container_usage`).
    pub usage: Mutex<HashMap<String, (f32, u64)>>,
    /// Reseaux existants (crees par `ensure_network` ou `add_network`).
    pub networks: Mutex<HashSet<String>>,
    /// Volumes existants : nom -> etiquettes.
    pub volumes: Mutex<HashMap<String, HashMap<String, String>>>,
    /// `remove` et `remove_with_volumes` echouent (erreur d'API) tant que vrai.
    pub fail_remove: AtomicBool,
    /// `remove_project_volumes` echoue tant que vrai.
    pub fail_volumes: AtomicBool,
    /// `remove_network` echoue tant que vrai.
    pub fail_network: AtomicBool,
    /// Etiquettes d'image presentes (`remove_deployment_images` retire
    /// celles du deploiement).
    pub images: Mutex<Vec<String>>,
    /// `remove_deployment_images` echoue (erreur d'API) tant que vrai.
    pub fail_rmi: AtomicBool,
}

impl FakeDocker {
    /// Ajoute un conteneur gere (cree hors de `run` : bac a sable, Compose).
    pub fn add_container(&self, id: &str, name: &str, labels: &[(&str, &str)], ip: Option<&str>) {
        let mut labels: HashMap<String, String> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        labels.insert(MANAGED_LABEL.to_string(), "true".to_string());
        self.containers.lock().unwrap().insert(
            id.to_string(),
            FakeContainer {
                name: name.to_string(),
                image: String::new(),
                labels,
                ip: ip.map(|ip| ip.parse().unwrap()),
                running: true,
            },
        );
    }

    /// Ajoute un reseau existant.
    pub fn add_network(&self, name: &str) {
        self.networks.lock().unwrap().insert(name.to_string());
    }

    /// Ajoute un volume du projet Compose `compose_project` (cle `key`).
    pub fn add_volume(&self, compose_project: &str, key: &str) {
        self.volumes.lock().unwrap().insert(
            format!("{compose_project}_{key}"),
            HashMap::from([(COMPOSE_PROJECT_KEY.to_string(), compose_project.to_string())]),
        );
    }

    /// Volume `name` cree par le projet Compose `created_by` (etiquette de
    /// Compose), par exemple `ciphera-d-<id>` pour un volume monte seulement
    /// par le service expose.
    pub fn add_volume_created_by(&self, name: &str, created_by: &str) {
        self.volumes.lock().unwrap().insert(
            name.to_string(),
            HashMap::from([(COMPOSE_PROJECT_KEY.to_string(), created_by.to_string())]),
        );
    }

    pub fn has_network(&self, name: &str) -> bool {
        self.networks.lock().unwrap().contains(name)
    }

    pub fn volume_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.volumes.lock().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    /// Canal des journaux du conteneur `id`, lu par le prochain `logs`.
    pub fn feed_logs(&self, id: &str) -> mpsc::UnboundedSender<ContainerLog> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.log_feeds.lock().unwrap().insert(id.to_string(), rx);
        tx
    }

    pub fn container(&self, id: &str) -> Option<FakeContainer> {
        self.containers.lock().unwrap().get(id).cloned()
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn set_usage(&self, container: &str, cpu_percent: f32, mem_used_bytes: u64) {
        self.usage
            .lock()
            .unwrap()
            .insert(container.to_string(), (cpu_percent, mem_used_bytes));
    }

    pub fn set_health(&self, container: &str, health: Health) {
        self.health
            .lock()
            .unwrap()
            .insert(container.to_string(), health);
    }

    async fn maybe_hang(&self) {
        if self.hang.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
    }
}

pub fn metrics() -> NodeMetrics {
    NodeMetrics {
        cpu_percent: 12.5,
        mem_used_bytes: 1 << 30,
        mem_total_bytes: 4 << 30,
        disk_used_bytes: 10 << 30,
        disk_total_bytes: 40 << 30,
        net_rx_bytes: 1000,
        net_tx_bytes: 2000,
        containers: Vec::new(),
    }
}

impl DockerOps for FakeDocker {
    async fn run(&self, spec: &ContainerSpec) -> Result<ContainerId, DockerError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("run:{}", spec.name()));
        self.runs.lock().unwrap().push(spec.clone());
        if let Some(e) = self.fail_run.lock().unwrap().clone() {
            return Err(DockerError::Api(e));
        }
        let id = format!("id-{}", spec.name());
        let mut containers = self.containers.lock().unwrap();
        let n = containers.len() + 10;
        let mut labels: HashMap<String, String> = spec.labels().iter().cloned().collect();
        labels.insert(MANAGED_LABEL.to_string(), "true".to_string());
        containers.insert(
            id.clone(),
            FakeContainer {
                name: spec.name().to_string(),
                image: spec.image().to_string(),
                labels,
                ip: Some(format!("172.18.0.{n}").parse().unwrap()),
                running: true,
            },
        );
        Ok(ContainerId(id))
    }

    async fn stop(&self, id: &ContainerId) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("stop:{}", id.0));
        if self.fail_stop.load(Ordering::SeqCst) {
            return Err(DockerError::Api("arret refuse".into()));
        }
        if let Some(c) = self.containers.lock().unwrap().get_mut(&id.0) {
            c.running = false;
            // Comme Docker : un conteneur arrete n'a plus d'adresse.
            c.ip = None;
        }
        Ok(())
    }

    async fn restart(&self, id: &ContainerId) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("restart:{}", id.0));
        if let Some(c) = self.containers.lock().unwrap().get_mut(&id.0) {
            c.running = true;
            // Docker peut attribuer une autre adresse au redemarrage.
            match c.ip {
                Some(IpAddr::V4(ip)) => {
                    let [a, b, _, d] = ip.octets();
                    c.ip = Some(IpAddr::V4(std::net::Ipv4Addr::new(a, b, 1, d)));
                }
                Some(IpAddr::V6(_)) => {}
                None => c.ip = Some(IpAddr::V4(std::net::Ipv4Addr::new(172, 18, 2, 1))),
            }
        }
        Ok(())
    }

    async fn runs_as_root(&self, id: &ContainerId) -> Result<bool, DockerError> {
        // Utilisateur demande a la creation ; absent : celui de l'image (root).
        if self.fail_inspect.load(Ordering::SeqCst) {
            return Err(DockerError::Api("inspect refuse".into()));
        }
        // Conteneur inconnu du faux : non.
        let runs = self.runs.lock().unwrap();
        Ok(runs
            .iter()
            .rev()
            .find(|s| format!("id-{}", s.name()) == id.0)
            .is_some_and(|s| ciphera_agent::docker::user_is_root(s.user())))
    }

    async fn health(&self, id: &ContainerId) -> Result<Health, DockerError> {
        self.maybe_hang().await;
        Ok(self
            .health
            .lock()
            .unwrap()
            .get(&id.0)
            .copied()
            .unwrap_or(Health::Missing))
    }

    async fn logs(
        &self,
        id: &ContainerId,
        tail: u32,
        follow: bool,
    ) -> BoxStream<'static, ContainerLog> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("logs:{}:{tail}:{follow}", id.0));
        match self.log_feeds.lock().unwrap().remove(&id.0) {
            Some(rx) => stream::unfold(rx, |mut rx| async move {
                rx.recv().await.map(|line| (line, rx))
            })
            .boxed(),
            None => stream::empty().boxed(),
        }
    }

    async fn stats(&self) -> Result<NodeMetrics, DockerError> {
        self.maybe_hang().await;
        Ok(metrics())
    }

    async fn container_usage(&self, id: &ContainerId) -> Result<Option<(f32, u64)>, DockerError> {
        self.maybe_hang().await;
        Ok(self.usage.lock().unwrap().get(&id.0).copied())
    }

    async fn ensure_network(&self, name: &str) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("network:{name}"));
        self.networks.lock().unwrap().insert(name.to_string());
        Ok(())
    }

    async fn remove(&self, id: &ContainerId) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("remove:{}", id.0));
        if self.fail_remove.load(Ordering::SeqCst) {
            return Err(DockerError::Api(format!("suppression de {} refusee", id.0)));
        }
        self.containers.lock().unwrap().remove(&id.0);
        self.health.lock().unwrap().remove(&id.0);
        Ok(())
    }

    async fn remove_with_volumes(&self, id: &ContainerId) -> Result<(), DockerError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("remove-v:{}", id.0));
        if self.fail_remove.load(Ordering::SeqCst) {
            return Err(DockerError::Api(format!("suppression de {} refusee", id.0)));
        }
        self.containers.lock().unwrap().remove(&id.0);
        self.health.lock().unwrap().remove(&id.0);
        Ok(())
    }

    async fn list_managed(
        &self,
        labels: &[(&str, &str)],
    ) -> Result<Vec<ManagedContainer>, DockerError> {
        let mut out: Vec<ManagedContainer> = self
            .containers
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, c)| {
                labels
                    .iter()
                    .all(|(k, v)| c.labels.get(*k).is_some_and(|x| x == v))
            })
            .map(|(id, c)| ManagedContainer {
                id: ContainerId(id.clone()),
                name: c.name.clone(),
                labels: c.labels.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn project_address(&self, id: &ContainerId) -> Result<Option<IpAddr>, DockerError> {
        Ok(self
            .containers
            .lock()
            .unwrap()
            .get(&id.0)
            .and_then(|c| c.ip))
    }

    async fn remove_network(&self, name: &str) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("rmnet:{name}"));
        if self.fail_network.load(Ordering::SeqCst) {
            return Err(DockerError::Api(format!("reseau {name} : refus")));
        }
        // Comme Docker : refuse tant qu'un conteneur du projet y est attache.
        if let Some(project) = network_project(name) {
            let project = project.to_string();
            let attached = self
                .containers
                .lock()
                .unwrap()
                .values()
                .any(|c| c.labels.get(PROJECT_LABEL) == Some(&project));
            if attached {
                return Err(DockerError::Api(format!("reseau {name} encore utilise")));
            }
        }
        self.networks.lock().unwrap().remove(name);
        Ok(())
    }

    async fn project_networks(&self) -> Result<Vec<String>, DockerError> {
        let mut names: Vec<String> = self
            .networks
            .lock()
            .unwrap()
            .iter()
            .filter(|n| network_project(n).is_some())
            .cloned()
            .collect();
        names.sort();
        Ok(names)
    }

    async fn remove_project_volumes(&self, project: ProjectId) -> Result<usize, DockerError> {
        if self.fail_volumes.load(Ordering::SeqCst) {
            return Err(DockerError::Api("volumes : refus".into()));
        }
        let prefix = compose_project_prefix(project);
        let mut volumes = self.volumes.lock().unwrap();
        let doomed: Vec<String> = volumes
            .iter()
            .filter(|(name, labels)| is_project_volume(name, labels, &prefix))
            .map(|(name, _)| name.clone())
            .collect();
        for name in &doomed {
            self.calls.lock().unwrap().push(format!("rmvol:{name}"));
            volumes.remove(name);
        }
        Ok(doomed.len())
    }

    async fn remove_deployment_images(&self, deployment: DeploymentId) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("rmi:{deployment}"));
        let delay = *self.rmi_delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if self.fail_rmi.load(Ordering::SeqCst) {
            return Err(DockerError::Api("suppression d'image refusee".into()));
        }
        self.images
            .lock()
            .unwrap()
            .retain(|t| !ciphera_agent::docker::is_deployment_image(t, deployment));
        Ok(())
    }

    async fn image_deployments(&self) -> Result<Vec<DeploymentId>, DockerError> {
        let mut out: Vec<DeploymentId> = self
            .images
            .lock()
            .unwrap()
            .iter()
            .filter_map(|t| ciphera_agent::docker::image_deployment(t))
            .collect();
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }
}

/// Chemin temporaire unique, supprime (avec les fichiers WAL de SQLite) a la fin.
pub struct TempPath(pub PathBuf);

impl TempPath {
    pub fn new(prefix: &str) -> Self {
        Self(std::env::temp_dir().join(format!("{prefix}-{}", shared::ids::DeploymentId::new())))
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut p = self.0.clone().into_os_string();
            p.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(p));
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
