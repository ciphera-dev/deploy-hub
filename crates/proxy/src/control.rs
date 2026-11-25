//! Socket de pilotage du proxy (Unix seulement, jamais TCP) :
//! `/run/ciphera/proxy.sock`, mode 0660, groupe `ciphera-agent`.
//!
//! En production, le socket est cree par systemd (`ciphera-proxy.socket`,
//! activation par socket) : proprietaire, groupe et mode sont fixes par
//! l'unite et le proxy n'a besoin d'aucun droit d'ecriture dans
//! `/run/ciphera`. Hors systemd (tests, developpement), `bind` le cree en
//! 0660.
//!
//! Protocole : trames du codec `proto` (longueur u32 + postcard, 4 Mio au
//! plus). Seuls `ControlToAgent::ApplyRoutes`, `IssueCertificate`,
//! `CheckCertificate` et `RemoveCertificate` sont acceptes ; chacun recoit
//! une `ProxyReply`. Une route retiree garde son certificat : seul
//! `RemoveCertificate` (domaine supprime) efface certificat et cle. Une trame illisible, trop
//! grande ou d'un autre type recoit `Rejected` puis la connexion est fermee ;
//! la table ne change pas. La connexion de l'agent peut rester ouverte entre
//! deux tables. Une emission ACME occupe sa connexion jusqu'a son issue :
//! l'agent l'envoie sur une connexion a part.
//!
//! Acces : le groupe du socket n'est qu'une premiere barriere. A chaque
//! connexion, l'uid du pair (`SO_PEERCRED`) doit etre celui de root ou du
//! compte `ciphera-agent` ; sinon la connexion est fermee sans reponse. Une
//! fois le premier octet d'un message arrive, le message entier doit l'etre
//! dans les 10 s (`READ_TIMEOUT`) : sinon la connexion est fermee. Une
//! connexion au repos entre deux messages n'est pas concernee.

use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use pingora::server::ShutdownWatch;
use pingora::services::background::BackgroundService;
use proto::{CertificateOutcome, ControlToAgent, ProxyReply, TlsMode, codec};
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Semaphore;

use crate::acme::AcmeIssuer;
use crate::routes::RouteTable;

/// Chemin du socket de pilotage.
pub const DEFAULT_SOCKET: &str = "/run/ciphera/proxy.sock";
/// Connexions de pilotage simultanees (l'agent n'en ouvre qu'une).
pub const MAX_CONNECTIONS: usize = 4;
/// Attente de la route d'un hote dont l'emission est demandee : la table
/// arrive par une autre connexion de l'agent, parfois juste apres.
pub const ROUTE_WAIT: Duration = Duration::from_secs(5);
const ROUTE_POLL: Duration = Duration::from_millis(100);
const SOCKET_MODE: u32 = 0o660;
/// Delai pour lire un message de pilotage une fois son premier octet recu.
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Compte de l'agent, seul (avec root) a piloter le proxy.
pub const AGENT_USER: &str = "ciphera-agent";
/// Premier descripteur transmis par systemd (`SD_LISTEN_FDS_START`).
const SD_LISTEN_FDS_START: i32 = 3;

/// Qui peut piloter le proxy, et avec quel delai de lecture.
#[derive(Clone, Debug)]
pub struct ControlAccess {
    uids: Vec<u32>,
    read_timeout: Duration,
}

impl ControlAccess {
    /// Root et les uid donnes ; `READ_TIMEOUT` par message.
    pub fn new(uids: impl IntoIterator<Item = u32>) -> Self {
        let mut uids: Vec<u32> = uids.into_iter().collect();
        uids.push(0);
        uids.sort_unstable();
        uids.dedup();
        Self {
            uids,
            read_timeout: READ_TIMEOUT,
        }
    }

    /// Root et le compte `ciphera-agent` (lu dans `/etc/passwd`). Sans ce
    /// compte, seul root est accepte (avertissement).
    pub fn production() -> Self {
        let agent = std::fs::read_to_string("/etc/passwd")
            .ok()
            .and_then(|passwd| uid_of(&passwd, AGENT_USER));
        if agent.is_none() {
            tracing::warn!(
                "compte {AGENT_USER} introuvable dans /etc/passwd (absent, illisible ou hors fichier) : seul root peut piloter le proxy"
            );
        }
        Self::new(agent)
    }

    /// Delai de lecture par message (tests).
    pub fn with_read_timeout(mut self, read_timeout: Duration) -> Self {
        self.read_timeout = read_timeout;
        self
    }

    pub fn allows(&self, uid: u32) -> bool {
        self.uids.contains(&uid)
    }
}

