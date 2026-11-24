//! Table de routes du proxy du noeud, tenue par l'agent.
//!
//! Le plan de controle envoie la table voulue (`ApplyRoutes`) : hote,
//! prefixe, TLS, redirection et service servi. Les upstreams ne viennent
//! jamais de lui : l'agent les calcule a partir du deploiement servi de
//! chaque service (etat local), avec l'adresse de son conteneur sur le
//! reseau de son projet (`ciphera-p-*`) et son port expose. Un conteneur hors
//! d'un reseau de projet ne devient jamais un upstream (ni PostgreSQL, ni
//! buildkitd, ni l'hote). Seule exception : la route du panneau (sans
//! service ni redirection), dont l'unique amont admis est l'ecoute locale du
//! panneau fixee dans la configuration de l'agent ; toute autre adresse
//! envoyee pour elle est ignoree.
//!
//! La table est recalculee et renvoyee a chaque nouvelle table du plan de
//! controle et a chaque bascule ou retrait d'un deploiement servi. Tant
//! qu'aucune table n'est connue depuis le demarrage, rien n'est envoye (le
//! proxy garde la sienne ; le plan de controle renvoie la table a chaque
//! connexion) et `refresh` echoue (`RouteError::NotSynced`) : aucune
//! bascule ne passe sans table.
//!
//! Chaque table du plan de controle est gardee sur disque
//! ([`Router::with_store`], `routes.json` a cote de l'etat local, ecrit
//! atomiquement) : apres un redemarrage de la machine sans plan de controle
//! joignable, l'agent la reprend ([`Router::restore`]) et renvoie au proxy
//! une table aux amonts recalcules (adresses des conteneurs relances).

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use proto::Route;
use tokio::sync::Mutex;

use crate::docker::{DockerError, DockerOps};
use crate::state::{DeploymentRecord, StateDb, StateError};

/// Destination des tables calculees : le client du socket du proxy en
/// production, un enregistreur dans les tests.
pub trait RouteSink: Send + Sync + 'static {
    fn apply(&self, routes: Vec<Route>);
}

#[cfg(unix)]
impl RouteSink for crate::proxy_client::ProxyHandle {
    fn apply(&self, routes: Vec<Route>) {
        crate::proxy_client::ProxyHandle::apply(self, routes);
    }
}

/// Pas de proxy (plateformes sans socket Unix) : la table est ignoree.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProxy;

impl RouteSink for NoProxy {
    fn apply(&self, routes: Vec<Route>) {
        tracing::warn!(
            routes = routes.len(),
            "pas de proxy du noeud sur cette plateforme : table ignoree"
        );
    }
}

#[derive(Debug)]
pub enum RouteError {
    Docker(DockerError),
    State(StateError),
    /// Aucune table du plan de controle recue depuis le demarrage : rien
    /// n'a ete envoye au proxy.
    NotSynced,
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Docker(e) => write!(f, "routes : {e}"),
            Self::State(e) => write!(f, "routes : {e}"),
            Self::NotSynced => write!(
                f,
                "routes not synced : aucune table du plan de controle recue depuis le demarrage"
            ),
        }
    }
}

impl std::error::Error for RouteError {}

/// Table voulue par le plan de controle, upstreams calcules par l'agent
/// (`panel` : ecoute locale du panneau). Une erreur de Docker ou de l'etat
/// local n'envoie rien (le proxy garde la table precedente).
pub async fn render<D: DockerOps>(
    desired: &[Route],
    serving: &[DeploymentRecord],
    docker: &D,
    panel: Option<SocketAddr>,
) -> Result<Vec<Route>, DockerError> {
    let mut out = Vec::with_capacity(desired.len());
    for route in desired {
        let mut upstreams = Vec::new();
        if route.service.is_none() && route.redirect_to.is_none() {
            // Route du panneau : l'ecoute locale configuree, et elle seule.
            if let Some(panel) = panel
                && route.upstreams.contains(&panel)
            {
                upstreams.push(panel);
            }
        } else if let Some(service) = route.service
            && let Some(record) = serving.iter().find(|r| r.deployment.service_id == service)
            && let Some(port) = record.port
            && let Some(ip) = docker.project_address(&record.container).await?
        {
            upstreams.push(SocketAddr::new(ip, port));
        }
        out.push(Route {
            upstreams,
            ..route.clone()
        });
    }
    Ok(out)
}

