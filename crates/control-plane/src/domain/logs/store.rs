//! Lignes des deploiements enregistrees en base (`deployment_logs`) :
//! historique du flux en direct et `logs.txt`.
//!
//! Plafonds : `MAX_LINES_PER_DEPLOYMENT` lignes par deploiement (les
//! dernieres sont gardees, les plus anciennes supprimees),
//! `MAX_LINE_BYTES` octets par ligne, et
//! `MAX_DOWNLOAD_BYTES` pour un telechargement. Les lignes sont masquees
//! avant l'insertion ; elles sont supprimees avec leur deploiement (projet
//! supprime, nettoyage de la retention). Masquage indisponible : les lignes
//! sont retenues en memoire (au plus `MAX_HELD` par deploiement, puis « N
//! lignes perdues » a la liberation) derriere un
//! seul marqueur enregistre, et reessayees au lot suivant ou a l'echeance.

use std::collections::HashMap;
use std::time::Duration;

use proto::{AgentToControl, LogStream, LogTarget};
use shared::ids::ServerId;
use sqlx::{PgConnection, PgPool};
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

pub use super::MAX_LINE_BYTES;
use super::redact::Redactor;
use super::{LogEvent, truncate};
use crate::state::AppState;

/// Lignes gardees au plus par deploiement.
pub const MAX_LINES_PER_DEPLOYMENT: i64 = 10_000;
/// Taille maximale d'un `logs.txt` (octets), marqueur de fin compris.
pub const MAX_DOWNLOAD_BYTES: usize = 4 * 1024 * 1024;
/// Derniere ligne d'un fichier tronque.
pub const TRUNCATED: &str = "[journal tronqué]\n";
/// Lignes lues par requete.
const PAGE: i64 = 500;
/// Lignes enregistrees au plus par lot.
const BATCH: usize = 500;
/// Projets des deploiements gardes au plus (vides a chaque echeance).
const PROJECTS_TTL: Duration = Duration::from_secs(60);
/// Lignes retenues au plus par deploiement quand le masquage est indisponible.
const MAX_HELD: usize = 5_000;
/// Octets bruts retenus au plus par file (une ligne brute peut peser 64 Kio) :
/// meme budget qu'avec des lignes de 16 Kio. `Held` a deux files (`lines` et
/// `unchecked`) : 2 x 80 Mio, soit 160 Mio par deploiement. Aucun plafond
/// global sur le nombre de deploiements retenus (existant avant ce correctif).
const MAX_HELD_BYTES: usize = MAX_HELD * 16 * 1024;
/// Reessai des lignes retenues.
const HELD_RETRY: Duration = Duration::from_secs(2);

/// Ligne a enregistrer (deja masquee).
#[derive(Clone, Debug)]
pub struct NewLine {
    pub timestamp_ms: i64,
    pub stream: LogStream,
    pub line: String,
}

/// Ligne enregistree.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct StoredLine {
    pub seq: i64,
    pub ts_ms: i64,
    pub stream: String,
    pub line: String,
}

impl StoredLine {
    /// Ligne pour le panneau, masquee de nouveau (secrets ajoutes depuis).
    pub fn event(self, redactor: &Redactor) -> LogEvent {
        let stream = if self.stream == "stderr" {
            LogStream::Stderr
        } else {
            LogStream::Stdout
        };
        LogEvent::line(
            self.ts_ms,
            stream,
            super::BUILD_CONTAINER,
            redactor.redact_line(&self.line),
        )
    }
}

fn stream_name(stream: LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "stdout",
        LogStream::Stderr => "stderr",
    }
}

