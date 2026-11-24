//! Verification d'adresse e-mail (`/api/auth/email-verification/*`) : lien
//! recu a la nouvelle adresse (changement d'e-mail) ou a l'adresse d'une
//! inscription ouverte.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

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

/// Corps de `POST /api/auth/email-verification/{token}/confirm` : mot de passe
/// du compte, exige (12 a 1024 caracteres) pour le motif `signup`, ignore
/// sinon.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmEmailVerification {
    #[schema(value_type = String, format = Password, min_length = 12, max_length = 1024)]
    pub password: Zeroizing<String>,
}

impl fmt::Debug for ConfirmEmailVerification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfirmEmailVerification")
            .field("password", &"***")
            .finish()
    }
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
/// le compte avec le mot de passe choisi (inscription). Un changement ferme les
/// autres sessions du compte.
#[utoipa::path(
    post,
    path = "/api/auth/email-verification/{token}/confirm",
    tag = "auth",
    params(("token" = String, Path, description = "Jeton du lien de verification")),
    request_body(content = Option<ConfirmEmailVerification>, description = "Obligatoire pour une inscription (motif `signup`)"),
    responses(
        (status = 204, description = "Adresse confirmee ; le lien est consomme"),
        (status = 404, description = "`email_verification_invalid` : inconnu ou deja utilise"),
        (status = 409, description = "`email_taken` : adresse prise entre-temps"),
        (status = 410, description = "`email_verification_expired`"),
        (status = 422, description = "Mot de passe absent ou refuse par la politique (inscription)"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn confirm_email_verification() {}
