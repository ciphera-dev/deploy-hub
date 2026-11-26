//! Creation de comptes hors assistant : invitations (`/api/team/invitations`,
//! `/api/auth/invitations/*`) et inscription publique (`/api/auth/register`).

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Corps de `POST /api/team/invitations`.
#[derive(Deserialize, ToSchema)]
pub struct CreateInvitation {
    pub email: String,
    /// Nom propose a l'invite (modifiable a l'acceptation).
    #[serde(default)]
    pub name: Option<String>,
    /// `owner`, `admin`, `ops`, `developer` ou `viewer` ; jamais au-dessus du
    /// role de l'invitant.
    pub role: String,
    /// Mot de passe de l'invitant (confirmation renforcee).
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
    /// Code du second facteur (application d'authentification ou code de
    /// secours) : exige si le compte de l'invitant en a un.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = Password, max_length = 64)]
    pub code: Option<Zeroizing<String>>,
}

impl fmt::Debug for CreateInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateInvitation")
            .field("email", &self.email)
            .field("name", &self.name)
            .field("role", &self.role)
            .field("password", &"***")
            .field("code", &self.code.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Reponse 201 de `POST /api/team/invitations`.
#[derive(Serialize, ToSchema)]
pub struct InvitationLink {
    /// Lien a usage unique, valable 72 heures : `/invite/<jeton>` (prefixe de
    /// `CIPHERA_PUBLIC_URL` quand il est defini). Envoye aussi par e-mail a
    /// l'invite quand SMTP est configure.
    pub link: String,
}

// Pas de derive : le lien porte le jeton.
impl fmt::Debug for InvitationLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InvitationLink")
            .field("link", &"***")
            .finish()
    }
}

/// Reponse de `GET /api/auth/invitations/{token}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct InvitationInfo {
    pub email: String,
    /// Nom propose par l'invitant (vide sinon).
    pub name: String,
    pub role: String,
    /// Expiration, en secondes depuis l'epoque Unix.
    pub expires_at: i64,
}

/// Corps de `POST /api/auth/invitations/{token}/accept`.
#[derive(Deserialize, ToSchema)]
pub struct AcceptInvitation {
    pub name: String,
    /// Efface de la memoire a la destruction. Jamais renvoye ni journalise.
    #[schema(value_type = String, format = Password, min_length = 12, max_length = 1024)]
    pub password: Zeroizing<String>,
}

impl fmt::Debug for AcceptInvitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcceptInvitation")
            .field("name", &self.name)
            .field("password", &"***")
            .finish()
    }
}

/// Reponse de `GET /api/auth/register`.
#[derive(Debug, Serialize, ToSchema)]
pub struct SignupStatus {
    /// `false` (par defaut) : comptes crees sur invitation seulement.
    pub public_signup: bool,
}

/// Corps de `POST /api/auth/register`.
#[derive(Deserialize, ToSchema)]
pub struct RegisterRequest {
    pub name: String,
    pub email: String,
}

impl fmt::Debug for RegisterRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisterRequest")
            .field("name", &self.name)
            .field("email", &"***")
            .finish()
    }
}

/// Invite un membre : renvoie un lien a usage unique valable 72 heures.
/// Confirmation renforcee : mot de passe de l'invitant, plus le code de son
/// second facteur s'il en a un ; les autres proprietaires sont prevenus.
#[utoipa::path(
    post,
    path = "/api/team/invitations",
    tag = "team",
    request_body = CreateInvitation,
    params(("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)")),
    responses(
        (status = 201, description = "Invitation creee (une invitation en attente pour cet e-mail est remplacee)", body = InvitationLink),
        (status = 401, description = "Pas de session complete, `invalid_credentials` (mot de passe), `second_factor_required` (code du second facteur absent) ou `invalid_code`"),
        (status = 403, description = "`forbidden` (Equipe : Admin requis, ou role au-dessus du sien) ou jeton CSRF invalide"),
        (status = 409, description = "`email_taken` (un compte existe deja pour cet e-mail) ou `second_factor_unavailable`"),
        (status = 422, description = "E-mail, nom, role ou mot de passe invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn create_invitation() {}

/// Invitation en attente, pour l'ecran d'acceptation.
#[utoipa::path(
    get,
    path = "/api/auth/invitations/{token}",
    tag = "auth",
    params(("token" = String, Path, description = "Jeton du lien d'invitation")),
    responses(
        (status = 200, description = "Invitation valable", body = InvitationInfo),
        (status = 404, description = "`invitation_invalid` : inconnue ou deja utilisee"),
        (status = 410, description = "`invitation_expired`"),
    )
)]
#[allow(dead_code)]
pub fn invitation_info() {}

/// Accepte une invitation : cree le compte avec le role invite, sans ouvrir
/// de session (la connexion suit, double authentification comprise).
#[utoipa::path(
    post,
    path = "/api/auth/invitations/{token}/accept",
    tag = "auth",
    params(("token" = String, Path, description = "Jeton du lien d'invitation")),
    request_body = AcceptInvitation,
    responses(
        (status = 204, description = "Compte cree ; l'invitation est consommee"),
        (status = 404, description = "`invitation_invalid` : inconnue ou deja utilisee"),
        (status = 409, description = "`email_taken` : un compte existe deja pour cet e-mail"),
        (status = 410, description = "`invitation_expired`"),
        (status = 422, description = "Nom ou mot de passe invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn accept_invitation() {}

/// Inscription publique ouverte ou non.
#[utoipa::path(
    get,
    path = "/api/auth/register",
    tag = "auth",
    responses((status = 200, description = "Politique d'inscription", body = SignupStatus))
)]
#[allow(dead_code)]
pub fn signup_status() {}

/// Inscription publique (role Lecteur), si l'instance l'autorise. Nom et
/// adresse seulement : le mot de passe se choisit en ouvrant le lien envoye a
/// l'adresse (preuve de possession de la boite). Le compte reste en attente
/// jusqu'a la confirmation ; meme reponse que l'adresse soit deja connue ou non.
#[utoipa::path(
    post,
    path = "/api/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 202, description = "Demande enregistree ; un e-mail de verification part si l'adresse est libre"),
        (status = 403, description = "`signup_disabled` : inscription sur invitation seulement"),
        (status = 409, description = "`smtp_required` : inscription impossible sans envoi d'e-mails"),
        (status = 422, description = "Nom ou e-mail invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn register() {}
