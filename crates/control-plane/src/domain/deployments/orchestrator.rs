//! Pilotage des deploiements.
//!
//! La tache `deploy` est l'orchestrateur : elle cree le deploiement d'un
//! push, puis fait avancer l'environnement (demarre le plus ancien
//! deploiement en file si aucun ne tourne, envoie `Build` puis `Deploy`).
//! Les messages de l'agent (`DeployProgress`, `State`, `LogLine`) sont suivis
//! par `run` : ils font avancer l'etat de la base et, a la fin d'un
//! deploiement, remettent en file l'avancement de son environnement et le
//! nettoyage des images.
//!
//! Une issue terminale (reussite, echec) met en file sa notification e-mail
//! dans la meme transaction (`domain::notifications`).
//!
//! Chaque etape tient dans une transaction qui verrouille la ligne du
//! deploiement : une tache interrompue (bail perdu, arret) ne laisse pas
//! d'etat a moitie ecrit, et sa reprise refait le meme travail sans danger
//! (l'agent ignore un ordre rejoue ou renvoie son issue gardee).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use proto::{
    AgentState, AgentToControl, BuildMethod, BuildSource, BuildSpec, ComposeDeploy, ControlToAgent,
    DeploySpec, GitToken, Healthcheck, ImageRef, LogTarget, Stage, StageStatus,
};
use serde_json::json;
use shared::ids::{DeploymentId, EnvironmentId, ProjectId, ServerId, ServiceId};
use sqlx::PgConnection;
use tokio::sync::{broadcast, watch};
use uuid::Uuid;

use super::{
    CleanupJob, DeployJob, LOST_GRACE, STALE_AFTER, WATCHDOG_EVERY, clean_message, lock_environment,
};
use crate::agents::registry::SendError;
use crate::connectors::github;
use crate::domain::notifications::{self, Event};
use crate::domain::{domains, variables};
use crate::jobs::worker::{HandlerError, Handlers};
use crate::jobs::{self, Job, JobKind};
use crate::secrets::labels;
use crate::state::AppState;

/// Ecart minimal entre deux mises a jour de `last_event_at` dues aux lignes
/// de journal d'un meme deploiement.
const LOG_TOUCH_EVERY: Duration = Duration::from_secs(30);
/// Intervalle minimal entre deux demandes d'etat complet (`RequestState`)
/// apres une perte de messages d'agents.
pub const RESYNC_MIN_INTERVAL: Duration = Duration::from_secs(10);

/// Gestionnaires des taches `deploy` et `cleanup`.
pub fn register(handlers: Handlers, state: AppState) -> Handlers {
    let deploy_state = state.clone();
    handlers
        .register(JobKind::Deploy, move |job| {
            let state = deploy_state.clone();
            async move { handle_deploy(&state, job).await }
        })
        .register(JobKind::Cleanup, move |job| {
            let state = state.clone();
            async move { super::retention::handle_cleanup(&state, job).await }
        })
}

fn now(state: &AppState) -> f64 {
    state.clock.unix_secs()
}

pub(crate) fn db_error(context: &str, e: sqlx::Error) -> HandlerError {
    // Detail dans le journal du serveur seulement.
    tracing::error!(error = %e, "{context}");
    HandlerError::retry(format!("{context} : base de donnees indisponible"))
}

/// Tache `deploy`.
pub async fn handle_deploy(state: &AppState, job: Job) -> Result<(), HandlerError> {
    let payload: DeployJob = serde_json::from_value(job.payload)
        .map_err(|_| HandlerError::fatal("charge utile de deploiement illisible"))?;
    let environment = match payload {
        DeployJob::Push {
            project_id,
            environment_id,
            commit_sha,
        } => {
            if shared::validate::commit_sha(&commit_sha).is_err() {
                return Err(HandlerError::fatal("commit invalide"));
            }
            create_push(state, job.id.0, project_id, environment_id, &commit_sha)
                .await
                .map_err(|e| db_error("creation du deploiement", e))?;
            Some(environment_id)
        }
        DeployJob::Deployment { deployment_id } => {
            sqlx::query_scalar("SELECT environment_id FROM deployments WHERE id = $1")
                .bind(deployment_id)
                .fetch_optional(&state.db)
                .await
                .map_err(|e| db_error("lecture du deploiement", e))?
        }
        DeployJob::Environment { environment_id } => Some(environment_id),
    };
    match environment {
        Some(environment) => advance(state, environment).await,
        // Projet supprime entre-temps : rien a faire.
        None => Ok(()),
    }
}

/// Deploiement `push` d'un webhook, sauf si cette tache l'a deja cree ou si
/// le meme commit est deja en file ou en cours sur l'environnement.
async fn create_push(
    state: &AppState,
    job: Uuid,
    project: Uuid,
    environment: Uuid,
    commit: &str,
) -> sqlx::Result<()> {
    let mut tx = state.db.begin().await?;
    lock_environment(&mut tx, environment).await?;
    let branch: Option<String> = sqlx::query_scalar(
        "SELECT g.branch FROM environments e JOIN git_sources g ON g.project_id = e.project_id \
         WHERE e.id = $1 AND e.project_id = $2",
    )
    .bind(environment)
    .bind(project)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(branch) = branch else {
        tracing::info!(%project, "push ignore : projet supprime ou sans source Git");
        return Ok(());
    };
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM deployments WHERE job_id = $1 \
         OR (environment_id = $2 AND commit_sha = $3 AND status IN ('queued', 'running')) LIMIT 1",
    )
    .bind(job)
    .bind(environment)
    .bind(commit)
    .fetch_optional(&mut *tx)
    .await?;
    if existing.is_some() {
        tx.commit().await?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, trigger, commit_sha, branch, \
         job_id, created_at) VALUES ($1, $2, $3, 'push', $4, $5, $6, to_timestamp($7::float8))",
    )
    .bind(Uuid::new_v4())
    .bind(project)
    .bind(environment)
    .bind(commit)
    .bind(&branch)
    .bind(job)
    .bind(now(state))
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Erreur d'une demande de l'API.
#[derive(Debug)]
pub enum RequestError {
    NotFound,
    /// Deploiement deja termine (annulation).
    AlreadyFinished,
    /// Deploiement non reussi ou image supprimee (rollback).
    NotRollbackable,
    /// Un deploiement est deja en file ou en cours (redeploiement).
    InProgress,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for RequestError {
    fn from(e: sqlx::Error) -> Self {
        RequestError::Db(e)
    }
}

async fn audit(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    action: &str,
    deployment: Uuid,
    project: Uuid,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, $2, $3, 'deployment', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(json!({ "deployment_id": deployment, "project_id": project }))
    .execute(conn)
    .await?;
    Ok(())
}

/// Deploiement manuel de `environment` (par son nom) : mis en file, et
/// tache d'avancement creee dans la meme transaction.
pub async fn create_manual(
    state: &AppState,
    project: Uuid,
    environment: &str,
    actor: Uuid,
) -> Result<Uuid, RequestError> {
    match request(
        state,
        project,
        environment,
        Origin::Manual(actor),
        Guard::None,
    )
    .await?
    {
        Requested::Created(id) | Requested::AlreadyQueued(id) => Ok(id),
    }
}

/// Auteur d'une demande de deploiement.
#[derive(Clone, Copy, Debug)]
pub enum Origin {
    /// Utilisateur du panneau (`manual`).
    Manual(Uuid),
    /// Deploy hook (`hook`), sans utilisateur.
    Hook(Uuid),
}

/// Garde d'unicite, evaluee sous le verrou de l'environnement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// Toujours mettre en file.
    None,
    /// Refuser (`InProgress`) si un deploiement est en file ou en cours.
    RefuseActive,
    /// Reutiliser un deploiement manuel ou hook deja en file (il construira
    /// le dernier commit a son demarrage) ; sinon mettre en file.
    ReuseQueued,
}