/// Ajoute des lignes a un deploiement ; au-dela du plafond, les plus
/// anciennes sont supprimees. Renvoie le nombre de lignes enregistrees (0
/// si le deploiement n'existe pas).
pub async fn append(db: &PgPool, deployment: Uuid, lines: &[NewLine]) -> sqlx::Result<u64> {
    if lines.is_empty() {
        return Ok(0);
    }
    let mut tx = db.begin().await?;
    let count: Option<i32> =
        sqlx::query_scalar("SELECT log_lines FROM deployments WHERE id = $1 FOR UPDATE")
            .bind(deployment)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(count) = count else {
        return Ok(0);
    };
    let cap = usize::try_from(MAX_LINES_PER_DEPLOYMENT).unwrap_or(usize::MAX);
    // Lot plus grand que le plafond : ses dernieres lignes seulement.
    let lines = &lines[lines.len().saturating_sub(cap)..];
    let take = lines.len();
    let ts: Vec<i64> = lines.iter().map(|l| l.timestamp_ms).collect();
    let streams: Vec<&str> = lines.iter().map(|l| stream_name(l.stream)).collect();
    // PostgreSQL refuse l'octet nul dans un `text`.
    let texts: Vec<String> = lines
        .iter()
        .map(|l| truncate(l.line.replace('\0', "")))
        .collect();
    sqlx::query(
        "INSERT INTO deployment_logs (deployment_id, ts_ms, stream, line) \
         SELECT $1, t.ts, t.stream, t.line \
         FROM UNNEST($2::bigint[], $3::text[], $4::text[]) WITH ORDINALITY AS t(ts, stream, line, n) \
         ORDER BY t.n",
    )
    .bind(deployment)
    .bind(&ts)
    .bind(&streams)
    .bind(&texts)
    .execute(&mut *tx)
    .await?;
    let total = i64::from(count) + i64::try_from(take).unwrap_or(i64::MAX);
    let excess = total - MAX_LINES_PER_DEPLOYMENT;
    if excess > 0 {
        // Les plus anciennes au-dela du plafond.
        sqlx::query(
            "DELETE FROM deployment_logs WHERE deployment_id = $1 AND seq IN \
             (SELECT seq FROM deployment_logs WHERE deployment_id = $1 ORDER BY seq LIMIT $2)",
        )
        .bind(deployment)
        .bind(excess)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE deployments SET log_lines = $2 WHERE id = $1")
        .bind(deployment)
        .bind(i32::try_from(total.min(MAX_LINES_PER_DEPLOYMENT)).unwrap_or(i32::MAX))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(take as u64)
}

/// Les `n` dernieres lignes d'un deploiement, dans l'ordre.
pub async fn tail(db: &PgPool, deployment: Uuid, n: i64) -> sqlx::Result<Vec<StoredLine>> {
    let mut rows: Vec<StoredLine> = sqlx::query_as(
        "SELECT seq, ts_ms, stream, line FROM deployment_logs \
         WHERE deployment_id = $1 ORDER BY seq DESC LIMIT $2",
    )
    .bind(deployment)
    .bind(n)
    .fetch_all(db)
    .await?;
    rows.reverse();
    Ok(rows)
}

/// Contenu de `logs.txt` : lignes masquees, au plus `MAX_DOWNLOAD_BYTES`
/// octets (lu par pages, jamais en entier en memoire au-dela du plafond).
pub async fn download(db: &PgPool, deployment: Uuid, redactor: &Redactor) -> sqlx::Result<String> {
    let mut out = String::new();
    let mut after = 0i64;
    loop {
        let page: Vec<StoredLine> = sqlx::query_as(
            "SELECT seq, ts_ms, stream, line FROM deployment_logs \
             WHERE deployment_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
        )
        .bind(deployment)
        .bind(after)
        .bind(PAGE)
        .fetch_all(db)
        .await?;
        let Some(last) = page.last() else {
            return Ok(out);
        };
        after = last.seq;
        for row in page {
            let line = export_line(row, redactor);
            if out.len() + line.len() + TRUNCATED.len() > MAX_DOWNLOAD_BYTES {
                out.push_str(TRUNCATED);
                return Ok(out);
            }
            out.push_str(&line);
        }
    }
}

/// Masque les lignes avant l'enregistrement (ANSI retire d'abord).
fn mask_lines(lines: &mut [NewLine], redactor: &Redactor) {
    for line in lines {
        line.line = redactor.redact_line(&line.line);
    }
}

/// Ligne de `logs.txt` : sequences ANSI et caracteres de controle retires
/// AVANT le masquage (un secret entrecoupe de codes ANSI doit etre reconnu).
fn export_line(mut row: StoredLine, redactor: &Redactor) -> String {
    row.line = super::neutralize(&row.line);
    row.event(redactor).text_line()
}

/// Supprime les lignes d'un deploiement (nettoyage de la retention).
pub async fn delete(conn: &mut PgConnection, deployment: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM deployment_logs WHERE deployment_id = $1")
        .bind(deployment)
        .execute(conn)
        .await?;
    Ok(())
}

/// Projet de chaque deploiement (le masquage est garde par le `LogHub`,
/// relu apres une modification des secrets).
struct Projects {
    /// Projet et serveur de chaque deploiement (`None` : inconnu).
    cache: HashMap<Uuid, Option<(Uuid, Option<Uuid>)>>,
    /// Lignes brutes retenues faute de masquage (ou de base), par deploiement.
    held: HashMap<Uuid, Held>,
}

#[derive(Default)]
struct Held {
    lines: Vec<NewLine>,
    /// Octets de `lines` (total courant).
    lines_bytes: usize,
    /// Lignes dont le serveur n'a pas pu etre verifie (base indisponible),
    /// avec le serveur qui les a envoyees : reverifiees au prochain essai.
    unchecked: Vec<(Uuid, NewLine)>,
    /// Octets de `unchecked` (total courant).
    unchecked_bytes: usize,
    /// Marqueur `MASKING_DOWN` deja enregistre.
    marked: bool,
    /// Lignes retenues perdues au-dela de `MAX_HELD` (marqueur « N lignes
    /// perdues » enregistre a la liberation).
    lost: u64,
}

impl Held {
    /// Retient `lines` ; au-dela de `MAX_HELD`, les plus anciennes sont
    /// perdues et comptees.
    fn keep(&mut self, mut lines: Vec<NewLine>) {
        self.lines_bytes += lines.iter().map(|l| l.line.len()).sum::<usize>();
        self.lines.append(&mut lines);
        let excess = excess_held(&self.lines, &mut self.lines_bytes, |l| l.line.len());
        self.lines.drain(..excess);
        self.lost += excess as u64;
    }

    /// Vide les lignes retenues (liberation).
    fn take_lines(&mut self) -> Vec<NewLine> {
        self.lines_bytes = 0;
        std::mem::take(&mut self.lines)
    }

    /// Vide les lignes a verifier (nouvel essai).
    fn take_unchecked(&mut self) -> Vec<(Uuid, NewLine)> {
        self.unchecked_bytes = 0;
        std::mem::take(&mut self.unchecked)
    }

    /// Retient une ligne a verifier ; meme borne que `keep`.
    fn keep_unchecked(&mut self, server: Uuid, line: NewLine) {
        self.unchecked_bytes += line.line.len();
        self.unchecked.push((server, line));
        let excess = excess_held(&self.unchecked, &mut self.unchecked_bytes, |(_, l)| {
            l.line.len()
        });
        self.unchecked.drain(..excess);
        self.lost += excess as u64;
    }
}

/// Nombre de lignes les plus anciennes a perdre pour tenir dans `MAX_HELD`
/// lignes et `MAX_HELD_BYTES` octets. `bytes` est le total courant de
/// `items`, mis a jour (pas de re-somme a chaque ligne).
fn excess_held<T>(items: &[T], bytes: &mut usize, size: impl Fn(&T) -> usize) -> usize {
    let mut excess = 0;
    while excess < items.len() && (items.len() - excess > MAX_HELD || *bytes > MAX_HELD_BYTES) {
        *bytes = bytes.saturating_sub(size(&items[excess]));
        excess += 1;
    }
    excess
}

/// Serveur d'une ligne de journal par rapport a son deploiement.
enum Sender {
    /// Le serveur du deploiement.
    Its,
    /// Un autre serveur, ou un deploiement inconnu ou sans serveur.
    Other,
    /// Base indisponible : a reverifier.
    Unknown,
}

impl Projects {
    /// `None` : deploiement inconnu (lignes ignorees).
    async fn get(&mut self, state: &AppState, deployment: Uuid) -> sqlx::Result<Option<Uuid>> {
        Ok(self
            .lookup(state, deployment)
            .await?
            .map(|(project, _)| project))
    }

    /// Projet et serveur du deploiement ; un serveur encore inconnu
    /// (deploiement en file) n'est pas garde en cache.
    async fn lookup(
        &mut self,
        state: &AppState,
        deployment: Uuid,
    ) -> sqlx::Result<Option<(Uuid, Option<Uuid>)>> {
        if let Some(Some((project, Some(server)))) = self.cache.get(&deployment) {
            return Ok(Some((*project, Some(*server))));
        }
        if let Some(None) = self.cache.get(&deployment) {
            return Ok(None);
        }
        let row: Option<(Uuid, Option<Uuid>)> =
            sqlx::query_as("SELECT project_id, server_id FROM deployments WHERE id = $1")
                .bind(deployment)
                .fetch_optional(&state.db)
                .await?;
        self.cache.insert(deployment, row);
        Ok(row)
    }

    /// Ligne envoyee par l'agent du serveur du deploiement ?
    async fn sender(&mut self, state: &AppState, deployment: Uuid, server: Uuid) -> Sender {
        match self.lookup(state, deployment).await {
            Ok(Some((_, Some(expected)))) if expected == server => Sender::Its,
            Ok(_) => Sender::Other,
            Err(_) => Sender::Unknown,
        }
    }
}

/// Ligne d'un deploiement envoyee par l'agent `server`.
fn deployment_line((server, msg): (ServerId, AgentToControl)) -> Option<(Uuid, Uuid, NewLine)> {
    match msg {
        AgentToControl::LogLine(l) => match l.target {
            LogTarget::Deployment(id) => Some((
                id.0,
                server.0,
                NewLine {
                    timestamp_ms: l.timestamp_ms,
                    stream: l.stream,
                    line: l.line,
                },
            )),
            LogTarget::Service(_) => None,
        },
        _ => None,
    }
}

async fn save(state: &AppState, projects: &mut Projects, batch: Vec<(Uuid, Uuid, NewLine)>) {
    let mut by_deployment: Vec<(Uuid, Vec<NewLine>)> = Vec::new();
    // Lignes retenues d'abord (ordre d'arrivee) ; celles dont le serveur
    // n'a pas pu etre verifie repassent par le controle.
    let mut pending = Vec::new();
    for (id, held) in &mut projects.held {
        by_deployment.push((*id, held.take_lines()));
        pending.extend(
            held.take_unchecked()
                .into_iter()
                .map(|(server, line)| (*id, server, line)),
        );
    }
    pending.extend(batch);
    for (id, server, line) in pending {
        // Seul l'agent du serveur du deploiement en ecrit le journal.
        match projects.sender(state, id, server).await {
            Sender::Its => {}
            Sender::Other => {
                tracing::debug!(deployment = %id, %server, "ligne de journal d'un autre serveur : ignoree");
                continue;
            }
            Sender::Unknown => {
                projects
                    .held
                    .entry(id)
                    .or_default()
                    .keep_unchecked(server, line);
                continue;
            }
        }
        match by_deployment.iter_mut().find(|(d, _)| *d == id) {
            Some((_, lines)) => lines.push(line),
            None => by_deployment.push((id, vec![line])),
        }
    }
    for (id, mut lines) in by_deployment {
        let redactor = match projects.get(state, id).await {
            Ok(Some(project)) => state.logs.project_redactor(project).await,
            Ok(None) => {
                projects.held.remove(&id);
                continue;
            }
            Err(e) => {
                // Base indisponible : lignes retenues, reessayees ensuite.
                tracing::error!(error = %e, deployment = %id, "journal retenu");
                projects.held.entry(id).or_default().keep(lines);
                continue;
            }
        };
        let Ok(redactor) = redactor else {
            // Echec ferme : rien n'est enregistre sans masquage.
            let held = projects.held.entry(id).or_default();
            if !held.marked {
                let marker = NewLine {
                    timestamp_ms: jiff::Timestamp::now().as_millisecond(),
                    stream: LogStream::Stderr,
                    line: super::hub::MASKING_DOWN.to_string(),
                };
                held.marked = append(&state.db, id, &[marker]).await.is_ok();
            }
            held.keep(lines);
            continue;
        };
        // Lignes encore a verifier : gardees (et le compte des pertes).
        let lost = match projects.held.get_mut(&id) {
            Some(h) if !h.unchecked.is_empty() => {
                h.marked = false;
                std::mem::take(&mut h.lost)
            }
            Some(_) => projects.held.remove(&id).map_or(0, |h| h.lost),
            None => 0,
        };
        mask_lines(&mut lines, &redactor);
        if lost > 0 {
            // Meme texte que le marqueur du flux en direct.
            let marker = NewLine {
                timestamp_ms: lines.first().map_or_else(
                    || jiff::Timestamp::now().as_millisecond(),
                    |l| l.timestamp_ms,
                ),
                stream: LogStream::Stderr,
                line: LogEvent::lost(lost).message,
            };
            lines.insert(0, marker);
        }
        if let Err(e) = append(&state.db, id, &lines).await {
            tracing::error!(error = %e, deployment = %id, "journal non enregistre");
        }
    }
}

/// Enregistre les lignes des deploiements jusqu'a l'arret (par lots).
/// `events` (`AgentRegistry::subscribe_logs`) doit etre abonne avant le
/// demarrage du lien agents.
pub async fn run(
    state: AppState,
    mut events: broadcast::Receiver<(ServerId, AgentToControl)>,
    mut stop: watch::Receiver<bool>,
) {
    let mut projects = Projects {
        cache: HashMap::new(),
        held: HashMap::new(),
    };
    let mut expire = tokio::time::interval(PROJECTS_TTL);
    let mut retry = tokio::time::interval(HELD_RETRY);
    loop {
        let first = tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
                continue;
            }
            _ = expire.tick() => {
                projects.cache.clear();
                continue;
            }
            _ = retry.tick(), if !projects.held.is_empty() => {
                save(&state, &mut projects, Vec::new()).await;
                continue;
            }
            event = events.recv() => match event {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("{n} messages d'agents perdus (enregistrement des journaux en retard)");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
        };
        let mut batch: Vec<(Uuid, Uuid, NewLine)> = deployment_line(first).into_iter().collect();
        if batch.is_empty() {
            continue;
        }
        while batch.len() < BATCH {
            match events.try_recv() {
                Ok(event) => batch.extend(deployment_line(event)),
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    tracing::warn!(
                        "{n} messages d'agents perdus (enregistrement des journaux en retard)"
                    );
                }
                Err(_) => break,
            }
        }
        save(&state, &mut projects, batch).await;
    }
}

