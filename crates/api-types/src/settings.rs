//! Parametres de l'instance (`/api/settings/{section}`) et organisation
//! (`/api/organization`). Administrateur requis ; proprietaire seulement
//! pour la zone de danger (export, suppression).

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Nombre de deploiements reussis conserves par environnement (rollback).
pub const DEPLOY_RETENTION: u32 = 5;

/// Phrase a saisir pour supprimer l'organisation.
pub const DELETE_PHRASE: &str = "SUPPRIMER";

/// Parametres › General.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct GeneralSettings {
    /// Identifiant de l'instance.
    pub id: String,
    pub name: String,
    pub slug: Option<String>,
    /// Fuseau horaire IANA.
    pub timezone: String,
    /// `fr` ou `en`.
    pub language: String,
    /// Comptes de l'organisation.
    pub members: i64,
    /// 2FA obligatoire (rappel de l'en-tete des parametres).
    pub mfa_required: bool,
}

/// Corps de `PATCH /api/settings/general` (champs absents : inchanges).
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGeneralSettings {
    /// 1 a 100 caracteres.
    pub name: Option<String>,
    /// `[a-z0-9-]`, 1 a 63 caracteres.
    pub slug: Option<String>,
    pub timezone: Option<String>,
    pub language: Option<String>,
}

/// Parametres › Deploiements : valeurs par defaut des projets.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct DeploySettings {
    /// Deploiement automatique a chaque push, pour les nouveaux projets.
    pub auto: bool,
    /// Deploiements reussis conserves par environnement (fixe en v0.1).
    pub retention: u32,
}

/// Corps de `PATCH /api/settings/deployments`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDeploySettings {
    pub auto: Option<bool>,
}

/// Parametres › Securite de l'organisation.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct OrgSecurity {
    /// 2FA obligatoire : appliquee a chaque compte, sessions deja ouvertes
    /// comprises (sans second facteur : enrolement avant toute autre page).
    pub mfa_required: bool,
    /// Adresses ou blocs CIDR autorises sur l'API et le panneau ; vide :
    /// aucune restriction.
    pub ip_allowlist: Vec<String>,
    /// Adresse de la requete, telle que le serveur la voit.
    pub current_ip: String,
    /// La session qui fait la requete doit d'abord enrôler un second facteur
    /// (2FA imposee alors que le compte n'en a aucun) : l'interface redirige
    /// vers l'enrolement.
    pub self_enrollment_required: bool,
}

/// Corps de `PATCH /api/settings/security`.
#[derive(Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateOrgSecurity {
    /// Imposer la 2FA : les sessions ouvertes des comptes sans second facteur
    /// passent en enrolement obligatoire (administrateur ou proprietaire). La
    /// desactiver est reserve au proprietaire, avec `current_password`.
    pub mfa_required: Option<bool>,
    /// Mot de passe du proprietaire (re-authentification), exige pour
    /// desactiver la 2FA imposee ou modifier la liste d'IP.
    #[schema(value_type = Option<String>, format = Password, max_length = 1024)]
    pub current_password: Option<Zeroizing<String>>,
    /// 100 entrees au plus (`203.0.113.7`, `198.51.100.0/24`, `2001:db8::/32`).
    /// Une liste qui exclut l'adresse de la requete est refusee (422
    /// `ip_allowlist_lockout`).
    pub ip_allowlist: Option<Vec<String>>,
}

/// Parametres › Zone de danger (proprietaire seulement).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct DangerZone {
    pub name: String,
    pub projects: i64,
    pub members: i64,
    /// Nom de la GitHub App connectee.
    pub github_app: Option<String>,
}

/// Corps de `DELETE /api/organization`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteOrganization {
    /// Mot de passe du compte (re-authentification).
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
    /// Code du second facteur (application d'authentification ou code de
    /// secours) : exige si le compte en a un.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = Password, max_length = 64)]
    pub code: Option<Zeroizing<String>>,
    /// Doit valoir exactement `SUPPRIMER`.
    pub confirm: String,
}

impl fmt::Debug for DeleteOrganization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeleteOrganization")
            .field("password", &"***")
            .field("code", &self.code.as_ref().map(|_| "***"))
            .field("confirm", &self.confirm)
            .finish()
    }
}

/// Projet non supprime (agent injoignable) : conserve pour un nouvel essai.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RemainingProject {
    pub id: String,
    pub name: String,
}

/// GitHub App retiree de l'instance : a supprimer a la main sur GitHub.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct GithubAppNotice {
    pub name: String,
    /// Page de l'App sur GitHub.
    pub html_url: String,
}