/// Issue d'une demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requested {
    Created(Uuid),
    AlreadyQueued(Uuid),
}

/// Demande de deploiement de `environment` (bouton Redeployer, deploy
/// hook) : meme chemin que le deploiement manuel (tache `Deploy`, jeton
/// d'installation obtenu a l'envoi), sous le verrou consultatif de
/// l'environnement pour que la garde ne laisse passer aucun doublon.
pub async fn request(
    state: &AppState,
    project: Uuid,
    environment: &str,
    origin: Origin,
    guard: Guard,
) -> Result<Requested, RequestError> {
    let mut tx = state.db.begin().await?;
    let env: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT e.id, g.branch FROM environments e \
         LEFT JOIN git_sources g ON g.project_id = e.project_id \
         WHERE e.project_id = $1 AND e.name = $2",
    )
    .bind(project)
    .bind(environment)
    .fetch_optional(&mut *tx)
    .await?;
    let (env, branch) = env.ok_or(RequestError::NotFound)?;
    if guard != Guard::None {
        lock_environment(&mut tx, env).await?;
        // Refus : tout deploiement en file ou en cours. Reutilisation : un
        // deploiement en file qui resoudra la branche a son demarrage
        // (manuel ou hook), jamais un push (commit fixe) ni un rollback.
        let (statuses, triggers): (&[&str], &[&str]) = match guard {
            Guard::RefuseActive => (
                &["queued", "running"],
                &["push", "manual", "rollback", "hook"],
            ),
            _ => (&["queued"], &["manual", "hook"]),
        };
        let active: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM deployments WHERE environment_id = $1 AND status = ANY($2) \
             AND trigger = ANY($3) \
             ORDER BY created_at, number LIMIT 1",
        )
        .bind(env)
        .bind(statuses)
        .bind(triggers)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(active) = active {
            return match guard {
                Guard::RefuseActive => Err(RequestError::InProgress),
                _ => {
                    tx.commit().await?;
                    Ok(Requested::AlreadyQueued(active))
                }
            };
        }
    }
    let (trigger, actor) = match origin {
        Origin::Manual(user) => ("manual", Some(user)),
        Origin::Hook(_) => ("hook", None),
    };
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, trigger, branch, created_by, \
         created_at) VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7::float8))",
    )
    .bind(id)
    .bind(project)
    .bind(env)
    .bind(trigger)
    .bind(branch)
    .bind(actor)
    .bind(now(state))
    .execute(&mut *tx)
    .await?;
    enqueue_deployment(&mut tx, id).await?;
    match origin {
        Origin::Manual(user) => {
            audit(&mut tx, Some(user), "deployment.create", id, project).await?
        }
        Origin::Hook(hook) => {
            sqlx::query(
                "INSERT INTO audit_events (id, actor_id, action, target, detail) \
                 VALUES ($1, NULL, 'deployment.hook', 'deployment', $2)",
            )
            .bind(Uuid::new_v4())
            .bind(json!({ "deployment_id": id, "project_id": project, "hook_id": hook }))
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(Requested::Created(id))
}

async fn enqueue_deployment(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    jobs::enqueue_in(
        conn,
        JobKind::Deploy,
        serde_json::to_value(DeployJob::Deployment { deployment_id: id })
            .map_err(|e| sqlx::Error::Encode(Box::new(e)))?,
        None,
    )
    .await?;
    Ok(())
}

/// Rollback vers `source` : nouveau deploiement qui reutilise son image
/// (l'ensemble Compose pour un projet Compose), sur le meme serveur.
pub async fn rollback(state: &AppState, source: Uuid, actor: Uuid) -> Result<Uuid, RequestError> {
    let mut tx = state.db.begin().await?;
    let env: Option<Uuid> =
        sqlx::query_scalar("SELECT environment_id FROM deployments WHERE id = $1")
            .bind(source)
            .fetch_optional(&mut *tx)
            .await?;
    let env = env.ok_or(RequestError::NotFound)?;
    // Meme verrou que la retention : l'image ne peut pas disparaitre entre
    // la verification et l'enregistrement du rollback.
    lock_environment(&mut tx, env).await?;
    #[derive(sqlx::FromRow)]
    struct Source {
        project_id: Uuid,
        server_id: Option<Uuid>,
        image: Option<String>,
        image_owner: Uuid,
        compose_service: Option<String>,
        commit_sha: Option<String>,
        branch: Option<String>,
        rollbackable: bool,
    }
    let src: Source = sqlx::query_as(
        "SELECT d.project_id, d.server_id, d.image, COALESCE(d.image_from, d.id) AS image_owner, \
                d.compose_service, d.commit_sha, d.branch, \
                (d.status = 'success' AND d.cleaned_at IS NULL AND d.server_id IS NOT NULL \
                 AND d.image IS NOT NULL \
                 AND (d.image_from IS NULL OR EXISTS (SELECT 1 FROM deployments o \
                      WHERE o.id = d.image_from AND o.cleaned_at IS NULL))) AS rollbackable \
         FROM deployments d WHERE d.id = $1",
    )
    .bind(source)
    .fetch_one(&mut *tx)
    .await?;
    if !src.rollbackable {
        return Err(RequestError::NotRollbackable);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, trigger, commit_sha, \
         branch, rollback_of, image_from, image, compose_service, created_by, created_at) \
         VALUES ($1, $2, $3, $4, 'rollback', $5, $6, $7, $8, $9, $10, $11, to_timestamp($12::float8))",
    )
    .bind(id)
    .bind(src.project_id)
    .bind(env)
    .bind(src.server_id)
    .bind(&src.commit_sha)
    .bind(&src.branch)
    .bind(source)
    .bind(src.image_owner)
    .bind(&src.image)
    .bind(&src.compose_service)
    .bind(actor)
    .bind(now(state))
    .execute(&mut *tx)
    .await?;
    enqueue_deployment(&mut tx, id).await?;
    audit(
        &mut tx,
        Some(actor),
        "deployment.rollback",
        id,
        src.project_id,
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Annulation demandee par l'utilisateur. En file, ou en cours sans ordre
/// envoye : `cancelled` tout de suite. En cours : `Stop` a l'agent, qui
/// confirme par `Cancelled` (ou aboutit si la bascule est faite).
pub async fn cancel(state: &AppState, id: Uuid, actor: Uuid) -> Result<(), RequestError> {
    let mut tx = state.db.begin().await?;
    let row: Option<(String, Uuid, Uuid, Option<Uuid>, bool, bool)> = sqlx::query_as(
        "SELECT status, environment_id, project_id, server_id, \
         (build_sent_at IS NOT NULL OR deploy_sent_at IS NOT NULL), cancel_requested \
         FROM deployments WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let (status, env, project, server, sent, requested) = row.ok_or(RequestError::NotFound)?;
    match status.as_str() {
        "queued" => finish(&mut tx, state, id, env, Finish::Cancelled).await?,
        "running" if !sent => finish(&mut tx, state, id, env, Finish::Cancelled).await?,
        "running" => {
            if !requested {
                sqlx::query("UPDATE deployments SET cancel_requested = true WHERE id = $1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            if let Some(server) = server {
                // Agent absent : `Stop` renvoye a sa reconnexion (reconcile).
                send_logged(
                    state,
                    server,
                    ControlToAgent::Stop {
                        deployment_id: DeploymentId(id),
                    },
                );
            }
        }
        _ => return Err(RequestError::AlreadyFinished),
    }
    audit(&mut tx, Some(actor), "deployment.cancel", id, project).await?;
    tx.commit().await?;
    Ok(())
}

/// Ordres de suppression envoyes aux agents pour tous les deploiements d'un
/// projet (avant sa suppression en base). Au mieux : lu sur une connexion a
/// part (une erreur n'interrompt pas la transaction de suppression) ; une
/// erreur de base ou un agent absent est journalise, ses conteneurs restent
/// (etiquetes par deploiement). Renvoie vrai si tous les ordres sont partis.
pub async fn remove_project_resources(state: &AppState, project: Uuid) -> bool {
    let rows: Result<Vec<(Uuid, Uuid)>, _> = sqlx::query_as(
        "SELECT id, server_id FROM deployments WHERE project_id = $1 AND server_id IS NOT NULL \
         AND cleaned_at IS NULL AND (build_sent_at IS NOT NULL OR deploy_sent_at IS NOT NULL)",
    )
    .bind(project)
    .fetch_all(&state.db)
    .await;
    match rows {
        Ok(rows) => {
            let mut all_sent = true;
            for (id, server) in rows {
                all_sent &= send_logged(
                    state,
                    server,
                    ControlToAgent::RemoveDeployment {
                        deployment_id: DeploymentId(id),
                    },
                );
            }
            all_sent
        }
        Err(e) => {
            tracing::error!(error = %e, %project, "deploiements du projet illisibles : nettoyage des conteneurs saute");
            false
        }
    }
}

/// Serveurs des conteneurs encore en place du projet dont l'agent n'est pas
/// connecte (rien ne pourrait leur etre envoye).
pub async fn unreachable_servers(state: &AppState, project: Uuid) -> sqlx::Result<Vec<Uuid>> {
    let servers: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT server_id FROM deployments WHERE project_id = $1 AND server_id IS NOT NULL \
         AND cleaned_at IS NULL AND (build_sent_at IS NOT NULL OR deploy_sent_at IS NOT NULL)",
    )
    .bind(project)
    .fetch_all(&state.db)
    .await?;
    Ok(servers
        .into_iter()
        .filter(|s| !state.agents.is_connected(ServerId(*s)))
        .collect())
}

fn send_logged(state: &AppState, server: Uuid, msg: ControlToAgent) -> bool {
    match state.agents.send(ServerId(server), msg) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(%server, "ordre non envoye a l'agent : {e}");
            false
        }
    }
}

/// Message d'erreur enregistre : masque (secrets du projet) avant d'etre
/// nettoye et coupe. Masquage indisponible : message generique, jamais le
/// texte brut.
async fn masked_message(state: &AppState, project: Uuid, message: &str) -> String {
    match state.logs.project_redactor(project).await {
        Ok(redactor) => clean_message(&redactor.redact(message.to_string())),
        Err(_) => "erreur masquée (masquage indisponible)".to_string(),
    }
}

/// Issue d'un deploiement.
#[derive(Clone, Debug)]
enum Finish {
    Success,
    Failed { message: String, presumed: bool },
    Cancelled,
}

/// Termine un deploiement (ligne deja verrouillee) et remet en file
/// l'avancement de l'environnement et le nettoyage des images.
async fn finish(
    conn: &mut PgConnection,
    state: &AppState,
    id: Uuid,
    environment: Uuid,
    how: Finish,
) -> sqlx::Result<()> {
    let at = now(state);
    let (status, error, presumed) = match &how {
        Finish::Success => ("success", None, false),
        Finish::Failed { message, presumed } => {
            let project: Uuid =
                sqlx::query_scalar("SELECT project_id FROM deployments WHERE id = $1")
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            (
                "failed",
                Some(masked_message(state, project, message).await),
                *presumed,
            )
        }
        Finish::Cancelled => ("cancelled", None, false),
    };
    sqlx::query(
        "UPDATE deployments SET status = $2, error = $3, presumed_failed = $4, \
         finished_at = to_timestamp($5::float8), \
         cancel_requested = cancel_requested AND $2 <> 'success' WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(&error)
    .bind(presumed)
    .bind(at)
    .execute(&mut *conn)
    .await?;
    if !matches!(how, Finish::Success) {
        let step = if matches!(how, Finish::Cancelled) {
            "cancelled"
        } else {
            "failed"
        };
        sqlx::query(
            "UPDATE deployment_stages SET status = $2, finished_at = to_timestamp($3::float8) \
             WHERE deployment_id = $1 AND status = 'running'",
        )
        .bind(id)
        .bind(step)
        .bind(at)
        .execute(&mut *conn)
        .await?;
    }
    match how {
        Finish::Success => {
            notifications::enqueue(conn, state, Event::DeploySucceeded { deployment_id: id })
                .await?
        }
        Finish::Failed { .. } => {
            notifications::enqueue(conn, state, Event::DeployFailed { deployment_id: id }).await?
        }
        Finish::Cancelled => {}
    }
    enqueue_followups(conn, environment).await
}

async fn enqueue_followups(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<()> {
    let encode = |e: serde_json::Error| sqlx::Error::Encode(Box::new(e));
    jobs::enqueue_in(
        conn,
        JobKind::Deploy,
        serde_json::to_value(DeployJob::Environment {
            environment_id: environment,
        })
        .map_err(encode)?,
        None,
    )
    .await?;
    jobs::enqueue_in(
        conn,
        JobKind::Cleanup,
        serde_json::to_value(CleanupJob {
            environment_id: environment,
        })
        .map_err(encode)?,
        None,
    )
    .await?;
    Ok(())
}

/// Serveur des nouveaux deploiements : l'agent local (v0.1, un seul
/// serveur), sinon le plus ancien serveur non revoque.
async fn pick_server(conn: &mut PgConnection) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM servers WHERE revoked_at IS NULL \
         ORDER BY (kind = 'local') DESC, created_at LIMIT 1",
    )
    .fetch_optional(conn)
    .await
}

/// Fait avancer un environnement : demarre le plus ancien deploiement en
/// file si aucun ne tourne, et envoie l'ordre attendu par celui qui tourne.
pub async fn advance(state: &AppState, environment: Uuid) -> Result<(), HandlerError> {
    let running = start_next(state, environment)
        .await
        .map_err(|e| db_error("demarrage du deploiement suivant", e))?;
    match running {
        Some(id) => dispatch(state, id).await,
        None => Ok(()),
    }
}

/// Sous le verrou de l'environnement : le deploiement en cours, ou le plus
/// ancien en file, passe `running` (serveur choisi, horloge des nouvelles
/// demarree).
async fn start_next(state: &AppState, environment: Uuid) -> sqlx::Result<Option<Uuid>> {
    let mut tx = state.db.begin().await?;
    lock_environment(&mut tx, environment).await?;
    let running: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM deployments WHERE environment_id = $1 AND status = 'running'",
    )
    .bind(environment)
    .fetch_optional(&mut *tx)
    .await?;
    if running.is_some() {
        tx.commit().await?;
        return Ok(running);
    }
    loop {
        let next: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
            "SELECT id, server_id FROM deployments WHERE environment_id = $1 AND status = 'queued' \
             ORDER BY created_at, number LIMIT 1 FOR UPDATE",
        )
        .bind(environment)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((id, server)) = next else {
            tx.commit().await?;
            return Ok(None);
        };
        let server = match server {
            Some(s) => Some(s),
            None => pick_server(&mut tx).await?,
        };
        let at = now(state);
        let Some(server) = server else {
            sqlx::query(
                "UPDATE deployments SET status = 'failed', started_at = to_timestamp($2::float8), \
                 finished_at = to_timestamp($2::float8), error = $3 WHERE id = $1",
            )
            .bind(id)
            .bind(at)
            .bind("Aucun serveur enregistré pour exécuter le déploiement.")
            .execute(&mut *tx)
            .await?;
            notifications::enqueue(&mut tx, state, Event::DeployFailed { deployment_id: id })
                .await?;
            continue;
        };
        sqlx::query(
            "UPDATE deployments SET status = 'running', server_id = $2, \
             started_at = to_timestamp($3::float8), last_event_at = to_timestamp($3::float8) \
             WHERE id = $1",
        )
        .bind(id)
        .bind(server)
        .bind(at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Some(id));
    }
}

