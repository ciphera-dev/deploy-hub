//! Domaine du panneau (`/api/settings/panel-domain`, proprietaire seulement).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Etat du domaine du panneau.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelDomainStatus {
    /// Aucun domaine : panneau en 8443 (certificat auto-signe).
    None,
    /// DNS verifie, certificat en cours d'emission.
    Issuing,
    /// Certificat actif : a confirmer depuis le nouveau domaine.
    AwaitingConfirmation,
    /// Confirme : l'acces 8443 est ferme.
    Active,
    /// DNS ou certificat en erreur (voir `error`).
    Error,
}

/// Reponse des routes du domaine du panneau.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct PanelDomain {
    pub domain: Option<String>,
    pub status: PanelDomainStatus,
    pub error: Option<String>,
}

/// Corps de `PUT /api/settings/panel-domain`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPanelDomain {
    /// Nom complet qui pointe vers l'IP du noeud.
    pub domain: String,
}

/// Corps (facultatif) de `POST /api/settings/panel-domain/confirm`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmPanelDomain {
    /// GitHub App connectee : l'URL de configuration (Setup URL) de l'App a
    /// ete remplacee a la main par `https://<domaine>/connectors` (l'API
    /// GitHub ne permet pas de la changer). Sans cet accord : 409
    /// `github_setup_url`.
    #[serde(default)]
    pub github_setup_url_updated: bool,
}

/// Domaine du panneau et son etat.
#[utoipa::path(
    get,
    path = "/api/settings/panel-domain",
    tag = "settings",
    responses(
        (status = 200, description = "Domaine du panneau", body = PanelDomain),
        (status = 403, description = "Proprietaire seulement"),
    )
)]
#[allow(dead_code)]
pub fn get_panel_domain() {}

/// Verifie le DNS du domaine et demande son certificat ; le panneau y sera
/// servi par le proxy (443). L'acces 8443 reste ouvert jusqu'a la
/// confirmation.
#[utoipa::path(
    put,
    path = "/api/settings/panel-domain",
    tag = "settings",
    request_body = SetPanelDomain,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 202, description = "Certificat demande", body = PanelDomain),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
        (status = 409, description = "`reserved_host`, `host_taken` ou `panel_domain_active` (retirer d'abord le domaine confirme)"),
        (status = 422, description = "`validation_failed` ou `dns_error`"),
    )
)]
#[allow(dead_code)]
pub fn put_panel_domain() {}

/// Confirme le domaine depuis ce domaine (requete servie par le proxy) :
/// l'acces 8443 se ferme. Avec une GitHub App, son webhook est d'abord
/// deplace vers le nouveau domaine (API GitHub).
#[utoipa::path(
    post,
    path = "/api/settings/panel-domain/confirm",
    tag = "settings",
    request_body(content = Option<ConfirmPanelDomain>, description = "Facultatif"),
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Domaine actif", body = PanelDomain),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Aucun domaine"),
        (status = 409, description = "`confirm_from_domain` (ouvrir le panneau sur le domaine), `not_ready` (certificat pas encore actif, ou bascule abandonnee apres 24 h) ou `github_setup_url` (changer d'abord l'URL de configuration de la GitHub App)"),
        (status = 422, description = "`validation_failed`"),
        (status = 502, description = "`github_hook_not_updated` : webhook de la GitHub App non deplace, rien n'est confirme"),
    )
)]
#[allow(dead_code)]
pub fn confirm_panel_domain() {}

/// Revérifie le DNS du domaine du panneau et replanifie tout de suite son
/// certificat (reprise après une panne passagère). DNS faux : 422, rien
/// n'est modifié.
#[utoipa::path(
    post,
    path = "/api/settings/panel-domain/recheck",
    tag = "settings",
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 202, description = "Vérification et certificat replanifiés", body = PanelDomain),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
        (status = 404, description = "Aucun domaine du panneau"),
        (status = 422, description = "`dns_error`"),
    )
)]
#[allow(dead_code)]
pub fn recheck_panel_domain() {}

/// Retire le domaine du panneau (annulation avant confirmation) : route et
/// certificat retires ; l'acces 8443 se rouvre.
#[utoipa::path(
    delete,
    path = "/api/settings/panel-domain",
    tag = "settings",
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 204, description = "Domaine retire"),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
    )
)]
#[allow(dead_code)]
pub fn delete_panel_domain() {}
