//! Sessions : jeton aleatoire de 32 octets remis au navigateur en cookie ;
//! seule l'empreinte SHA-256 est stockee en base.
//!
//! Duree : 12 heures et cookie de session du navigateur par defaut ; 30 jours
//! et cookie persistant avec « rester connecte » (`persistent`). Le jeton
//! change a chaque connexion (l'ancienne session presentee est supprimee) et
//! apres la double authentification (`rotate`), contre la fixation de session.

use axum::http::HeaderMap;
use axum::http::header::COOKIE;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;
use zeroize::Zeroizing;

/// `__Host-` : le navigateur exige `Secure`, `Path=/` et aucun `Domain`.
pub const COOKIE_NAME: &str = "__Host-ciphera_session";
/// Duree d'une session ordinaire, en secondes (12 heures).
pub const SHORT_TTL_SECS: i64 = 12 * 3600;
/// Duree d'une session « rester connecte », en secondes (30 jours).
pub const LONG_TTL_SECS: i64 = 30 * 24 * 3600;

/// Duree de vie cote serveur.
pub fn ttl_secs(persistent: bool) -> i64 {
    if persistent {
        LONG_TTL_SECS
    } else {
        SHORT_TTL_SECS
    }
}

/// Jeton de session en clair : a ne jamais journaliser ni stocker.
pub struct SessionToken(Zeroizing<String>);

impl SessionToken {
    /// 32 octets du generateur du systeme, en base64 URL sans remplissage.
    pub fn generate() -> Result<Self, rand::rngs::SysError> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        SysRng.try_fill_bytes(bytes.as_mut())?;
        Ok(Self(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref()))))
    }

    /// Valeur en clair (cookie, derivation du jeton CSRF).
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Empreinte stockee en base.
    pub fn hash(&self) -> Vec<u8> {
        hash_token(&self.0)
    }

    /// Valeur de l'en-tete `Set-Cookie`. Sans `persistent`, pas de `Max-Age` :
    /// le cookie disparait a la fermeture du navigateur.
    pub fn set_cookie(&self, persistent: bool) -> String {
        let base = format!(
            "{COOKIE_NAME}={}; HttpOnly; Secure; SameSite=Strict; Path=/",
            self.expose()
        );
        if persistent {
            format!("{base}; Max-Age={LONG_TTL_SECS}")
        } else {
            base
        }
    }
}

/// `Set-Cookie` qui efface le cookie de session.
pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0")
}

/// SHA-256 d'un jeton presente par le navigateur.
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Valeur du cookie de session dans les en-tetes `Cookie` (vide = absent).
pub fn token_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

/// Session valide (non expiree, rattachee a un compte).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    /// Mot de passe verifie, double authentification attendue.
    pub mfa_pending: bool,
    /// L'organisation impose la 2FA et le compte n'en a pas : enrolement seulement.
    pub enrollment_required: bool,
    pub persistent: bool,
    /// Expiration, en secondes depuis l'epoque Unix.
    pub expires_at: i64,
}

