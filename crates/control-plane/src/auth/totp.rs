//! Double authentification par TOTP (RFC 6238) : SHA-1, 6 chiffres,
//! fenetres de 30 secondes (le reglage que toutes les applications lisent).
//!
//! - La graine (20 octets du generateur du systeme) est scellee par le coffre
//!   age de l'instance avant d'etre stockee (`user_totp.secret_sealed`).
//! - Seules la fenetre courante et la precedente sont acceptees, et seulement
//!   si elles sont posterieures a la derniere fenetre utilisee
//!   (`last_used_step`) : un code ne sert qu'une fois. L'enregistrement de la
//!   fenetre est une mise a jour conditionnelle : de deux soumissions
//!   simultanees du meme code, une seule passe.

use std::time::{SystemTime, UNIX_EPOCH};

use qrcodegen::{QrCode, QrCodeEcc};
use rand::TryRng;
use rand::rngs::SysRng;
use sqlx::{PgConnection, PgPool};
use totp_rs::{Algorithm, Builder, Totp, TotpError};
use uuid::Uuid;
use zeroize::Zeroizing;

/// Duree d'une fenetre, en secondes.
pub const STEP_SECS: u64 = 30;
pub const DIGITS: u8 = 6;
/// Emetteur affiche par l'application d'authentification.
pub const ISSUER: &str = "Ciphera";
/// Taille de la graine : 160 bits (recommandation de la RFC 4226).
const SECRET_LEN: usize = 20;

/// Nouvelle graine aleatoire.
pub fn generate_secret() -> Result<Zeroizing<Vec<u8>>, rand::rngs::SysError> {
    let mut secret = Zeroizing::new(vec![0u8; SECRET_LEN]);
    SysRng.try_fill_bytes(&mut secret)?;
    Ok(secret)
}

/// Generateur sans tolerance propre : les fenetres acceptees sont choisies
/// par `matching_step`.
fn build(secret: &[u8], account: &str) -> Result<Totp, TotpError> {
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(DIGITS)
        .with_step_duration(STEP_SECS)
        .with_skew(0)
        .with_secret(secret.to_vec())
        .with_issuer(Some(ISSUER))
        // « : » separe l'emetteur du compte dans l'URI : interdit dans le nom.
        .with_account_name(account.replace(':', "_"))
        .build()
}

/// URI `otpauth://totp/Ciphera:<compte>?secret=...&issuer=Ciphera` (contient la graine).
pub fn otpauth_uri(secret: &[u8], account: &str) -> Result<Zeroizing<String>, TotpError> {
    build(secret, account)?.to_url().map(Zeroizing::new)
}

/// Secondes depuis l'epoque Unix.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Fenetre acceptee pour `code` a l'instant `now` : la courante ou la
/// precedente, strictement posterieure a `last_used`. Comparaison a temps
/// constant (totp-rs).
pub fn matching_step(secret: &[u8], code: &str, now: u64, last_used: Option<i64>) -> Option<u64> {
    if code.len() != usize::from(DIGITS) || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let totp = build(secret, "").ok()?;
    let current = now / STEP_SECS;
    [Some(current), current.checked_sub(1)]
        .into_iter()
        .flatten()
        .filter(|step| last_used.is_none_or(|last| (*step as i64) > last))
        .find(|step| totp.check(code, step * STEP_SECS) == Some(*step))
}

/// QR code de `text` en SVG autonome (modules noirs sur fond blanc, marge de
/// 4 modules). Ni script, ni lien, ni ressource externe.
pub fn qr_svg(text: &str) -> Option<String> {
    let qr = QrCode::encode_text(text, QrCodeEcc::Medium).ok()?;
    let border = 4;
    let size = qr.size() + 2 * border;
    let mut path = String::new();
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\">\
         <rect width=\"{size}\" height=\"{size}\" fill=\"#ffffff\"/>\
         <path d=\"{path}\" fill=\"#000000\"/></svg>"
    ))
}

/// `true` si le compte a une TOTP confirmee.
pub async fn is_enabled(db: &PgPool, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL)",
    )
    .bind(user_id)
    .fetch_one(db)
    .await
}

/// Enregistre une graine en attente de confirmation, en remplacant une
/// eventuelle graine non confirmee. `false` si une TOTP est deja confirmee.
pub async fn save_pending(db: &PgPool, user_id: Uuid, sealed: &[u8]) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed) VALUES ($1, $2) \
         ON CONFLICT (user_id) DO UPDATE \
         SET secret_sealed = EXCLUDED.secret_sealed, last_used_step = NULL, created_at = now() \
         WHERE user_totp.confirmed_at IS NULL",
    )
    .bind(user_id)
    .bind(sealed)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Graine scellee et derniere fenetre utilisee de la TOTP du compte,
