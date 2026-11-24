//! Projets, environnements et services : `/api/projects`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Source Git : un depot GitHub et une branche.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GitSource {
    /// URL `https://github.com/proprietaire/nom` ou `proprietaire/nom`
    /// (renvoye sous la forme `proprietaire/nom`).
    pub repository: String,
    /// Reference Git : lettres, chiffres, `.`, `_`, `-` et `/`.
    pub branch: String,
}

/// Source image : une reference d'image de conteneur.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageSource {
    /// Ex. `ghcr.io/org/app:1.2` ; sans espace ni metacaractere de shell.
    pub image: String,
}

/// D'ou vient le code du projet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Source {
    Git(GitSource),
    Image(ImageSource),
}

/// Methode de build d'une source Git.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Build {
    Railpack,
    Dockerfile {
        /// Chemin du Dockerfile dans le depot (`^[A-Za-z0-9._/-]{1,255}$`, sans `..`).
        path: String,
    },
    Compose {
        /// Chemin du fichier Compose dans le depot.
        path: String,
        /// Service Compose expose.
        service: String,
    },
}

/// Etat du projet, derive de ses deploiements (ce que les utilisateurs
/// obtiennent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    /// Jamais deploye.
    None,
    /// Rien de servi, un deploiement en file ou en cours.
    Building,
    /// Un deploiement sert, meme si le dernier a echoue.
    Running,
    /// Rien de servi, le dernier deploiement termine a echoue.
    Failed,
    /// La production est arretee depuis le panneau.
    Stopped,
    /// L'agent du serveur de la production est deconnecte.
    Unreachable,
    /// L'agent signale le conteneur de la production arrete, en echec ou
    /// disparu (hors du panneau).
    Down,
}

/// Corps de `POST /api/projects`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewProject {
    /// `^[a-z0-9][a-z0-9-]{0,62}$`.
    pub name: String,
    pub source: Source,
    /// Obligatoire pour une source Git, interdit pour une image.
    #[serde(default)]
    pub build: Option<Build>,
}

/// Corps de `PATCH /api/projects/{id}` : seuls les champs presents changent.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProject {
    pub name: Option<String>,
    /// Source Git seulement.
    pub branch: Option<String>,
    /// Source Git seulement.
    pub build: Option<Build>,
}

/// Ligne de `GET /api/projects`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    /// Environnement affiche (`production` en v0.1).
    pub environment: String,
    pub status: ProjectStatus,
    pub source: Source,
    /// Absent pour une source image.
    pub build: Option<Build>,
    /// Nombre de services.
    pub services: u32,
    /// RFC 3339.
    pub created_at: String,
    /// RFC 3339.
    pub updated_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Environment {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Service {
    pub id: String,
    pub name: String,
    /// Nom de l'environnement du service.
    pub environment: String,
}

/// Reponse de `GET|POST|PATCH /api/projects[/{id}]`.
#[derive(Debug, Serialize, ToSchema)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub status: ProjectStatus,
    pub source: Source,
    pub build: Option<Build>,
    pub environments: Vec<Environment>,
    pub services: Vec<Service>,
    pub created_at: String,
    pub updated_at: String,
}

/// Projets de l'instance (lecture : viewer et plus).
#[utoipa::path(
    get,
    path = "/api/projects",
    tag = "projects",
    responses(
        (status = 200, description = "Projets, par nom", body = Vec<ProjectSummary>),
        (status = 401, description = "Pas de session"),
        (status = 403, description = "Role insuffisant"),
    )
)]
#[allow(dead_code)]
pub fn list_projects() {}

/// Cree un projet et son environnement `production` (developer et plus).
#[utoipa::path(
    post,
    path = "/api/projects",
    tag = "projects",
    request_body = NewProject,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 201, description = "Projet cree", body = Project),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 409, description = "`name_taken` : nom deja utilise"),
        (status = 422, description = "Nom, depot, branche, chemin ou image invalide"),
    )
)]
#[allow(dead_code)]
pub fn create_project() {}

/// Detail d'un projet (viewer et plus).
#[utoipa::path(
    get,
    path = "/api/projects/{id}",
    tag = "projects",
    params(("id" = String, Path, description = "Identifiant du projet")),
    responses(
        (status = 200, description = "Projet", body = Project),
        (status = 404, description = "Projet introuvable"),
    )
)]
#[allow(dead_code)]
pub fn get_project() {}

/// Modifie le nom, la branche ou le build (developer et plus).
#[utoipa::path(
    patch,
    path = "/api/projects/{id}",
    tag = "projects",
    request_body = UpdateProject,
    params(
        ("id" = String, Path, description = "Identifiant du projet"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 200, description = "Projet modifie", body = Project),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Projet introuvable"),
        (status = 409, description = "`name_taken` : nom deja utilise"),
        (status = 422, description = "Valeur invalide, aucun champ, ou champ sans objet pour une image"),
    )
)]
#[allow(dead_code)]
pub fn update_project() {}

/// Supprime le projet de la base (owner et admin). Les conteneurs eventuels
/// sont nettoyes par le module de deploiement.
#[utoipa::path(
    delete,
    path = "/api/projects/{id}",
    tag = "projects",
    params(
        ("id" = String, Path, description = "Identifiant du projet"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 204, description = "Projet supprime"),
        (status = 403, description = "Role insuffisant, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Projet introuvable"),
    )
)]
#[allow(dead_code)]
pub fn delete_project() {}
