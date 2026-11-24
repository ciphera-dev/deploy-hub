//! Apercu et services d'un projet : `GET /api/projects/{id}/overview`,
//! `GET /api/projects/{id}/services` et
//! `POST /api/projects/{id}/services/{sid}/{restart,stop}`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::deployments::Deployment;
use crate::projects::ProjectStatus;

/// Etat d'un service, derive de son deploiement servi (le dernier reussi de
/// son environnement) et de l'agent du serveur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    /// Jamais deploye.
    None,
    /// Rien de servi, un deploiement en file ou en cours.
    Deploying,
    /// Servi, agent connecte.
    Running,
    /// Servi, conteneurs arretes depuis le panneau.
    Stopped,
    /// Rien de servi, le dernier deploiement termine a echoue.
    Failed,
    /// Servi, mais l'agent du serveur est deconnecte.
    Unreachable,
    /// Servi, mais l'agent signale son conteneur arrete, en echec ou disparu
    /// (hors du panneau).
    Down,
}

/// Replicas en marche et voulus (un seul en v0.1 ; mise a l'echelle en v0.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct Replicas {
    pub running: u32,
    pub desired: u32,
}

/// Service d'un projet, tel que l'apercu et l'onglet Services l'affichent.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectService {
    pub id: String,
    pub name: String,
    /// Nom de l'environnement du service.
    pub environment: String,
    pub state: ServiceState,
    /// Deploiement servi (absent : rien en ligne).
    pub deployment_id: Option<String>,
    pub deployment_number: Option<i64>,
    /// Image du deploiement servi.
    pub image: Option<String>,
    /// Port ecoute sur le reseau du projet, s'il est expose.
    pub internal_port: Option<u16>,
    pub replicas: Replicas,
    /// CPU en pourcentage d'un coeur (mesure de moins de 2 minutes) :
    /// `null` sans mesure courante.
    pub cpu_percent: Option<f64>,
    /// Memoire en octets (mesure de moins de 2 minutes) : `null` sans mesure
    /// courante.
    pub memory_bytes: Option<u64>,
    /// Utilisateur du conteneur demande (`uid[:gid]`) ; `null` : celui de
    /// l'image.
    pub run_as: Option<String>,
    /// Racine du conteneur en lecture seule (avec un `/tmp` de 64 Mio).
    pub read_only_rootfs: bool,
    /// L'agent signale le conteneur servi en root (utilisateur effectif).
    pub runs_as_root: bool,
}

/// Corps de `PATCH /api/projects/{id}/services/{sid}` : reglages de securite
/// du service, qui valent pour les prochains deploiements.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateService {
    /// `uid` ou `uid:gid` numeriques (`^[0-9]{1,10}(:[0-9]{1,10})?$`) ;
    /// `null` : l'utilisateur de l'image.
    pub run_as: Option<String>,
    pub read_only_rootfs: bool,
}

/// Indicateur et sa serie (plus ancienne valeur d'abord). Sans metriques :
/// `value` nulle et serie vide (requetes, latence et erreurs n'ont pas
/// encore de source en v0.1).
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct Sparkline {
    pub value: Option<f64>,
    pub series: Vec<f64>,
}

/// Indicateurs de l'apercu.
#[derive(Debug, Default, Serialize, ToSchema)]
pub struct OverviewMetrics {
    /// Requetes sur 24 h (series horaires).
    pub requests: Sparkline,
    /// Latence p95 en millisecondes.
    pub latency_p95_ms: Sparkline,
    /// Taux d'erreur en pourcentage.
    pub error_rate: Sparkline,
    /// Disponibilite sur 30 jours en pourcentage (series journalieres des
    /// jours mesures), d'apres l'etat du conteneur servi remonte par l'agent.
    pub uptime: Sparkline,
}

/// Reponse de `GET /api/projects/{id}/overview`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectOverview {
    pub status: ProjectStatus,
    /// Deploiement servi de la production (absent : rien en ligne).
    pub production: Option<Deployment>,
    /// Domaine principal de la production (absent : aucun domaine).
    pub domain: Option<String>,
    pub metrics: OverviewMetrics,
    pub services: Vec<ProjectService>,
    /// Activite recente : les 4 derniers deploiements, le plus recent d'abord.
    pub activity: Vec<Deployment>,
}

/// Reponse de `POST /api/projects/{id}/services/{sid}/{restart,stop}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceActionAccepted {
    /// Deploiement servi vise par l'ordre envoye a l'agent.
    pub deployment_id: String,
}

/// Apercu d'un projet (viewer et plus).
#[utoipa::path(
    get,
    path = "/api/projects/{id}/overview",
    tag = "projects",
    params(("id" = String, Path, description = "Identifiant du projet")),
    responses(
        (status = 200, description = "Apercu", body = ProjectOverview),
        (status = 403, description = "Role insuffisant"),
        (status = 404, description = "Projet introuvable"),
    )
)]
#[allow(dead_code)]
pub fn get_project_overview() {}

/// Services d'un projet et leur etat (viewer et plus).
#[utoipa::path(
    get,
    path = "/api/projects/{id}/services",
    tag = "projects",
    params(("id" = String, Path, description = "Identifiant du projet")),
    responses(
        (status = 200, description = "Services", body = Vec<ProjectService>),
        (status = 403, description = "Role insuffisant"),
        (status = 404, description = "Projet introuvable"),
    )
)]
#[allow(dead_code)]
pub fn list_project_services() {}

/// Reglages de securite du service (developer, admin, owner : `Projects`
/// en ecriture) : utilisateur du
/// conteneur et racine en lecture seule. Ils valent pour les prochains
/// deploiements ; les deux champs sont remplaces ensemble.
#[utoipa::path(
    patch,
    path = "/api/projects/{id}/services/{sid}",
    tag = "projects",
    request_body = UpdateService,
    params(
        ("id" = String, Path, description = "Identifiant du projet"),
        ("sid" = String, Path, description = "Identifiant du service"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 200, description = "Reglages enregistres", body = ProjectService),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Projet ou service introuvable"),
        (status = 422, description = "`run_as` hors du format `uid[:gid]` numerique"),
    )
)]
#[allow(dead_code)]
pub fn update_service() {}

/// Redemarre les conteneurs du deploiement servi du service (developer et
/// plus) ; relance un service arrete.
#[utoipa::path(
    post,
    path = "/api/projects/{id}/services/{sid}/restart",
    tag = "projects",
    params(
        ("id" = String, Path, description = "Identifiant du projet"),
        ("sid" = String, Path, description = "Identifiant du service"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 202, description = "Redemarrage demande a l'agent", body = ServiceActionAccepted),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Projet ou service introuvable"),
        (status = 409, description = "`nothing_serving` : aucun deploiement servi"),
        (status = 503, description = "`agent_unavailable` : agent du serveur deconnecte"),
    )
)]
#[allow(dead_code)]
pub fn restart_service() {}

/// Arrete les conteneurs du deploiement servi du service (developer et
/// plus). Le service reste arrete jusqu'a un redemarrage ou un nouveau
/// deploiement.
#[utoipa::path(
    post,
    path = "/api/projects/{id}/services/{sid}/stop",
    tag = "projects",
    params(
        ("id" = String, Path, description = "Identifiant du projet"),
        ("sid" = String, Path, description = "Identifiant du service"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 202, description = "Arret demande a l'agent", body = ServiceActionAccepted),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Projet ou service introuvable"),
        (status = 409, description = "`nothing_serving` : aucun deploiement servi"),
        (status = 503, description = "`agent_unavailable` : agent du serveur deconnecte"),
    )
)]
#[allow(dead_code)]
pub fn stop_service() {}