/// confirmee (`confirmed = true`) ou en attente.
pub async fn load(
    db: &PgPool,
    user_id: Uuid,
    confirmed: bool,
) -> Result<Option<(Vec<u8>, Option<i64>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT secret_sealed, last_used_step FROM user_totp \
         WHERE user_id = $1 AND (confirmed_at IS NOT NULL) = $2",
    )
    .bind(user_id)
    .bind(confirmed)
    .fetch_optional(db)
    .await
}

/// Confirme la graine en attente `sealed` (pas une graine remplacee entre-temps
/// par un nouveau `start`) et enregistre la fenetre utilisee.
pub async fn confirm(
    conn: &mut PgConnection,
    user_id: Uuid,
    sealed: &[u8],
    step: u64,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE user_totp SET confirmed_at = now(), last_used_step = $3 \
         WHERE user_id = $1 AND confirmed_at IS NULL AND secret_sealed = $2",
    )
    .bind(user_id)
    .bind(sealed)
    .bind(step as i64)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Enregistre la fenetre `step` si elle est posterieure a la derniere
/// utilisee. `false` : deja utilisee (rejeu ou soumission concurrente).
pub async fn consume_step(
    conn: &mut PgConnection,
    user_id: Uuid,
    step: u64,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE user_totp SET last_used_step = $2 \
         WHERE user_id = $1 AND confirmed_at IS NOT NULL \
           AND (last_used_step IS NULL OR last_used_step < $2)",
    )
    .bind(user_id)
    .bind(step as i64)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Supprime la TOTP du compte ; `true` s'il y en avait une.
pub async fn delete(conn: &mut PgConnection, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM user_totp WHERE user_id = $1")
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(done.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"12345678901234567890";

    fn code_at(t: u64) -> String {
        build(SECRET, "").unwrap().generate(t).to_string()
    }

    #[test]
    fn rfc6238_sha1_vector() {
        // RFC 6238, annexe B (8 chiffres : 94287082) ; sur 6 chiffres : 287082.
        assert_eq!(code_at(59), "287082");
    }

    #[test]
    fn current_and_previous_window_only() {
        let now = 1_000_000_020; // fenetre 33_333_334
        let step = now / STEP_SECS;
        assert_eq!(matching_step(SECRET, &code_at(now), now, None), Some(step));
        assert_eq!(
            matching_step(SECRET, &code_at(now - 30), now, None),
            Some(step - 1)
        );
        assert_eq!(matching_step(SECRET, &code_at(now - 60), now, None), None);
        assert_eq!(matching_step(SECRET, &code_at(now + 30), now, None), None);
    }

    #[test]
    fn windows_up_to_last_used_are_refused() {
        let now = 1_000_000_020;
        let step = (now / STEP_SECS) as i64;
        assert_eq!(matching_step(SECRET, &code_at(now), now, Some(step)), None);
        assert_eq!(
            matching_step(SECRET, &code_at(now - 30), now, Some(step - 1)),
            None
        );
        assert_eq!(
            matching_step(SECRET, &code_at(now), now, Some(step - 1)),
            Some(step as u64)
        );
    }

    #[test]
    fn malformed_codes_are_refused() {
        let now = 1_000_000_020;
        let good = code_at(now);
        for bad in [
            String::new(),
            good[..5].to_string(),
            format!("{good}0"),
            format!("+{}", &good[1..]),
            format!(" {}", &good[1..]),
            "abcdef".to_string(),
        ] {
            assert_eq!(matching_step(SECRET, &bad, now, None), None, "{bad:?}");
        }
    }

    #[test]
    fn uri_carries_issuer_and_secret() {
        let uri = otpauth_uri(SECRET, "a:b@example.com").unwrap();
        assert!(
            uri.starts_with("otpauth://totp/Ciphera:a_b%40example.com?"),
            "{}",
            *uri
        );
        assert!(uri.contains("secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"));
        assert!(uri.contains("issuer=Ciphera"));
    }

    #[test]
    fn qr_is_plain_svg() {
        let svg = qr_svg("otpauth://totp/Ciphera:a?secret=ABC").unwrap();
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("<path d=\"M"));
        for forbidden in ["<script", "href", "on", "<image", "<foreignObject"] {
            assert!(!svg.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn secrets_are_random_160_bits() {
        let a = generate_secret().unwrap();
        let b = generate_secret().unwrap();
        assert_eq!(a.len(), 20);
        assert_ne!(*a, *b);
    }
}
