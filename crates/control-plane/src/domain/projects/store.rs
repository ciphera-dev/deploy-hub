//! Acces SQL aux projets, environnements, services et sources Git.

use api_types::projects::{
    Build, Environment, GitSource, ImageSource, Project, ProjectStatus, ProjectSummary, Service,
    Source,
};
use shared::ids::ServerId;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::agents::registry::AgentRegistry;
use crate::connectors::github::app::RepoLink;

/// Environnement cree avec chaque projet (le seul de la v0.1).
pub const PRODUCTION: &str = "production";

const TS: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'";

#[derive(Debug)]
pub enum StoreError {
    NameTaken,
    NotFound,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Db(e)
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|code| code == "23505")
}

/// Projet valide, pret a etre ecrit.
pub struct Draft {
    pub name: String,
    pub source: Source,
    pub build: Option<Build>,
}

/// Nom du service principal : le service Compose expose, sinon le projet.
pub fn service_name(name: &str, build: Option<&Build>) -> String {
    match build {
        Some(Build::Compose { service, .. }) => service.clone(),
        _ => name.to_string(),
    }
}

fn build_columns(build: Option<&Build>) -> (Option<&'static str>, Option<&str>, Option<&str>) {
    match build {
        None => (None, None, None),
        Some(Build::Railpack) => (Some("railpack"), None, None),
        Some(Build::Dockerfile { path }) => (Some("dockerfile"), Some(path), None),
        Some(Build::Compose { path, service }) => (Some("compose"), Some(path), Some(service)),
    }
}

pub async fn create(
    conn: &mut PgConnection,
    draft: &Draft,
    created_by: Uuid,
    link: RepoLink,
) -> Result<Uuid, StoreError> {
    let id = Uuid::new_v4();
    let (kind, path, service) = build_columns(draft.build.as_ref());
    let (source_kind, image_ref) = match &draft.source {
        Source::Git(_) => ("git", None),
        Source::Image(i) => ("image", Some(i.image.as_str())),
    };
    sqlx::query(
        "INSERT INTO projects (id, name, source_kind, image_ref, build_kind, build_path, build_service, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(id)
    .bind(&draft.name)
    .bind(source_kind)
    .bind(image_ref)
    .bind(kind)
    .bind(path)
    .bind(service)
    .bind(created_by)
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            StoreError::NameTaken
        } else {
            e.into()
        }
    })?;
    if let Source::Git(git) = &draft.source {
        // Deploiement automatique : valeur par defaut de l'organisation
        // (Parametres > Deploiements).
        sqlx::query(
            "INSERT INTO git_sources (project_id, repository, branch, auto_deploy, \
                                      github_repo_id, via_app) \
             VALUES ($1, $2, $3, coalesce((SELECT deploy_auto FROM instance LIMIT 1), true), $4, $5)",
        )
        .bind(id)
        .bind(&git.repository)
        .bind(&git.branch)
        .bind(link.repo_id)
        .bind(link.via_app)
        .execute(&mut *conn)
        .await?;
    }
    let env_id = Uuid::new_v4();
    sqlx::query("INSERT INTO environments (id, project_id, name) VALUES ($1, $2, $3)")
        .bind(env_id)
        .bind(id)
        .bind(PRODUCTION)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO services (id, environment_id, name) VALUES ($1, $2, $3)")
        .bind(Uuid::new_v4())
        .bind(env_id)
        .bind(service_name(&draft.name, draft.build.as_ref()))
        .execute(&mut *conn)
        .await?;
    Ok(id)
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    source_kind: String,
    image_ref: Option<String>,
    build_kind: Option<String>,
    build_path: Option<String>,
    build_service: Option<String>,
    repository: Option<String>,
    branch: Option<String>,
    environment: Option<String>,
    services: i64,
    /// Un deploiement reussi du projet sert (dernier reussi de son
    /// environnement).
    serving: bool,
    /// Un deploiement du projet est en file ou en cours.
    active: bool,
    /// Statut du dernier deploiement termine (`success` ou `failed`).
    last_finished: Option<String>,
    /// Deploiement servi de la production, son serveur, sa marque d'arret
    /// et l'etat de son conteneur remonte par l'agent.
    prod_id: Option<Uuid>,
    prod_server: Option<Uuid>,
    prod_stopped: bool,
    prod_running: Option<bool>,
    created_at: String,
    updated_at: String,
}