/// Ce que le deploiement a besoin de savoir pour construire ses ordres.
#[derive(sqlx::FromRow)]
struct Info {
    id: Uuid,
    status: String,
    cancel_requested: bool,
    server_id: Option<Uuid>,
    environment_id: Uuid,
    project_id: Uuid,
    image_from: Option<Uuid>,
    image: Option<String>,
    image_built: bool,
    build_sent: bool,
    deploy_sent: bool,
    commit_sha: Option<String>,
    branch: Option<String>,
    compose_service: Option<String>,
    project_name: String,
    source_kind: String,
    image_ref: Option<String>,
    build_kind: Option<String>,
    build_path: Option<String>,
    build_service: Option<String>,
    repository: Option<String>,
    git_branch: Option<String>,
    root_directory: Option<String>,
    environment: String,
    service_id: Option<Uuid>,
    service_name: Option<String>,
    internal_port: Option<i32>,
    healthcheck_path: Option<String>,
    run_as: Option<String>,
    read_only_rootfs: Option<bool>,
}

impl Info {
    /// Un rollback reutilise une image : aucun build.
    fn needs_build(&self) -> bool {
        self.image_from.is_none()
    }

    fn running(&self) -> bool {
        self.status == "running"
    }
}

/// Lit (et verrouille) le deploiement et ce qu'il faut pour ses ordres.
async fn info(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<Info>> {
    sqlx::query_as(
        "SELECT d.id, d.status, d.cancel_requested, d.server_id, d.environment_id, d.project_id, \
                d.image_from, d.image, d.image_built_at IS NOT NULL AS image_built, \
                d.build_sent_at IS NOT NULL AS build_sent, d.deploy_sent_at IS NOT NULL AS deploy_sent, \
                d.commit_sha, d.branch, d.compose_service, p.name AS project_name, p.source_kind, \
                p.image_ref, p.build_kind, p.build_path, p.build_service, g.repository, \
                g.branch AS git_branch, g.root_directory, e.name AS environment, s.id AS service_id, \
                s.name AS service_name, s.internal_port, s.healthcheck_path, s.run_as, s.read_only_rootfs \
         FROM deployments d \
         JOIN projects p ON p.id = d.project_id \
         JOIN environments e ON e.id = d.environment_id \
         LEFT JOIN git_sources g ON g.project_id = d.project_id \
         LEFT JOIN LATERAL (SELECT id, name, internal_port, healthcheck_path, run_as, read_only_rootfs FROM services \
              WHERE environment_id = d.environment_id ORDER BY created_at, name LIMIT 1) s ON true \
         WHERE d.id = $1 FOR UPDATE OF d",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Envoie l'ordre attendu par un deploiement en cours : `Build` s'il reste
/// a construire, sinon `Deploy` (precede de la table des routes).
pub async fn dispatch(state: &AppState, id: Uuid) -> Result<(), HandlerError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| db_error("connexion a la base", e))?;
    let Some(info) = info(&mut conn, id)
        .await
        .map_err(|e| db_error("lecture du deploiement", e))?
    else {
        return Ok(());
    };
    drop(conn);
    if !info.running() {
        return Ok(());
    }
    if info.needs_build() && !info.build_sent {
        send_build(state, info).await
    } else if (!info.needs_build() || info.image_built) && !info.deploy_sent {
        let mut tx = state
            .db
            .begin()
            .await
            .map_err(|e| db_error("transaction du deploiement", e))?;
        let fresh = info_locked_running(&mut tx, id).await?;
        if let Some(info) = fresh
            && !info.deploy_sent
        {
            let sent = send_deploy(&mut tx, state, &info)
                .await
                .map_err(|e| db_error("envoi du deploiement", e))?;
            if sent == DeploySent::Busy {
                // Annule la tache remise en file : celle-ci est reprise avec
                // delai (bail du travailleur).
                return Err(HandlerError::retry("file d'envoi de l'agent pleine"));
            }
        }
        tx.commit()
            .await
            .map_err(|e| db_error("transaction du deploiement", e))
    } else {
        Ok(())
    }
}

async fn info_locked_running(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<Info>, HandlerError> {
    Ok(info(conn, id)
        .await
        .map_err(|e| db_error("lecture du deploiement", e))?
        .filter(Info::running))
}

/// Source Git : jeton d'installation (si l'App est installee sur le depot)
/// et commit a construire (celui du push, sinon la tete de la branche).
async fn git_access(
    state: &AppState,
    repository: &str,
    branch: &str,
    commit: Option<&str>,
) -> Result<(Option<GitToken>, String), GitAccessError> {
    let app = github::load(&state.db)
        .await
        .map_err(|e| GitAccessError::Retry(db_error("lecture de la GitHub App", e)))?;
    let base = &state.cfg.github_api_url;
    let now = state.clock.unix_secs() as i64;
    let token = match app {
        Some(app) => {
            let pem = state
                .secrets
                .open_bound(labels::GITHUB_PRIVATE_KEY, &app.private_key_sealed)
                .map_err(|_| {
                    tracing::error!("cle de la GitHub App illisible");
                    GitAccessError::Fail("Clé de la GitHub App illisible.".into())
                })?;
            state
                .github
                .repository_token(base, app.app_id, &pem, repository, now)
                .await
                .map_err(GitAccessError::from_github)?
        }
        None => None,
    };
    let commit = match commit {
        Some(c) => c.to_string(),
        None => state
            .github
            .branch_head(
                base,
                repository,
                branch,
                token.as_deref().map(String::as_str),
            )
            .await
            .map_err(|e| match e {
                github::app::GithubError::Status(404 | 422) => {
                    GitAccessError::Fail(format!("Branche « {branch} » introuvable sur GitHub."))
                }
                other => GitAccessError::from_github(other),
            })?,
    };
    Ok((token.map(|t| GitToken::new(t.to_string())), commit))
}

enum GitAccessError {
    /// Reessayer plus tard (GitHub ou la base indisponibles).
    Retry(HandlerError),
    /// Le deploiement echoue (cause affichee).
    Fail(String),
}

impl GitAccessError {
    fn from_github(e: github::app::GithubError) -> Self {
        match e {
            github::app::GithubError::Key => {
                GitAccessError::Fail("Clé de la GitHub App inutilisable.".into())
            }
            github::app::GithubError::Status(status) => {
                tracing::warn!(status, "GitHub : statut inattendu");
                GitAccessError::Retry(HandlerError::retry("GitHub ne repond pas comme prevu"))
            }
            _ => {
                tracing::warn!("GitHub injoignable ou reponse illisible");
                GitAccessError::Retry(HandlerError::retry("GitHub injoignable"))
            }
        }
    }
}

fn build_method(info: &Info) -> BuildMethod {
    match (
        info.build_kind.as_deref(),
        &info.build_path,
        &info.build_service,
    ) {
        (Some("dockerfile"), Some(path), _) => BuildMethod::Dockerfile { path: path.clone() },
        (Some("compose"), Some(path), Some(service)) => BuildMethod::Compose {
            path: path.clone(),
            service: service.clone(),
        },
        // Railpack, et source image (methode ignoree par l'agent).
        _ => BuildMethod::Railpack,
    }
}

/// Envoie `Build`. Le jeton GitHub est obtenu juste avant et n'existe que
/// dans l'ordre (efface de la memoire a sa liberation).
async fn send_build(state: &AppState, info: Info) -> Result<(), HandlerError> {
    let id = info.id;
    let (source, token, commit) = match info.source_kind.as_str() {
        "git" => {
            let (Some(repository), Some(branch)) = (
                info.repository.clone(),
                info.branch.clone().or(info.git_branch.clone()),
            ) else {
                return fail_running(state, id, "Source Git du projet incomplète.").await;
            };
            match git_access(state, &repository, &branch, info.commit_sha.as_deref()).await {
                Ok((token, commit)) => (
                    BuildSource::Git {
                        url: format!("https://github.com/{repository}"),
                        branch,
                        commit_sha: commit.clone(),
                    },
                    token,
                    Some(commit),
                ),
                Err(GitAccessError::Retry(e)) => return Err(e),
                Err(GitAccessError::Fail(message)) => {
                    return fail_running(state, id, &message).await;
                }
            }
        }
        _ => match &info.image_ref {
            Some(reference) => (
                BuildSource::Image {
                    reference: ImageRef(reference.clone()),
                },
                None,
                None,
            ),
            None => return fail_running(state, id, "Image du projet absente.").await,
        },
    };
    let compose_service = match info.build_kind.as_deref() {
        Some("compose") => info.build_service.clone(),
        _ => None,
    };
    let image = format!(
        "ciphera/{}-{}:{}",
        info.project_name, info.environment, info.id
    );
    let spec = BuildSpec {
        deployment_id: DeploymentId(id),
        project: info.project_name.clone(),
        environment: info.environment.clone(),
        source,
        build: build_method(&info),
        git_token: token,
        // Valeur deja normalisee a l'enregistrement ; vide = racine.
        root_directory: info.root_directory.clone().filter(|r| !r.is_empty()),
        project_id: Some(ProjectId(info.project_id)),
        environment_id: Some(EnvironmentId(info.environment_id)),
    };

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_error("transaction du deploiement", e))?;
    // Relu sous verrou : une annulation arrivee pendant l'appel a GitHub
    // l'emporte, et `Stop` suit toujours `Build` dans la file de l'agent.
    let Some(fresh) = info_locked_running(&mut tx, id).await? else {
        return Ok(());
    };
    if fresh.build_sent {
        return Ok(());
    }
    if fresh.cancel_requested {
        finish(&mut tx, state, id, fresh.environment_id, Finish::Cancelled)
            .await
            .map_err(|e| db_error("annulation du deploiement", e))?;
        return tx
            .commit()
            .await
            .map_err(|e| db_error("transaction du deploiement", e));
    }
    let Some(server) = fresh.server_id else {
        return Ok(());
    };
    match state
        .agents
        .send(ServerId(server), ControlToAgent::Build(spec))
    {
        Ok(()) => {}
        Err(SendError::NotConnected) => {
            // Renvoye a la reconnexion de l'agent (reconcile), ou echec
            // apres 10 minutes sans nouvelles.
            tracing::warn!(deployment = %id, %server, "agent absent : build en attente");
            return Ok(());
        }
        Err(SendError::Busy) => return Err(HandlerError::retry("file d'envoi de l'agent pleine")),
    }
    sqlx::query(
        "UPDATE deployments SET build_sent_at = to_timestamp($2::float8), image = $3, \
         commit_sha = COALESCE(commit_sha, $4), compose_service = $5, \
         last_event_at = to_timestamp($2::float8) WHERE id = $1",
    )
    .bind(id)
    .bind(now(state))
    .bind(&image)
    .bind(commit)
    .bind(compose_service)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_error("enregistrement de l'envoi du build", e))?;
    tx.commit()
        .await
        .map_err(|e| db_error("transaction du deploiement", e))
}

