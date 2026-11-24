//! Passkeys (WebAuthn) : enregistrement et gestion (`/api/me/webauthn/*`),
//! assertion comme second facteur ou connexion sans mot de passe
//! (`/api/auth/webauthn/*`).
//!
//! Les options et les reponses de l'authentificateur suivent le format JSON
//! de WebAuthn (octets en base64url sans remplissage) : `options.publicKey`
//! se passe a `navigator.credentials.create()` / `.get()` apres decodage.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Reponse de `GET /api/auth/webauthn/status`.
#[derive(Debug, Serialize, ToSchema)]
pub struct WebauthnStatus {
    /// `false` quand le panneau est joint par une adresse IP (les navigateurs
    /// refusent un RP ID qui en est une) : masquer les boutons passkey.
    pub available: bool,
}

/// Options d'une ceremonie WebAuthn.
#[derive(Debug, Serialize, ToSchema)]
pub struct WebauthnOptions {
    /// A renvoyer a la verification (ceremonie a usage unique, 5 minutes).
    pub ceremony_id: String,
    /// `{ "publicKey": ... }` (CredentialCreationOptions ou CredentialRequestOptions en JSON).
    pub options: serde_json::Value,
}

/// Corps de `POST /api/me/webauthn/register/verify`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct WebauthnRegister {
    pub ceremony_id: String,
    /// Nom de la cle (64 caracteres au plus) ; vide : « Clé de sécurité ».
    #[serde(default)]
    pub name: String,
    /// `PublicKeyCredential` renvoye par `navigator.credentials.create()`, en JSON.
    pub credential: serde_json::Value,
}

/// Reponse de `POST /api/me/webauthn/register/verify`. La session a tourne :
/// nouveau cookie et nouveau jeton CSRF.
#[derive(Serialize, ToSchema)]
pub struct WebauthnRegistered {
    pub id: String,
    pub csrf_token: String,
    /// 10 codes de secours `XXXXX-XXXXX`, emis quand le compte n'en avait plus
    /// d'utilisable (premiere passkey sans TOTP) : affiches une seule fois.
    /// Vide sinon.
    pub recovery_codes: Vec<String>,
}

impl fmt::Debug for WebauthnRegistered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebauthnRegistered")
            .field("id", &self.id)
            .field("recovery_codes", &self.recovery_codes.len())
            .finish_non_exhaustive()
    }
}

/// Cle enregistree, telle que l'affiche le profil.
#[derive(Debug, Serialize, ToSchema)]
pub struct WebauthnCredential {
    pub id: String,
    pub name: String,
    /// Secondes depuis l'epoque Unix.
    pub created_at: i64,
    /// Derniere utilisation, en secondes depuis l'epoque Unix.
    pub last_used_at: Option<i64>,
}

/// Reponse de `GET /api/me/webauthn`.
#[derive(Debug, Serialize, ToSchema)]
pub struct WebauthnCredentials {
    pub credentials: Vec<WebauthnCredential>,
}

/// Corps de `DELETE /api/me/webauthn/{id}` : mot de passe redemande.
#[derive(Deserialize, ToSchema)]
pub struct DeleteWebauthn {
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
}

impl fmt::Debug for DeleteWebauthn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeleteWebauthn")
            .field("password", &"***")
            .finish()
    }
}

/// Usage d'une assertion.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssertMode {
    /// Connexion sans mot de passe (passkey decouvrable, verification de
    /// l'utilisateur exigee) : ouvre une session complete.
    Login,
    /// Second facteur d'une session en attente de MFA.
    SecondFactor,
}

/// Corps de `POST /api/auth/webauthn/assert/options`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssertOptionsRequest {
    pub mode: AssertMode,
}

/// Corps de `POST /api/auth/webauthn/assert/verify`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AssertVerify {
    pub ceremony_id: String,
    /// `PublicKeyCredential` renvoye par `navigator.credentials.get()`, en JSON.
    pub credential: serde_json::Value,
    /// Connexion sans mot de passe : session de 30 jours (sinon 12 heures).
    #[serde(default)]
    pub remember: bool,
}

