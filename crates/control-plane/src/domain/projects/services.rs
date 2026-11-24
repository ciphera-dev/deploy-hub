//! Services d'un projet : etat derive (deploiement servi, agent, marque
//! d'arret) et ordres de redemarrage et d'arret envoyes a l'agent.
//!
//! Le deploiement servi d'un environnement est son dernier deploiement
//! reussi, comme `current` dans l'API des deploiements. En v0.1, un
//! environnement a un seul service, et ce deploiement porte tous ses
//! conteneurs (ensemble Compose compris) : redemarrer ou arreter un service
//! vise donc le deploiement servi de son environnement.

use api_types::project_overview::{ProjectService, Replicas, ServiceState};
use proto::ControlToAgent;
use serde_json::json;
use shared::ids::{DeploymentId, ServerId};
use sqlx::PgConnection;
use uuid::Uuid;

use super::store::PRODUCTION;
use crate::domain::deployments::lock_environment;
use crate::domain::metrics;
use crate::state::AppState;

/// Replicas voulus par service (mise a l'echelle en v0.2).
pub const DESIRED_REPLICAS: u32 = 1;

/// Ce que l'on sait d'un service pour en deriver l'etat.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    /// Un deploiement reussi sert l'environnement.
    pub serving: bool,
    /// Ses conteneurs ont ete arretes depuis le panneau.
    pub stopped: bool,
    /// L'agent du serveur du deploiement servi est connecte.
    pub connected: bool,
    /// L'agent signale le conteneur servi arrete, en echec ou disparu.
    pub down: bool,
    /// Un deploiement de l'environnement est en file ou en cours.
    pub active: bool,
    /// Le dernier deploiement termine de l'environnement a echoue.
    pub last_failed: bool,
}

