//! File de taches PostgreSQL, faite main (voir `decisions.md` : pas d'apalis).
//! Elle porte les deploiements (Task 25), le nettoyage, l'emission ACME
//! (Task 30), les agregats de metriques, les mises a jour et les
//! notifications e-mail (`notify`).
//!
//! # Garanties
//!
//! - **Au moins une fois** : une tache dont le travailleur tombe est reprise
//!   quand son bail (5 minutes, prolonge par battement de coeur) expire. Les
//!   gestionnaires DOIVENT donc etre idempotents.
//! - **Pas de course a la prise** : `claim` choisit et marque la tache dans
//!   une seule instruction (`SELECT ... FOR UPDATE SKIP LOCKED`).
//! - **Seul le detenteur du bail conclut** : `complete`, `fail` et `heartbeat`
//!   ne reussissent que pour le travailleur courant, bail non echu. Une tache
//!   reprise ne peut plus etre conclue par le travailleur tombe.
//! - **Tentatives bornees** : chaque prise compte une tentative ; au-dela de
//!   `max_attempts` la tache passe a l'etat terminal `dead`, avec sa derniere
//!   erreur. Un echec peut demander une reprise differee (`run_at`).
//! - **Deduplication** : une `dedupe_key` est unique parmi les taches non
//!   terminales (`queued`, `running`) ; `enqueue` renvoie alors la tache
//!   existante, sans erreur. `enqueue_unless_queued` ne cree pas de seconde
//!   tache identique (meme genre, meme charge utile) en attente.
//! - **Priorite** : fixee par le genre ([`JobKind::priority`]) ; les
//!   deploiements passent avant les certificats, le nettoyage et les
//!   notifications. Emissions ACME et notifications ont chacune leur plafond
//!   de taches en cours ([`ACME_CONCURRENCY`], [`NOTIFY_CONCURRENCY`]) : un
//!   serveur lent ne prive pas les deploiements de travailleurs.
//! - **Fin d'une tache morte** : le gestionnaire de fin du genre
//!   (`Handlers::on_dead`) est appele au plus une fois quand une tache
//!   passe `dead` (un arret du processus peut le perdre).
//! - **Purge** : les taches terminees (`done`, `dead`) sont supprimees apres
//!   [`FINISHED_RETENTION`] (entretien horaire, `purge_finished`).
//!
//! # Donnees
//!
//! Les charges utiles ne portent QUE des identifiants (projet, service,
//! deploiement...), jamais de secret : elles sont stockees en clair en base.
//! Un gestionnaire relit ce dont il a besoin (secrets compris) a l'execution.
//! Les erreurs stockees sont tronquees (`MAX_ERROR_CHARS`) et ne doivent
//! contenir aucun secret.

pub mod queue;
pub mod worker;

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

pub use queue::{
    Schedule, claim, claim_next, complete, enqueue, enqueue_in, enqueue_unless_queued,
    enqueue_with, fail, heartbeat, purge_finished, reap_expired,
};
use serde_json::Value;
use shared::ids::JobId;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

/// Duree du bail pris a chaque `claim` (et a chaque battement de coeur).
pub const LEASE: Duration = Duration::from_secs(5 * 60);

/// Longueur maximale (en caracteres) de l'erreur conservee.
pub const MAX_ERROR_CHARS: usize = 1000;

/// Canal LISTEN/NOTIFY qui reveille les travailleurs a chaque ajout.
pub const NOTIFY_CHANNEL: &str = "ciphera_jobs";

/// Emissions ACME en cours a la fois sur l'instance (chacune attend jusqu'a
/// 200 s l'issue du proxy) : les autres travailleurs restent aux autres
/// taches.
pub const ACME_CONCURRENCY: i64 = 1;

/// Notifications e-mail en cours a la fois sur l'instance (un serveur SMTP
/// muet retient sa tache jusqu'a 2 minutes).
pub const NOTIFY_CONCURRENCY: i64 = 1;

/// Duree de conservation des taches terminees (`done`, `dead`).
pub const FINISHED_RETENTION: Duration = Duration::from_secs(7 * 86_400);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobKind {
    Deploy,
    Cleanup,
    AcmeIssue,
    MetricsRollup,
    Update,
    Notify,
}

impl JobKind {
    pub const ALL: [JobKind; 6] = [
        JobKind::Deploy,
        JobKind::Cleanup,
        JobKind::AcmeIssue,
        JobKind::MetricsRollup,
        JobKind::Update,
        JobKind::Notify,
    ];

    /// Priorite de prise (plus grand = plus urgent) : deploiement 100,
    /// emission ACME 50, nettoyage 10, le reste 0.
    pub fn priority(self) -> i16 {
        match self {
            JobKind::Deploy => 100,
            JobKind::AcmeIssue => 50,
            JobKind::Cleanup => 10,
            JobKind::MetricsRollup | JobKind::Update | JobKind::Notify => 0,
        }
    }

    /// Valeur stockee (contrainte `CHECK` de la migration).
    pub fn as_str(self) -> &'static str {
        match self {
            JobKind::Deploy => "deploy",
            JobKind::Cleanup => "cleanup",
            JobKind::AcmeIssue => "acme_issue",
            JobKind::MetricsRollup => "metrics_rollup",
            JobKind::Update => "update",
            JobKind::Notify => "notify",
        }
    }
}

impl fmt::Display for JobKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for JobKind {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        JobKind::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| format!("genre de tache inconnu : {s}"))
    }
}

/// Tache prise par un travailleur.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: JobId,
    pub kind: JobKind,
    /// Identifiants seulement, jamais de secret.
    pub payload: Value,
    /// Tentative en cours (1 a la premiere prise).
    pub attempts: i32,
    pub max_attempts: i32,
}

/// Issue d'un echec signale par `fail`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Remise en file apres ce delai, tant qu'il reste des tentatives (sinon `dead`).
    Retry(Duration),
    /// Etat terminal `dead` tout de suite.
    Terminal,
}
