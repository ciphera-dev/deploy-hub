//! Etat local de l'agent (SQLite, `/var/lib/ciphera-agent/state.db`) :
//! deploiements en cours (port expose, deploiement servi par service),
//! ensembles Compose des builds et images deja tirees par service. Il survit aux
//! redemarrages de l'agent ; c'est lui que la reconciliation renvoie au plan
//! de controle a chaque reconnexion.
//!
//! Acces synchrones et courts (base locale) sous un verrou ; fichier en 0600.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use proto::{AgentState, DeploymentOutcome, ImageRef, RunningDeployment, Stage, StageStatus};
use rusqlite::{Connection, params};
use shared::ids::{DeploymentId, ProjectId, ServiceId};

use crate::docker::ContainerId;

const SCHEMA_VERSION: i64 = 5;

/// Tables de la version 1 (Task 19).
const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS deployments (
    deployment_id TEXT PRIMARY KEY,
    service_id    TEXT NOT NULL,
    image         TEXT NOT NULL,
    container_id  TEXT NOT NULL,
    healthy       INTEGER NOT NULL DEFAULT 0 CHECK (healthy IN (0, 1)),
    updated_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS images (
    service_id TEXT NOT NULL,
    image      TEXT NOT NULL,
    pulled_at  INTEGER NOT NULL,
    PRIMARY KEY (service_id, image)
);
";

/// Version 2 : port expose et deploiement servi par service (un seul,
/// index unique partiel), ensembles Compose par deploiement.
const MIGRATE_V2: &str = "
ALTER TABLE deployments ADD COLUMN port INTEGER CHECK (port BETWEEN 1 AND 65535);
ALTER TABLE deployments ADD COLUMN serving INTEGER NOT NULL DEFAULT 0 CHECK (serving IN (0, 1));
CREATE UNIQUE INDEX IF NOT EXISTS deployments_serving ON deployments (service_id) WHERE serving = 1;
CREATE TABLE IF NOT EXISTS compose_bundles (
    deployment_id TEXT PRIMARY KEY,
    bundle        TEXT NOT NULL,
    created_at    INTEGER NOT NULL
);
";

/// Version 3 : issues definitives des builds et deploiements (rejeu
/// idempotent, reconciliation), garde bornee.
const MIGRATE_V3: &str = "
CREATE TABLE IF NOT EXISTS outcomes (
    deployment_id TEXT PRIMARY KEY,
    outcome       TEXT NOT NULL,
    recorded_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS outcomes_recent ON outcomes (recorded_at);
";

/// Version 4 : projet du deploiement (les actions du cycle de vie ne
/// s'etendent qu'a ce projet). Vide pour un deploiement plus ancien.
const MIGRATE_V4: &str = "
ALTER TABLE deployments ADD COLUMN project_id TEXT;
";

/// Version 5 : derniere valeur lue de l'utilisateur effectif (`runs_as_root`),
/// gardee quand `inspect` echoue.
const MIGRATE_V5: &str = "
ALTER TABLE deployments ADD COLUMN runs_as_root INTEGER NOT NULL DEFAULT 0 CHECK (runs_as_root IN (0, 1));
";

#[derive(Debug)]
pub enum StateError {
    Sqlite(rusqlite::Error),
    /// Valeur illisible en base (identifiant invalide).
    Corrupt(&'static str),
    /// Base creee par une version plus recente de l'agent.
    NewerSchema(i64),
    Io(std::io::Error),
    /// Deploiement absent de l'etat local.
    UnknownDeployment(DeploymentId),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "etat local (SQLite) : {e}"),
            Self::Corrupt(what) => write!(f, "etat local : {what} illisible"),
            Self::NewerSchema(v) => write!(
                f,
                "etat local en version {v}, plus recente que celle de l'agent ({SCHEMA_VERSION})"
            ),
            Self::Io(e) => write!(f, "etat local : {e}"),
            Self::UnknownDeployment(id) => write!(f, "etat local : deploiement {id} inconnu"),
        }
    }
}

impl std::error::Error for StateError {}

impl From<rusqlite::Error> for StateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

/// Deploiement enregistre, son conteneur et son port expose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentRecord {
    pub deployment: RunningDeployment,
    pub container: ContainerId,
    pub port: Option<u16>,
    /// Projet du deploiement, tel que le plan de controle l'a donne au
    /// `Deploy` (jamais lu d'une etiquette de conteneur). `None` : etat
    /// ecrit avant que l'agent ne le garde.
    pub project: Option<ProjectId>,
}

