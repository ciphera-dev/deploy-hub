//! Profil du compte connecte : `/api/me` (modification), avatar, mot de
//! passe, sessions, preferences de notification.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Corps de `PATCH /api/me` : seuls les champs presents sont modifies.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMe {
    /// 1 a 200 caracteres, sans caractere de controle.
    pub name: Option<String>,
    /// Nouvelle adresse e-mail : exige `current_password`.
    pub email: Option<String>,
    /// Mot de passe actuel (changement d'e-mail). Jamais renvoye ni journalise.
    #[schema(value_type = Option<String>, format = Password, max_length = 1024)]
    pub current_password: Option<Zeroizing<String>>,
    /// `fr` ou `en`.
    pub language: Option<String>,
    /// Fuseau horaire IANA (ex. `Europe/Zurich`).
    pub timezone: Option<String>,
}

// Pas de derive : ni le mot de passe ni l'e-mail dans un journal.
impl fmt::Debug for UpdateMe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateMe")
            .field("name", &self.name)
            .field("email", &self.email.as_ref().map(|_| "***"))
            .field(
                "current_password",
                &self.current_password.as_ref().map(|_| "***"),
            )
            .field("language", &self.language)
            .field("timezone", &self.timezone)
            .finish()
    }
}

/// Reponse de `PUT /api/me/avatar`.
#[derive(Debug, Serialize, ToSchema)]
pub struct AvatarUpdated {
    /// URL (meme origine, versionnee) de l'avatar re-encode.
    pub avatar_url: String,
}

/// Corps de `POST /api/me/password`.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePassword {
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub current_password: Zeroizing<String>,
    /// 12 a 1024 caracteres.
    #[schema(value_type = String, format = Password, min_length = 12, max_length = 1024)]
    pub new_password: Zeroizing<String>,
}

impl fmt::Debug for ChangePassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangePassword")
            .field("current_password", &"***")
            .field("new_password", &"***")
            .finish()
    }
}

/// Une session du compte (jamais le jeton ni son empreinte).
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionInfo {
    pub id: String,
    /// Adresse IP de la connexion.
    pub ip: Option<String>,
    /// En-tete `User-Agent` de la connexion (tronque).
    pub user_agent: Option<String>,
    /// Secondes depuis l'epoque Unix.
    pub created_at: i64,
    /// Derniere requete (precision d'une minute), secondes depuis l'epoque Unix.
    pub last_seen_at: i64,
    /// Session de cette requete.
    pub current: bool,
}

/// Reponse de `GET /api/me/sessions`.
#[derive(Debug, Serialize, ToSchema)]
pub struct Sessions {
    /// Les plus recemment actives d'abord.
    pub sessions: Vec<SessionInfo>,
}

/// Preferences de notification du compte : canal e-mail seulement (v0.1).
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationPrefs {
    /// Canal e-mail active.
    pub email_enabled: bool,
    /// Deploiement reussi.
    pub deploy_succeeded: bool,
    /// Echec de build ou de deploiement.
    pub deploy_failed: bool,
    /// Certificats (expiration, renouvellement).
    pub certificates: bool,
    /// Equipe (invitations, changements de role).
    pub team: bool,
}

/// Reponse de `GET|PUT /api/me/notification-prefs`.
#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationPrefsView {
    /// L'instance peut envoyer des e-mails (SMTP active, `CIPHERA_PUBLIC_URL` defini).
    pub email_available: bool,
    pub email_enabled: bool,
    pub deploy_succeeded: bool,
    pub deploy_failed: bool,
    pub certificates: bool,
    pub team: bool,
}

/// Modifie le profil (nom, e-mail, langue, fuseau horaire).
#[utoipa::path(
    patch,
    path = "/api/me",
    tag = "me",
    request_body = UpdateMe,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Profil modifie", body = super::auth::Me),
        (status = 401, description = "`invalid_credentials` (mot de passe actuel) ou pas de session"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 409, description = "`email_taken` : adresse deja utilisee"),
        (status = 422, description = "Champ invalide (langue, fuseau, nom, e-mail, mot de passe manquant)"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn update_me() {}

/// Avatar du compte connecte : PNG 256x256 re-encode par le serveur.
#[utoipa::path(
    get,
    path = "/api/me/avatar",
    tag = "me",
    responses(
        (status = 200, description = "Image PNG", content_type = "image/png", body = Vec<u8>),
        (status = 404, description = "Aucun avatar"),
    )
)]
#[allow(dead_code)]
pub fn get_avatar() {}

/// Remplace l'avatar : PNG, JPEG ou WebP de 1 Mo au plus (format lu dans les
/// octets) ; recadre, reduit et re-encode en PNG (metadonnees retirees).
#[utoipa::path(
    put,
    path = "/api/me/avatar",
    tag = "me",
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "Image PNG, JPEG ou WebP"),
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Avatar enregistre", body = AvatarUpdated),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 413, description = "Plus de 1 Mo"),
        (status = 415, description = "Format autre que PNG, JPEG ou WebP"),
        (status = 422, description = "Image illisible ou dimensions trop grandes"),
    )
)]
#[allow(dead_code)]
pub fn put_avatar() {}

/// Supprime l'avatar (retour aux initiales).
#[utoipa::path(
    delete,
    path = "/api/me/avatar",
    tag = "me",
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 204, description = "Avatar supprime (ou absent)"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
    )
)]
#[allow(dead_code)]
pub fn delete_avatar() {}

/// Change le mot de passe ; garde la session courante, revoque les autres.
#[utoipa::path(
    post,
    path = "/api/me/password",
    tag = "me",
    request_body = ChangePassword,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 204, description = "Mot de passe change ; autres sessions revoquees"),
        (status = 401, description = "`invalid_credentials` (mot de passe actuel) ou pas de session"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 422, description = "Nouveau mot de passe hors politique, ou requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn change_password() {}

/// Sessions du compte connecte.
#[utoipa::path(
    get,
    path = "/api/me/sessions",
    tag = "me",
    responses((status = 200, description = "Sessions non expirees du compte", body = Sessions))
)]
#[allow(dead_code)]
pub fn list_sessions() {}

/// Revoque une autre session du compte.
#[utoipa::path(
    delete,
    path = "/api/me/sessions/{id}",
    tag = "me",
    params(
        ("id" = String, Path, description = "Identifiant de la session"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 204, description = "Session revoquee"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 404, description = "Aucune session de ce compte avec cet identifiant"),
        (status = 409, description = "`current_session` : utiliser la deconnexion"),
    )
)]
#[allow(dead_code)]
pub fn revoke_session() {}

/// Preferences de notification du compte.
#[utoipa::path(
    get,
    path = "/api/me/notification-prefs",
    tag = "me",
    responses((status = 200, description = "Preferences (valeurs par defaut si jamais enregistrees)", body = NotificationPrefsView))
)]
#[allow(dead_code)]
pub fn get_notification_prefs() {}

/// Enregistre les preferences de notification (canal e-mail seulement).
#[utoipa::path(
    put,
    path = "/api/me/notification-prefs",
    tag = "me",
    request_body = NotificationPrefs,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Preferences enregistrees", body = NotificationPrefsView),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 422, description = "Champ manquant ou inconnu"),
    )
)]
#[allow(dead_code)]
pub fn put_notification_prefs() {}
