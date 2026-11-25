//! Diffusion des lignes des agents aux clients du panneau.
//!
//! Une entree par cible (`LogTarget`) tant qu'elle a un lecteur : canal de
//! diffusion, lignes recentes (pour un lecteur arrive en cours de route),
//! masquage du projet et serveur suivi. Le premier lecteur d'une cible de
//! service demande le suivi a l'agent (`Logs`), le dernier parti l'arrete
//! (`StopLogs`) : un seul suivi par cible quel que soit le nombre de
//! lecteurs.
//!
//! Clients lents : chaque lecteur a son propre tampon de `CLIENT_BUFFER`
//! lignes (canal `broadcast`). La diffusion n'attend jamais un lecteur ; en
//! cas de debordement, les plus anciennes lignes du lecteur sont perdues et
//! il recoit un seul marqueur « N lignes perdues ».
//!
//! Masquage : relu au plus tard toutes les `REDACTOR_TTL` pour chaque
//! projet, et aussitot apres `invalidate` (secrets du projet modifies) ;
//! partage avec l'enregistrement des lignes (`store`). Echec ferme : si les
//! secrets du projet sont illisibles, ses lignes sont retenues (au plus
//! `RECENT_LINES` par cible) derriere un seul marqueur `MASKING_DOWN`, rien
//! n'est garde en cache.
//!
//! Masquage hors du verrou des cibles : une ligne est masquee avant de
//! prendre le verrou (diffusion), les lignes retenues sont sorties sous le
//! verrou, masquees sans lui, puis diffusees (liberation). Le verrou n'attend
//! donc jamais un masquage (abonnements, departs, liberations des autres
//! projets). Les messages des agents restent traites en serie par `run` :
//! la ligne suivante attend le masquage de la precedente, lineaire et borne
//! (64 Kio par ligne). Ordre garde : tant que
//! des lignes retenues d'une cible attendent ou sont en cours de liberation,
//! les nouvelles lignes de cette cible sont retenues derriere elles.
//!
//! Sans blocage : la lecture des secrets d'un projet se fait dans une tache a
//! part (`LOAD_TIMEOUT` au plus par essai, un essai par `BACKOFF` tant que la
//! lecture echoue et que des lignes attendent). La diffusion l'attend au plus
//! `INLINE_WAIT`, puis retient les lignes du projet ; la tache les libere,
//! masquees et dans l'ordre, des que les secrets sont lus. Une source lente ou
//! en panne ne retarde donc pas les lignes des autres projets.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use proto::{AgentToControl, ControlToAgent, LogTarget};
use shared::ids::ServerId;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, oneshot, watch};
use uuid::Uuid;

use super::LogEvent;
use super::redact::{Redactor, SecretValues, SecretsUnavailable};
use crate::agents::registry::AgentRegistry;

/// Tampon de chaque lecteur (lignes).
pub const CLIENT_BUFFER: usize = 1024;
/// Lignes recentes gardees par cible pour un nouveau lecteur.
pub const RECENT_LINES: usize = 1000;
/// Octets bruts retenus au plus par cible quand le masquage est indisponible
/// (en plus de `RECENT_LINES` lignes) : une ligne brute peut peser 64 Kio.
pub const MAX_HELD_BYTES: usize = 16 * 1024 * 1024;
/// Nombre maximal de lignes demandees a l'ouverture (`tail`).
pub const MAX_TAIL: u32 = 1000;
/// Flux ouverts au plus sur l'instance.
pub const MAX_STREAMS: usize = 256;
/// Duree de vie du masquage garde par projet (filet si `invalidate` manque).
pub const REDACTOR_TTL: Duration = Duration::from_secs(10);
/// Marqueur envoye une fois quand les lignes d'un projet sont retenues.
pub const MASKING_DOWN: &str = "masquage indisponible, lignes retenues";
/// Duree maximale d'une lecture des secrets d'un projet.
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(2);
/// Apres un echec, pas de nouvelle lecture des secrets du projet avant ce
/// delai (lignes retenues sans requete).
pub const BACKOFF: Duration = Duration::from_secs(1);
/// Telechargements de `logs.txt` en cours au plus sur l'instance (chacun
/// lit jusqu'a 4 Mio en base).
pub const MAX_DOWNLOADS: usize = 4;
/// Attente maximale d'une lecture par la diffusion avant de retenir la ligne.
pub const INLINE_WAIT: Duration = Duration::from_millis(250);