/// Parametres d'une nouvelle session.
#[derive(Debug, Default)]
pub struct NewSession {
    pub mfa_pending: bool,
    pub enrollment_required: bool,
    pub persistent: bool,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Enregistre une session pour `user_id` (dans la transaction de l'appelant).
pub async fn create(
    conn: &mut PgConnection,
    user_id: Uuid,
    token: &SessionToken,
    new: NewSession,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, token_hash, expires_at, mfa_pending, enrollment_required, persistent, ip, user_agent) \
         VALUES ($1, $2, $3, now() + make_interval(secs => $4), $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(user_id)
    .bind(token.hash())
    .bind(ttl_secs(new.persistent) as f64)
    .bind(new.mfa_pending)
    .bind(new.enrollment_required)
    .bind(new.persistent)
    .bind(new.ip)
    .bind(new.user_agent)
    .execute(conn)
    .await?;
    Ok(id)
}

/// Session valide pour ce jeton ; rafraichit `last_seen_at` (au plus une fois par minute).
pub async fn lookup(db: &PgPool, token: &str) -> Result<Option<Session>, sqlx::Error> {
    let row: Option<(Uuid, Uuid, bool, bool, bool, i64)> = sqlx::query_as(
        "WITH s AS ( \
             SELECT id, user_id, mfa_pending, enrollment_required, persistent, \
                    extract(epoch FROM expires_at)::bigint AS expires_at \
             FROM sessions \
             WHERE token_hash = $1 AND expires_at > now() AND user_id IS NOT NULL \
         ), touched AS ( \
             UPDATE sessions SET last_seen_at = now() FROM s \
             WHERE sessions.id = s.id AND sessions.last_seen_at < now() - interval '1 minute' \
         ) \
         SELECT id, user_id, mfa_pending, enrollment_required, persistent, expires_at FROM s",
    )
    .bind(hash_token(token))
    .fetch_optional(db)
    .await?;
    Ok(row.map(
        |(id, user_id, mfa_pending, enrollment_required, persistent, expires_at)| Session {
            id,
            user_id,
            mfa_pending,
            enrollment_required,
            persistent,
            expires_at,
        },
    ))
}

/// Remplace la session par une nouvelle (nouveau jeton, nouvel identifiant,
/// duree recalculee), en une seule instruction. Sert apres la double
/// authentification (`mfa_pending = false`) et apres l'enrolement TOTP. La
/// nouvelle session n'est jamais restreinte a l'enrolement.
pub async fn rotate(
    conn: &mut PgConnection,
    current: &Session,
    mfa_pending: bool,
) -> Result<SessionToken, sqlx::Error> {
    let token = SessionToken::generate().map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
    let inserted = sqlx::query(
        "WITH old AS (DELETE FROM sessions WHERE id = $1 RETURNING user_id, persistent, ip, user_agent) \
         INSERT INTO sessions (id, user_id, token_hash, expires_at, mfa_pending, persistent, ip, user_agent) \
         SELECT $2, user_id, $3, \
                now() + make_interval(secs => CASE WHEN persistent THEN $4 ELSE $5 END), \
                $6, persistent, ip, user_agent \
         FROM old",
    )
    .bind(current.id)
    .bind(Uuid::new_v4())
    .bind(token.hash())
    .bind(LONG_TTL_SECS as f64)
    .bind(SHORT_TTL_SECS as f64)
    .bind(mfa_pending)
    .execute(conn)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(token)
}

/// Supprime la session de ce jeton ; renvoie le compte concerne s'il y en avait une.
pub async fn delete_by_token(
    conn: &mut PgConnection,
    token: &str,
) -> Result<Option<Option<Uuid>>, sqlx::Error> {
    sqlx::query_scalar("DELETE FROM sessions WHERE token_hash = $1 RETURNING user_id")
        .bind(hash_token(token))
        .fetch_optional(conn)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn cookie_has_host_prefix_and_strict_attributes() {
        let t = SessionToken::generate().unwrap();
        for persistent in [false, true] {
            let c = t.set_cookie(persistent);
            assert!(c.starts_with("__Host-ciphera_session="));
            for attr in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
                assert!(c.contains(attr));
            }
            assert!(!c.contains("Domain"));
            assert_eq!(c.contains("Max-Age=2592000"), persistent);
        }
        assert!(clear_cookie().contains("Max-Age=0"));
    }

    #[test]
    fn tokens_are_unique_and_hash_is_sha256() {
        let a = SessionToken::generate().unwrap();
        let b = SessionToken::generate().unwrap();
        assert_ne!(a.expose(), b.expose());
        assert_eq!(a.expose().len(), 43);
        assert_eq!(a.hash().len(), 32);
        assert_eq!(a.hash(), hash_token(a.expose()));
    }

    #[test]
    fn cookie_is_read_among_others() {
        let mut h = HeaderMap::new();
        assert_eq!(token_from_headers(&h), None);
        h.append(COOKIE, HeaderValue::from_static("a=1; b=2"));
        h.append(
            COOKIE,
            HeaderValue::from_static("x=y; __Host-ciphera_session=abc ;z=1"),
        );
        assert_eq!(token_from_headers(&h), Some("abc"));
        let mut h = HeaderMap::new();
        h.insert(COOKIE, HeaderValue::from_static("__Host-ciphera_session="));
        assert_eq!(token_from_headers(&h), None);
        let mut h = HeaderMap::new();
        h.insert(COOKIE, HeaderValue::from_static("ciphera_session=abc"));
        assert_eq!(token_from_headers(&h), None);
    }
}