/// Taille maximale du fichier de la table gardee.
const MAX_STORE_BYTES: u64 = 16 * 1024 * 1024;

/// Ecrit `routes` dans `path` : fichier temporaire 0600 du meme dossier,
/// donnees sur disque, renommage, puis dossier sur disque.
fn store_routes(path: &Path, routes: &[Route]) -> std::io::Result<()> {
    let json = serde_json::to_vec(routes).map_err(std::io::Error::other)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map_or_else(|| "routes".into(), |n| n.to_string_lossy().into_owned());
    let tmp = dir.join(format!(".{name}.tmp"));
    // Ecritures serialisees par le verrou de `Router::desired`.
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)?;
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}

/// Table gardee dans `path` ; `None` si absente.
fn load_routes(path: &Path) -> std::io::Result<Option<Vec<Route>>> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err(std::io::Error::other("fichier trop long"));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(std::io::Error::other)
}

/// Table voulue courante et envoi au proxy. Les calculs sont serialises :
/// deux bascules simultanees n'envoient jamais une table perimee en dernier.
pub struct Router<S: RouteSink> {
    desired: Mutex<Option<Vec<Route>>>,
    sink: S,
    panel: Option<SocketAddr>,
    /// Table du plan de controle gardee sur disque.
    store: Option<PathBuf>,
}

impl<S: RouteSink> Router<S> {
    pub fn new(sink: S) -> Self {
        Self {
            desired: Mutex::new(None),
            sink,
            panel: None,
            store: None,
        }
    }

    /// Garde chaque table du plan de controle dans `path`.
    pub fn with_store(mut self, path: PathBuf) -> Self {
        self.store = Some(path);
        self
    }

    /// Ecoute locale du panneau, seul amont admis pour sa route.
    pub fn with_panel(mut self, panel: Option<SocketAddr>) -> Self {
        self.panel = panel;
        self
    }

    /// Nouvelle table du plan de controle : calculee et envoyee.
    pub async fn set_desired<D: DockerOps>(
        &self,
        routes: Vec<Route>,
        docker: &D,
        db: &StateDb,
    ) -> Result<(), RouteError> {
        let mut desired = self.desired.lock().await;
        if let Some(path) = &self.store
            && let Err(e) = store_routes(path, &routes)
        {
            tracing::warn!(
                "table de routes non gardee ({}) : {e} ; elle sera perdue au redemarrage",
                path.display()
            );
        }
        *desired = Some(routes);
        self.push(desired.as_deref(), docker, db).await
    }

    /// Demarrage : sans table du plan de controle encore, reprend celle
    /// gardee sur disque ; puis recalcule et envoie la table connue (amonts
    /// des conteneurs relances). `false` : aucune table connue, rien envoye.
    /// Une table gardee illisible est ignoree (le plan de controle renvoie
    /// la sienne a la connexion).
    pub async fn restore<D: DockerOps>(&self, docker: &D, db: &StateDb) -> bool {
        let mut desired = self.desired.lock().await;
        if desired.is_none()
            && let Some(path) = &self.store
        {
            match load_routes(path) {
                Ok(Some(routes)) => {
                    tracing::info!(
                        routes = routes.len(),
                        "table de routes reprise de {}",
                        path.display()
                    );
                    *desired = Some(routes);
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("table de routes gardee ignoree ({}) : {e}", path.display())
                }
            }
        }
        if desired.is_none() {
            return false;
        }
        match self.push(desired.as_deref(), docker, db).await {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("table de routes reprise non envoyee : {e}");
                false
            }
        }
    }

    /// Recalcule la table (deploiement servi change) et l'envoie ;
    /// `NotSynced` si aucune table du plan de controle n'est connue.
    pub async fn refresh<D: DockerOps>(&self, docker: &D, db: &StateDb) -> Result<(), RouteError> {
        let desired = self.desired.lock().await;
        self.push(desired.as_deref(), docker, db).await
    }

    async fn push<D: DockerOps>(
        &self,
        desired: Option<&[Route]>,
        docker: &D,
        db: &StateDb,
    ) -> Result<(), RouteError> {
        let Some(desired) = desired else {
            tracing::info!("aucune table du plan de controle encore : rien envoye au proxy");
            return Err(RouteError::NotSynced);
        };
        let serving = db.serving().map_err(RouteError::State)?;
        let table = render(desired, &serving, docker, self.panel)
            .await
            .map_err(RouteError::Docker)?;
        self.sink.apply(table);
        Ok(())
    }
}
