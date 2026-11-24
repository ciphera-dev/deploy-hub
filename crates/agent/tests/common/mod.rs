#![allow(dead_code)]
//! `DockerOps` simule : enregistre les conteneurs et reseaux demandes, sante
//! fixee par le test, metriques constantes.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use ciphera_agent::docker::{
    ContainerId, ContainerLog, ContainerSpec, DockerError, DockerOps, Health, MANAGED_LABEL,
    ManagedContainer,
};
use futures_util::stream::{self, BoxStream, StreamExt};
use proto::NodeMetrics;
use shared::ids::DeploymentId;
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
        Ok(())
    }

    async fn remove(&self, id: &ContainerId) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("remove:{}", id.0));
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

    async fn remove_deployment_images(&self, deployment: DeploymentId) -> Result<(), DockerError> {
        self.calls.lock().unwrap().push(format!("rmi:{deployment}"));
        let delay = *self.rmi_delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        Ok(())
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