async fn fail_running(state: &AppState, id: Uuid, message: &str) -> Result<(), HandlerError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_error("transaction du deploiement", e))?;
    if let Some(info) = info_locked_running(&mut tx, id).await? {
        finish(
            &mut tx,
            state,
            id,
            info.environment_id,
            Finish::Failed {
                message: message.to_string(),
                presumed: false,
            },
        )
        .await
        .map_err(|e| db_error("echec du deploiement", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| db_error("transaction du deploiement", e))
}

/// Issue de l'envoi de `Deploy`.
#[derive(Debug, PartialEq, Eq)]
enum DeploySent {
    /// Parti (ou deploiement termine faute d'image ou de service).
    Done,
    /// Agent absent : rien n'est note, l'ordre partira a sa reconnexion.
    Waiting,
    /// File d'envoi de l'agent pleine : une tache `deploy` est mise en file
    /// (dans la transaction de l'appelant) pour le renvoyer avec reprise.
    Busy,
}

/// Envoie la table des routes puis `Deploy` (ligne verrouillee par
/// l'appelant) et note l'envoi.
async fn send_deploy(
    conn: &mut PgConnection,
    state: &AppState,
    info: &Info,
) -> sqlx::Result<DeploySent> {
    let (Some(server), Some(image), Some(service)) =
        (info.server_id, info.image.clone(), info.service_id)
    else {
        finish(
            conn,
            state,
            info.id,
            info.environment_id,
            Finish::Failed {
                message: "Image ou service introuvable pour ce déploiement.".into(),
                presumed: false,
            },
        )
        .await?;
        return Ok(DeploySent::Done);
    };
    // Variables resolues et secrets dechiffres juste avant l'envoi : le clair
    // n'existe que dans l'ordre (efface de la memoire a sa liberation). Heure
    // prise avant la lecture : une modification faite ensuite reste « non
    // appliquee ».
    // Lecture serialisee avec les ecritures de variables (qui verrouillent
    // l'environnement `FOR UPDATE` avant d'horodater) : une ecriture en cours
    // est attendue et lue ; une ecriture suivante portera une heure
    // posterieure a `resolved_at`.
    sqlx::query("SELECT 1 FROM environments WHERE id = $1 FOR SHARE")
        .bind(info.environment_id)
        .execute(&mut *conn)
        .await?;
    let resolved_at = now(state);
    let env = match variables::resolve_env(
        conn,
        &state.secrets,
        info.project_id,
        info.environment_id,
        &info.environment,
    )
    .await
    {
        Ok(env) => env,
        Err(variables::ResolveError::Db(e)) => return Err(e),
        Err(e) => {
            finish(
                conn,
                state,
                info.id,
                info.environment_id,
                Finish::Failed {
                    message: e.to_string(),
                    presumed: false,
                },
            )
            .await?;
            return Ok(DeploySent::Done);
        }
    };
    let spec = DeploySpec {
        deployment_id: DeploymentId(info.id),
        service_id: ServiceId(service),
        image: ImageRef(image),
        internal_port: info.internal_port.and_then(|p| u16::try_from(p).ok()),
        project_id: ProjectId(info.project_id),
        healthcheck: Healthcheck {
            path: info
                .healthcheck_path
                .clone()
                .unwrap_or_else(|| Healthcheck::default().path),
            ..Healthcheck::default()
        },
        compose: info.compose_service.clone().map(|service| ComposeDeploy {
            service,
            bundle: DeploymentId(info.image_from.unwrap_or(info.id)),
        }),
        service_name: info.service_name.clone(),
        env,
        environment_id: Some(EnvironmentId(info.environment_id)),
        run_as: info.run_as.clone(),
        read_only_rootfs: info.read_only_rootfs.unwrap_or(false),
    };
    // Table complete des routes (domaines) : elle part avant chaque `Deploy`
    // (l'agent refuse de basculer sans table), meme vide.
    let table = domains::route_table(conn, &state.cfg, server).await?;
    let routes = state
        .agents
        .send(ServerId(server), ControlToAgent::ApplyRoutes(table));
    let sent = routes.and_then(|()| {
        state
            .agents
            .send(ServerId(server), ControlToAgent::Deploy(spec))
    });
    match sent {
        Ok(()) => {}
        Err(SendError::NotConnected) => {
            tracing::warn!(deployment = %info.id, %server, "agent absent : deploiement en attente");
            return Ok(DeploySent::Waiting);
        }
        Err(SendError::Busy) => {
            tracing::warn!(deployment = %info.id, %server, "file d'envoi de l'agent pleine : Deploy remis en file");
            enqueue_deployment(conn, info.id).await?;
            return Ok(DeploySent::Busy);
        }
    }
    sqlx::query(
        "UPDATE deployments SET deploy_sent_at = to_timestamp($2::float8), \
         last_event_at = to_timestamp($2::float8), \
         variables_resolved_at = to_timestamp($3::float8) WHERE id = $1",
    )
    .bind(info.id)
    .bind(now(state))
    .bind(resolved_at)
    .execute(conn)
    .await?;
    Ok(DeploySent::Done)
}

/// Message d'un agent.
pub async fn on_agent_message(state: &AppState, server: ServerId, msg: AgentToControl) {
    let result = match msg {
        AgentToControl::DeployProgress {
            deployment_id,
            stage,
            status,
        } => on_progress(state, server.0, deployment_id.0, stage, &status).await,
        AgentToControl::State(agent) => {
            let health = crate::domain::metrics::record_state(
                &state.db,
                server.0,
                &agent.deployments,
                now(state),
            )
            .await;
            let reconciled = reconcile(state, server.0, &agent).await;
            health.and(reconciled)
        }
        AgentToControl::Metrics(m) => {
            crate::domain::metrics::record(&state.db, server.0, &m, now(state)).await
        }
        AgentToControl::LogLine(line) => match line.target {
            LogTarget::Deployment(id) => touch(state, server.0, id.0).await,
            LogTarget::Service(_) => Ok(()),
        },
        _ => Ok(()),
    };
    if let Err(e) = result {
        tracing::error!(error = %e, %server, "message de l'agent non traite");
    }
}

/// Nouvelle de l'agent pour un deploiement en cours.
async fn touch(state: &AppState, server: Uuid, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE deployments SET last_event_at = to_timestamp($3::float8) \
         WHERE id = $1 AND server_id = $2 AND status = 'running'",
    )
    .bind(id)
    .bind(server)
    .bind(now(state))
    .execute(&state.db)
    .await?;
    Ok(())
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Clone => "clone",
        Stage::Build => "build",
        Stage::Image => "image",
        Stage::Start => "start",
        Stage::Healthcheck => "healthcheck",
        Stage::Switch => "switch",
    }
}

