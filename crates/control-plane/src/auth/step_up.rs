//! Confirmation renforcee (step-up) des actions les plus sensibles d'une
//! session complete (suppression de l'organisation, creation d'un deploy hook
//! ou prolongation de son expiration) : mot de passe du compte, plus le code
//! du second facteur (application d'authentification ou code de secours) si
//! le compte en a un (TOTP ou passkey).
//!
//! Limites reprises des seconds facteurs (`rate_limit::admit_mfa`) : 5 echecs
//! de mot de passe par session, codes TOTP limites par compte (et plafond
//! journalier), codes de secours par session et par IP. Le code n'est consomme
//! (fenetre TOTP marquee, code de secours utilise) que dans la transaction de
//! l'action, par [`Proof::commit`] : une action refusee plus loin ne le brule
//! pas. Ni le mot de passe ni le code ne sont journalises ou audites.

use std::net::IpAddr;

use axum::response::Response;
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{CurrentUser, internal, rate_limit, recovery, totp};
use crate::http::me::{Reauth, reauthenticate};
use crate::http::mfa::{Candidate, MAX_CODE_LEN, rate_limited};
use crate::state::AppState;

/// Ce que l'action recoit d'un step-up reussi.
pub enum StepUp {
    /// Verifie : a valider dans la transaction de l'action.
    Verified(Proof),
    /// Limite atteinte : reponse 429 prete.
    Limited(Response),
}

/// Preuve d'un step-up : tentatives a marquer reussies et code a consommer.
#[must_use = "la preuve doit etre validee dans la transaction de l'action"]
pub struct Proof {
    user_id: Uuid,
    password_attempt: i64,
    factor: Option<(i64, Candidate)>,
}

impl Proof {
    /// Marque les tentatives reussies et consomme le code, dans la
    /// transaction de l'action. Code consomme entre-temps par une autre
    /// requete : 401 `invalid_code` (l'action est annulee avec la transaction).
    pub async fn commit(self, conn: &mut PgConnection) -> Result<(), ApiError> {
        if let Some((attempt, candidate)) = self.factor {
            let consumed =
                match candidate {
                    Candidate::Step(step) => totp::consume_step(&mut *conn, self.user_id, step)
                        .await
                        .map_err(|e| internal("enregistrement de la fenetre TOTP", e))?,
                    Candidate::Code(id) => recovery::mark_used(&mut *conn, id)
                        .await
                        .map_err(|e| internal("consommation du code de secours", e))?,
                };
            if !consumed {
                return Err(invalid_code());
            }
            rate_limit::mark_mfa_success(&mut *conn, attempt)
                .await
                .map_err(|e| internal("enregistrement de la tentative", e))?;
        }
        rate_limit::mark_mfa_success(&mut *conn, self.password_attempt)
            .await
            .map_err(|e| internal("enregistrement de la tentative", e))?;
        Ok(())
    }
}

fn invalid_code() -> ApiError {
    ApiError::new(401, "invalid_code", "Code de vérification incorrect.")
}

fn code_required() -> ApiError {
    ApiError::new(
        401,
        "second_factor_required",
        "Saisissez aussi le code de votre application d'authentification ou un code de secours.",
    )
}

/// Seconds facteurs du compte : (TOTP confirmee, passkey, code de secours inutilise).
async fn factors(state: &AppState, user_id: Uuid) -> Result<(bool, bool, bool), ApiError> {
    sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL), \
                EXISTS (SELECT 1 FROM webauthn_credentials WHERE user_id = $1), \
                EXISTS (SELECT 1 FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL)",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| internal("lecture des seconds facteurs", e))
}

/// Code a 6 chiffres (espaces ignores) : code TOTP ; sinon code de secours.
fn looks_like_totp(code: &str) -> bool {
    let digits: Vec<char> = code.chars().filter(|c| !c.is_whitespace()).collect();
    digits.len() == 6 && digits.iter().all(char::is_ascii_digit)
}

/// Verifie le mot de passe puis, si le compte a un second facteur, le code.
/// Aucun essai n'est compte si le code manque alors qu'il est exige.
pub async fn verify(
    state: &AppState,
    user: &CurrentUser,
    ip: IpAddr,
    password: Zeroizing<String>,
    code: Option<Zeroizing<String>>,
    failure_action: &str,
) -> Result<StepUp, ApiError> {
    let code = code.filter(|c| !c.trim().is_empty());
    if code.as_ref().is_some_and(|c| c.len() > MAX_CODE_LEN) {
        return Err(ApiError::new(422, "validation_failed", "Requête invalide."));
    }
    let (has_totp, has_passkey, has_recovery) = factors(state, user.id).await?;
    let required = has_totp || has_passkey;
    if required && code.is_none() {
        return Err(code_required());
    }
    if required && !has_totp && !has_recovery {
        // Passkey seule, sans code de secours restant : aucun code possible.
        return Err(ApiError::new(
            409,
            "second_factor_unavailable",
            "Confirmez avec un code : activez l'application d'authentification dans votre profil.",
        ));
    }
    let ip_text = ip.to_string();
    let password_attempt =
        match reauthenticate(state, user, &ip_text, password, failure_action).await? {
            Reauth::Verified(attempt, _) => attempt,
            Reauth::Limited(response) => return Ok(StepUp::Limited(response)),
        };
    let (Some(code), true) = (code, required) else {
        return Ok(StepUp::Verified(Proof {
            user_id: user.id,
            password_attempt,
            factor: None,
        }));
    };

    let use_totp = has_totp && looks_like_totp(&code);
    let factor = if use_totp {
        rate_limit::MfaFactor::Totp
    } else {
        rate_limit::MfaFactor::Recovery
    };
    let ip_key = rate_limit::IpKey::login(ip);
    let attempt =
        match rate_limit::admit_mfa(&state.db, user.id, user.session.id, factor, Some(&ip_key))
            .await
            .map_err(|e| internal("limitation de debit", e))?
        {
            rate_limit::Admission::Allowed(id) => id,
            rate_limit::Admission::Limited(retry_after) => {
                return Ok(StepUp::Limited(rate_limited(retry_after)));
            }
        };
    let candidate = if use_totp {
        let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        crate::http::mfa::totp_candidate(state, user.id, &digits).await?
    } else {
        crate::http::mfa::recovery_candidate(state, user.id, &code, attempt).await?
    };
    let Some(candidate) = candidate else {
        let mut conn = state
            .db
            .acquire()
            .await
            .map_err(|e| internal("connexion a la base", e))?;
        crate::http::auth::audit(
            &mut conn,
            Some(user.id),
            failure_action,
            json!({ "ip": ip_text, "reason": "invalid_code" }),
        )
        .await
        .map_err(|e| internal("audit du step-up", e))?;
        return Err(invalid_code());
    };
    Ok(StepUp::Verified(Proof {
        user_id: user.id,
        password_attempt,
        factor: Some((attempt, candidate)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_digits_are_a_totp_code() {
        assert!(looks_like_totp("123456"));
        assert!(looks_like_totp("123 456"));
        assert!(!looks_like_totp("12345"));
        assert!(!looks_like_totp("ABCDE-FGHIJ"));
        assert!(!looks_like_totp("12345a"));
    }
}
