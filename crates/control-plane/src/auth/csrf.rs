//! Protection CSRF : en-tete `X-CSRF-Token` obligatoire sur toute requete
//! d'ecriture (methode autre que GET, HEAD, OPTIONS) sous `/api/`, sauf les
//! routes de `EXEMPT`.
//!
//! Le jeton est lie a la session : HMAC-SHA256 dont la cle est le jeton de
//! session lui-meme (32 octets aleatoires, jamais stockes ; la base n'en
//! garde que le SHA-256). Il change donc avec la session (connexion, MFA),
//! ne se deduit pas d'une fuite de la base, et se recalcule sans etat a
//! chaque requete. Il est renvoye par `POST /api/auth/login` et `GET /api/me`.
//! Le cookie `__Host-` et `SameSite=Strict` empechent un site tiers ou une app
//! d'un sous-domaine voisin de poser ou d'envoyer le cookie.

use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use shared::error::ApiError;
use subtle::ConstantTimeEq;

use super::session;

pub const HEADER: &str = "x-csrf-token";
const CONTEXT: &[u8] = b"ciphera-csrf-v1";

enum Rule {
    /// Ce chemin exactement.
    Exact(&'static str),
    /// Tout chemin qui commence ainsi (terminer par `/`).
    Prefix(&'static str),
    /// `<prefixe><un segment non vide><suffixe>` (ex. le jeton d'une invitation).
    Segment(&'static str, &'static str),
}

/// Routes d'ecriture exemptees, toutes au meme endroit, au plus pres des
/// endpoints (pas de prefixe large sous `/api/auth/`). Elles precedent
/// l'existence d'une session complete ou portent leur propre preuve :
/// - `/api/setup/*` : en-tete `X-Setup-Token` ;
/// - connexion, inscription, acceptation d'une invitation, mot de passe
///   oublie et sa reinitialisation, confirmation d'une adresse e-mail (le lien
///   porte sa preuve) : aucune session encore ;
/// - verification de la double authentification (TOTP, code de secours,
///   assertion WebAuthn) : session en attente de MFA. L'enrolement MFA d'un
///   compte connecte, lui, reste protege ;
/// - `/v1/webhooks/*`, `/v1/hooks/*` : appels de machines, signes (HMAC) ou
///   portant un secret d'URL. Hors de `/api/`, ils ne sont de toute facon pas
///   controles ; listes ici pour qu'aucune extension future ne les oublie.
const EXEMPT: &[Rule] = &[
    Rule::Prefix("/api/setup/"),
    Rule::Exact("/api/auth/login"),
    Rule::Exact("/api/auth/register"),
    Rule::Segment("/api/auth/invitations/", "/accept"),
    Rule::Segment("/api/auth/email-verification/", "/confirm"),
    Rule::Exact("/api/auth/password/forgot"),
    Rule::Exact("/api/auth/password/reset"),
    Rule::Exact("/api/auth/mfa/verify"),
    Rule::Exact("/api/auth/recovery-code"),
    Rule::Exact("/api/auth/webauthn/assert/options"),
    Rule::Exact("/api/auth/webauthn/assert/verify"),
    Rule::Prefix("/v1/webhooks/"),
    Rule::Prefix("/v1/hooks/"),
];

/// Jeton CSRF de la session dont le jeton (valeur du cookie) est donne.
pub fn token_for(session_token: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(session_token.as_bytes()).expect("HMAC accepte toute cle");
    mac.update(CONTEXT);
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

pub fn is_exempt(path: &str) -> bool {
    EXEMPT.iter().any(|rule| match rule {
        Rule::Exact(p) => path == *p,
        Rule::Prefix(p) => path.starts_with(p),
        Rule::Segment(prefix, suffix) => path
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
            .is_some_and(|segment| !segment.is_empty() && !segment.contains('/')),
    })
}

/// `true` si la requete doit porter un jeton CSRF valide.
pub fn is_protected(method: &Method, path: &str) -> bool {
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    !safe && path.starts_with("/api/") && !is_exempt(path)
}

/// Compare a temps constant le jeton fourni a celui de la session.
fn valid(session_token: Option<&str>, given: Option<&str>) -> bool {
    let (Some(session_token), Some(given)) = (session_token, given) else {
        return false;
    };
    let expected = token_for(session_token);
    // ct_eq renvoie faux (sans court-circuit sur le contenu) si les longueurs different.
    bool::from(expected.as_bytes().ct_eq(given.as_bytes()))
}

/// Couche axum : 403 `csrf` si le jeton manque ou ne correspond pas.
pub async fn layer(request: Request, next: Next) -> Response {
    if is_protected(request.method(), request.uri().path()) {
        let headers = request.headers();
        let given = headers.get(HEADER).and_then(|v| v.to_str().ok());
        if !valid(session::token_from_headers(headers), given) {
            return ApiError::new(403, "csrf", "Jeton CSRF absent ou invalide.").into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_bound_to_session() {
        let a = token_for("session-a");
        assert_eq!(a, token_for("session-a"));
        assert_ne!(a, token_for("session-b"));
        assert_eq!(a.len(), 43);
        assert!(valid(Some("session-a"), Some(&a)));
        assert!(!valid(Some("session-b"), Some(&a)));
        assert!(!valid(None, Some(&a)));
        assert!(!valid(Some("session-a"), None));
        assert!(!valid(Some("session-a"), Some("")));
    }

    #[test]
    fn writes_under_api_are_protected_except_exempt_routes() {
        for path in [
            "/api/auth/logout",
            "/api/projects",
            "/api/auth/login/",
            "/api/auth/loginx",
            "/api/setup",
            "/api/auth/mfa",
            "/api/auth/mfa/setup",
            "/api/auth/mfa/verify/x",
            "/api/auth/recovery-code/x",
            "/api/auth/webauthn/assert/other",
            "/api/auth/webauthn/register/verify",
            "/api/auth/invitations/x/other",
            "/api/auth/invitations//accept",
            "/api/auth/invitations/a/b/accept",
            "/api/auth/invitations/accept",
            "/api/auth/email-verification/confirm",
            "/api/auth/email-verification/a/b/confirm",
        ] {
            assert!(is_protected(&Method::POST, path), "{path}");
        }
        for path in [
            "/api/setup/complete",
            "/api/auth/login",
            "/api/auth/register",
            "/api/auth/invitations/abc/accept",
            "/api/auth/email-verification/abc/confirm",
            "/api/auth/password/forgot",
            "/api/auth/password/reset",
            "/api/auth/mfa/verify",
            "/api/auth/recovery-code",
            "/api/auth/webauthn/assert/options",
            "/api/auth/webauthn/assert/verify",
            "/v1/webhooks/github",
            "/v1/hooks/abc",
        ] {
            assert!(!is_protected(&Method::POST, path), "{path}");
        }
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(!is_protected(&method, "/api/me"));
        }
        for method in [Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(is_protected(&method, "/api/projects/1"));
        }
        assert!(!is_protected(&Method::POST, "/"), "hors API");
    }
}