/// Uid du compte `name` dans un contenu de `/etc/passwd`.
pub fn uid_of(passwd: &str, name: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != name {
            return None;
        }
        fields.nth(1)?.parse().ok()
    })
}

/// Cree le socket `path` (mode 0660). Un ancien socket au meme chemin est
/// remplace ; tout autre fichier est laisse intact (erreur).
pub fn bind(path: &Path) -> io::Result<StdUnixListener> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => std::fs::remove_file(path)?,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} existe et n'est pas un socket", path.display()),
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = StdUnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(SOCKET_MODE))?;
    Ok(listener)
}

/// Socket transmis par systemd (activation par socket), s'il y en a un pour
/// ce processus. Le descripteur n'est pris qu'une fois.
pub fn from_systemd() -> io::Result<Option<StdUnixListener>> {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    let ours = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
        == Some(std::process::id());
    if !ours || TAKEN.swap(true, Ordering::SeqCst) {
        return Ok(None);
    }
    let count = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|n| n.parse::<u32>().ok());
    if count != Some(1) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "un seul socket attendu de systemd (LISTEN_FDS)",
        ));
    }
    // SAFETY : systemd transmet les sockets actives a partir du descripteur 3
    // (sd_listen_fds) ; LISTEN_PID designe ce processus, donc ce descripteur
    // lui appartient, et `TAKEN` garantit qu'il n'est pris qu'une fois.
    let fd = unsafe { OwnedFd::from_raw_fd(SD_LISTEN_FDS_START) };
    let listener = StdUnixListener::from(fd);
    // Refuse un descripteur qui ne serait pas un socket Unix.
    listener.local_addr()?;
    Ok(Some(listener))
}

/// Sert le socket jusqu'a l'arret (`shutdown` passe a `true` ou se ferme).
pub async fn serve(
    listener: StdUnixListener,
    table: Arc<RouteTable>,
    issuer: Option<Arc<AcmeIssuer>>,
    access: ControlAccess,
    mut shutdown: ShutdownWatch,
) {
    let listener = match listener
        .set_nonblocking(true)
        .and_then(|()| UnixListener::from_std(listener))
    {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("socket de pilotage inutilisable : {e}");
            return;
        }
    };
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        tracing::warn!("socket de pilotage : accept : {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    tracing::warn!("socket de pilotage : trop de connexions, refusee");
                    continue;
                };
                match stream.peer_cred() {
                    Ok(cred) if access.allows(cred.uid()) => {}
                    Ok(cred) => {
                        tracing::warn!(
                            uid = cred.uid(),
                            "socket de pilotage : pair refuse (uid non autorise)"
                        );
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!("socket de pilotage : uid du pair illisible : {e}");
                        continue;
                    }
                }
                let table = table.clone();
                let issuer = issuer.clone();
                let read_timeout = access.read_timeout;
                tokio::spawn(async move {
                    handle(stream, &table, issuer.as_deref(), read_timeout).await;
                    drop(slot);
                });
            }
        }
    }
}

/// Lit le message suivant : attend sans limite son premier octet (lecture
/// reelle, pas la disponibilite du socket, qui reste positive apres une
/// trame entiere), puis `read_timeout` pour recevoir le reste. `None` :
/// delai depasse (connexion a fermer sans reponse).
async fn next_message(
    stream: &mut UnixStream,
    read_timeout: Duration,
) -> Option<codec::Result<Option<ControlToAgent>>> {
    let mut first = [0u8; 1];
    match stream.read(&mut first).await {
        Ok(0) => return Some(Ok(None)),
        Ok(_) => {}
        Err(e) => return Some(Err(codec::Error::Io(e))),
    }
    let mut message = (&first[..]).chain(&mut *stream);
    match tokio::time::timeout(read_timeout, codec::read_frame(&mut message)).await {
        Ok(result) => Some(result),
        Err(_) => {
            tracing::warn!("socket de pilotage : message trop lent, connexion fermee");
            None
        }
    }
}

