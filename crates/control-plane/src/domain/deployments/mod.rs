//! Deploiements : file par environnement, pilotage de l'agent, rollback et
//! retention des images.
//!
//! - `orchestrator` : gestionnaire de la tache `deploy` (cree et fait avancer
//!   les deploiements), suivi des messages de l'agent, reconciliation a la
//!   reconnexion, chien de garde des 10 minutes, annulation et rollback ;
//! - `retention` : tache `cleanup` (5 images conservees par environnement).
//!
//! # Garanties
//!
//! - Un seul deploiement `running` par environnement : verrou consultatif
//!   transactionnel par environnement, plus un index unique partiel en base.
//!   Les autres restent `queued`, dans l'ordre d'arrivee.
//! - Le premier statut terminal fait foi, sauf : une annulation demandee par
//!   l'utilisateur qui croise un `Failed` de l'agent finit `cancelled`, et un
//!   echec presume par le plan de controle (aucune nouvelle de l'agent) cede
//!   devant l'issue reelle rapportee par l'agent a sa reconnexion.
//! - Les taches ne portent que des identifiants. Le jeton d'installation
//!   GitHub d'un build est obtenu au moment de l'envoi, place dans
//!   `BuildSpec.git_token` et jamais stocke ni journalise.

pub mod orchestrator;
pub mod retention;

use std::time::Duration;

use api_types::deployments::{Deployment, DeploymentStatus, DeploymentStep};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// Images conservees par environnement pour le rollback (spec : N = 5).
pub const RETAINED_IMAGES: i64 = 5;

/// Deploiements en file (`queued`) au plus par environnement : au-dela, une
/// nouvelle demande est refusee (429 `too_many_queued`).
pub const MAX_QUEUED: i64 = 10;

/// Sans nouvelle de l'agent pendant ce delai, un deploiement en cours passe
/// `failed` (Review Focus 4).
pub const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

/// Intervalle du chien de garde.
pub const WATCHDOG_EVERY: Duration = Duration::from_secs(60);

/// A la reconnexion, un ordre envoye depuis moins longtemps n'est pas
/// declare perdu (l'agent a pu calculer son etat avant de le lire).
pub const LOST_GRACE: Duration = Duration::from_secs(30);

/// Longueur maximale (caracteres) d'une cause d'echec conservee.
pub const MAX_MESSAGE_CHARS: usize = 512;

/// Charge utile d'une tache `deploy` (identifiants seulement).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeployJob {
    /// Push Git (webhook) : cree le deploiement `push` s'il n'existe pas.
    Push {
        project_id: Uuid,
        environment_id: Uuid,
        commit_sha: String,
    },
    /// Deploiement deja enregistre (manuel, rollback) : fait avancer son
    /// environnement.
    Deployment { deployment_id: Uuid },
    /// Fait avancer un environnement (apres la fin d'un deploiement).
    Environment { environment_id: Uuid },
}

/// Charge utile d'une tache `cleanup`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupJob {
    pub environment_id: Uuid,
}

/// Verrou consultatif de l'environnement, jusqu'a la fin de la transaction.
pub(crate) async fn lock_environment(
    conn: &mut PgConnection,
    environment: Uuid,
) -> sqlx::Result<()> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-deploy-env:' || $1::text, 0))",
    )
    .bind(environment)
    .execute(conn)
    .await?;
    Ok(())
}

/// Vrai si l'environnement a deja [`MAX_QUEUED`] deploiements en file (a
/// appeler sous le verrou de l'environnement).
pub(crate) async fn queue_full(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<bool> {
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM deployments WHERE environment_id = $1 AND status = 'queued'",
    )
    .bind(environment)
    .fetch_one(conn)
    .await?;
    Ok(queued >= MAX_QUEUED)
}

/// Cause d'echec stockable : sans caractere de controle, tronquee.
pub(crate) fn clean_message(message: &str) -> String {
    let mut out: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE_CHARS)
        .collect();
    if message.chars().count() > MAX_MESSAGE_CHARS {
        out.push('…');
    }
    out
}

const TS: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'";

fn ts(column: &str) -> String {
    format!("to_char({column} AT TIME ZONE 'UTC', {TS})")
}