fn select() -> String {
    format!(
        "SELECT p.id, p.name, p.source_kind, p.image_ref, p.build_kind, p.build_path, p.build_service, \
                g.repository, g.branch, \
                (SELECT e.name FROM environments e WHERE e.project_id = p.id ORDER BY e.created_at, e.name LIMIT 1) AS environment, \
                (SELECT count(*) FROM services s JOIN environments e ON e.id = s.environment_id WHERE e.project_id = p.id) AS services, \
                EXISTS (SELECT 1 FROM deployments d WHERE d.project_id = p.id AND d.status = 'success') AS serving, \
                EXISTS (SELECT 1 FROM deployments d WHERE d.project_id = p.id AND d.status IN ('queued', 'running')) AS active, \
                (SELECT d.status FROM deployments d WHERE d.project_id = p.id AND d.status IN ('success', 'failed') \
                 ORDER BY d.finished_at DESC NULLS LAST, d.number DESC LIMIT 1) AS last_finished, \
                prod.id AS prod_id, prod.server_id AS prod_server, \
                EXISTS (SELECT 1 FROM services s WHERE s.environment_id = prod.environment_id \
                        AND s.stopped_deployment = prod.id) AS prod_stopped, \
                prod.container_running AS prod_running, \
                to_char(p.created_at AT TIME ZONE 'UTC', {TS}) AS created_at, \
                to_char(p.updated_at AT TIME ZONE 'UTC', {TS}) AS updated_at \
         FROM projects p LEFT JOIN git_sources g ON g.project_id = p.id \
         LEFT JOIN LATERAL (SELECT c.id, c.server_id, c.environment_id, c.container_running \
              FROM deployments c JOIN environments e ON e.id = c.environment_id \
              WHERE c.project_id = p.id AND e.name = '{PRODUCTION}' AND c.status = 'success' \
              ORDER BY c.finished_at DESC, c.number DESC LIMIT 1) prod ON true"
    )
}

impl Row {
    fn source(&self) -> Result<Source, sqlx::Error> {
        let broken = || sqlx::Error::Protocol(format!("projet {} incoherent", self.id));
        match self.source_kind.as_str() {
            "git" => Ok(Source::Git(GitSource {
                repository: self.repository.clone().ok_or_else(broken)?,
                branch: self.branch.clone().ok_or_else(broken)?,
            })),
            "image" => Ok(Source::Image(ImageSource {
                image: self.image_ref.clone().ok_or_else(broken)?,
            })),
            _ => Err(broken()),
        }
    }

    fn build(&self) -> Result<Option<Build>, sqlx::Error> {
        let broken = || sqlx::Error::Protocol(format!("projet {} incoherent", self.id));
        Ok(match self.build_kind.as_deref() {
            None => None,
            Some("railpack") => Some(Build::Railpack),
            Some("dockerfile") => Some(Build::Dockerfile {
                path: self.build_path.clone().ok_or_else(broken)?,
            }),
            Some("compose") => Some(Build::Compose {
                path: self.build_path.clone().ok_or_else(broken)?,
                service: self.build_service.clone().ok_or_else(broken)?,
            }),
            Some(_) => return Err(broken()),
        })
    }
}

/// Etat affiche du projet, ce que les utilisateurs obtiennent : un
/// deploiement servi = en ligne (meme si le dernier a echoue ou qu'un autre
/// est en cours : l'echec reste visible dans l'activite et les
/// deploiements) ; sinon un deploiement en file ou en cours ; sinon l'echec
/// du dernier deploiement termine (une annulation ne compte pas) ; sinon
/// jamais deploye.
pub fn project_status(serving: bool, active: bool, last_finished: Option<&str>) -> ProjectStatus {
    match (serving, active, last_finished) {
        (true, _, _) => ProjectStatus::Running,
        (false, true, _) => ProjectStatus::Building,
        (false, false, Some("failed")) => ProjectStatus::Failed,
        _ => ProjectStatus::None,
    }
}

/// Ce que l'on sait du deploiement servi de la production.
#[derive(Clone, Copy, Debug)]
pub struct Production {
    /// Arrete depuis le panneau.
    pub stopped: bool,
    /// Agent de son serveur connecte.
    pub connected: bool,
    /// Etat du conteneur remonte par l'agent (`None` : rien encore).
    pub running: Option<bool>,
}

/// Statut « en ligne » precise par la production servie : arretee depuis le
/// panneau, agent injoignable, conteneur arrete hors du panneau.
pub fn refine(status: ProjectStatus, production: Option<Production>) -> ProjectStatus {
    match (status, production) {
        (ProjectStatus::Running, Some(p)) if p.stopped => ProjectStatus::Stopped,
        (ProjectStatus::Running, Some(p)) if !p.connected => ProjectStatus::Unreachable,
        (ProjectStatus::Running, Some(p)) if p.running == Some(false) => ProjectStatus::Down,
        (status, _) => status,
    }
}

impl Row {
    fn status(&self, agents: &AgentRegistry) -> ProjectStatus {
        let production = self.prod_id.map(|_| Production {
            stopped: self.prod_stopped,
            connected: self
                .prod_server
                .is_some_and(|s| agents.is_connected(ServerId(s))),
            running: self.prod_running,
        });
        refine(
            project_status(self.serving, self.active, self.last_finished.as_deref()),
            production,
        )
    }
}

pub async fn list(
    pool: &PgPool,
    agents: &AgentRegistry,
) -> Result<Vec<ProjectSummary>, sqlx::Error> {
    let rows: Vec<Row> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} ORDER BY p.name", select())))
            .fetch_all(pool)
            .await?;
    rows.into_iter()
        .map(|r| {
            Ok(ProjectSummary {
                id: r.id.to_string(),
                name: r.name.clone(),
                environment: r.environment.clone().unwrap_or_else(|| PRODUCTION.into()),
                status: r.status(agents),
                source: r.source()?,
                build: r.build()?,
                services: u32::try_from(r.services).unwrap_or(u32::MAX),
                created_at: r.created_at.clone(),
                updated_at: r.updated_at.clone(),
            })
        })
        .collect()
}

