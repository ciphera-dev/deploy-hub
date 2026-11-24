//! Authentification : mots de passe, sessions, CSRF, limitation de debit,
//! double authentification (TOTP, codes de secours, passkeys), et l'extracteur
//! `CurrentUser` des routes reservees aux comptes connectes.

pub mod csrf;
pub mod factors;
pub mod password;
pub mod rate_limit;
pub mod recovery;
pub mod session;
pub mod totp;
pub mod webauthn;

use axum::extract::FromRequestParts;
use axum::http::Method;
use axum::http::request::Parts;
use shared::error::ApiError;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::state::AppState;

/// Routes ouvertes a une session restreinte a l'enrolement (2FA imposee par
/// l'organisation, compte sans 2FA). `POST /api/auth/logout` n'exige pas de
/// `CurrentUser` et reste donc ouvert aussi.
const ENROLLMENT_ROUTES: &[(Method, &str)] = &[
    (Method::GET, "/api/me"),
    (Method::POST, "/api/me/mfa/totp/start"),
    (Method::POST, "/api/me/mfa/totp/confirm"),
    (Method::POST, "/api/me/webauthn/register/options"),
    (Method::POST, "/api/me/webauthn/register/verify"),
];

fn open_during_enrollment(method: &Method, path: &str) -> bool {
    ENROLLMENT_ROUTES
        .iter()
        .any(|(m, p)| m == method && *p == path)
}

/// Compte de la session courante (session complete : MFA faite si requise ;
/// une session restreinte a l'enrolement n'atteint que `ENROLLMENT_ROUTES`).
#[derive(Debug)]
pub struct CurrentUser {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub role: String,
    pub session: session::Session,
    csrf_token: String,
}

impl CurrentUser {
    /// Jeton CSRF de la session, a renvoyer dans `X-CSRF-Token`.
    pub fn csrf_token(&self) -> &str {
        &self.csrf_token
    }
}

pub fn unauthenticated() -> ApiError {
    ApiError::new(401, "unauthenticated", "Authentification requise.")
}

pub fn internal(context: &str, error: impl std::fmt::Display) -> ApiError {
    // Le detail va dans le journal du serveur, jamais dans la reponse.
    tracing::error!(%error, "{context}");
    ApiError::new(500, "internal_error", "Erreur interne du serveur.")
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = session::token_from_headers(&parts.headers)
            .map(|t| Zeroizing::new(t.to_string()))
            .ok_or_else(unauthenticated)?;
        let session = session::lookup(&state.db, &token)
            .await
            .map_err(|e| internal("lecture de la session", e))?
            .ok_or_else(unauthenticated)?;
        if session.mfa_pending {
            return Err(ApiError::new(
                401,
                "mfa_required",
                "Vérification en deux étapes requise.",
            ));
        }
        if session.enrollment_required && !open_during_enrollment(&parts.method, parts.uri.path()) {
            return Err(ApiError::new(
                403,
                "mfa_enrollment_required",
                "Votre organisation impose la vérification en deux étapes : activez-la pour continuer.",
            ));
        }
        let (name, email, role): (String, String, String) =
            sqlx::query_as("SELECT name, email, role FROM users WHERE id = $1")
                .bind(session.user_id)
                .fetch_optional(&state.db)
                .await
                .map_err(|e| internal("lecture du compte", e))?
                .ok_or_else(unauthenticated)?;
        Ok(CurrentUser {
            id: session.user_id,
            name,
            email,
            role,
            session,
            csrf_token: csrf::token_for(&token),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrollment_session_reaches_only_enrollment_routes() {
        assert!(open_during_enrollment(&Method::GET, "/api/me"));
        assert!(open_during_enrollment(
            &Method::POST,
            "/api/me/mfa/totp/start"
        ));
        assert!(open_during_enrollment(
            &Method::POST,
            "/api/me/mfa/totp/confirm"
        ));
        assert!(open_during_enrollment(
            &Method::POST,
            "/api/me/webauthn/register/options"
        ));
        assert!(open_during_enrollment(
            &Method::POST,
            "/api/me/webauthn/register/verify"
        ));
        assert!(!open_during_enrollment(&Method::GET, "/api/me/webauthn"));
        assert!(!open_during_enrollment(&Method::DELETE, "/api/me/mfa/totp"));
        assert!(!open_during_enrollment(&Method::POST, "/api/me"));
        assert!(!open_during_enrollment(&Method::GET, "/api/me/"));
        assert!(!open_during_enrollment(&Method::GET, "/api/projects"));
        assert!(!open_during_enrollment(
            &Method::POST,
            "/api/me/mfa/totp/start/x"
        ));
    }
}