/// Disponibilite de WebAuthn pour l'hote par lequel le panneau est joint.
#[utoipa::path(
    get,
    path = "/api/auth/webauthn/status",
    tag = "webauthn",
    responses((status = 200, description = "Disponibilite", body = WebauthnStatus))
)]
#[allow(dead_code)]
pub fn webauthn_status() {}

/// Options d'enregistrement d'une passkey (credentials deja enregistres exclus).
#[utoipa::path(
    post,
    path = "/api/me/webauthn/register/options",
    tag = "webauthn",
    request_body(content = crate::mfa::AddFactor, description = "Mot de passe actuel (toute session, enrolement obligatoire compris)"),
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Options de creation", body = WebauthnOptions),
        (status = 401, description = "Pas de session complete, ou mot de passe absent ou incorrect"),
        (status = 429, description = "Trop de tentatives de mot de passe"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 409, description = "`webauthn_unavailable` : panneau joint par une adresse IP"),
    )
)]
#[allow(dead_code)]
pub fn webauthn_register_options() {}

/// Verifie et enregistre la passkey ; la session tourne.
#[utoipa::path(
    post,
    path = "/api/me/webauthn/register/verify",
    tag = "webauthn",
    request_body = WebauthnRegister,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 200, description = "Passkey enregistree ; session tournee", body = WebauthnRegistered),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "Jeton CSRF absent ou invalide"),
        (status = 409, description = "`credential_exists` ou `webauthn_unavailable`"),
        (status = 422, description = "`webauthn_failed` (ceremonie expiree, deja utilisee ou reponse invalide) ou `validation_failed`"),
    )
)]
#[allow(dead_code)]
pub fn webauthn_register_verify() {}

/// Passkeys du compte connecte.
#[utoipa::path(
    get,
    path = "/api/me/webauthn",
    tag = "webauthn",
    responses(
        (status = 200, description = "Cles enregistrees", body = WebauthnCredentials),
        (status = 401, description = "Pas de session complete"),
    )
)]
#[allow(dead_code)]
pub fn webauthn_list() {}

/// Supprime une passkey du compte connecte (mot de passe redemande).
#[utoipa::path(
    delete,
    path = "/api/me/webauthn/{id}",
    tag = "webauthn",
    request_body = DeleteWebauthn,
    params(
        ("id" = String, Path, description = "Identifiant de la cle"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 204, description = "Cle supprimee"),
        (status = 401, description = "`invalid_credentials` (mot de passe) ou pas de session"),
        (status = 403, description = "Jeton CSRF absent ou invalide, ou `mfa_enrollment_required`"),
        (status = 404, description = "Aucune cle de ce compte avec cet identifiant"),
        (status = 409, description = "`mfa_required_by_org` : derniere methode de 2FA alors que l'organisation l'impose"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn webauthn_delete() {}

/// Options d'assertion : second facteur (session en attente de MFA) ou
/// connexion sans mot de passe.
#[utoipa::path(
    post,
    path = "/api/auth/webauthn/assert/options",
    tag = "webauthn",
    request_body = AssertOptionsRequest,
    responses(
        (status = 200, description = "Options d'assertion", body = WebauthnOptions),
        (status = 401, description = "`second_factor` sans session en attente"),
        (status = 409, description = "`webauthn_unavailable`, `mfa_not_pending` ou `webauthn_no_credentials`"),
        (status = 429, description = "Trop de ceremonies en cours ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn webauthn_assert_options() {}

/// Verifie l'assertion. Second facteur : la session tourne et devient
/// complete. Connexion sans mot de passe : nouvelle session complete.
#[utoipa::path(
    post,
    path = "/api/auth/webauthn/assert/verify",
    tag = "webauthn",
    request_body = AssertVerify,
    responses(
        (status = 200, description = "Session complete ; nouveau cookie de session pose", body = crate::mfa::MfaVerified),
        (status = 401, description = "`webauthn_failed`, ou `unauthenticated`"),
        (status = 409, description = "`mfa_not_pending` ou `webauthn_unavailable`"),
        (status = 422, description = "Requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn webauthn_assert_verify() {}