/// Progression signalee par l'agent `server` (livree au moins une fois :
/// idempotent).
async fn on_progress(
    state: &AppState,
    server: Uuid,
    id: Uuid,
    stage: Stage,
    status: &StageStatus,
) -> sqlx::Result<()> {
    let mut tx = state.db.begin().await?;
    let Some(info) = info(&mut tx, id).await? else {
        return Ok(());
    };
    if info.server_id != Some(server) {
        tracing::warn!(deployment = %id, %server, "progression d'un deploiement d'un autre serveur : ignoree");
        return Ok(());
    }
    let terminal = status.is_terminal(stage);
    let at = now(state);
    // Deploiement reussi : l'etat complet de l'agent (dont l'utilisateur
    // effectif des conteneurs, `runs_as_root`) est redemande apres le commit.
    let mut refresh_state = false;
    match info.status.as_str() {
        "running" => {
            let (step, message) = match status {
                StageStatus::Started => ("running", None),
                StageStatus::Succeeded => ("succeeded", None),
                StageStatus::Failed { message } => (
                    "failed",
                    Some(masked_message(state, info.project_id, message).await),
                ),
                StageStatus::Cancelled => ("cancelled", None),
            };
            sqlx::query(
                "INSERT INTO deployment_stages (deployment_id, stage, status, message, started_at, finished_at) \
                 VALUES ($1, $2, $3, $4, to_timestamp($5::float8), \
                         CASE WHEN $3 = 'running' THEN NULL ELSE to_timestamp($5::float8) END) \
                 ON CONFLICT (deployment_id, stage) DO UPDATE SET \
                   status = CASE WHEN deployment_stages.status = 'running' THEN EXCLUDED.status ELSE deployment_stages.status END, \
                   message = CASE WHEN deployment_stages.status = 'running' THEN EXCLUDED.message ELSE deployment_stages.message END, \
                   finished_at = CASE WHEN deployment_stages.status = 'running' THEN EXCLUDED.finished_at ELSE deployment_stages.finished_at END",
            )
            .bind(id)
            .bind(stage_name(stage))
            .bind(step)
            .bind(&message)
            .bind(at)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE deployments SET stage = $2, last_event_at = to_timestamp($3::float8) WHERE id = $1",
            )
            .bind(id)
            .bind(stage_name(stage))
            .bind(at)
            .execute(&mut *tx)
            .await?;
            if terminal {
                match (stage, status) {
                    (Stage::Image, StageStatus::Succeeded) => {
                        sqlx::query(
                            "UPDATE deployments SET image_built_at = COALESCE(image_built_at, to_timestamp($2::float8)) \
                             WHERE id = $1",
                        )
                        .bind(id)
                        .bind(at)
                        .execute(&mut *tx)
                        .await?;
                        if info.cancel_requested {
                            // Build fini avant le `Stop` : rien ne tourne,
                            // l'image est retiree par le nettoyage.
                            finish(&mut tx, state, id, info.environment_id, Finish::Cancelled)
                                .await?;
                        } else if !info.deploy_sent {
                            send_deploy(&mut tx, state, &info).await?;
                        }
                    }
                    (_, StageStatus::Succeeded) => {
                        finish(&mut tx, state, id, info.environment_id, Finish::Success).await?;
                        refresh_state = true;
                    }
                    (_, StageStatus::Failed { message }) => {
                        finish(
                            &mut tx,
                            state,
                            id,
                            info.environment_id,
                            Finish::Failed {
                                message: message.clone(),
                                presumed: false,
                            },
                        )
                        .await?;
                    }
                    _ => {
                        finish(&mut tx, state, id, info.environment_id, Finish::Cancelled).await?;
                    }
                }
            }
        }
        "failed" if terminal => {
            // Echec presume devenu reussite : etat de l'agent relu aussi.
            refresh_state = override_failure(&mut tx, state, &info, stage, status).await?;
        }
        _ => {}
    }
    tx.commit().await?;
    if refresh_state {
        send_logged(state, server, ControlToAgent::RequestState);
    }
    Ok(())
}