#[cfg(test)]
mod held_tests {
    use super::*;

    fn new_line(n: usize) -> NewLine {
        NewLine {
            timestamp_ms: 0,
            stream: LogStream::Stdout,
            line: "x".repeat(n),
        }
    }

    #[test]
    fn ansi_inside_a_secret_is_stripped_before_masking_in_the_export() {
        let redactor = Redactor::new(vec![zeroize::Zeroizing::new("s3cr3t-value".to_string())]);
        let row = StoredLine {
            seq: 1,
            ts_ms: 0,
            stream: "stdout".into(),
            line: "k=s3cr\x1b[31m3t-va\x1b[0mlue fin".into(),
        };
        let text = export_line(row, &redactor);
        assert!(text.contains("k=•••• fin"), "{text}");
        assert!(!text.contains("3t-va"), "{text}");
    }

    #[test]
    fn ansi_inside_a_secret_is_stripped_before_masking_on_save_and_replay() {
        let redactor = Redactor::new(vec![zeroize::Zeroizing::new("s3cr3t-value".to_string())]);
        let raw = "k=s3cr\x1b[31m3t-va\x1b[0mlue fin";
        // Enregistrement.
        let mut lines = vec![new_line(0)];
        lines[0].line = raw.to_string();
        mask_lines(&mut lines, &redactor);
        assert_eq!(lines[0].line, "k=•••• fin");
        // Relecture (lignes deja en base, enregistrees avant ce correctif).
        let row = StoredLine {
            seq: 1,
            ts_ms: 0,
            stream: "stdout".into(),
            line: raw.into(),
        };
        assert_eq!(row.event(&redactor).message, "k=•••• fin");
    }

    #[test]
    fn held_lines_are_bounded_in_bytes_not_only_in_count() {
        let mut held = Held::default();
        let big = 64 * 1024;
        let fit = MAX_HELD_BYTES / big;
        held.keep((0..fit + 20).map(|_| new_line(big)).collect());
        assert_eq!(held.lines.len(), fit);
        assert_eq!(held.lost, 20);
        let total: usize = held.lines.iter().map(|l| l.line.len()).sum();
        assert!(total <= MAX_HELD_BYTES);
        // Lignes a verifier : meme borne.
        let mut held = Held::default();
        for _ in 0..fit + 5 {
            held.keep_unchecked(Uuid::nil(), new_line(big));
        }
        assert_eq!(held.unchecked.len(), fit);
        assert_eq!(held.lost, 5);
        // Petites lignes : plafond en nombre de lignes inchange.
        let mut held = Held::default();
        held.keep((0..MAX_HELD + 7).map(|_| new_line(10)).collect());
        assert_eq!(held.lines.len(), MAX_HELD);
        assert_eq!(held.lost, 7);
    }
}