#[derive(Clone)]
pub struct StateDb {
    conn: Arc<Mutex<Connection>>,
}

impl fmt::Debug for StateDb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateDb").finish_non_exhaustive()
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl StateDb {
    /// Ouvre (ou cree) la base et son schema.
    pub fn open(path: &Path) -> Result<Self, StateError> {
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(StateError::Io)?;
        }
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StateError::NewerSchema(version));
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SCHEMA_V1)?;
        if version < 2 {
            tx.execute_batch(MIGRATE_V2)?;
        }
        if version < 3 {
            tx.execute_batch(MIGRATE_V3)?;
        }
        if version < 4 {
            tx.execute_batch(MIGRATE_V4)?;
        }
        if version < 5 {
            tx.execute_batch(MIGRATE_V5)?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        tx.commit()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // Une requete qui panique ne laisse pas la connexion incoherente
        // (chaque ecriture est une instruction).
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Enregistre (ou remplace) un deploiement en cours, sans port.
    pub fn put_deployment(
        &self,
        d: &RunningDeployment,
        container: &ContainerId,
    ) -> Result<(), StateError> {
        self.put_record(&DeploymentRecord {
            deployment: d.clone(),
            container: container.clone(),
            port: None,
            project: None,
        })
    }

    /// Enregistre (ou remplace) un deploiement en cours.
    pub fn put_record(&self, r: &DeploymentRecord) -> Result<(), StateError> {
        let d = &r.deployment;
        self.conn().execute(
            "INSERT INTO deployments
               (deployment_id, service_id, image, container_id, healthy, port, serving, updated_at,
                project_id, runs_as_root)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (deployment_id) DO UPDATE SET
               service_id = excluded.service_id, image = excluded.image,
               container_id = excluded.container_id, healthy = excluded.healthy,
               port = excluded.port, serving = excluded.serving,
               updated_at = excluded.updated_at, project_id = excluded.project_id,
               runs_as_root = excluded.runs_as_root",
            params![
                d.deployment_id.to_string(),
                d.service_id.to_string(),
                d.image.0,
                r.container.0,
                d.healthy,
                r.port,
                d.serving,
                now_secs(),
                r.project.map(|p| p.to_string()),
                d.runs_as_root
            ],
        )?;
        Ok(())
    }

    pub fn set_healthy(&self, id: DeploymentId, healthy: bool) -> Result<(), StateError> {
        self.conn().execute(
            "UPDATE deployments SET healthy = ?2, updated_at = ?3 WHERE deployment_id = ?1",
            params![id.to_string(), healthy, now_secs()],
        )?;
        Ok(())
    }

    pub fn set_runs_as_root(&self, id: DeploymentId, root: bool) -> Result<(), StateError> {
        self.conn().execute(
            "UPDATE deployments SET runs_as_root = ?2 WHERE deployment_id = ?1",
            params![id.to_string(), root],
        )?;
        Ok(())
    }

    /// `true` si le deploiement etait enregistre.
    pub fn remove_deployment(&self, id: DeploymentId) -> Result<bool, StateError> {
        let n = self.conn().execute(
            "DELETE FROM deployments WHERE deployment_id = ?1",
            params![id.to_string()],
        )?;
        Ok(n > 0)
    }

    pub fn deployments(&self) -> Result<Vec<DeploymentRecord>, StateError> {
        self.select("", &[])
    }

    /// Un deploiement enregistre.
    pub fn record(&self, id: DeploymentId) -> Result<Option<DeploymentRecord>, StateError> {
        Ok(self
            .select("WHERE deployment_id = ?1", &[&id.to_string()])?
            .pop())
    }

    /// Deploiements servis (un au plus par service).
    pub fn serving(&self) -> Result<Vec<DeploymentRecord>, StateError> {
        self.select("WHERE serving = 1", &[])
    }

    fn select(
        &self,
        filter: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<DeploymentRecord>, StateError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT deployment_id, service_id, image, container_id, healthy, port, serving,
                    project_id, runs_as_root
             FROM deployments {filter} ORDER BY deployment_id"
        ))?;
        let rows = stmt.query_map(args, |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, bool>(4)?,
                r.get::<_, Option<u16>>(5)?,
                r.get::<_, bool>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, bool>(8)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (
                deployment_id,
                service_id,
                image,
                container,
                healthy,
                port,
                serving,
                project,
                runs_as_root,
            ) = row?;
            out.push(DeploymentRecord {
                deployment: RunningDeployment {
                    deployment_id: deployment_id
                        .parse()
                        .map_err(|_| StateError::Corrupt("deployment_id"))?,
                    service_id: service_id
                        .parse()
                        .map_err(|_| StateError::Corrupt("service_id"))?,
                    image: ImageRef(image),
                    healthy,
                    serving,
                    runs_as_root,
                },
                container: ContainerId(container),
                port,
                project: project
                    .map(|p| p.parse().map_err(|_| StateError::Corrupt("project_id")))
                    .transpose()?,
            });
        }
        Ok(out)
    }

    /// Fait de `id` le deploiement servi de son service ; renvoie celui qui
    /// l'etait avant (`None` s'il n'y en avait pas ou si c'etait deja `id`).
    pub fn set_serving(&self, id: DeploymentId) -> Result<Option<DeploymentId>, StateError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let key = id.to_string();
        let service: String = tx
            .query_row(
                "SELECT service_id FROM deployments WHERE deployment_id = ?1",
                params![key],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => StateError::UnknownDeployment(id),
                e => StateError::Sqlite(e),
            })?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT deployment_id FROM deployments WHERE service_id = ?1 AND serving = 1",
                params![service],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        tx.execute(
            "UPDATE deployments SET serving = 0, updated_at = ?2
             WHERE service_id = ?1 AND serving = 1",
            params![service, now_secs()],
        )?;
        tx.execute(
            "UPDATE deployments SET serving = 1, updated_at = ?2 WHERE deployment_id = ?1",
            params![key, now_secs()],
        )?;
        tx.commit()?;
        previous
            .filter(|p| *p != key)
            .map(|p| p.parse().map_err(|_| StateError::Corrupt("deployment_id")))
            .transpose()
    }

    /// `id` n'est plus servi (retire ou arrete).
    pub fn clear_serving(&self, id: DeploymentId) -> Result<(), StateError> {
        self.conn().execute(
            "UPDATE deployments SET serving = 0, updated_at = ?2 WHERE deployment_id = ?1",
            params![id.to_string(), now_secs()],
        )?;
        Ok(())
    }

    /// Ensemble Compose (JSON) produit par le build d'un deploiement.
    pub fn put_bundle(&self, id: DeploymentId, bundle: &str) -> Result<(), StateError> {
        self.conn().execute(
            "INSERT OR REPLACE INTO compose_bundles (deployment_id, bundle, created_at)
             VALUES (?1, ?2, ?3)",
            params![id.to_string(), bundle, now_secs()],
        )?;
        Ok(())
    }

    pub fn bundle(&self, id: DeploymentId) -> Result<Option<String>, StateError> {
        self.conn()
            .query_row(
                "SELECT bundle FROM compose_bundles WHERE deployment_id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(StateError::Sqlite(e)),
            })
    }

    /// `true` si un ensemble etait enregistre.
    pub fn remove_bundle(&self, id: DeploymentId) -> Result<bool, StateError> {
        let n = self.conn().execute(
            "DELETE FROM compose_bundles WHERE deployment_id = ?1",
            params![id.to_string()],
        )?;
        Ok(n > 0)
    }

    /// Deploiements en cours, tels qu'envoyes dans `State`.
    pub fn running(&self) -> Result<Vec<RunningDeployment>, StateError> {
        Ok(self
            .deployments()?
            .into_iter()
            .map(|r| r.deployment)
            .collect())
    }

    /// Note qu'une image a ete tiree pour un service (retour arriere).
    pub fn record_image(&self, service: ServiceId, image: &ImageRef) -> Result<(), StateError> {
        // REPLACE donne un nouveau rowid : departage deux images de la meme seconde.
        self.conn().execute(
            "INSERT OR REPLACE INTO images (service_id, image, pulled_at) VALUES (?1, ?2, ?3)",
            params![service.to_string(), image.0, now_secs()],
        )?;
        Ok(())
    }

    /// Images tirees pour un service, la plus recente d'abord.
    pub fn images(&self, service: ServiceId) -> Result<Vec<ImageRef>, StateError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT image FROM images WHERE service_id = ?1 ORDER BY pulled_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map(params![service.to_string()], |r| r.get::<_, String>(0))?;
        rows.map(|r| r.map(ImageRef).map_err(StateError::from))
            .collect()
    }
    /// Enregistre l'issue definitive de `id`, le message d'echec coupe a
    /// `AgentState::MAX_OUTCOME_MESSAGE` octets ; puis applique la garde
    /// (30 jours, `AgentState::MAX_OUTCOMES`). Remplace une reussite
    /// precedente (build conclu, puis deploiement), jamais un echec ni une
    /// annulation (la premiere issue definitive fait foi).
    pub fn put_outcome(
        &self,
        id: DeploymentId,
        stage: Stage,
        status: &StageStatus,
    ) -> Result<(), StateError> {
        self.put_outcome_at(id, stage, status, now_secs())
    }

    /// `put_outcome` a l'instant `at` (secondes Unix ; tests de la garde).
    pub fn put_outcome_at(
        &self,
        id: DeploymentId,
        stage: Stage,
        status: &StageStatus,
        at: i64,
    ) -> Result<(), StateError> {
        let status = match status {
            StageStatus::Failed { message } => StageStatus::Failed {
                message: truncate_utf8(message, AgentState::MAX_OUTCOME_MESSAGE).to_string(),
            },
            other => other.clone(),
        };
        let text =
            serde_json::to_string(&(stage, status)).map_err(|_| StateError::Corrupt("issue"))?;
        let retention = i64::try_from(AgentState::OUTCOME_RETENTION_SECS).unwrap_or(i64::MAX);
        let keep = i64::try_from(AgentState::MAX_OUTCOMES).unwrap_or(i64::MAX);
        let conn = self.conn();
        let tx = conn.unchecked_transaction()?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT outcome FROM outcomes WHERE deployment_id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                e => Err(e),
            })?;
        if let Some(text) = previous
            && let Ok(old) = parse_outcome(id, &text)
            && matches!(
                old.status,
                StageStatus::Failed { .. } | StageStatus::Cancelled
            )
        {
            return Ok(());
        }
        tx.execute(
            "INSERT OR REPLACE INTO outcomes (deployment_id, outcome, recorded_at)
             VALUES (?1, ?2, ?3)",
            params![id.to_string(), text, at],
        )?;
        tx.execute(
            "DELETE FROM outcomes WHERE recorded_at < ?1",
            params![now_secs().max(at).saturating_sub(retention)],
        )?;
        tx.execute(
            "DELETE FROM outcomes WHERE rowid NOT IN (
                 SELECT rowid FROM outcomes ORDER BY recorded_at DESC, rowid DESC LIMIT ?1)",
            params![keep],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Issue definitive enregistree pour `id`.
    pub fn outcome(&self, id: DeploymentId) -> Result<Option<DeploymentOutcome>, StateError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT outcome FROM outcomes WHERE deployment_id = ?1")?;
        let mut rows = stmt.query(params![id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(parse_outcome(id, &row.get::<_, String>(0)?)?)),
            None => Ok(None),
        }
    }

    /// Issues gardees, la plus recente d'abord.
    pub fn outcomes(&self) -> Result<Vec<DeploymentOutcome>, StateError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT deployment_id, outcome FROM outcomes ORDER BY recorded_at DESC, rowid DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, text) = row?;
            let id = id
                .parse::<DeploymentId>()
                .map_err(|_| StateError::Corrupt("identifiant de deploiement"))?;
            out.push(parse_outcome(id, &text)?);
        }
        Ok(out)
    }

    /// Oublie l'issue de `id` (deploiement retire).
    pub fn remove_outcome(&self, id: DeploymentId) -> Result<(), StateError> {
        self.conn().execute(
            "DELETE FROM outcomes WHERE deployment_id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}

fn parse_outcome(id: DeploymentId, text: &str) -> Result<DeploymentOutcome, StateError> {
    let (stage, status): (Stage, StageStatus) =
        serde_json::from_str(text).map_err(|_| StateError::Corrupt("issue"))?;
    Ok(DeploymentOutcome {
        deployment_id: id,
        stage,
        status,
    })
}

/// `s` coupe a `max` octets au plus, sur une limite de caractere.
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