/// Statut terminal recu pour un deploiement deja `failed`. Le premier
/// statut terminal fait foi, sauf :
/// - echec presume (chien de garde) : seul un verdict reel de l'agent le
///   remplace. `Switch Succeeded` -> `success` ; `Failed` -> reste `failed`
///   avec la vraie cause. Un `Cancelled` (consequence du `Stop` du chien de
///   garde : personne n'a annule) garde `failed` et la cause des 10 minutes,
///   sauf annulation demandee par l'utilisateur ;
/// - annulation demandee par l'utilisateur qui croise un `Failed` : annule.
async fn override_failure(
    conn: &mut PgConnection,
    state: &AppState,
    info: &Info,
    stage: Stage,
    status: &StageStatus,
) -> sqlx::Result<bool> {
    let presumed: bool =
        sqlx::query_scalar("SELECT presumed_failed FROM deployments WHERE id = $1")
            .bind(info.id)
            .fetch_one(&mut *conn)
            .await?;
    // Nouveau statut et cause : `None` = cause inchangee, `Some(None)` =
    // effacee, `Some(Some(_))` = remplacee.
    let change: Option<(&str, Option<Option<String>>)> = match (presumed, stage, status) {
        (true, Stage::Switch, StageStatus::Succeeded) => Some(("success", Some(None))),
        (true, _, StageStatus::Failed { message }) => Some((
            "failed",
            Some(Some(masked_message(state, info.project_id, message).await)),
        )),
        (true, _, StageStatus::Cancelled) if info.cancel_requested => {
            Some(("cancelled", Some(None)))
        }
        // `Cancelled` du `Stop` du chien de garde, build seul conclu : l'echec
        // est confirme tel quel.
        (true, _, _) => Some(("failed", None)),
        (false, _, StageStatus::Cancelled) if info.cancel_requested => {
            Some(("cancelled", Some(None)))
        }
        _ => None,
    };
    let Some((new_status, error)) = change else {
        return Ok(false);
    };
    let replace_error = error.is_some();
    let error = error.flatten();
    sqlx::query(
        "UPDATE deployments SET status = $2, presumed_failed = false, cancel_requested = false, \
         error = CASE WHEN $3 THEN $4 ELSE error END WHERE id = $1",
    )
    .bind(info.id)
    .bind(new_status)
    .bind(replace_error)
    .bind(&error)
    .execute(&mut *conn)
    .await?;
    // Echec presume devenu reussite : notifiee (l'echec l'a deja ete).
    if new_status == "success" {
        let event = Event::DeploySucceeded {
            deployment_id: info.id,
        };
        notifications::enqueue(conn, state, event).await?;
    }
    if let (StageStatus::Failed { .. }, Some(message)) = (status, &error) {
        // Vraie cause sur l'etape en echec (frise).
        sqlx::query(
            "INSERT INTO deployment_stages (deployment_id, stage, status, message, started_at, finished_at) \
             VALUES ($1, $2, 'failed', $3, to_timestamp($4::float8), to_timestamp($4::float8)) \
             ON CONFLICT (deployment_id, stage) DO UPDATE SET status = 'failed', message = EXCLUDED.message",
        )
        .bind(info.id)
        .bind(stage_name(stage))
        .bind(message)
        .bind(now(state))
        .execute(&mut *conn)
        .await?;
    }
    if presumed {
        // Retention a recalculer : le deploiement peut etre servi, ou n'est
        // plus presume et peut etre nettoye.
        jobs::enqueue_in(
            conn,
            JobKind::Cleanup,
            serde_json::to_value(CleanupJob {
                environment_id: info.environment_id,
            })
            .map_err(|e| sqlx::Error::Encode(Box::new(e)))?,
            None,
        )
        .await?;
    }
    Ok(new_status == "success")
}

