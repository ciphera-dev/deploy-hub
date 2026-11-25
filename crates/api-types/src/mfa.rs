//! Double authentification : enrolement TOTP (`/api/me/mfa/totp/*`) et
//! verification a la connexion (`/api/auth/mfa/verify`, `/api/auth/recovery-code`).

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Code saisi : TOTP a 6 chiffres, ou code de secours (`XXXXX-XXXXX`,
/// casse, espaces et tirets indifferents).
#[derive(Deserialize, ToSchema)]
pub struct MfaCode {
    #[schema(value_type = String, max_length = 64)]
    pub code: Zeroizing<String>,
}

// Pas de derive : le code ne doit jamais apparaitre dans un journal.
impl fmt::Debug for MfaCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MfaCode").field("code", &"***").finish()
    }
}

/// Corps de `POST /api/me/mfa/totp/start` et de `POST /api/me/webauthn/register/options` :
/// mot de passe actuel, redemande avant l'ajout d'un second facteur (toute
/// session, enrolement obligatoire compris).
#[derive(Deserialize, ToSchema)]
pub struct AddFactor {
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub current_password: Zeroizing<String>,
}

impl fmt::Debug for AddFactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddFactor")
            .field("current_password", &"***")
            .finish()
    }
}

/// Reponse de `POST /api/me/mfa/totp/start`.
#[derive(Serialize, ToSchema)]
pub struct TotpStart {
    /// URI `otpauth://totp/...` (contient la graine) pour une saisie manuelle.
    pub otpauth_uri: String,
    /// QR code de l'URI, en SVG autonome (a afficher en `img` `data:`).
    pub qr_svg: String,
}

impl fmt::Debug for TotpStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TotpStart").finish_non_exhaustive()
    }
}

/// Reponse de `POST /api/me/mfa/totp/confirm`. La session a tourne : nouveau
/// cookie et nouveau jeton CSRF.
#[derive(Serialize, ToSchema)]
pub struct TotpConfirmed {
    /// Les 10 codes de secours, affiches cette seule fois.
    pub recovery_codes: Vec<String>,
    pub csrf_token: String,
}

impl fmt::Debug for TotpConfirmed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TotpConfirmed").finish_non_exhaustive()
    }
}

/// Reponse 200 de la verification du second facteur : la session est
/// complete et a tourne (nouveau cookie, nouveau jeton CSRF).
#[derive(Debug, Serialize, ToSchema)]
pub struct MfaVerified {
    pub csrf_token: String,
}

/// Reponse de `GET /api/auth/mfa/methods` : seconds facteurs utilisables par
/// la session en attente de MFA.
#[derive(Debug, Serialize, ToSchema)]
pub struct MfaMethods {
    /// Application d'authentification.
    pub totp: bool,
    /// Passkey enregistree et WebAuthn disponible pour cet hote.
    pub webauthn: bool,
    /// Il reste au moins un code de secours non utilise.
    pub recovery: bool,
    /// Passkey enregistree mais inutilisable depuis cet hote : adresse
    /// publique du panneau a laquelle elle fonctionne.
    pub public_url: Option<String>,
}

/// Corps de `DELETE /api/me/mfa/totp` : mot de passe redemande.
#[derive(Deserialize, ToSchema)]
pub struct DisableTotp {
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
}

impl fmt::Debug for DisableTotp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DisableTotp")
            .field("password", &"***")
            .finish()
    }
}

/// Demarre l'enrolement TOTP (remplace une graine non confirmee).
#[utoipa::path(
    post,
    path = "/api/me/mfa/totp/start",
    tag = "mfa",
    request_body(content = AddFactor, description = "Mot de passe actuel (toute session, enrolement obligatoire compris)"),
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Graine creee, a confirmer", body = TotpStart),
        (status = 401, description = "Pas de session complete, ou mot de passe absent ou incorrect"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 409, description = "`totp_already_enabled` : la desactiver d'abord"),
        (status = 429, description = "Trop de tentatives de mot de passe"),
    )
)]
#[allow(dead_code)]
pub fn totp_start() {}

/// Confirme l'enrolement avec un code de l'application ; renvoie les codes de secours.
#[utoipa::path(
    post,
    path = "/api/me/mfa/totp/confirm",
    tag = "mfa",
    request_body = MfaCode,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "TOTP activee ; session tournee", body = TotpConfirmed),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 409, description = "`totp_not_started` : aucun enrolement en cours"),
        (status = 422, description = "`invalid_code`"),
    )
)]
#[allow(dead_code)]
pub fn totp_confirm() {}

/// Desactive la TOTP et supprime les codes de secours (mot de passe redemande).
#[utoipa::path(
    delete,
    path = "/api/me/mfa/totp",
    tag = "mfa",
    request_body = DisableTotp,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 204, description = "TOTP et codes de secours supprimes"),
        (status = 401, description = "`invalid_credentials` (mot de passe) ou pas de session"),
        (status = 403, description = "Jeton CSRF absent ou invalide, ou `mfa_enrollment_required`"),
        (status = 404, description = "Aucune TOTP"),
        (status = 409, description = "`mfa_required_by_org` : 2FA imposee par l'organisation"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn totp_disable() {}

/// Verifie le code TOTP d'une session en attente de MFA.
#[utoipa::path(
    post,
    path = "/api/auth/mfa/verify",
    tag = "mfa",
    request_body = MfaCode,
    responses(
        (status = 200, description = "Session complete ; nouveau cookie de session pose", body = MfaVerified),
        (status = 401, description = "`invalid_code`, ou `unauthenticated` (pas de session en attente)"),
        (status = 409, description = "`mfa_not_pending` : la session est deja complete"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn mfa_verify() {}

/// Seconds facteurs proposes a la session en attente de MFA.
#[utoipa::path(
    get,
    path = "/api/auth/mfa/methods",
    tag = "mfa",
    responses(
        (status = 200, description = "Methodes utilisables", body = MfaMethods),
        (status = 401, description = "`unauthenticated` (pas de session)"),
        (status = 409, description = "`mfa_not_pending` : la session est deja complete"),
    )
)]
#[allow(dead_code)]
pub fn mfa_methods() {}

/// Utilise un code de secours (usage unique) a la place du code TOTP.
#[utoipa::path(
    post,
    path = "/api/auth/recovery-code",
    tag = "mfa",
    request_body = MfaCode,
    responses(
        (status = 200, description = "Session complete ; nouveau cookie de session pose", body = MfaVerified),
        (status = 401, description = "`invalid_code`, ou `unauthenticated` (pas de session en attente)"),
        (status = 409, description = "`mfa_not_pending` : la session est deja complete"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn recovery_code() {}
