//! Parametres › Notifications : serveur SMTP de l'instance.
//! `GET /api/settings/notifications` (Parametres : Admin requis, lecture
//! seule) et `PUT` (proprietaire seulement).
//!
//! Le mot de passe SMTP est en ecriture seule : chiffre par le coffre de
//! l'instance, jamais renvoye (seulement `password_set`), conserve quand un
//! `PUT` n'en fournit pas. Chiffrement obligatoire (`starttls` ou `tls`).

use api_types::notifications::{NotificationSettings, UpdateNotificationSettings};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use std::net::IpAddr;
use uuid::Uuid;

use crate::auth::internal;
use crate::http::client_ip::ClientIp;
use crate::http::settings::{Admin, owner_only};
use crate::http::setup::validate_email;
use crate::mail::{self, Mailer, TlsMode};
use crate::state::AppState;

const MAX_BODY: usize = 16 * 1024;
const MAX_HOST_LEN: usize = 253;
const MAX_USERNAME_CHARS: usize = 256;
const MAX_PASSWORD_CHARS: usize = 1024;
const MAX_FROM_NAME_CHARS: usize = 100;
/// Port propose quand rien n'est enregistre (STARTTLS).
const DEFAULT_PORT: u16 = 587;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/settings/notifications", get(show).put(update))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message.to_string())
}

/// Nom d'hote DNS : etiquettes de 1 a 63 caracteres `[A-Za-z0-9-]`, sans
/// tiret au debut ni a la fin ; 253 caracteres au plus.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_HOST_LEN
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// Texte sans caractere de controle (pas d'injection d'en-tete ni de commande SMTP).
fn clean_text(value: &str, max_chars: usize) -> bool {
    value.chars().count() <= max_chars && !value.chars().any(char::is_control)
}