/// Etat complet envoye par l'agent `server` a sa (re)connexion. Pour chaque
/// deploiement que le plan de controle croit en cours sur ce serveur :
/// 1. son issue gardee par l'agent s'applique ;
/// 2. sinon, en cours sur l'agent : on attend (et `Stop` est renvoye si une
///    annulation est demandee) ;
/// 3. sinon, servi : reussi ;
/// 4. sinon, aucun ordre parti : il est envoye maintenant ;
/// 5. sinon, perdu : `failed` et `RemoveDeployment`.
///
/// Puis la table des routes est renvoyee et le nettoyage remis en file,
/// meme si une etape precedente a echoue (base indisponible) : le proxy du
/// noeud ne reste jamais sans table apres une reconnexion.
pub async fn reconcile(state: &AppState, server: Uuid, agent: &AgentState) -> sqlx::Result<()> {
    let pending = reconcile_deployments(state, server, agent).await;
    // Table des routes apres chaque `Hello` (agent reconnecte).
    let routes = async {
        let mut conn = state.db.acquire().await?;
        domains::route_table(&mut conn, &state.cfg, server).await
    }
    .await
    .map(|table| {
        send_logged(state, server, ControlToAgent::ApplyRoutes(table));
    });
    let cleanups = enqueue_cleanups(state, server).await;
    pending.and(routes).and(cleanups)
}

async fn reconcile_deployments(
    state: &AppState,
    server: Uuid,
    agent: &AgentState,
) -> sqlx::Result<()> {
    #[derive(sqlx::FromRow)]
    struct Pending {
        id: Uuid,
        status: String,
        cancel_requested: bool,
        presumed_failed: bool,
        environment_id: Uuid,
        sent: bool,
        deploy_sent: bool,
        recent: bool,
    }
    let grace = now(state) - LOST_GRACE.as_secs_f64();
    let pending: Vec<Pending> = sqlx::query_as(
        "SELECT id, status, cancel_requested, presumed_failed, environment_id, \
                (build_sent_at IS NOT NULL OR deploy_sent_at IS NOT NULL) AS sent, \
                deploy_sent_at IS NOT NULL AS deploy_sent, \
                COALESCE(GREATEST(build_sent_at, deploy_sent_at) > to_timestamp($2::float8), false) AS recent \
         FROM deployments WHERE server_id = $1 \
         AND (status = 'running' OR (status = 'failed' AND (presumed_failed OR cancel_requested) AND finished_at > to_timestamp($2::float8) - interval '30 days')) \
         ORDER BY created_at",
    )
    .bind(server)
    .bind(grace)
    .fetch_all(&state.db)
    .await?;
    for p in pending {
        let did = DeploymentId(p.id);
        let in_progress = agent.in_progress.contains(&did);
        let serving = agent
            .deployments
            .iter()
            .any(|d| d.deployment_id == did && d.serving);
        if let Some(outcome) = agent.outcomes.iter().find(|o| o.deployment_id == did) {
            on_progress(state, server, p.id, outcome.stage, &outcome.status).await?;
            // Build conclu, `Deploy` deja parti mais inconnu de l'agent : perdu
            // en route, renvoye (l'agent ignore un doublon).
            if p.status == "running"
                && p.deploy_sent
                && !in_progress
                && outcome.stage == Stage::Image
                && outcome.status == StageStatus::Succeeded
            {
                resend_deploy(state, p.id).await?;
            }
            continue;
        }
        if p.status != "running" {
            if p.presumed_failed {
                if serving {
                    on_progress(state, server, p.id, Stage::Switch, &StageStatus::Succeeded)
                        .await?;
                } else if in_progress {
                    // Encore en cours alors qu'il a ete abandonne : interrompu.
                    send_logged(state, server, ControlToAgent::Stop { deployment_id: did });
                } else {
                    confirm_failure(state, p.id).await?;
                }
            }
            continue;
        }
        if in_progress {
            touch(state, server, p.id).await?;
            if p.cancel_requested {
                send_logged(state, server, ControlToAgent::Stop { deployment_id: did });
            }
        } else if serving {
            on_progress(state, server, p.id, Stage::Switch, &StageStatus::Succeeded).await?;
        } else if !p.sent {
            if let Err(e) = dispatch(state, p.id).await {
                // File d'envoi pleine ou base indisponible : une tache
                // `deploy` reprend l'envoi (avec delai), sans attendre le
                // chien de garde.
                tracing::warn!(deployment = %p.id, "envoi a la reconnexion impossible : {} (remis en file)", e.message);
                let mut conn = state.db.acquire().await?;
                enqueue_deployment(&mut conn, p.id).await?;
            }
        } else if p.recent {
            touch(state, server, p.id).await?;
        } else {
            lose(state, server, p.id, p.environment_id).await?;
        }
    }
    Ok(())
}

