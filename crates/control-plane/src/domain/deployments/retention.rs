//! Retention des images (tache `cleanup`) : par environnement, les
//! `RETAINED_IMAGES` derniers deploiements reussis restent disponibles pour
//! le rollback ; les autres deploiements termines sont supprimes sur leur
//! agent (`RemoveDeployment` : conteneurs, images, ensemble Compose).
//!
//! Ne sont jamais supprimes : un deploiement conserve, l'image qu'un
//! deploiement conserve ou en file/en cours reutilise (rollback), ni un
//! echec seulement presume (l'agent n'a pas encore rendu son verdict).
//! Calcul et suppression se font sous le verrou de l'environnement, comme
//! la creation d'un rollback : une image choisie ne disparait pas en route.
//! Les journaux enregistres d'un deploiement supprime le sont aussi.
//!
//! Nettoyage confirme : `cleaned_at` n'est pose qu'a l'accuse de l'agent
//! (`DeploymentRemoved`, voir [`removed`]). Tant qu'il manque, chaque tache
//! `cleanup` renvoie l'ordre (connexion coupee avant l'ecriture, agent
//! redemarre, echec de Docker) ; `removal_requested_at` retire entre-temps le
//! deploiement des retours arriere possibles. Un deploiement jamais envoye a
//! un agent n'a rien a supprimer : nettoye tout de suite.

use proto::ControlToAgent;
use shared::ids::{DeploymentId, ServerId};
use uuid::Uuid;

use super::orchestrator::db_error;
use super::{CleanupJob, RETAINED_IMAGES, lock_environment};
use crate::domain::logs;
use crate::jobs::Job;
use crate::jobs::worker::HandlerError;
use crate::state::AppState;

/// Tache `cleanup`.
pub async fn handle_cleanup(state: &AppState, job: Job) -> Result<(), HandlerError> {
    let payload: CleanupJob = serde_json::from_value(job.payload)
        .map_err(|_| HandlerError::fatal("charge utile de nettoyage illisible"))?;
    let pending = cleanup(state, payload.environment_id)
        .await
        .map_err(|e| db_error("nettoyage des images", e))?;
    if pending > 0 {
        // Agent absent : reessai (et a sa reconnexion).
        return Err(HandlerError::retry(format!(
            "{pending} deploiement(s) a supprimer, agent injoignable"
        )));
    }
    Ok(())
}

/// Supprime ce qui depasse la retention de `environment` (ordre renvoye tant
/// que l'agent n'a pas accuse la suppression). Renvoie le nombre de
/// deploiements dont l'ordre n'a pas pu partir (agent absent).
pub async fn cleanup(state: &AppState, environment: Uuid) -> sqlx::Result<usize> {
    let mut tx = state.db.begin().await?;
    lock_environment(&mut tx, environment).await?;
    let candidates: Vec<(Uuid, Option<Uuid>, bool)> = sqlx::query_as(
        "WITH kept AS ( \
             SELECT id, image_from FROM deployments \
             WHERE environment_id = $1 AND status = 'success' \
             ORDER BY finished_at DESC, number DESC LIMIT $2), \
         protected AS ( \
             SELECT id FROM kept \
             UNION SELECT image_from FROM kept WHERE image_from IS NOT NULL \
             UNION SELECT image_from FROM deployments \
                   WHERE environment_id = $1 AND status IN ('queued', 'running') \
                   AND image_from IS NOT NULL) \
         SELECT d.id, d.server_id, \
                (d.build_sent_at IS NOT NULL OR d.deploy_sent_at IS NOT NULL) AS dispatched \
         FROM deployments d \
         WHERE d.environment_id = $1 AND d.status IN ('success', 'failed', 'cancelled') \
         AND d.cleaned_at IS NULL AND NOT d.presumed_failed \
         AND d.id NOT IN (SELECT id FROM protected) \
         ORDER BY d.finished_at, d.number",
    )
    .bind(environment)
    .bind(RETAINED_IMAGES)
    .fetch_all(&mut *tx)
    .await?;
    let mut pending = 0;
    for (id, server, dispatched) in candidates {
        let Some(server) = server.filter(|_| dispatched) else {
            // Rien n'a jamais ete envoye a un agent : rien a supprimer.
            mark_cleaned(&mut tx, state, id).await?;
            continue;
        };
        match state.agents.send(
            ServerId(server),
            ControlToAgent::RemoveDeployment {
                deployment_id: DeploymentId(id),
            },
        ) {
            Ok(()) => {
                // Nettoye a l'accuse de l'agent seulement.
                sqlx::query(
                    "UPDATE deployments \
                     SET removal_requested_at = COALESCE(removal_requested_at, to_timestamp($2::float8)) \
                     WHERE id = $1",
                )
                .bind(id)
                .bind(state.clock.unix_secs())
                .execute(&mut *tx)
                .await?;
            }
            Err(e) => {
                tracing::warn!(deployment = %id, %server, "suppression differee : {e}");
                pending += 1;
            }
        }
    }
    tx.commit().await?;
    Ok(pending)
}

/// Pose `cleaned_at` et supprime les journaux enregistres de `id`.
async fn mark_cleaned(
    conn: &mut sqlx::PgConnection,
    state: &AppState,
    id: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE deployments SET cleaned_at = to_timestamp($2::float8) WHERE id = $1")
        .bind(id)
        .bind(state.clock.unix_secs())
        .execute(&mut *conn)
        .await?;
    // Ses journaux partent avec ses conteneurs et ses images.
    logs::store::delete(conn, id).await
}

/// Accuse `DeploymentRemoved` de l'agent `server` : conteneurs et images de
/// `id` supprimes (ou deja absents). Seul un deploiement termine de ce
/// serveur, pas encore nettoye, est marque ; un accuse d'un autre serveur ou
/// d'un deploiement inconnu (projet supprime) est ignore.
pub async fn removed(state: &AppState, server: Uuid, id: Uuid) -> sqlx::Result<()> {
    let mut tx = state.db.begin().await?;
    let found: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM deployments WHERE id = $1 AND server_id = $2 AND cleaned_at IS NULL \
         AND status IN ('success', 'failed', 'cancelled') FOR UPDATE",
    )
    .bind(id)
    .bind(server)
    .fetch_optional(&mut *tx)
    .await?;
    if found.is_some() {
        mark_cleaned(&mut tx, state, id).await?;
    }
    tx.commit().await
}