/// Cible demandee par un lecteur.
#[derive(Clone, Debug)]
pub struct Source {
    pub target: LogTarget,
    /// Projet de la cible (valeurs secretes a masquer).
    pub project: Uuid,
    /// Nom affiche des lignes (`build` ou nom du service).
    pub container: String,
    /// Serveur a qui demander le suivi et nombre de lignes passees (aucun :
    /// les lignes arrivent sans demande, comme celles d'un build).
    pub follow: Option<(ServerId, u32)>,
    /// Seul serveur dont les lignes sont acceptees pour cette cible (celui
    /// du deploiement). Inconnu (deploiement pas encore demarre) : le
    /// premier serveur qui envoie une ligne, ou le serveur suivi.
    pub origin: Option<ServerId>,
}

struct Entry {
    /// Identifiant de l'entree (une cible supprimee puis recreee en a un
    /// autre) : une liberation ne diffuse que dans l'entree d'ou viennent
    /// ses lignes.
    id: u64,
    tx: broadcast::Sender<LogEvent>,
    subscribers: usize,
    /// Serveur qui suit la cible a notre demande.
    follow: Option<ServerId>,
    /// Seul serveur dont les lignes sont acceptees (`Source::origin`).
    origin: Option<ServerId>,
    container: String,
    project: Uuid,
    recent: VecDeque<LogEvent>,
    /// Lignes brutes retenues faute de masquage (jamais envoyees telles quelles).
    held: VecDeque<proto::LogLine>,
    /// Octets des lignes de `held`.
    held_bytes: usize,
    /// Lignes retenues perdues (au-dela de `RECENT_LINES`).
    held_lost: u64,
    /// Marqueur `MASKING_DOWN` deja envoye pour les lignes retenues.
    down_notified: bool,
    /// Lignes retenues sorties pour etre masquees hors du verrou, pas
    /// encore diffusees : les nouvelles lignes attendent derriere.
    releasing: bool,
}

impl Entry {
    /// Ligne deja masquee : gardee pour un nouveau lecteur et diffusee.
    fn emit(&mut self, event: LogEvent) {
        if self.recent.len() == RECENT_LINES {
            self.recent.pop_front();
        }
        self.recent.push_back(event.clone());
        // Sans lecteur pret, la ligne reste dans les tampons.
        let _ = self.tx.send(event);
    }

    fn holds(&self) -> bool {
        !self.held.is_empty() || self.held_lost > 0
    }

    /// Une nouvelle ligne doit attendre derriere des lignes retenues.
    fn busy(&self) -> bool {
        self.holds() || self.releasing
    }

    /// Masquage indisponible : un seul marqueur tant que des lignes attendent.
    fn notify_down(&mut self) {
        if self.holds() && !self.down_notified {
            self.down_notified = true;
            self.emit(LogEvent::notice(MASKING_DOWN));
        }
    }

    fn hold(&mut self, line: &proto::LogLine) {
        self.held_bytes += line.line.len();
        self.held.push_back(line.clone());
        // Bornes en lignes et en octets ; les plus anciennes sont perdues.
        while self.held.len() > RECENT_LINES || self.held_bytes > MAX_HELD_BYTES {
            let Some(old) = self.held.pop_front() else {
                break;
            };
            self.held_bytes = self.held_bytes.saturating_sub(old.line.len());
            self.held_lost += 1;
        }
    }

    /// Masquage revenu : lignes retenues sorties (a masquer hors du verrou),
    /// avec le nombre de lignes perdues ; la cible reste occupee
    /// (`releasing`) jusqu'a leur diffusion.
    fn take_held(&mut self, target: LogTarget) -> Option<Release> {
        if !self.holds() {
            return None;
        }
        self.down_notified = false;
        self.releasing = true;
        self.held_bytes = 0;
        Some(Release {
            target,
            id: self.id,
            container: self.container.clone(),
            lost: std::mem::take(&mut self.held_lost),
            lines: std::mem::take(&mut self.held).into(),
        })
    }
}