async fn resend_deploy(state: &AppState, id: Uuid) -> sqlx::Result<()> {
    let mut tx = state.db.begin().await?;
    if let Some(info) = info(&mut tx, id).await?.filter(Info::running)
        && send_deploy(&mut tx, state, &info).await? == DeploySent::Busy
    {
        // File d'envoi pleine : la tache mise en file par `send_deploy`
        // doit renvoyer l'ordre, donc il n'est plus note comme parti.
        sqlx::query("UPDATE deployments SET deploy_sent_at = NULL WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

async fn confirm_failure(state: &AppState, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE deployments SET presumed_failed = false WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Deploiement inconnu de l'agent alors qu'un ordre est parti : `failed`.
async fn lose(state: &AppState, server: Uuid, id: Uuid, environment: Uuid) -> sqlx::Result<()> {
    let mut tx = state.db.begin().await?;
    if info(&mut tx, id).await?.filter(Info::running).is_none() {
        return Ok(());
    }
    finish(
        &mut tx,
        state,
        id,
        environment,
        Finish::Failed {
            message: "Déploiement perdu par l'agent (ni en cours, ni conclu).".into(),
            presumed: false,
        },
    )
    .await?;
    tx.commit().await?;
    send_logged(
        state,
        server,
        ControlToAgent::RemoveDeployment {
            deployment_id: DeploymentId(id),
        },
    );
    Ok(())
}

/// Nettoyage remis en file pour les environnements de ce serveur qui ont
/// des deploiements termines pas encore nettoyes (agent absent jusque-la).
async fn enqueue_cleanups(state: &AppState, server: Uuid) -> sqlx::Result<()> {
    let envs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT environment_id FROM deployments WHERE server_id = $1 \
         AND status IN ('success', 'failed', 'cancelled') AND cleaned_at IS NULL",
    )
    .bind(server)
    .fetch_all(&state.db)
    .await?;
    let mut conn = state.db.acquire().await?;
    for env in envs {
        jobs::enqueue_in(
            &mut conn,
            JobKind::Cleanup,
            serde_json::to_value(CleanupJob {
                environment_id: env,
            })
            .map_err(|e| sqlx::Error::Encode(Box::new(e)))?,
            None,
        )
        .await?;
    }
    Ok(())
}

/// Chien de garde : un deploiement en cours sans nouvelle de l'agent depuis
/// `STALE_AFTER` passe `failed` (echec presume : l'issue reelle rapportee a
/// la reconnexion prime). L'ordre est retire de l'agent s'il est joignable.
pub async fn sweep_stale(state: &AppState) -> sqlx::Result<usize> {
    let limit = now(state) - STALE_AFTER.as_secs_f64();
    let stale: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM deployments WHERE status = 'running' \
         AND last_event_at < to_timestamp($1::float8)",
    )
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let mut failed = 0;
    for id in stale {
        let mut tx = state.db.begin().await?;
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
            "SELECT environment_id, server_id FROM deployments WHERE id = $1 \
             AND status = 'running' AND last_event_at < to_timestamp($2::float8) FOR UPDATE",
        )
        .bind(id)
        .bind(limit)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((env, server)) = row else {
            continue;
        };
        finish(
            &mut tx,
            state,
            id,
            env,
            Finish::Failed {
                message: "Aucune nouvelle de l'agent depuis 10 minutes.".into(),
                presumed: true,
            },
        )
        .await?;
        tx.commit().await?;
        failed += 1;
        tracing::warn!(deployment = %id, "deploiement sans nouvelles depuis 10 min : failed");
        if let Some(server) = server {
            // Interrompu s'il tourne encore (trop tard apres la bascule : son
            // issue reelle sera alors rapportee).
            let _ = state.agents.send(
                ServerId(server),
                ControlToAgent::Stop {
                    deployment_id: DeploymentId(id),
                },
            );
        }
    }
    Ok(failed)
}

/// Suit les messages des agents et fait tourner le chien de garde jusqu'a
/// l'arret. `events` (`AgentRegistry::subscribe`) et `logs`
/// (`subscribe_logs`) doivent etre abonnes avant le demarrage du lien
/// agents, pour ne manquer aucun `State`. Les lignes de journal arrivent sur
/// leur propre canal : un flot de lignes ne fait perdre aucun message de
/// deploiement.
pub async fn run(
    state: AppState,
    mut events: broadcast::Receiver<(ServerId, AgentToControl)>,
    mut logs: broadcast::Receiver<(ServerId, AgentToControl)>,
    mut stop: watch::Receiver<bool>,
) {
    let mut watchdog = tokio::time::interval(WATCHDOG_EVERY);
    let mut log_touches: HashMap<DeploymentId, Instant> = HashMap::new();
    // Derniere demande d'etat complet (une seule par `RESYNC_MIN_INTERVAL`).
    let mut last_resync: Option<Instant> = None;
    // Perte survenue pendant l'intervalle : une demande, une seule, a son
    // echeance (sans elle, les messages perdus entre-temps ne seraient
    // jamais rattrapes avant le chien de garde).
    let mut resync_pending = false;
    let request_state = |state: &AppState| {
        for server in state.agents.connected() {
            send_logged(state, server.0, ControlToAgent::RequestState);
        }
    };
    loop {
        let resync_at = tokio::time::Instant::from_std(
            last_resync.map_or_else(Instant::now, |t| t + RESYNC_MIN_INTERVAL),
        );
        tokio::select! {
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
            event = events.recv() => match event {
                Ok((server, msg)) => on_agent_message(&state, server, msg).await,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    // Resynchronisation : chaque agent renvoie son etat
                    // complet (`reconcile`), sans attendre le chien de garde ;
                    // une seule demande par `RESYNC_MIN_INTERVAL` (le suivi
                    // est deja en retard : pas de charge en plus).
                    if last_resync.is_some_and(|t| t.elapsed() < RESYNC_MIN_INTERVAL) {
                        tracing::warn!("{n} messages d'agents perdus : etat redemande a la fin de l'intervalle");
                        resync_pending = true;
                    } else {
                        last_resync = Some(Instant::now());
                        resync_pending = false;
                        tracing::warn!("{n} messages d'agents perdus (retard du suivi des deploiements) : etat redemande");
                        request_state(&state);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            () = tokio::time::sleep_until(resync_at), if resync_pending => {
                last_resync = Some(Instant::now());
                resync_pending = false;
                request_state(&state);
            }
            line = logs.recv() => match line {
                Ok((server, AgentToControl::LogLine(line))) => {
                    if let LogTarget::Deployment(id) = line.target {
                        let due = log_touches
                            .get(&id)
                            .is_none_or(|t| t.elapsed() >= LOG_TOUCH_EVERY);
                        if due {
                            log_touches.insert(id, Instant::now());
                            on_agent_message(&state, server, AgentToControl::LogLine(line)).await;
                        }
                    }
                }
                Ok(_) => {}
                // Lignes perdues : seul le signe de vie d'un build est
                // retarde (prochaine ligne ou chien de garde).
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = watchdog.tick() => {
                log_touches.retain(|_, t| t.elapsed() < LOG_TOUCH_EVERY);
                if let Err(e) = sweep_stale(&state).await {
                    tracing::error!(error = %e, "chien de garde des deploiements");
                }
            }
        }
    }
}
