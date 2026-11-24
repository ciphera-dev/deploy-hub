//! Mot de passe oublie et reinitialisation (`/api/auth/password/*`), lien de
//! reinitialisation genere par un administrateur (`/api/team/members/*`).

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Reponse de `GET /api/auth/password/forgot`.
#[derive(Debug, Serialize, ToSchema)]
pub struct PasswordResetStatus {
    /// `true` : un lien peut etre envoye par e-mail (SMTP configure et
    /// `CIPHERA_PUBLIC_URL` defini). Sinon l'ecran invite a contacter un
    /// administrateur. Ne dit rien d'un compte en particulier.
    pub email_available: bool,
}

/// Corps de `POST /api/auth/password/forgot`.
#[derive(Deserialize, ToSchema)]
pub struct ForgotPassword {
    pub email: String,
}

impl fmt::Debug for ForgotPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ForgotPassword")
            .field("email", &"***")
            .finish()
    }
}

/// Reponse de `GET /api/auth/password/reset/{token}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ResetTokenInfo {
    /// Expiration du lien, en secondes depuis l'epoque Unix.
    pub expires_at: i64,
}

/// Corps de `POST /api/auth/password/reset`.
#[derive(Deserialize, ToSchema)]
pub struct ResetPassword {
    /// Jeton du lien `/reset/<jeton>`.
    #[schema(value_type = String)]
    pub token: Zeroizing<String>,
    /// Nouveau mot de passe. Efface de la memoire a la destruction.
    #[schema(value_type = String, format = Password, min_length = 12, max_length = 1024)]
    pub password: Zeroizing<String>,
}

impl fmt::Debug for ResetPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResetPassword")
            .field("token", &"***")
            .field("password", &"***")
            .finish()
    }
}

/// Reponse 201 de `POST /api/team/members/{id}/reset-link`.
#[derive(Serialize, ToSchema)]
pub struct ResetLink {
    /// Lien a usage unique, valable 30 minutes : `/reset/<jeton>` (prefixe de
    /// `CIPHERA_PUBLIC_URL` quand il est defini).
    pub link: String,
}

// Pas de derive : le lien porte le jeton.
impl fmt::Debug for ResetLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResetLink").field("link", &"***").finish()
    }
}

/// L'envoi d'un lien par e-mail est-il possible ?
#[utoipa::path(
    get,
    path = "/api/auth/password/forgot",
    tag = "auth",
    responses((status = 200, description = "Disponibilite de l'envoi par e-mail", body = PasswordResetStatus))
)]
#[allow(dead_code)]
pub fn password_reset_status() {}

/// Demande un lien de reinitialisation (valable 30 minutes, usage unique),
/// envoye par e-mail si le compte existe et que l'envoi est possible. La
/// reponse est la meme que le compte existe ou non.
#[utoipa::path(
    post,
    path = "/api/auth/password/forgot",
    tag = "auth",
    request_body = ForgotPassword,
    responses(
        (status = 202, description = "Demande prise en compte (toujours, compte existant ou non)"),
        (status = 422, description = "Requete invalide"),
        (status = 429, description = "Trop de demandes ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn forgot_password() {}

/// Lien de reinitialisation, pour l'ecran de saisie.
#[utoipa::path(
    get,
    path = "/api/auth/password/reset/{token}",
    tag = "auth",
    params(("token" = String, Path, description = "Jeton du lien de reinitialisation")),
    responses(
        (status = 200, description = "Lien valable", body = ResetTokenInfo),
        (status = 404, description = "`reset_invalid` : inconnu, remplace ou deja utilise"),
        (status = 410, description = "`reset_expired`"),
    )
)]
#[allow(dead_code)]
pub fn reset_token_info() {}

/// Choisit un nouveau mot de passe : toutes les sessions du compte sont
/// revoquees, la double authentification est conservee, aucune session
/// n'est ouverte.
#[utoipa::path(
    post,
    path = "/api/auth/password/reset",
    tag = "auth",
    request_body = ResetPassword,
    responses(
        (status = 204, description = "Mot de passe change ; le lien est consomme"),
        (status = 404, description = "`reset_invalid` : inconnu, remplace ou deja utilise"),
        (status = 410, description = "`reset_expired`"),
        (status = 422, description = "Mot de passe hors politique ou requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn reset_password() {}

/// Genere un lien de reinitialisation pour un membre (a lui transmettre).
#[utoipa::path(
    post,
    path = "/api/team/members/{id}/reset-link",
    tag = "team",
    params(
        ("id" = String, Path, description = "Identifiant du compte"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 201, description = "Lien cree (un lien encore inutilise est remplace)", body = ResetLink),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "`forbidden` (Equipe : Admin requis, ou compte de rang egal ou superieur au sien) ; `use_password_change` (son propre compte : utiliser `POST /api/me/password`) ; ou jeton CSRF invalide"),
        (status = 404, description = "Compte introuvable"),
    )
)]
#[allow(dead_code)]
pub fn member_reset_link() {}
