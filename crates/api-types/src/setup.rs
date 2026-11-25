//! Assistant d'installation : `/api/setup/*`.

use std::fmt;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use zeroize::Zeroizing;

/// Reponse de `GET /api/setup/status`.
#[derive(Debug, Serialize, ToSchema)]
pub struct SetupStatus {
    /// `true` des qu'un compte existe : l'assistant est alors verrouille.
    pub initialized: bool,
}

/// Une verification locale du serveur.
#[derive(Debug, Serialize, ToSchema)]
pub struct SetupCheck {
    /// Identifiant stable : `database`, `tls`, `data_dir`, `docker`.
    pub id: String,
    /// `false` : la verification echoue et bloque l'installation.
    pub ok: bool,
    /// `true` : verification reussie mais a surveiller (ne bloque pas).
    pub warning: bool,
    /// Detail lisible, en francais.
    pub detail: String,
}

/// Reponse de `POST /api/setup/checks`.
#[derive(Debug, Serialize, ToSchema)]
pub struct SetupChecks {
    pub checks: Vec<SetupCheck>,
}

/// Premier compte : il recoit le role `owner`.
#[derive(Deserialize, ToSchema)]
pub struct SetupAdmin {
    pub name: String,
    pub email: String,
    /// 12 caracteres au minimum, 1024 au maximum. Jamais renvoye ni journalise,
    /// efface de la memoire apres usage.
    #[schema(value_type = String, format = Password, min_length = 12, max_length = 1024)]
    pub password: Zeroizing<String>,
}

// Pas de derive : le mot de passe ne doit jamais apparaitre dans un journal.
impl fmt::Debug for SetupAdmin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupAdmin")
            .field("name", &self.name)
            .field("email", &self.email)
            .field("password", &"***")
            .finish()
    }
}

/// Organisation de l'instance.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetupOrganization {
    pub name: String,
    /// Minuscules, chiffres et tirets.
    pub slug: String,
    /// Fuseau IANA, ex. `Europe/Paris`.
    pub timezone: String,
    /// `fr` ou `en`.
    pub language: String,
}

/// Corps de `POST /api/setup/complete`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SetupComplete {
    pub admin: SetupAdmin,
    pub organization: SetupOrganization,
}

/// Compte cree par l'assistant.
#[derive(Debug, Serialize, ToSchema)]
pub struct SetupUser {
    pub id: String,
    pub name: String,
    pub email: String,
    /// Toujours `owner`.
    pub role: String,
}

/// Reponse 201 de `POST /api/setup/complete` (le cookie de session est dans `Set-Cookie`).
#[derive(Debug, Serialize, ToSchema)]
pub struct SetupCompleted {
    pub user: SetupUser,
}

/// Etat de l'assistant (sans authentification).
#[utoipa::path(
    get,
    path = "/api/setup/status",
    tag = "setup",
    responses((status = 200, description = "Etat de l'assistant", body = SetupStatus))
)]
#[allow(dead_code)]
pub fn status() {}

/// Verifications locales du serveur (jeton de setup requis).
#[utoipa::path(
    post,
    path = "/api/setup/checks",
    tag = "setup",
    params(("X-Setup-Token" = String, Header, description = "Jeton de setup imprime par `ciphera init`")),
    responses(
        (status = 200, description = "Resultat des verifications", body = SetupChecks),
        (status = 401, description = "`invalid_setup_token`, ou `setup_token_expired` (24 h passees : `ciphera admin setup-token`)"),
        (status = 410, description = "Assistant deja termine"),
        (status = 503, description = "Fichier du jeton absent"),
    )
)]
#[allow(dead_code)]
pub fn checks() {}

/// Cree le compte proprietaire et l'organisation (usage unique).
#[utoipa::path(
    post,
    path = "/api/setup/complete",
    tag = "setup",
    params(("X-Setup-Token" = String, Header, description = "Jeton de setup imprime par `ciphera init`")),
    request_body = SetupComplete,
    responses(
        (status = 201, description = "Compte cree ; cookie de session pose", body = SetupCompleted),
        (status = 401, description = "`invalid_setup_token`, ou `setup_token_expired` (24 h passees : `ciphera admin setup-token`)"),
        (status = 410, description = "Assistant deja termine"),
        (status = 422, description = "Champ invalide"),
        (status = 503, description = "Fichier du jeton absent"),
    )
)]
#[allow(dead_code)]
pub fn complete() {}
