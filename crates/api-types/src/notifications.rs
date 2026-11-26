//! Parametres › Notifications : serveur SMTP de l'instance
//! (`/api/settings/notifications`).

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Reponse de `GET` et `PUT /api/settings/notifications`. Le mot de passe
/// SMTP n'est jamais renvoye (ecriture seule) : seulement `password_set`.
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationSettings {
    /// Un serveur SMTP a ete enregistre.
    pub configured: bool,
    /// Envoi d'e-mails active (invitations, reinitialisation du mot de passe).
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    /// `starttls` (port 587 en general) ou `tls` (TLS implicite, port 465).
    pub tls: String,
    /// Identifiant SMTP (vide : pas d'authentification).
    pub username: String,
    /// Un mot de passe SMTP est enregistre (chiffre).
    pub password_set: bool,
    pub from_address: String,
    pub from_name: String,
    /// `CIPHERA_PUBLIC_URL` : sans elle, aucun lien n'est envoye par e-mail.
    pub public_url: Option<String>,
    /// Les e-mails partent effectivement (SMTP active et origine publique definie).
    pub email_available: bool,
}

/// Corps de `PUT /api/settings/notifications`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNotificationSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    /// `starttls` ou `tls` ; toute autre valeur est refusee (pas d'envoi en clair).
    pub tls: String,
    #[serde(default)]
    pub username: String,
    /// Absent ou vide : le mot de passe enregistre est conserve. Jamais renvoye.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = Password)]
    pub password: Option<Zeroizing<String>>,
    pub from_address: String,
    pub from_name: String,
    /// Mot de passe du compte du proprietaire (confirmation renforcee : ces
    /// reglages decident si les avis de securite partent).
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub current_password: Zeroizing<String>,
    /// Code du second facteur du proprietaire (application d'authentification
    /// ou code de secours) : exige si son compte en a un.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = Password, max_length = 64)]
    pub code: Option<Zeroizing<String>>,
}

impl fmt::Debug for UpdateNotificationSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateNotificationSettings")
            .field("enabled", &self.enabled)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("from_address", &self.from_address)
            .field("from_name", &self.from_name)
            .field("current_password", &"***")
            .field("code", &self.code.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Serveur SMTP de l'instance (sans le mot de passe).
#[utoipa::path(
    get,
    path = "/api/settings/notifications",
    tag = "settings",
    responses(
        (status = 200, description = "Reglages SMTP", body = NotificationSettings),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "`forbidden` : Parametres, Admin requis"),
    )
)]
#[allow(dead_code)]
pub fn get_notifications() {}

/// Enregistre le serveur SMTP (STARTTLS ou TLS obligatoire) ; proprietaire seulement.
#[utoipa::path(
    put,
    path = "/api/settings/notifications",
    tag = "settings",
    request_body = UpdateNotificationSettings,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Reglages enregistres", body = NotificationSettings),
        (status = 401, description = "Pas de session complete, `invalid_credentials` (mot de passe du compte), `second_factor_required` (code du second facteur absent) ou `invalid_code`"),
        (status = 403, description = "Proprietaire seulement, ou jeton CSRF absent ou invalide"),
        (status = 409, description = "`second_factor_unavailable`"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
        (status = 422, description = "`validation_failed` (hote, port, mode TLS, identifiants, expediteur, mot de passe a ressaisir), `smtp_host_private` (hote local ou prive) ou `smtp_host_unresolved` (nom non resolu)"),
    )
)]
#[allow(dead_code)]
pub fn put_notifications() {}