/// Ligne masquee (hors de tout verrou).
fn redacted(line: &proto::LogLine, container: &str, redactor: &Redactor) -> LogEvent {
    LogEvent::line(
        line.timestamp_ms,
        line.stream,
        container,
        redactor.redact_line(&line.line),
    )
}

/// Lignes retenues d'une entree, sorties pour etre masquees hors du verrou.
struct Release {
    target: LogTarget,
    id: u64,
    container: String,
    lost: u64,
    lines: Vec<proto::LogLine>,
}

#[derive(Default)]
struct Targets {
    entries: HashMap<LogTarget, Entry>,
    /// Cibles dont l'arret a deja ete demande (lignes encore en vol).
    stopped: HashSet<LogTarget>,
    /// Identifiant de la prochaine entree.
    next_id: u64,
}

struct Cached {
    at: Instant,
    redactor: Arc<Redactor>,
}

/// Masquage d'un projet pour la diffusion.
enum Live {
    Ready(Arc<Redactor>),
    /// Lecture en cours : ligne retenue sans marqueur.
    Pending,
    /// Derniere lecture en echec : ligne retenue, marqueur.
    Down,
}

#[derive(Default)]
struct Redactors {
    projects: HashMap<Uuid, Cached>,
    /// Projets dont une tache lit les secrets.
    loading: HashSet<Uuid>,
    /// Derniere lecture en echec, par projet (`BACKOFF`).
    down: HashMap<Uuid, Instant>,
    /// Augmente a chaque `invalidate` : un masquage lu avant n'est pas
    /// garde.
    version: u64,
}

struct Inner {
    agents: AgentRegistry,
    secrets: Arc<dyn SecretValues>,
    targets: Mutex<Targets>,
    redactors: Mutex<Redactors>,
    downloads: Arc<Semaphore>,
}

#[derive(Clone)]
pub struct LogHub {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for LogHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogHub").finish_non_exhaustive()
    }
}

impl LogHub {
    pub fn new(agents: AgentRegistry, secrets: Arc<dyn SecretValues>) -> Self {
        Self {
            inner: Arc::new(Inner {
                agents,
                secrets,
                targets: Mutex::new(Targets::default()),
                redactors: Mutex::new(Redactors::default()),
                downloads: Arc::new(Semaphore::new(MAX_DOWNLOADS)),
            }),
        }
    }

    /// Place pour un telechargement de `logs.txt` (`MAX_DOWNLOADS` en
    /// cours au plus), rendue a la liberation ; `None` : toutes prises.
    pub fn try_download(&self) -> Option<OwnedSemaphorePermit> {
        self.inner.downloads.clone().try_acquire_owned().ok()
    }

