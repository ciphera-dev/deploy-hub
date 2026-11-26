//! Version de l'instance et mises a jour de Ciphera (`/api/system/*`).

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Reponse de `GET /api/system/version`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct SystemVersion {
    /// Version installee (`vX.Y.Z`).
    pub current: String,
    /// Derniere version publiee, si la verification est activee
    /// (`CIPHERA_UPDATE_CHECK=1`) et a abouti.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// Notes de version de `latest` (https), si publiees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changelog_url: Option<String>,
}

/// Corps de `POST /api/system/update`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestUpdate {
    /// Version a installer (`vX.Y.Z`), superieure a la version installee.
    pub version: String,
    /// Mot de passe du compte (re-authentification).
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
}

impl fmt::Debug for RequestUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestUpdate")
            .field("version", &self.version)
            .field("password", &"***")
            .finish()
    }
}

/// Reponse 202 de `POST /api/system/update`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct UpdateRequested {
    /// Version demandee.
    pub version: String,
    /// Avertissement a afficher : en cas d'echec, retour automatique a la
    /// version precedente et a la base sauvegardee juste avant la mise a
    /// jour ; les modifications faites pendant la mise a jour sont perdues.
    pub notice: String,
}

/// Version installee et derniere version publiee (administrateur).
#[utoipa::path(
    get,
    path = "/api/system/version",
    tag = "system",
    responses(
        (status = 200, description = "Versions", body = SystemVersion),
        (status = 401, description = "Session absente"),
        (status = 403, description = "Administrateur requis"),
    )
)]
#[allow(dead_code)]
pub fn get_system_version() {}

/// Demande une mise a jour de Ciphera : proprietaire, re-authentification
/// par mot de passe. La demande signee est deposee pour l'updater (root),
/// qui verifie la release (minisign + SHA-256), sauvegarde la base, installe
/// et revient en arriere si le panneau ne repond plus.
#[utoipa::path(
    post,
    path = "/api/system/update",
    tag = "system",
    request_body = RequestUpdate,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 202, description = "Demande deposee", body = UpdateRequested),
        (status = 401, description = "`invalid_credentials` (mot de passe)"),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
        (status = 422, description = "`not_newer` (version inferieure ou egale a l'installee) ou requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
        (status = 503, description = "`updates_unavailable` : dossier de donnees different de celui surveille par l'updater (/var/lib/ciphera), ou cle des demandes de mise a jour (update-hmac.key) absente"),
    )
)]
#[allow(dead_code)]
pub fn request_update() {}