pub async fn get(
    conn: &mut PgConnection,
    agents: &AgentRegistry,
    id: Uuid,
) -> Result<Option<Project>, sqlx::Error> {
    let row: Option<Row> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE p.id = $1", select())))
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    let environments: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, name FROM environments WHERE project_id = $1 ORDER BY created_at, name",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let services: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT s.id, s.name, e.name FROM services s JOIN environments e ON e.id = s.environment_id \
         WHERE e.project_id = $1 ORDER BY e.created_at, e.name, s.name",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Some(Project {
        id: r.id.to_string(),
        name: r.name.clone(),
        status: r.status(agents),
        source: r.source()?,
        build: r.build()?,
        environments: environments
            .into_iter()
            .map(|(id, name)| Environment {
                id: id.to_string(),
                name,
            })
            .collect(),
        services: services
            .into_iter()
            .map(|(id, name, environment)| Service {
                id: id.to_string(),
                name,
                environment,
            })
            .collect(),
        created_at: r.created_at.clone(),
        updated_at: r.updated_at.clone(),
    }))
}

/// Verrouille la ligne du projet et renvoie son etat actuel (nom, source, build).
pub async fn lock(
    conn: &mut PgConnection,
    id: Uuid,
) -> Result<Option<(String, Source, Option<Build>)>, sqlx::Error> {
    let locked: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    if locked.is_none() {
        return Ok(None);
    }
    let row: Row = sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE p.id = $1", select())))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(Some((row.name.clone(), row.source()?, row.build()?)))
}

/// Ecrit l'etat complet d'un projet existant (nom, branche, build) et
/// renomme le service principal en consequence.
pub async fn update(conn: &mut PgConnection, id: Uuid, draft: &Draft) -> Result<(), StoreError> {
    let (kind, path, service) = build_columns(draft.build.as_ref());
    sqlx::query(
        "UPDATE projects SET name = $2, build_kind = $3, build_path = $4, build_service = $5, updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&draft.name)
    .bind(kind)
    .bind(path)
    .bind(service)
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        if is_unique_violation(&e) {
            StoreError::NameTaken
        } else {
            e.into()
        }
    })?;
    if let Source::Git(git) = &draft.source {
        sqlx::query("UPDATE git_sources SET branch = $2 WHERE project_id = $1")
            .bind(id)
            .bind(&git.branch)
            .execute(&mut *conn)
            .await?;
    }
    // v0.1 : un seul service par environnement.
    sqlx::query(
        "UPDATE services SET name = $2 WHERE environment_id IN (SELECT id FROM environments WHERE project_id = $1)",
    )
    .bind(id)
    .bind(service_name(&draft.name, draft.build.as_ref()))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Result<(), StoreError> {
    let deleted = sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_is_refined_by_the_serving_production() {
        let ok = Production {
            stopped: false,
            connected: true,
            running: None,
        };
        let running = ProjectStatus::Running;
        assert_eq!(refine(running, Some(ok)), ProjectStatus::Running);
        assert_eq!(refine(running, None), ProjectStatus::Running);
        assert_eq!(
            refine(
                running,
                Some(Production {
                    running: Some(true),
                    ..ok
                })
            ),
            ProjectStatus::Running
        );
        assert_eq!(
            refine(
                running,
                Some(Production {
                    running: Some(false),
                    ..ok
                })
            ),
            ProjectStatus::Down
        );
        assert_eq!(
            refine(
                running,
                Some(Production {
                    connected: false,
                    running: Some(false),
                    ..ok
                })
            ),
            ProjectStatus::Unreachable
        );
        assert_eq!(
            refine(
                running,
                Some(Production {
                    stopped: true,
                    connected: false,
                    ..ok
                })
            ),
            ProjectStatus::Stopped
        );
        // Les autres statuts ne changent pas.
        assert_eq!(
            refine(
                ProjectStatus::Failed,
                Some(Production {
                    stopped: true,
                    ..ok
                })
            ),
            ProjectStatus::Failed
        );
    }

    #[test]
    fn status_follows_the_deployments() {
        // Jamais deploye.
        assert_eq!(project_status(false, false, None), ProjectStatus::None);
        // Un deploiement sert : en ligne, meme si le dernier a echoue ou
        // qu'un autre est en cours.
        assert_eq!(
            project_status(true, false, Some("success")),
            ProjectStatus::Running
        );
        assert_eq!(
            project_status(true, false, Some("failed")),
            ProjectStatus::Running
        );
        assert_eq!(
            project_status(true, true, Some("failed")),
            ProjectStatus::Running
        );
        // Rien de servi : en cours, puis echec.
        assert_eq!(project_status(false, true, None), ProjectStatus::Building);
        assert_eq!(
            project_status(false, true, Some("failed")),
            ProjectStatus::Building
        );
        assert_eq!(
            project_status(false, false, Some("failed")),
            ProjectStatus::Failed
        );
    }
}