/// Requete de lecture d'un deploiement, a completer par `WHERE`/`ORDER BY`.
fn select() -> String {
    format!(
        "SELECT d.id, d.number, d.project_id, p.name AS project_name, d.environment_id, \
                e.name AS environment, d.status, d.trigger, d.commit_sha, d.branch, d.image, \
                d.rollback_of, d.stage, d.error, d.cancel_requested, u.name AS created_by, \
                {created} AS created_at, {started} AS started_at, {finished} AS finished_at, \
                (EXTRACT(EPOCH FROM (d.finished_at - d.started_at)) * 1000)::bigint AS duration_ms, \
                (cur.id IS NOT NULL AND cur.id = d.id) AS current, \
                (d.status = 'success' AND d.cleaned_at IS NULL AND d.removal_requested_at IS NULL \
                 AND d.server_id IS NOT NULL \
                 AND (d.image_from IS NULL OR EXISTS (SELECT 1 FROM deployments o \
                      WHERE o.id = d.image_from AND o.cleaned_at IS NULL \
                      AND o.removal_requested_at IS NULL))) AS can_rollback, \
                COALESCE((SELECT json_agg(json_build_object( \
                      'stage', s.stage, 'status', s.status, \
                      'started_at', {s_started}, 'finished_at', {s_finished}, \
                      'duration_ms', (EXTRACT(EPOCH FROM (s.finished_at - s.started_at)) * 1000)::bigint, \
                      'message', s.message) \
                    ORDER BY array_position(ARRAY['clone','build','image','start','healthcheck','switch'], s.stage)) \
                  FROM deployment_stages s WHERE s.deployment_id = d.id), '[]'::json)::text AS steps \
         FROM deployments d \
         JOIN projects p ON p.id = d.project_id \
         JOIN environments e ON e.id = d.environment_id \
         LEFT JOIN users u ON u.id = d.created_by \
         LEFT JOIN LATERAL (SELECT c.id FROM deployments c \
              WHERE c.environment_id = d.environment_id AND c.status = 'success' \
              ORDER BY c.finished_at DESC, c.number DESC LIMIT 1) cur ON true",
        created = ts("d.created_at"),
        started = ts("d.started_at"),
        finished = ts("d.finished_at"),
        s_started = ts("s.started_at"),
        s_finished = ts("s.finished_at"),
    )
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    number: i64,
    project_id: Uuid,
    project_name: String,
    environment_id: Uuid,
    environment: String,
    status: String,
    trigger: String,
    commit_sha: Option<String>,
    branch: Option<String>,
    image: Option<String>,
    rollback_of: Option<Uuid>,
    stage: Option<String>,
    error: Option<String>,
    cancel_requested: bool,
    created_by: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    duration_ms: Option<i64>,
    current: bool,
    can_rollback: bool,
    steps: String,
}

impl Row {
    fn into_api(self) -> sqlx::Result<Deployment> {
        let broken =
            |what: &str| sqlx::Error::Protocol(format!("deploiement {} : {what}", self.id));
        let status = DeploymentStatus::parse(&self.status).ok_or_else(|| broken("statut"))?;
        let trigger = api_types::deployments::DeploymentTrigger::parse(&self.trigger)
            .ok_or_else(|| broken("declencheur"))?;
        let stage = match &self.stage {
            Some(s) => Some(
                api_types::deployments::DeploymentStage::parse(s).ok_or_else(|| broken("etape"))?,
            ),
            None => None,
        };
        let steps: Vec<DeploymentStep> =
            serde_json::from_str(&self.steps).map_err(|_| broken("etapes"))?;
        Ok(Deployment {
            id: self.id.to_string(),
            number: self.number,
            project_id: self.project_id.to_string(),
            project_name: self.project_name,
            environment_id: self.environment_id.to_string(),
            environment: self.environment,
            status,
            trigger,
            commit_sha: self.commit_sha,
            branch: self.branch,
            image: self.image,
            rollback_of: self.rollback_of.map(|id| id.to_string()),
            stage,
            steps,
            error: self.error,
            cancel_requested: self.cancel_requested,
            current: self.current,
            can_rollback: self.can_rollback,
            created_by: self.created_by,
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            duration_ms: self.duration_ms,
        })
    }
}

/// Un deploiement, tel que l'API le renvoie.
pub async fn get(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<Deployment>> {
    let row: Option<Row> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE d.id = $1", select())))
            .bind(id)
            .fetch_optional(conn)
            .await?;
    row.map(Row::into_api).transpose()
}

/// Filtres de `GET /api/deployments`.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub project: Option<Uuid>,
    pub status: Option<DeploymentStatus>,
    pub limit: i64,
}

/// Deploiements, du plus recent au plus ancien.
pub async fn list(db: &PgPool, filter: &Filter) -> sqlx::Result<Vec<Deployment>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE ($1::uuid IS NULL OR d.project_id = $1) AND ($2::text IS NULL OR d.status = $2) \
         ORDER BY d.created_at DESC, d.number DESC LIMIT $3",
        select()
    )))
    .bind(filter.project)
    .bind(filter.status.map(DeploymentStatus::as_str))
    .bind(filter.limit)
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Row::into_api).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_payloads_are_ids_only_and_parse_in_order() {
        let p = Uuid::new_v4();
        let e = Uuid::new_v4();
        let push: DeployJob = serde_json::from_value(serde_json::json!({
            "project_id": p, "environment_id": e, "commit_sha": "ab"
        }))
        .unwrap();
        assert!(matches!(push, DeployJob::Push { .. }));
        let env: DeployJob =
            serde_json::from_value(serde_json::json!({ "environment_id": e })).unwrap();
        assert_eq!(env, DeployJob::Environment { environment_id: e });
        let dep: DeployJob =
            serde_json::from_value(serde_json::json!({ "deployment_id": e })).unwrap();
        assert_eq!(dep, DeployJob::Deployment { deployment_id: e });
    }

    #[test]
    fn messages_are_cleaned_and_truncated() {
        assert_eq!(clean_message("a\nb\u{0}c"), "a b c");
        let long = "x".repeat(MAX_MESSAGE_CHARS + 10);
        assert_eq!(clean_message(&long).chars().count(), MAX_MESSAGE_CHARS + 1);
    }
}
