//! Equipe : membres (`/api/team/members`) et invitations en attente
//! (`/api/team/invitations`). Equipe : Admin requis ; un admin ne gere que
//! les roles ops, developer et viewer, le proprietaire gere tous les autres
//! membres ; jamais soi-meme, jamais le dernier proprietaire.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Membre de l'organisation.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamMember {
    pub id: String,
    pub name: String,
    pub email: String,
    /// `owner`, `admin`, `ops`, `developer` ou `viewer`.
    pub role: String,
    /// Une TOTP confirmee ou au moins une passkey.
    pub two_factor: bool,
    /// Derniere activite d'une session, en secondes depuis l'epoque Unix ;
    /// absente si le membre ne s'est jamais connecte.
    pub last_active_at: Option<i64>,
    /// Creation du compte, en secondes depuis l'epoque Unix.
    pub created_at: i64,
    /// Inscription ouverte dont l'adresse n'est pas encore confirmee : le
    /// compte ne peut pas se connecter, son role et sa reinitialisation sont
    /// refuses (409 `account_pending`).
    pub pending: bool,
}

/// Corps de `PATCH /api/team/members/{id}`.
#[derive(Deserialize, ToSchema)]
pub struct UpdateMemberRole {
    /// Nouveau role : `owner`, `admin`, `ops`, `developer` ou `viewer`.
    pub role: String,
    /// Mot de passe de l'auteur (confirmation renforcee).
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
    /// Code du second facteur (application d'authentification ou code de
    /// secours) : exige si le compte de l'auteur en a un.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = Password, max_length = 64)]
    pub code: Option<Zeroizing<String>>,
}

impl fmt::Debug for UpdateMemberRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateMemberRole")
            .field("role", &self.role)
            .field("password", &"***")
            .field("code", &self.code.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Corps de `DELETE /api/team/members/{id}` et de
/// `POST /api/team/members/{id}/reset-link` : confirmation renforcee.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StepUpConfirmation {
    /// Mot de passe de l'auteur.
    #[schema(value_type = String, format = Password, max_length = 1024)]
    pub password: Zeroizing<String>,
    /// Code du second facteur (application d'authentification ou code de
    /// secours) : exige si le compte de l'auteur en a un.
    #[serde(default)]
    #[schema(value_type = Option<String>, format = Password, max_length = 64)]
    pub code: Option<Zeroizing<String>>,
}

impl fmt::Debug for StepUpConfirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepUpConfirmation")
            .field("password", &"***")
            .field("code", &self.code.as_ref().map(|_| "***"))
            .finish()
    }
}

/// Reponse de `DELETE /api/team/members/{id}`.
#[derive(Debug, Serialize, ToSchema)]
pub struct RemovedMember {
    /// Deploy hooks crees par le membre, supprimes avec lui.
    pub deleted_hooks: u64,
    /// Invitations non utilisees creees par le membre, supprimees avec lui.
    pub deleted_invitations: u64,
}

/// Invitation en attente (non utilisee, non expiree). Le lien n'est jamais
/// redonne : seul le hachage du jeton est stocke.
#[derive(Debug, Serialize, ToSchema)]
pub struct PendingInvitation {
    pub id: String,
    pub email: String,
    /// Nom propose a l'invite (vide sinon).
    pub name: String,
    pub role: String,
    /// Nom de l'invitant ; absent si son compte a ete supprime.
    pub invited_by: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
}

/// Liste les membres, du plus ancien au plus recent.
#[utoipa::path(
    get,
    path = "/api/team/members",
    tag = "team",
    responses(
        (status = 200, description = "Membres", body = Vec<TeamMember>),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "`forbidden` : Equipe : Admin requis"),
    )
)]
#[allow(dead_code)]
pub fn list_members() {}

/// Change le role d'un membre. Il faut pouvoir gerer le membre et accorder
/// le nouveau role ; les sessions du membre sont gardees, son role est relu
/// a chaque requete. Confirmation renforcee (mot de passe, plus le code du
/// second facteur si le compte en a un) ; les autres proprietaires sont
/// prevenus.
#[utoipa::path(
    patch,
    path = "/api/team/members/{id}",
    tag = "team",
    request_body = UpdateMemberRole,
    params(
        ("id" = String, Path, description = "Identifiant du membre"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 204, description = "Role change"),
        (status = 401, description = "Pas de session complete, `invalid_credentials` (mot de passe), `second_factor_required` (code du second facteur absent) ou `invalid_code`"),
        (status = 403, description = "`forbidden` (rang insuffisant), `self_forbidden` (soi-meme) ou jeton CSRF invalide"),
        (status = 404, description = "Membre introuvable"),
        (status = 409, description = "`last_owner` (il doit rester un proprietaire), `account_pending` ou `second_factor_unavailable`"),
        (status = 422, description = "Role ou requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn update_member() {}

/// Retire un membre : compte, sessions, facteurs et jetons supprimes, flux de
/// logs ouverts fermes, deploy hooks qu'il a crees supprimes. Confirmation
/// renforcee ; les autres proprietaires sont prevenus.
#[utoipa::path(
    delete,
    path = "/api/team/members/{id}",
    tag = "team",
    request_body = StepUpConfirmation,
    params(
        ("id" = String, Path, description = "Identifiant du membre"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 200, description = "Membre retire", body = RemovedMember),
        (status = 401, description = "Pas de session complete, `invalid_credentials` (mot de passe), `second_factor_required` (code du second facteur absent) ou `invalid_code`"),
        (status = 403, description = "`forbidden` (rang insuffisant), `self_forbidden` (soi-meme) ou jeton CSRF invalide"),
        (status = 404, description = "Membre introuvable"),
        (status = 409, description = "`last_owner` (il doit rester un proprietaire) ou `second_factor_unavailable`"),
        (status = 422, description = "Requete invalide"),
        (status = 429, description = "Trop de tentatives ; en-tete Retry-After"),
    )
)]
#[allow(dead_code)]
pub fn remove_member() {}

/// Liste les invitations non utilisees et non expirees, la plus recente
/// d'abord.
#[utoipa::path(
    get,
    path = "/api/team/invitations",
    tag = "team",
    responses(
        (status = 200, description = "Invitations en attente", body = Vec<PendingInvitation>),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "`forbidden` : Equipe : Admin requis"),
    )
)]
#[allow(dead_code)]
pub fn list_invitations() {}

/// Annule une invitation en attente ; le role invite ne doit pas depasser
/// celui de l'annulateur.
#[utoipa::path(
    delete,
    path = "/api/team/invitations/{id}",
    tag = "team",
    params(
        ("id" = String, Path, description = "Identifiant de l'invitation"),
        ("X-CSRF-Token" = Option<String>, Header, description = "Jeton CSRF de la session (obligatoire)"),
    ),
    responses(
        (status = 204, description = "Invitation annulee"),
        (status = 401, description = "Pas de session complete"),
        (status = 403, description = "`forbidden` (role invite au-dessus du sien) ou jeton CSRF invalide"),
        (status = 404, description = "Invitation introuvable, deja utilisee ou expiree"),
    )
)]
#[allow(dead_code)]
pub fn cancel_invitation() {}
