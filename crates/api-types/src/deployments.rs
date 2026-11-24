//! Deploiements : `/api/projects/{id}/deployments` et `/api/deployments`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Etat d'un deploiement. `queued` : en file (un autre deploiement occupe
/// l'environnement, ou la tache n'est pas encore prise) ; `running` : un
/// seul par environnement ; les trois autres sont terminaux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
}

impl DeploymentStatus {
    pub const ALL: [DeploymentStatus; 5] = [
        DeploymentStatus::Queued,
        DeploymentStatus::Running,
        DeploymentStatus::Success,
        DeploymentStatus::Failed,
        DeploymentStatus::Cancelled,
    ];

    /// Valeur stockee et valeur du filtre `?status=`.
    pub fn as_str(self) -> &'static str {
        match self {
            DeploymentStatus::Queued => "queued",
            DeploymentStatus::Running => "running",
            DeploymentStatus::Success => "success",
            DeploymentStatus::Failed => "failed",
            DeploymentStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            DeploymentStatus::Success | DeploymentStatus::Failed | DeploymentStatus::Cancelled
        )
    }
}

/// Origine du deploiement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentTrigger {
    /// Push Git (webhook signe).
    Push,
    /// Bouton « Déployer » du panneau.
    Manual,
    /// Retour a une image conservee (aucun build).
    Rollback,
    /// Deploy hook (URL secrete appelee par une CI externe).
    Hook,
}

impl DeploymentTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            DeploymentTrigger::Push => "push",
            DeploymentTrigger::Manual => "manual",
            DeploymentTrigger::Rollback => "rollback",
            DeploymentTrigger::Hook => "hook",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [Self::Push, Self::Manual, Self::Rollback, Self::Hook]
            .into_iter()
            .find(|t| t.as_str() == value)
    }
}

/// Les 6 etapes du pipeline, dans l'ordre (Clonage, Build, Image,
/// Démarrage, Healthcheck, Bascule). Une source image commence a `image` ;
/// un rollback a `start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStage {
    Clone,
    Build,
    Image,
    Start,
    Healthcheck,
    Switch,
}

impl DeploymentStage {
    pub const ALL: [DeploymentStage; 6] = [
        DeploymentStage::Clone,
        DeploymentStage::Build,
        DeploymentStage::Image,
        DeploymentStage::Start,
        DeploymentStage::Healthcheck,
        DeploymentStage::Switch,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DeploymentStage::Clone => "clone",
            DeploymentStage::Build => "build",
            DeploymentStage::Image => "image",
            DeploymentStage::Start => "start",
            DeploymentStage::Healthcheck => "healthcheck",
            DeploymentStage::Switch => "switch",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// Etat d'une etape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            StepStatus::Running => "running",
            StepStatus::Succeeded => "succeeded",
            StepStatus::Failed => "failed",
            StepStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Running,
            Self::Succeeded,
            Self::Failed,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|s| s.as_str() == value)
    }
}

/// Etape signalee par l'agent (les etapes absentes n'ont pas commence).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct DeploymentStep {
    pub stage: DeploymentStage,
    pub status: StepStatus,
    /// RFC 3339.
    pub started_at: String,
    /// RFC 3339 ; absent tant que l'etape tourne.
    pub finished_at: Option<String>,
    /// Duree de l'etape terminee, en millisecondes.
    pub duration_ms: Option<i64>,
    /// Cause d'un echec (tronquee, sans secret).
    pub message: Option<String>,
}