/// Etat affiche d'un service. Servi : arrete (depuis le panneau),
/// injoignable, arrete hors du panneau ou en marche ; sinon le deploiement
/// en cours, puis l'echec, sinon jamais deploye.
pub fn state_of(f: Facts) -> ServiceState {
    match f {
        Facts {
            serving: true,
            stopped: true,
            ..
        } => ServiceState::Stopped,
        Facts {
            serving: true,
            connected: false,
            ..
        } => ServiceState::Unreachable,
        Facts {
            serving: true,
            down: true,
            ..
        } => ServiceState::Down,
        Facts { serving: true, .. } => ServiceState::Running,
        Facts { active: true, .. } => ServiceState::Deploying,
        Facts {
            last_failed: true, ..
        } => ServiceState::Failed,
        _ => ServiceState::None,
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    environment: String,
    internal_port: Option<i32>,
    stopped_deployment: Option<Uuid>,
    serving: Option<Uuid>,
    server_id: Option<Uuid>,
    number: Option<i64>,
    image: Option<String>,
    container_running: Option<bool>,
    active: bool,
    last_failed: bool,
}

/// Services du projet et leur etat (environnement puis nom). `None` :
/// projet introuvable.
pub async fn list(
    state: &AppState,
    conn: &mut PgConnection,
    project: Uuid,
) -> sqlx::Result<Option<Vec<ProjectService>>> {
    let exists: Option<bool> = sqlx::query_scalar("SELECT true FROM projects WHERE id = $1")
        .bind(project)
        .fetch_optional(&mut *conn)
        .await?;
    if exists.is_none() {
        return Ok(None);
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT s.id, s.name, e.name AS environment, s.internal_port, s.stopped_deployment, \
                cur.id AS serving, cur.server_id, cur.number, cur.image, cur.container_running, \
                EXISTS (SELECT 1 FROM deployments a WHERE a.environment_id = e.id \
                        AND a.status IN ('queued', 'running')) AS active, \
                COALESCE((SELECT l.status = 'failed' FROM deployments l WHERE l.environment_id = e.id \
                          AND l.status IN ('success', 'failed') \
                          ORDER BY l.finished_at DESC NULLS LAST, l.number DESC LIMIT 1), false) AS last_failed \
         FROM services s JOIN environments e ON e.id = s.environment_id \
         LEFT JOIN LATERAL (SELECT c.id, c.server_id, c.number, c.image, c.container_running FROM deployments c \
              WHERE c.environment_id = e.id AND c.status = 'success' \
              ORDER BY c.finished_at DESC, c.number DESC LIMIT 1) cur ON true \
         WHERE e.project_id = $1 ORDER BY e.created_at, e.name, s.name",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let usage = metrics::current_usage(conn, &ids, state.clock.unix_secs()).await?;
    Ok(Some(
        rows.into_iter()
            .map(|r| {
                let facts = Facts {
                    serving: r.serving.is_some(),
                    stopped: r.serving.is_some() && r.stopped_deployment == r.serving,
                    connected: r
                        .server_id
                        .is_some_and(|s| state.agents.is_connected(ServerId(s))),
                    down: r.container_running == Some(false),
                    active: r.active,
                    last_failed: r.last_failed,
                };
                let running = state_of(facts) == ServiceState::Running;
                let usage = usage.get(&r.id).filter(|_| running);
                let service_state = state_of(facts);
                ProjectService {
                    id: r.id.to_string(),
                    name: r.name,
                    environment: r.environment,
                    state: service_state,
                    deployment_id: r.serving.map(|d| d.to_string()),
                    deployment_number: r.number,
                    image: r.image,
                    internal_port: r.internal_port.and_then(|p| u16::try_from(p).ok()),
                    replicas: Replicas {
                        running: u32::from(service_state == ServiceState::Running),
                        desired: DESIRED_REPLICAS,
                    },
                    cpu_percent: usage.map(|u| u.0),
                    memory_bytes: usage.map(|u| u.1),
                }
            })
            .collect(),
    ))
}

/// Deploiement servi d'un environnement et son serveur (s'il en a un).
pub async fn serving(
    conn: &mut PgConnection,
    environment: Uuid,
) -> sqlx::Result<Option<(Uuid, Option<Uuid>)>> {
    sqlx::query_as(
        "SELECT id, server_id FROM deployments WHERE environment_id = $1 AND status = 'success' \
         ORDER BY finished_at DESC, number DESC LIMIT 1",
    )
    .bind(environment)
    .fetch_optional(conn)
    .await
}

/// Ce qu'un ordre vise : la production du projet, ou un de ses services.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Project,
    Service(Uuid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Restart,
    Stop,
}

#[derive(Debug)]
pub enum ActionError {
    /// Projet, environnement ou service introuvable (ou d'un autre projet).
    NotFound,
    /// Aucun deploiement servi.
    NothingServing,
    /// Agent du serveur deconnecte : rien n'est enregistre.
    AgentUnavailable,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for ActionError {
    fn from(e: sqlx::Error) -> Self {
        ActionError::Db(e)
    }
}

fn audit_action(target: Target, action: Action) -> &'static str {
    match (target, action) {
        (Target::Project, Action::Restart) => "project.restart",
        (Target::Project, Action::Stop) => "project.stop",
        (Target::Service(_), Action::Restart) => "service.restart",
        (Target::Service(_), Action::Stop) => "service.stop",
    }
}

/// Redemarre ou arrete le deploiement servi vise, sous le verrou de son
/// environnement (pas de bascule concurrente). La marque d'arret des
/// services de l'environnement suit l'ordre (posee par `Stop`, effacee par
/// `Restart`) et l'ordre est audite ; tout est annule si l'agent ne le
/// recoit pas. Renvoie le deploiement vise.
pub async fn act(
    state: &AppState,
    actor: Uuid,
    project: Uuid,
    target: Target,
    action: Action,
) -> Result<Uuid, ActionError> {
    let mut tx = state.db.begin().await?;
    let environment: Option<Uuid> = match target {
        Target::Project => {
            sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1 AND name = $2")
                .bind(project)
                .bind(PRODUCTION)
                .fetch_optional(&mut *tx)
                .await?
        }
        Target::Service(service) => {
            sqlx::query_scalar(
                "SELECT s.environment_id FROM services s JOIN environments e ON e.id = s.environment_id \
                 WHERE s.id = $1 AND e.project_id = $2",
            )
            .bind(service)
            .bind(project)
            .fetch_optional(&mut *tx)
            .await?
        }
    };
    let environment = environment.ok_or(ActionError::NotFound)?;
    lock_environment(&mut tx, environment).await?;
    let Some((deployment, Some(server))) = serving(&mut tx, environment).await? else {
        return Err(ActionError::NothingServing);
    };
    sqlx::query("UPDATE services SET stopped_deployment = $2 WHERE environment_id = $1")
        .bind(environment)
        .bind((action == Action::Stop).then_some(deployment))
        .execute(&mut *tx)
        .await?;
    let service = match target {
        Target::Service(s) => Some(s),
        Target::Project => None,
    };
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(audit_action(target, action))
    .bind(if service.is_some() { "service" } else { "project" })
    .bind(json!({ "project_id": project, "service_id": service, "deployment_id": deployment }))
    .execute(&mut *tx)
    .await?;
    let deployment_id = DeploymentId(deployment);
    let msg = match action {
        Action::Restart => ControlToAgent::Restart { deployment_id },
        Action::Stop => ControlToAgent::StopContainers { deployment_id },
    };
    if let Err(e) = state.agents.send(ServerId(server), msg) {
        tracing::warn!(%server, "ordre {action:?} non envoye a l'agent : {e}");
        return Err(ActionError::AgentUnavailable);
    }
    tx.commit().await?;
    Ok(deployment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_follows_serving_stop_and_agent() {
        let serving = Facts {
            serving: true,
            connected: true,
            ..Facts::default()
        };
        assert_eq!(state_of(Facts::default()), ServiceState::None);
        assert_eq!(state_of(serving), ServiceState::Running);
        // Un nouveau deploiement en cours ou un echec plus recent ne coupe
        // pas le service servi.
        assert_eq!(
            state_of(Facts {
                active: true,
                last_failed: true,
                ..serving
            }),
            ServiceState::Running
        );
        assert_eq!(
            state_of(Facts {
                stopped: true,
                ..serving
            }),
            ServiceState::Stopped
        );
        assert_eq!(
            state_of(Facts {
                connected: false,
                ..serving
            }),
            ServiceState::Unreachable
        );
        // Conteneur arrete hors du panneau ; l'arret depuis le panneau et
        // l'agent injoignable priment.
        let down = Facts {
            down: true,
            ..serving
        };
        assert_eq!(state_of(down), ServiceState::Down);
        assert_eq!(
            state_of(Facts {
                stopped: true,
                ..down
            }),
            ServiceState::Stopped
        );
        assert_eq!(
            state_of(Facts {
                connected: false,
                ..down
            }),
            ServiceState::Unreachable
        );
        assert_eq!(
            state_of(Facts {
                active: true,
                last_failed: true,
                ..Facts::default()
            }),
            ServiceState::Deploying
        );
        assert_eq!(
            state_of(Facts {
                last_failed: true,
                ..Facts::default()
            }),
            ServiceState::Failed
        );
    }
}