    fn targets(&self) -> MutexGuard<'_, Targets> {
        // Aucune operation ne panique sous ce verrou.
        self.inner
            .targets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn redactors(&self) -> MutexGuard<'_, Redactors> {
        // Aucune operation ne panique sous ce verrou.
        self.inner
            .redactors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Masquage des valeurs secretes actuelles du projet (relues, au plus
    /// `LOAD_TIMEOUT`). L'automate (jusqu'a quelques dizaines de Mio) est
    /// construit hors des threads de l'executeur.
    pub async fn redactor(&self, project: Uuid) -> Result<Redactor, SecretsUnavailable> {
        let values = tokio::time::timeout(LOAD_TIMEOUT, self.inner.secrets.values(project))
            .await
            .map_err(|_| SecretsUnavailable)??;
        tokio::task::spawn_blocking(move || Redactor::new(values))
            .await
            .map_err(|_| SecretsUnavailable)
    }

    /// Masquage du projet pour les lignes en direct : garde au plus
    /// `REDACTOR_TTL`, relu apres `invalidate`. Une erreur n'est jamais
    /// gardee : l'appel suivant relit.
    pub async fn project_redactor(
        &self,
        project: Uuid,
    ) -> Result<Arc<Redactor>, SecretsUnavailable> {
        let version = {
            let cache = self.redactors();
            if let Some(c) = cache.projects.get(&project)
                && c.at.elapsed() < REDACTOR_TTL
            {
                return Ok(c.redactor.clone());
            }
            // Echec recent : pas de nouvelle requete avant `BACKOFF`.
            if cache
                .down
                .get(&project)
                .is_some_and(|at| at.elapsed() < BACKOFF)
            {
                return Err(SecretsUnavailable);
            }
            cache.version
        };
        let redactor = match self.redactor(project).await {
            Ok(r) => Arc::new(r),
            Err(e) => {
                self.redactors().down.insert(project, Instant::now());
                return Err(e);
            }
        };
        let mut cache = self.redactors();
        cache.down.remove(&project);
        // Lu avant une invalidation : servi cette fois, pas garde.
        if cache.version == version {
            cache.projects.retain(|_, c| c.at.elapsed() < REDACTOR_TTL);
            cache.projects.insert(
                project,
                Cached {
                    at: Instant::now(),
                    redactor: redactor.clone(),
                },
            );
        }
        Ok(redactor)
    }

    /// Masquage pour la diffusion, sans attendre plus de `INLINE_WAIT` : une
    /// lecture est lancee en tache si besoin.
    async fn live_redactor(&self, project: Uuid) -> Live {
        let wait = {
            let mut cache = self.redactors();
            if let Some(c) = cache.projects.get(&project)
                && c.at.elapsed() < REDACTOR_TTL
            {
                return Live::Ready(c.redactor.clone());
            }
            if cache.loading.contains(&project) {
                return if cache.down.contains_key(&project) {
                    Live::Down
                } else {
                    Live::Pending
                };
            }
            cache.loading.insert(project);
            drop(cache);
            self.spawn_load(project)
        };
        let _ = tokio::time::timeout(INLINE_WAIT, wait).await;
        let cache = self.redactors();
        match cache.projects.get(&project) {
            Some(c) if c.at.elapsed() < REDACTOR_TTL => Live::Ready(c.redactor.clone()),
            _ if cache.down.contains_key(&project) => Live::Down,
            _ => Live::Pending,
        }
    }

    /// Lignes retenues pour le projet, sur l'une de ses cibles.
    fn holds(&self, project: Uuid) -> bool {
        self.targets()
            .entries
            .values()
            .any(|e| e.project == project && e.holds())
    }

    /// Une tache lit les secrets du projet si des lignes attendent.
    fn ensure_loading(&self, project: Uuid) {
        if self.redactors().loading.insert(project) {
            drop(self.spawn_load(project));
        }
    }

    /// Fin de la tache de lecture ; `false` si elle doit continuer (des
    /// lignes attendent et aucune autre tache ne lit).
    fn finish_load(&self, project: Uuid) -> bool {
        self.redactors().loading.remove(&project);
        if self.holds(project) {
            return !self.redactors().loading.insert(project);
        }
        true
    }

    /// Tache de lecture (`loading` deja pose) : libere les lignes retenues
    /// une fois les secrets lus, reessaie toutes les `BACKOFF` tant que des
    /// lignes attendent. Signale `wait` apres le premier essai.
    fn spawn_load(&self, project: Uuid) -> oneshot::Receiver<()> {
        let hub = self.clone();
        let (done, wait) = oneshot::channel();
        tokio::spawn(async move {
            let mut done = Some(done);
            loop {
                let result = hub.project_redactor(project).await;
                let ok = result.is_ok();
                match &result {
                    Ok(redactor) => hub.release_project(project, redactor),
                    Err(SecretsUnavailable) => {
                        let mut targets = hub.targets();
                        for entry in targets.entries.values_mut() {
                            if entry.project == project {
                                entry.notify_down();
                            }
                        }
                    }
                }
                if let Some(d) = done.take() {
                    let _ = d.send(());
                }
                if !ok && hub.holds(project) {
                    tokio::time::sleep(BACKOFF).await;
                    continue;
                }
                if hub.finish_load(project) {
                    break;
                }
            }
        });
        wait
    }

    /// Libere les lignes retenues du projet, dans l'ordre : sorties sous le
    /// verrou, masquees sans lui, diffusees sous le verrou ; recommence tant
    /// que des lignes sont arrivees entre-temps.
    fn release_project(&self, project: Uuid, redactor: &Redactor) {
        loop {
            let batches: Vec<Release> = {
                let mut targets = self.targets();
                targets
                    .entries
                    .iter_mut()
                    .filter(|(_, e)| e.project == project)
                    .filter_map(|(target, e)| e.take_held(*target))
                    .collect()
            };
            if batches.is_empty() {
                return;
            }
            // Masquage hors du verrou.
            let masked: Vec<(LogTarget, u64, Vec<LogEvent>)> = batches
                .into_iter()
                .map(|b| {
                    let mut events = Vec::with_capacity(b.lines.len() + 1);
                    if b.lost > 0 {
                        events.push(LogEvent::lost(b.lost));
                    }
                    events.extend(
                        b.lines
                            .iter()
                            .map(|line| redacted(line, &b.container, redactor)),
                    );
                    (b.target, b.id, events)
                })
                .collect();
            let mut targets = self.targets();
            for (target, id, events) in masked {
                // Entree supprimee ou recreee entre-temps : ces lignes ne
                // sont pas les siennes.
                let Some(entry) = targets.entries.get_mut(&target).filter(|e| e.id == id) else {
                    continue;
                };
                for event in events {
                    entry.emit(event);
                }
                entry.releasing = false;
            }
        }
    }

    /// Secrets du projet modifies : le masquage est relu a la ligne
    /// suivante (flux en direct et enregistrement).
    pub fn invalidate(&self, project: Uuid) {
        let mut cache = self.redactors();
        cache.version += 1;
        cache.projects.remove(&project);
    }

    /// Lecteurs en cours, toutes cibles confondues.
    pub fn streams(&self) -> usize {
        self.targets().entries.values().map(|e| e.subscribers).sum()
    }

    fn order(&self, server: ServerId, msg: ControlToAgent) -> bool {
        match self.inner.agents.send(server, msg) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(%server, "ordre de journaux non envoye : {e}");
                false
            }
        }
    }

    /// Nouveau lecteur de `source.target`, avec au plus `backlog` lignes
    /// recentes.
    pub fn subscribe(&self, source: Source, backlog: usize) -> Subscription {
        let target = source.target;
        let mut targets = self.targets();
        let Targets {
            entries,
            stopped,
            next_id,
        } = &mut *targets;
        let entry = entries.entry(target).or_insert_with(|| {
            stopped.remove(&target);
            *next_id += 1;
            Entry {
                id: *next_id,
                tx: broadcast::channel(CLIENT_BUFFER).0,
                subscribers: 0,
                follow: None,
                origin: None,
                container: source.container,
                project: source.project,
                recent: VecDeque::new(),
                held: VecDeque::new(),
                held_bytes: 0,
                held_lost: 0,
                down_notified: false,
                releasing: false,
            }
        });
        entry.subscribers += 1;
        if entry.origin.is_none() {
            entry.origin = source.origin.or(source.follow.map(|(server, _)| server));
        }
        if entry.follow.is_none()
            && let Some((server, tail)) = source.follow
        {
            // Retenu meme si l'agent est absent : redemande a sa reconnexion
            // (`State`).
            entry.follow = Some(server);
            self.order(
                server,
                ControlToAgent::Logs {
                    target,
                    tail: tail.min(MAX_TAIL),
                    follow: true,
                },
            );
        }
        let rx = entry.tx.subscribe();
        let skip = entry.recent.len().saturating_sub(backlog);
        let backlog = entry.recent.iter().skip(skip).cloned().collect();
        Subscription {
            hub: self.clone(),
            target,
            rx,
            backlog,
        }
    }

    fn release(&self, target: LogTarget) {
        let mut targets = self.targets();
        let Some(entry) = targets.entries.get_mut(&target) else {
            return;
        };
        entry.subscribers -= 1;
        if entry.subscribers > 0 {
            return;
        }
        let follow = entry.follow;
        targets.entries.remove(&target);
        if let Some(server) = follow {
            targets.stopped.insert(target);
            self.order(server, ControlToAgent::StopLogs { target });
        }
    }

    /// Message d'un agent : ligne diffusee (masquee) a ses lecteurs ; a la
    /// (re)connexion de l'agent (`State`), les suivis en cours lui sont
    /// redemandes. N'attend jamais un lecteur.
    pub async fn dispatch(&self, server: ServerId, msg: &AgentToControl) {
        match msg {
            AgentToControl::LogLine(line) => {
                let seen = match self.targets().entries.get(&line.target) {
                    // Ligne d'un autre agent que celui de la cible : ignoree,
                    // sans meme lire les secrets du projet.
                    Some(e) if e.origin.is_some_and(|origin| origin != server) => {
                        tracing::debug!(%server, "ligne de journal d'une cible d'un autre serveur : ignoree");
                        return;
                    }
                    Some(e) => Some((e.id, e.project, e.container.clone())),
                    None => None,
                };
                let project = seen.as_ref().map(|(_, project, _)| *project);
                let redactor = match project {
                    Some(project) => Some(self.live_redactor(project).await),
                    None => None,
                };
                // Masquage hors du verrou des cibles.
                let event = match (&redactor, &seen) {
                    (Some(Live::Ready(redactor)), Some((id, _, container))) => {
                        Some((*id, redacted(line, container, redactor)))
                    }
                    _ => None,
                };
                let mut targets = self.targets();
                if let Some(entry) = targets.entries.get(&line.target)
                    && entry.origin.is_some_and(|origin| origin != server)
                {
                    // Cible recreee entre-temps pour un autre serveur.
                    return;
                }
                if let Some(entry) = targets.entries.get_mut(&line.target)
                    && let Some(redactor) = redactor.filter(|_| Some(entry.project) == project)
                {
                    // Serveur encore inconnu : le premier qui envoie une ligne.
                    entry.origin.get_or_insert(server);
                    match (redactor, event) {
                        // Aucune ligne retenue devant elle, meme entree :
                        // diffusee telle que masquee.
                        (Live::Ready(_), Some((id, event))) if id == entry.id && !entry.busy() => {
                            entry.emit(event);
                        }
                        // Lignes retenues devant elle : elle attend son tour,
                        // une tache libere le tout dans l'ordre.
                        (Live::Ready(_), _) => entry.hold(line),
                        // Echec ferme : rien ne part sans masquage ; une
                        // tache libere les lignes retenues.
                        (Live::Pending, _) => entry.hold(line),
                        (Live::Down, _) => {
                            entry.hold(line);
                            entry.notify_down();
                        }
                    }
                    let waiting = entry.holds().then_some(entry.project);
                    drop(targets);
                    if let Some(project) = waiting {
                        self.ensure_loading(project);
                    }
                } else if !targets.entries.contains_key(&line.target)
                    && matches!(line.target, LogTarget::Service(_))
                    && targets.stopped.insert(line.target)
                {
                    // Suivi que personne n'ecoute ici (plan de controle
                    // redemarre) : l'agent est prie d'arreter, une fois.
                    self.order(
                        server,
                        ControlToAgent::StopLogs {
                            target: line.target,
                        },
                    );
                }
            }
            AgentToControl::State(_) => {
                let targets = self.targets();
                for (target, entry) in &targets.entries {
                    if entry.follow == Some(server) {
                        self.order(
                            server,
                            ControlToAgent::Logs {
                                target: *target,
                                tail: 0,
                                follow: true,
                            },
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Lecteur d'une cible ; son depart est compte a sa liberation.
pub struct Subscription {
    hub: LogHub,
    target: LogTarget,
    rx: broadcast::Receiver<LogEvent>,
    backlog: VecDeque<LogEvent>,
}

impl Subscription {
    /// Ligne suivante (lignes recentes d'abord), ou le marqueur des lignes
    /// perdues si ce lecteur a deborde.
    pub async fn next(&mut self) -> Option<LogEvent> {
        if let Some(event) = self.backlog.pop_front() {
            return Some(event);
        }
        match self.rx.recv().await {
            Ok(event) => Some(event),
            Err(broadcast::error::RecvError::Lagged(n)) => Some(LogEvent::lost(n)),
            Err(broadcast::error::RecvError::Closed) => None,
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.hub.release(self.target);
    }
}

/// Suit les messages des agents jusqu'a l'arret : `events`
/// (`AgentRegistry::subscribe`, pour `State`) et `logs` (`subscribe_logs`)
/// doivent etre abonnes avant le demarrage du lien agents.
pub async fn run(
    hub: LogHub,
    mut events: broadcast::Receiver<(ServerId, AgentToControl)>,
    mut logs: broadcast::Receiver<(ServerId, AgentToControl)>,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        let event = tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
                continue;
            }
            event = events.recv() => event,
            line = logs.recv() => line,
        };
        match event {
            Ok((server, msg)) => hub.dispatch(server, &msg).await,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("{n} messages d'agents perdus (diffusion des journaux en retard)");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod held_tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
            id: 1,
            tx: broadcast::channel(4).0,
            subscribers: 0,
            follow: None,
            origin: None,
            container: "web".into(),
            project: Uuid::nil(),
            recent: VecDeque::new(),
            held: VecDeque::new(),
            held_bytes: 0,
            held_lost: 0,
            down_notified: false,
            releasing: false,
        }
    }

    fn line(n: usize) -> proto::LogLine {
        proto::LogLine {
            target: LogTarget::Deployment(shared::ids::DeploymentId(Uuid::nil())),
            stream: proto::LogStream::Stdout,
            timestamp_ms: 0,
            line: "x".repeat(n),
        }
    }

    #[test]
    fn ansi_inside_a_secret_is_stripped_before_masking_live() {
        let redactor = Redactor::new(vec![zeroize::Zeroizing::new("s3cr3t-value".to_string())]);
        let mut l = line(0);
        l.line = "k=s3cr\x1b[31m3t-va\x1b[0mlue fin".into();
        let e = entry();
        assert_eq!(redacted(&l, &e.container, &redactor).message, "k=•••• fin");
    }

    fn source(project: Uuid) -> (LogTarget, Source) {
        let target = LogTarget::Deployment(shared::ids::DeploymentId(Uuid::new_v4()));
        let source = Source {
            target,
            project,
            container: "build".into(),
            follow: None,
            origin: None,
        };
        (target, source)
    }

    fn seed(hub: &LogHub, project: Uuid, redactor: Redactor) {
        hub.redactors().projects.insert(
            project,
            Cached {
                at: Instant::now(),
                redactor: Arc::new(redactor),
            },
        );
    }

    /// Une ligne d'un projet en cours de masquage ne retarde pas celles d'un
    /// autre projet : le masquage se fait hors du verrou des cibles.
    ///
    /// `run` appelle `dispatch` en serie : en production, la ligne suivante
    /// de la file attend la fin du masquage de la precedente (lineaire,
    /// quelques dizaines de µs pour 16 Kio). Ce test appelle `dispatch` en
    /// parallele pour isoler le verrou partage : abonnements, departs,
    /// liberations des lignes retenues (taches a part) et compteurs ne
    /// doivent jamais attendre un masquage. Il ne pretend pas que la file
    /// des agents soit traitee en parallele.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn masking_one_project_does_not_delay_another() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let hub = LogHub::new(
            AgentRegistry::new(),
            Arc::new(super::super::redact::NoSecrets),
        );
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let go_rx = Mutex::new(go_rx);
        let released_by_b = Arc::new(AtomicBool::new(false));
        let flag = released_by_b.clone();
        let mut gated = Redactor::new(vec![zeroize::Zeroizing::new("secret-of-a".to_string())]);
        gated.gate = Some(Arc::new(move || {
            let _ = entered_tx.send(());
            let ok = go_rx
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .recv_timeout(Duration::from_secs(5))
                .is_ok();
            flag.store(ok, Ordering::SeqCst);
        }));
        seed(&hub, a, gated);
        seed(&hub, b, Redactor::new(Vec::new()));
        let (target_a, source_a) = source(a);
        let (target_b, source_b) = source(b);
        let mut sub_a = hub.subscribe(source_a, 0);
        let mut sub_b = hub.subscribe(source_b, 0);
        let server = ServerId(Uuid::new_v4());
        let line_of = |target, text: &str| {
            AgentToControl::LogLine(proto::LogLine {
                target,
                stream: proto::LogStream::Stdout,
                timestamp_ms: 1,
                line: text.to_string(),
            })
        };

        // Ligne de A : son masquage reste bloque jusqu'au feu vert.
        let hub_a = hub.clone();
        let msg_a = line_of(target_a, "a secret-of-a");
        let task_a = tokio::spawn(async move { hub_a.dispatch(server, &msg_a).await });
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .expect("masquage de A commence");

        // Pendant ce temps, la ligne de B passe.
        hub.dispatch(server, &line_of(target_b, "ligne de b")).await;
        let got = tokio::time::timeout(Duration::from_secs(1), sub_b.next())
            .await
            .expect("ligne de B retardee")
            .unwrap();
        assert_eq!(got.message, "ligne de b");
        assert_eq!(hub.streams(), 2);

        go_tx.send(()).unwrap();
        task_a.await.unwrap();
        assert!(
            released_by_b.load(Ordering::SeqCst),
            "B attendait le masquage de A"
        );
        assert_eq!(sub_a.next().await.unwrap().message, "a ••••");
    }

    /// Lignes retenues puis liberees (masquees hors du verrou) : dans
    /// l'ordre, une nouvelle ligne arrivee entre-temps passe apres elles.
    #[tokio::test]
    async fn held_lines_released_in_order_before_new_ones() {
        let hub = LogHub::new(
            AgentRegistry::new(),
            Arc::new(super::super::redact::NoSecrets),
        );
        let project = Uuid::new_v4();
        let (target, source) = source(project);
        let mut sub = hub.subscribe(source, 0);
        {
            let mut targets = hub.targets();
            let entry = targets.entries.get_mut(&target).unwrap();
            for text in ["un secret-x", "deux"] {
                let mut l = line(0);
                l.target = target;
                l.line = text.into();
                entry.hold(&l);
            }
        }
        seed(
            &hub,
            project,
            Redactor::new(vec![zeroize::Zeroizing::new("secret-x".to_string())]),
        );
        let server = ServerId(Uuid::new_v4());
        hub.dispatch(
            server,
            &AgentToControl::LogLine(proto::LogLine {
                target,
                stream: proto::LogStream::Stdout,
                timestamp_ms: 2,
                line: "trois secret-x".into(),
            }),
        )
        .await;
        let mut got = Vec::new();
        for _ in 0..3 {
            let e = tokio::time::timeout(Duration::from_secs(5), sub.next())
                .await
                .unwrap()
                .unwrap();
            got.push(e.message);
        }
        assert_eq!(got, ["un ••••", "deux", "trois ••••"]);
    }

    #[test]
    fn held_lines_are_bounded_in_bytes_not_only_in_count() {
        let mut e = entry();
        // 64 Kio chacune : le plafond d'octets joue bien avant celui des lignes.
        let big = 64 * 1024;
        let fit = MAX_HELD_BYTES / big;
        for _ in 0..fit + 50 {
            e.hold(&line(big));
        }
        assert_eq!(e.held.len(), fit);
        assert!(e.held_bytes <= MAX_HELD_BYTES);
        assert_eq!(e.held_lost, 50);
        // Petites lignes : plafond en nombre de lignes inchange.
        let mut e = entry();
        for _ in 0..RECENT_LINES + 3 {
            e.hold(&line(10));
        }
        assert_eq!(e.held.len(), RECENT_LINES);
        assert_eq!(e.held_lost, 3);
    }
}