async fn current(state: &AppState) -> Result<Response, ApiError> {
    type Row = (bool, String, i32, String, String, bool, String, String);
    let row: Option<Row> = sqlx::query_as(
        "SELECT enabled, host, port, tls, username, password_sealed IS NOT NULL, \
                from_address, from_name \
         FROM smtp_settings",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture des reglages SMTP", e))?;
    let email_available = mail::available(state)
        .await
        .map_err(|e| internal("lecture des reglages SMTP", e))?;
    let public_url = state.public_url();
    let body = match row {
        Some((enabled, host, port, tls, username, password_set, from_address, from_name)) => {
            NotificationSettings {
                configured: true,
                enabled,
                host,
                port: u16::try_from(port).unwrap_or(DEFAULT_PORT),
                tls,
                username,
                password_set,
                from_address,
                from_name,
                public_url,
                email_available,
            }
        }
        None => NotificationSettings {
            configured: false,
            enabled: true,
            host: String::new(),
            port: DEFAULT_PORT,
            tls: TlsMode::StartTls.as_str().to_string(),
            username: String::new(),
            password_set: false,
            from_address: String::new(),
            from_name: "Ciphera".to_string(),
            public_url,
            email_available,
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// `GET /api/settings/notifications`
async fn show(State(state): State<AppState>, _auth: Admin) -> Result<Response, ApiError> {
    current(&state).await
}

/// `PUT /api/settings/notifications`
async fn update(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    auth: Admin,
    body: Result<Json<UpdateNotificationSettings>, JsonRejection>,
) -> Result<Response, ApiError> {
    owner_only(&auth)?;
    let Json(input) = body.map_err(|_| invalid("Requête invalide."))?;
    let tls = TlsMode::parse(&input.tls)
        .ok_or_else(|| invalid("Chiffrement : STARTTLS ou SSL/TLS uniquement."))?;
    if input.port == 0 {
        return Err(invalid("Port invalide."));
    }
    let host = input.host.trim().to_ascii_lowercase();
    // Avant la validation du nom : `::1` ou `[::1]` donnent le meme refus.
    refuse_private_host(&state, &host).await?;
    if !valid_host(&host) {
        return Err(invalid("Serveur SMTP : nom d'hôte invalide."));
    }
    let username = input.username.trim().to_string();
    if !clean_text(&username, MAX_USERNAME_CHARS) {
        return Err(invalid("Utilisateur SMTP invalide."));
    }
    let from_address = input.from_address.trim().to_string();
    if !validate_email(&from_address) {
        return Err(invalid("Adresse d'expédition invalide."));
    }
    let from_name = input.from_name.trim().to_string();
    if from_name.is_empty() || !clean_text(&from_name, MAX_FROM_NAME_CHARS) {
        return Err(invalid("Nom affiché : 1 à 100 caractères."));
    }
    // Absent ou vide : le mot de passe enregistre est conserve.
    let password = input.password.filter(|p| !p.is_empty());
    let sealed = match &password {
        Some(p) if !clean_text(p, MAX_PASSWORD_CHARS) => {
            return Err(invalid("Mot de passe SMTP invalide."));
        }
        Some(p) => Some(
            state
                .secrets
                .seal_bound(crate::secrets::labels::SMTP_PASSWORD, p.as_bytes())
                .map_err(|e| internal("chiffrement du mot de passe SMTP", e))?,
        ),
        None => None,
    };

    // Ancien transport (meilleur effort) : sert a prevenir les proprietaires.
    let previous = match mail::load_settings(&state.db, &state.secrets).await {
        Ok(settings) => settings,
        Err(e) => {
            tracing::warn!(error = %e, "anciens reglages SMTP illisibles");
            None
        }
    };

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction des reglages SMTP", e))?;
    // Le mot de passe enregistre n'est conserve que pour le meme serveur, le
    // meme port et le meme utilisateur : sinon il pourrait etre envoye a un
    // serveur controle par un tiers. Ligne verrouillee jusqu'a l'ecriture.
    if sealed.is_none() && !username.is_empty() {
        let stored: Option<(String, i32, String, bool)> = sqlx::query_as(
            "SELECT host, port, username, password_sealed IS NOT NULL FROM smtp_settings FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| internal("lecture des reglages SMTP", e))?;
        if let Some((stored_host, stored_port, stored_username, true)) = stored
            && (stored_host != host
                || stored_port != i32::from(input.port)
                || stored_username != username)
        {
            return Err(invalid(
                "Ressaisissez le mot de passe SMTP pour changer de serveur, de port ou d'utilisateur.",
            ));
        }
    }
    // Sans utilisateur, pas d'authentification : le mot de passe est efface.
    let missing_password: bool = sqlx::query_scalar(
        "INSERT INTO smtp_settings (id, enabled, host, port, tls, username, password_sealed, \
                                    from_address, from_name, updated_at, updated_by) \
         VALUES (true, $1, $2, $3, $4, $5, $6, $7, $8, now(), $9) \
         ON CONFLICT (id) DO UPDATE SET \
             enabled = EXCLUDED.enabled, host = EXCLUDED.host, port = EXCLUDED.port, \
             tls = EXCLUDED.tls, username = EXCLUDED.username, \
             password_sealed = coalesce(EXCLUDED.password_sealed, smtp_settings.password_sealed), \
             from_address = EXCLUDED.from_address, from_name = EXCLUDED.from_name, \
             updated_at = now(), updated_by = EXCLUDED.updated_by \
         RETURNING username <> '' AND password_sealed IS NULL",
    )
    .bind(input.enabled)
    .bind(&host)
    .bind(i32::from(input.port))
    .bind(tls.as_str())
    .bind(&username)
    .bind(&sealed)
    .bind(&from_address)
    .bind(&from_name)
    .bind(auth.user.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| internal("enregistrement des reglages SMTP", e))?;
    if missing_password {
        return Err(invalid("Mot de passe SMTP requis avec un utilisateur."));
    }
    if username.is_empty() {
        sqlx::query("UPDATE smtp_settings SET password_sealed = NULL")
            .execute(&mut *tx)
            .await
            .map_err(|e| internal("enregistrement des reglages SMTP", e))?;
    }
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, $2, 'settings.notifications.updated', 'settings', $3)",
    )
    .bind(Uuid::new_v4())
    .bind(auth.user.id)
    .bind(json!({
        "ip": ip.to_string(),
        "enabled": input.enabled,
        "host": host,
        "port": input.port,
        "tls": tls.as_str(),
        "password_changed": sealed.is_some(),
    }))
    .execute(&mut *tx)
    .await
    .map_err(|e| internal("audit des reglages SMTP", e))?;
    tx.commit()
        .await
        .map_err(|e| internal("transaction des reglages SMTP", e))?;
    if let Some(previous) = previous
        && (previous.host != host
            || previous.port != input.port
            || previous.from_address != from_address)
    {
        notify_owners(
            &state,
            &previous,
            &auth.user.email,
            (&host, input.port, &from_address),
        )
        .await;
    }
    current(&state).await
}

/// Refuse un serveur qui vise la machine ou le reseau prive : `localhost`,
/// adresse litterale ou resolue (boucle locale, privee, lien-local, ULA).
async fn refuse_private_host(state: &AppState, host: &str) -> Result<(), ApiError> {
    let refused = || {
        ApiError::new(
            422,
            "smtp_host_private",
            "Serveur SMTP : les adresses locales ou privées sont refusées.",
        )
    };
    let bare = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.');
    if bare == "localhost" || bare.ends_with(".localhost") {
        return Err(refused());
    }
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return if mail::is_private_ip(ip) {
            Err(refused())
        } else {
            Ok(())
        };
    }
    if !valid_host(bare) {
        return Ok(());
    }
    // Nom : chaque adresse resolue doit etre publique. Un nom inconnu n'a
    // aucune adresse (le resolveur renvoie alors une liste vide).
    match state.dns.resolve(bare).await {
        Ok(ips) if ips.iter().any(|ip| mail::is_private_ip(*ip)) => Err(refused()),
        Ok(ips) if !ips.is_empty() => Ok(()),
        // Nom inconnu ou panne du resolveur : pas de serveur utilisable.
        outcome => {
            if let Err(e) = outcome {
                tracing::warn!(error = %e, "resolution du serveur SMTP impossible");
            }
            Err(ApiError::new(
                422,
                "smtp_host_unresolved",
                "Serveur SMTP : le nom ne se résout pas (vérifiez l'orthographe ou réessayez).",
            ))
        }
    }
}

/// Previent les proprietaires par l'ancien transport (meilleur effort :
/// non configure, desactive ou injoignable, l'echec est seulement journalise).
async fn notify_owners(
    state: &AppState,
    previous: &mail::SmtpSettings,
    actor: &str,
    new: (&str, u16, &str),
) {
    let Some(mailer) = Mailer::from_settings(previous, state.dns.clone()) else {
        tracing::info!("reglages SMTP modifies : ancien transport inactif, aucun avis");
        return;
    };
    let owners: Vec<String> =
        match sqlx::query_scalar("SELECT email FROM users WHERE role = 'owner'")
            .fetch_all(&state.db)
            .await
        {
            Ok(owners) => owners,
            Err(e) => {
                tracing::warn!(error = %e, "proprietaires illisibles : avis SMTP non envoye");
                return;
            }
        };
    for to in owners {
        let message = mail::smtp_changed_mail(
            &to,
            actor,
            (&previous.host, previous.port, &previous.from_address),
            new,
        );
        state.mail.spawn(mailer.clone(), message, "smtp_changed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_dns_names() {
        for ok in ["smtp.example.com", "ssl0.ovh.net", "localhost", "10.0.0.5"] {
            assert!(valid_host(ok), "{ok}");
        }
        for bad in [
            "",
            "smtp example.com",
            "smtp.example.com\r\n",
            "-smtp.example.com",
            "smtp..example.com",
            "smtp.example.com.",
            "smtp_example.com",
            "[::1]",
            "user@smtp.example.com",
            &"a".repeat(64),
        ] {
            assert!(!valid_host(bad), "{bad}");
        }
    }

    #[test]
    fn control_characters_are_refused() {
        assert!(clean_text("Ciphera", 100));
        assert!(!clean_text("Ciphera\r\nBcc: x", 100));
        assert!(!clean_text(&"x".repeat(101), 100));
    }
}