async fn handle(
    mut stream: UnixStream,
    table: &RouteTable,
    issuer: Option<&AcmeIssuer>,
    read_timeout: Duration,
) {
    loop {
        let Some(message) = next_message(&mut stream, read_timeout).await else {
            return;
        };
        let reply = match message {
            Ok(None) => return,
            Ok(Some(ControlToAgent::ApplyRoutes(routes))) => match table.apply(routes) {
                Ok(n) => {
                    // Une route retiree (DNS en erreur, autre serveur) garde
                    // son certificat : seul `RemoveCertificate` le supprime.
                    tracing::info!(routes = n, "table de routes appliquee");
                    ProxyReply::Applied {
                        routes: u32::try_from(n).unwrap_or(u32::MAX),
                    }
                }
                Err(e) => {
                    tracing::warn!("table de routes refusee : {e}");
                    ProxyReply::Rejected {
                        reason: e.to_string(),
                    }
                }
            },
            Ok(Some(ControlToAgent::IssueCertificate { host })) => {
                ProxyReply::Certificate(issue(table, issuer, &host).await)
            }
            Ok(Some(ControlToAgent::CheckCertificate { host })) => {
                // Un certificat expire compte pour absent : le plan de
                // controle en demande un nouveau.
                ProxyReply::Certificate(match issuer.and_then(|i| i.store().current(&host)) {
                    Some(held) => CertificateOutcome::Issued {
                        not_after: held.not_after,
                        issuer: held.issuer.clone(),
                    },
                    None => CertificateOutcome::Missing,
                })
            }
            Ok(Some(ControlToAgent::RemoveCertificate { host })) => {
                // Aussi sans certificat installe : une emission en cours
                // pour cet hote n'installera rien (domaine supprime).
                if let Some(issuer) = issuer {
                    issuer.store().remove(&host);
                }
                ProxyReply::Certificate(CertificateOutcome::Removed)
            }
            Ok(Some(_)) => {
                tracing::warn!(
                    "socket de pilotage : message refuse (seuls ApplyRoutes et les ordres de certificat)"
                );
                refuse(
                    &mut stream,
                    "seuls ApplyRoutes et les ordres de certificat sont acceptes",
                )
                .await;
                return;
            }
            Err(e) => {
                tracing::warn!("socket de pilotage : trame refusee : {e}");
                refuse(&mut stream, "trame illisible").await;
                return;
            }
        };
        if codec::write_frame(&mut stream, &reply).await.is_err() {
            return;
        }
    }
}

/// La table courante a-t-elle une route `TlsMode::Acme` pour `host` ?
fn routed(table: &RouteTable, host: &str) -> bool {
    table
        .snapshot()
        .routes()
        .any(|r| r.host == host && r.tls == TlsMode::Acme)
}

/// Emission ACME pour `host`, seulement s'il a une route `TlsMode::Acme`
/// dans la table (le proxy n'emet que pour ce qu'il sert). La route est
/// attendue jusqu'a [`ROUTE_WAIT`] : l'agent envoie la table et la demande
/// sur deux connexions.
async fn issue(table: &RouteTable, issuer: Option<&AcmeIssuer>, host: &str) -> CertificateOutcome {
    let failed = |reason: String| {
        tracing::warn!(%host, "certificat non emis : {reason}");
        CertificateOutcome::Failed { reason }
    };
    let Some(issuer) = issuer else {
        return failed("ACME non configure sur ce proxy".into());
    };
    let deadline = tokio::time::Instant::now() + ROUTE_WAIT;
    while !routed(table, host) {
        if tokio::time::Instant::now() >= deadline {
            return failed("aucune route HTTPS pour cet hote dans la table du proxy".into());
        }
        tokio::time::sleep(ROUTE_POLL).await;
    }
    match issuer.issue(host).await {
        Ok(installed) => CertificateOutcome::Issued {
            not_after: installed.not_after,
            issuer: installed.issuer.clone(),
        },
        Err(e) => failed(e.to_string()),
    }
}

async fn refuse(stream: &mut UnixStream, reason: &str) {
    let reply = ProxyReply::Rejected {
        reason: reason.into(),
    };
    let _ = codec::write_frame(stream, &reply).await;
}

/// Service d'arriere-plan Pingora qui sert le socket de pilotage.
pub struct ControlService {
    listener: Mutex<Option<StdUnixListener>>,
    table: Arc<RouteTable>,
    issuer: Option<Arc<AcmeIssuer>>,
    access: ControlAccess,
}

impl ControlService {
    pub fn new(
        listener: StdUnixListener,
        table: Arc<RouteTable>,
        issuer: Option<Arc<AcmeIssuer>>,
        access: ControlAccess,
    ) -> Self {
        Self {
            listener: Mutex::new(Some(listener)),
            table,
            issuer,
            access,
        }
    }
}

#[async_trait]
impl BackgroundService for ControlService {
    async fn start(&self, shutdown: ShutdownWatch) {
        let listener = match self.listener.lock() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => {
                tracing::error!("socket de pilotage : verrou empoisonne, socket repris");
                poisoned.into_inner().take()
            }
        };
        match listener {
            Some(listener) => {
                serve(
                    listener,
                    self.table.clone(),
                    self.issuer.clone(),
                    self.access.clone(),
                    shutdown,
                )
                .await
            }
            None => tracing::error!(
                "socket de pilotage : aucun socket a servir (service demarre deux fois ?), la table de routes ne sera plus mise a jour"
            ),
        }
    }
}