/// Deploiement d'un environnement.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct Deployment {
    pub id: String,
    /// Numero lisible (`#42`).
    pub number: i64,
    pub project_id: String,
    pub project_name: String,
    pub environment_id: String,
    /// Nom de l'environnement (`production` en v0.1).
    pub environment: String,
    pub status: DeploymentStatus,
    pub trigger: DeploymentTrigger,
    /// Commit construit (40 ou 64 hex) ; absent tant qu'un deploiement
    /// manuel n'a pas resolu sa branche, et pour une source image.
    pub commit_sha: Option<String>,
    pub branch: Option<String>,
    /// Image principale deployee.
    pub image: Option<String>,
    /// Rollback : deploiement dont l'image est reutilisee.
    pub rollback_of: Option<String>,
    /// Derniere etape signalee.
    pub stage: Option<DeploymentStage>,
    /// Etapes commencees, dans l'ordre du pipeline.
    pub steps: Vec<DeploymentStep>,
    /// Cause de l'echec (tronquee, sans secret).
    pub error: Option<String>,
    /// Annulation demandee, en attente de la confirmation de l'agent.
    pub cancel_requested: bool,
    /// Deploiement servi actuellement par son environnement.
    pub current: bool,
    /// Image encore conservee : `POST /api/deployments/{id}/rollback` est
    /// accepte.
    pub can_rollback: bool,
    /// Nom de l'auteur (deploiement manuel ou rollback).
    pub created_by: Option<String>,
    /// RFC 3339.
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Du demarrage a la fin (millisecondes), une fois termine.
    pub duration_ms: Option<i64>,
}

/// Corps de `POST /api/projects/{id}/deployments`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewDeployment {
    /// Nom de l'environnement (`production` par defaut).
    #[serde(default)]
    pub environment: Option<String>,
}

/// Deploie la branche du projet (ou son image) sur un environnement
/// (developer et plus). Le deploiement est mis en file : un seul tourne a
/// la fois par environnement.
#[utoipa::path(
    post,
    path = "/api/projects/{id}/deployments",
    tag = "deployments",
    request_body = NewDeployment,
    params(
        ("id" = String, Path, description = "Identifiant du projet"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 201, description = "Deploiement mis en file (`queued`)", body = Deployment),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Projet ou environnement introuvable"),
        (status = 422, description = "Corps invalide"),
    )
)]
#[allow(dead_code)]
pub fn create_deployment() {}

/// Deploiements, du plus recent au plus ancien (viewer et plus).
#[utoipa::path(
    get,
    path = "/api/deployments",
    tag = "deployments",
    params(
        ("project" = Option<String>, Query, description = "Identifiant du projet"),
        ("status" = Option<DeploymentStatus>, Query, description = "Filtre sur l'etat"),
        ("limit" = Option<u32>, Query, description = "Nombre maximal (50 par defaut, 200 au plus)"),
    ),
    responses(
        (status = 200, description = "Deploiements", body = Vec<Deployment>),
        (status = 403, description = "Role insuffisant"),
        (status = 422, description = "Filtre invalide"),
    )
)]
#[allow(dead_code)]
pub fn list_deployments() {}

/// Detail d'un deploiement (viewer et plus).
#[utoipa::path(
    get,
    path = "/api/deployments/{id}",
    tag = "deployments",
    params(("id" = String, Path, description = "Identifiant du deploiement")),
    responses(
        (status = 200, description = "Deploiement", body = Deployment),
        (status = 404, description = "Deploiement introuvable"),
    )
)]
#[allow(dead_code)]
pub fn get_deployment() {}

/// Annule un deploiement (developer et plus) : en file, il passe
/// `cancelled` tout de suite ; en cours, l'agent l'interrompt et nettoie
/// (`cancel_requested` jusqu'a sa confirmation). Apres la bascule, il est
/// trop tard : le deploiement aboutit.
#[utoipa::path(
    post,
    path = "/api/deployments/{id}/cancel",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Identifiant du deploiement"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 200, description = "Deploiement annule ou annulation demandee", body = Deployment),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Deploiement introuvable"),
        (status = 409, description = "`already_finished` : deploiement termine"),
    )
)]
#[allow(dead_code)]
pub fn cancel_deployment() {}

/// Revient a l'image conservee d'un deploiement reussi (developer et plus) :
/// nouveau deploiement `rollback`, sans build ; pour Compose, l'ensemble des
/// images de ce deploiement.
#[utoipa::path(
    post,
    path = "/api/deployments/{id}/rollback",
    tag = "deployments",
    params(
        ("id" = String, Path, description = "Identifiant du deploiement a retrouver"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 201, description = "Rollback mis en file (`queued`)", body = Deployment),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Deploiement introuvable"),
        (status = 409, description = "`not_rollbackable` : deploiement non reussi ou image supprimee"),
    )
)]
#[allow(dead_code)]
pub fn rollback_deployment() {}
