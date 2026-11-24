//! Connexion et session : `/api/auth/*`, `/api/me`.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Corps de `POST /api/auth/login`.
#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    /// Efface de la memoire a la destruction. Jamais renvoye ni journalise.
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
    /// `true` : session de 30 jours (cookie persistant) ; sinon 12 heures,
    /// cookie de session du navigateur.
    #[serde(default)]
    pub remember: bool,
}

// Pas de derive : le mot de passe ne doit jamais apparaitre dans un journal.
impl fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("email", &"***")
            .field("password", &"***")
            .field("remember", &self.remember)
            .finish()
    }
}

/// Reponse 200 de `POST /api/auth/login` (le cookie de session est dans `Set-Cookie`).
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    /// `true` : la session attend la double authentification (`/api/auth/mfa/*`).
    pub mfa_required: bool,
    /// Jeton CSRF de la nouvelle session, a renvoyer dans `X-CSRF-Token`.
    pub csrf_token: String,
}

/// Reponse de `GET /api/me`.
#[derive(Debug, Serialize, ToSchema)]
pub struct Me {
    pub id: String,
    pub name: String,
    pub email: String,
    /// `owner`, `admin`, `ops`, `developer` ou `viewer`.
    pub role: String,
    /// Jeton CSRF de la session, a renvoyer dans `X-CSRF-Token`.
    pub csrf_token: String,
    /// Application d'authentification (TOTP) activee.
    pub totp_enabled: bool,
    /// Au moins une passkey (cle de securite) enregistree.
    pub webauthn_enabled: bool,
    /// L'organisation impose la 2FA et ce compte ne l'a pas : seul l'enrolement
    /// est ouvert (les autres routes repondent 403 `mfa_enrollment_required`).
    pub mfa_enrollment_required: bool,
    /// Langue de l'interface : `fr` ou `en`.
    pub language: String,
    /// Fuseau horaire IANA (ex. `Europe/Zurich`).
    pub timezone: String,
    /// URL (meme origine, versionnee) de l'avatar ; `null` : initiales.
    pub avatar_url: Option<String>,
}

/// Connexion par e-mail et mot de passe.
#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Connecte ; nouveau cookie de session pose", body = LoginResponse),
        (status = 401, description = "Identifiants refuses (message identique pour un e-mail inconnu)"),
        (status = 422, description = "Requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn login() {}

/// Deconnexion : supprime la session et efface le cookie.
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    // Ajoute par le client (intergiciel openapi-fetch), pas par chaque appel.
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 204, description = "Session supprimee, cookie efface"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
    )
)]
#[allow(dead_code)]
pub fn logout() {}

/// Compte connecte.
#[utoipa::path(
    get,
    path = "/api/me",
    tag = "auth",
    responses(
        (status = 200, description = "Compte de la session", body = Me),
        (status = 401, description = "`unauthenticated` (pas de session) ou `mfa_required`"),
    )
)]
#[allow(dead_code)]
pub fn me() {}
