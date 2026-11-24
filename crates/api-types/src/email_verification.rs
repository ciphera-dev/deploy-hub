//! Verification d'adresse e-mail (`/api/auth/email-verification/*`) : lien
//! recu a la nouvelle adresse (changement d'e-mail) ou a l'adresse d'une
//! inscription ouverte.

use serde::Serialize;
use utoipa::ToSchema;

/// Reponse de `GET /api/auth/email-verification/{token}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct EmailVerificationInfo {
    /// `change` (nouvelle adresse d'un compte existant) ou `signup`
    /// (inscription ouverte).
    pub purpose: String,
    /// Adresse a confirmer.
    pub email: String,
    /// Expiration du lien, en secondes depuis l'epoque Unix.
    pub expires_at: i64,
}

/// Lien de verification en attente, pour l'ecran de confirmation.
#[utoipa::path(
    get,
    path = "/api/auth/email-verification/{token}",
    tag = "auth",
    params(("token" = String, Path, description = "Jeton du lien de verification")),
    responses(
        (status = 200, description = "Lien valable", body = EmailVerificationInfo),
        (status = 404, description = "`email_verification_invalid` : inconnu ou deja utilise"),
        (status = 410, description = "`email_verification_expired`"),
    )
)]
#[allow(dead_code)]
pub fn email_verification_info() {}

/// Confirme l'adresse : remplace l'adresse du compte (changement) ou active
/// le compte (inscription). Un changement ferme les autres sessions du compte.
#[utoipa::path(
    post,
    path = "/api/auth/email-verification/{token}/confirm",
    tag = "auth",
    params(("token" = String, Path, description = "Jeton du lien de verification")),
    responses(
        (status = 204, description = "Adresse confirmee ; le lien est consomme"),
        (status = 404, description = "`email_verification_invalid` : inconnu ou deja utilise"),
        (status = 409, description = "`email_taken` : adresse prise entre-temps"),
        (status = 410, description = "`email_verification_expired`"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn confirm_email_verification() {}