/// Reponse de `DELETE /api/organization`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct OrganizationDeleted {
    /// Vrai si tous les projets ont ete supprimes.
    pub complete: bool,
    pub projects_deleted: i64,
    /// Projets conserves (agent injoignable) : relancer la suppression quand
    /// leur serveur est en ligne.
    pub remaining_projects: Vec<RemainingProject>,
    pub github_app: Option<GithubAppNotice>,
}

/// Parametres generaux de l'instance.
#[utoipa::path(
    get,
    path = "/api/settings/general",
    tag = "settings",
    responses(
        (status = 200, description = "Parametres generaux", body = GeneralSettings),
        (status = 403, description = "Administrateur requis"),
    )
)]
#[allow(dead_code)]
pub fn get_general_settings() {}

/// Modifie les parametres generaux.
#[utoipa::path(
    patch,
    path = "/api/settings/general",
    tag = "settings",
    request_body = UpdateGeneralSettings,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Parametres enregistres", body = GeneralSettings),
        (status = 403, description = "Administrateur requis, ou jeton CSRF absent ou invalide"),
        (status = 422, description = "`validation_failed`"),
    )
)]
#[allow(dead_code)]
pub fn patch_general_settings() {}

/// Valeurs par defaut des deploiements.
#[utoipa::path(
    get,
    path = "/api/settings/deployments",
    tag = "settings",
    responses(
        (status = 200, description = "Deploiements", body = DeploySettings),
        (status = 403, description = "Administrateur requis"),
    )
)]
#[allow(dead_code)]
pub fn get_deploy_settings() {}

/// Modifie les valeurs par defaut des deploiements.
#[utoipa::path(
    patch,
    path = "/api/settings/deployments",
    tag = "settings",
    request_body = UpdateDeploySettings,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Parametres enregistres", body = DeploySettings),
        (status = 403, description = "Administrateur requis, ou jeton CSRF absent ou invalide"),
        (status = 422, description = "`validation_failed`"),
    )
)]
#[allow(dead_code)]
pub fn patch_deploy_settings() {}

/// Securite de l'organisation.
#[utoipa::path(
    get,
    path = "/api/settings/security",
    tag = "settings",
    responses(
        (status = 200, description = "Securite de l'organisation", body = OrgSecurity),
        (status = 403, description = "Administrateur requis"),
    )
)]
#[allow(dead_code)]
pub fn get_org_security() {}

/// Modifie la securite de l'organisation (2FA obligatoire, liste d'IP).
#[utoipa::path(
    patch,
    path = "/api/settings/security",
    tag = "settings",
    request_body = UpdateOrgSecurity,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Parametres enregistres", body = OrgSecurity),
        (status = 401, description = "`invalid_credentials` : mot de passe absent ou incorrect (modification reservee au proprietaire)"),
        (status = 403, description = "Administrateur requis (proprietaire pour desactiver la 2FA imposee ou modifier la liste d'IP), ou jeton CSRF absent ou invalide"),
        (status = 429, description = "Trop de tentatives de mot de passe"),
        (status = 422, description = "`validation_failed` ou `ip_allowlist_lockout` (la liste exclut l'adresse de la requete)"),
    )
)]
#[allow(dead_code)]
pub fn patch_org_security() {}

/// Zone de danger : resume de ce que la suppression efface.
#[utoipa::path(
    get,
    path = "/api/settings/danger",
    tag = "settings",
    responses(
        (status = 200, description = "Zone de danger", body = DangerZone),
        (status = 403, description = "Proprietaire seulement"),
    )
)]
#[allow(dead_code)]
pub fn get_danger_zone() {}

/// Export JSON de l'organisation, sans aucun secret (ni valeur de variable,
/// ni jeton, ni cle, ni hachage).
#[utoipa::path(
    post,
    path = "/api/organization/export",
    tag = "settings",
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Export JSON", body = Object),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
    )
)]
#[allow(dead_code)]
pub fn export_organization() {}

/// Supprime l'organisation : projets (conteneurs, routes, certificats),
/// variables globales, membres (sauf le proprietaire qui la demande) et
/// GitHub App locale ; toutes les sessions sont revoquees. L'instance, ses
/// serveurs, son SMTP et le journal d'audit restent.
#[utoipa::path(
    delete,
    path = "/api/organization",
    tag = "settings",
    request_body = DeleteOrganization,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Organisation supprimee (voir `complete`)", body = OrganizationDeleted),
        (status = 401, description = "`invalid_credentials` (mot de passe), `second_factor_required` (code du second facteur absent) ou `invalid_code`"),
        (status = 409, description = "`other_owners` (autres proprietaires a retirer d'abord, Parametres > Equipe) ou `second_factor_unavailable`"),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
        (status = 422, description = "`confirmation_required` (saisir SUPPRIMER) ou requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn delete_organization() {}
