//! Deploy hooks : secret `dh_…`, empreinte stockee, verification a temps
//! constant et acces SQL.
//!
//! Le secret (32 octets du generateur du systeme, base64 URL) n'est montre
//! qu'a la creation ; la base n'en garde que le SHA-256, comme les jetons de
//! session (`session::hash_token`). La verification compare toujours une
//! empreinte de 32 octets, meme pour un hook inconnu (empreinte nulle) :
//! hook inconnu et secret faux suivent le meme chemin.

use api_types::project_git::DeployHook;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use sqlx::PgConnection;
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::auth::session::hash_token;

/// Prefixe des secrets de deploy hook.
pub const PREFIX: &str = "dh_";
/// Deploy hooks par projet.
pub const MAX_PER_PROJECT: i64 = 20;
/// Duree de vie d'un hook cree sans date d'expiration, en jours.
pub const DEFAULT_LIFETIME_DAYS: i32 = 90;
/// Longueur maximale d'un nom.
pub const MAX_NAME_CHARS: usize = 64;

/// Nouveau secret `dh_` + 32 octets aleatoires (43 caracteres base64 URL).
pub fn generate() -> Result<Zeroizing<String>, rand::rngs::SysError> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    SysRng.try_fill_bytes(bytes.as_mut())?;
    Ok(Zeroizing::new(format!(
        "{PREFIX}{}",
        URL_SAFE_NO_PAD.encode(bytes.as_ref())
    )))
}

/// `true` si `token` a pour empreinte `stored` ; `stored` absent (hook
/// inconnu) : faux, apres la meme comparaison sur une empreinte nulle.
pub fn verify(stored: Option<&[u8]>, token: &str) -> bool {
    let given = hash_token(token);
    let zero = [0u8; 32];
    let (expected, known) = match stored {
        Some(h) if h.len() == 32 => (h, true),
        _ => (&zero[..], false),
    };
    bool::from(given.ct_eq(expected)) & known
}

/// Nom affiche : 1 a 64 caracteres, sans caractere de controle.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return None;
    }
    Some(name.to_string())
}

/// Chemin public du hook (sans le secret).
pub fn path(id: Uuid) -> String {
    format!("/v1/hooks/deploy/{id}")
}

const TS: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'";

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    created_by: Option<String>,
    created_at: String,
    last_used_at: Option<String>,
    expires_at: Option<String>,
}

impl From<Row> for DeployHook {
    fn from(r: Row) -> Self {
        DeployHook {
            id: r.id.to_string(),
            name: r.name,
            path: path(r.id),
            created_by: r.created_by,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
            expires_at: r.expires_at,
        }
    }
}

fn select() -> String {
    format!(
        "SELECT h.id, h.name, u.name AS created_by, \
                to_char(h.created_at AT TIME ZONE 'UTC', {TS}) AS created_at, \
                to_char(h.last_used_at AT TIME ZONE 'UTC', {TS}) AS last_used_at, \
                to_char(h.expires_at AT TIME ZONE 'UTC', {TS}) AS expires_at \
         FROM deploy_hooks h LEFT JOIN users u ON u.id = h.created_by"
    )
}

/// Hooks du projet, du plus ancien au plus recent.
pub async fn list(conn: &mut PgConnection, project: Uuid) -> sqlx::Result<Vec<DeployHook>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE h.project_id = $1 ORDER BY h.created_at, h.id",
        select()
    )))
    .bind(project)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(DeployHook::from).collect())
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<DeployHook>> {
    let row: Option<Row> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{} WHERE h.id = $1", select())))
            .bind(id)
            .fetch_optional(conn)
            .await?;
    Ok(row.map(DeployHook::from))
}

pub async fn count(conn: &mut PgConnection, project: Uuid) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM deploy_hooks WHERE project_id = $1")
        .bind(project)
        .fetch_one(conn)
        .await
}

pub async fn insert(
    conn: &mut PgConnection,
    project: Uuid,
    name: &str,
    token: &str,
    created_by: Uuid,
    expires_at: Option<&str>,
) -> sqlx::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deploy_hooks (id, project_id, name, token_hash, created_by, expires_at) \
         VALUES ($1, $2, $3, $4, $5, COALESCE($6::timestamptz, now() + make_interval(days => $7)))",
    )
    .bind(id)
    .bind(project)
    .bind(name)
    .bind(hash_token(token))
    .bind(created_by)
    .bind(expires_at)
    .bind(DEFAULT_LIFETIME_DAYS)
    .execute(conn)
    .await?;
    Ok(id)
}

/// Duree de vie maximale d'une expiration choisie, en jours.
pub const MAX_LIFETIME_DAYS: i32 = 365;

/// Date d'expiration refusee.
#[derive(Debug)]
pub enum ExpiryError {
    /// Pas une date RFC 3339, ou deja passee.
    Invalid,
    Db(sqlx::Error),
}

/// Verifie qu'une date RFC 3339 est valide, dans le futur et au plus
/// `MAX_LIFETIME_DAYS` jours plus loin. La forme est
/// controlee ici, le calendrier par PostgreSQL (jour ou mois impossible).
pub async fn check_expiry(conn: &mut PgConnection, value: &str) -> Result<(), ExpiryError> {
    if !rfc3339_shape(value) {
        return Err(ExpiryError::Invalid);
    }
    match sqlx::query_scalar::<_, bool>(
        "SELECT $1::timestamptz > now() \
            AND $1::timestamptz <= now() + make_interval(days => $2)",
    )
    .bind(value)
    .bind(MAX_LIFETIME_DAYS)
    .fetch_one(conn)
    .await
    {
        Ok(true) => Ok(()),
        Ok(false) => Err(ExpiryError::Invalid),
        // Classe 22 : donnee invalide (calendrier, fuseau).
        Err(sqlx::Error::Database(e)) if e.code().is_some_and(|c| c.starts_with("22")) => {
            Err(ExpiryError::Invalid)
        }
        Err(e) => Err(ExpiryError::Db(e)),
    }
}

/// `AAAA-MM-JJTHH:MM:SS`, fraction facultative, puis `Z` ou `+HH:MM`/`-HH:MM`.
fn rfc3339_shape(v: &str) -> bool {
    let b = v.as_bytes();
    if !v.is_ascii() || b.len() < 20 || b.len() > 40 {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19))
    {
        return false;
    }
    let mut rest = &v[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let n = frac.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 || n > 9 {
            return false;
        }
        rest = &frac[n..];
    }
    match rest.as_bytes() {
        [b'Z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

/// Verrouille le hook et dit si `new` rallonge son expiration (`Some(true)`),
/// ce qui inclut la reactivation d'un hook expire ; `None` si le hook n'existe
/// pas. Un hook sans date que l'on borne n'est pas une extension.
pub async fn lock_for_expiry(
    conn: &mut PgConnection,
    project: Uuid,
    id: Uuid,
    new: &str,
) -> sqlx::Result<Option<bool>> {
    sqlx::query_scalar(
        "SELECT expires_at IS NOT NULL AND $3::timestamptz > expires_at \
         FROM deploy_hooks WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(project)
    .bind(new)
    .fetch_optional(conn)
    .await
}

/// Change la date d'expiration ; `false` si le hook n'existe pas.
pub async fn set_expiry(
    conn: &mut PgConnection,
    project: Uuid,
    id: Uuid,
    expires_at: &str,
) -> sqlx::Result<bool> {
    Ok(sqlx::query(
        "UPDATE deploy_hooks SET expires_at = $3::timestamptz WHERE id = $1 AND project_id = $2",
    )
    .bind(id)
    .bind(project)
    .bind(expires_at)
    .execute(conn)
    .await?
    .rows_affected()
        == 1)
}

/// Supprime le hook `id` du projet ; `false` s'il n'existe pas.
pub async fn delete(conn: &mut PgConnection, project: Uuid, id: Uuid) -> sqlx::Result<bool> {
    Ok(
        sqlx::query("DELETE FROM deploy_hooks WHERE id = $1 AND project_id = $2")
            .bind(id)
            .bind(project)
            .execute(conn)
            .await?
            .rows_affected()
            == 1,
    )
}

/// Projet et empreinte du hook `id`. Un hook expire garde sa ligne mais n'a
/// plus d'empreinte valable : `verify` le traite comme un secret faux.
pub async fn lookup(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<(Uuid, Vec<u8>)>> {
    sqlx::query_as(
        "SELECT project_id, CASE WHEN expires_at IS NOT NULL AND expires_at <= now() \
                THEN ''::bytea ELSE token_hash END \
         FROM deploy_hooks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

pub async fn touch(conn: &mut PgConnection, id: Uuid, now: f64) -> sqlx::Result<()> {
    sqlx::query("UPDATE deploy_hooks SET last_used_at = to_timestamp($2::float8) WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_prefixed_random_and_long() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert!(a.starts_with(PREFIX));
        assert_eq!(a.len(), PREFIX.len() + 43);
        assert_ne!(*a, *b);
    }

    #[test]
    fn verify_matches_only_the_stored_hash() {
        let token = generate().unwrap();
        let stored = hash_token(&token);
        assert!(verify(Some(&stored), &token));
        assert!(!verify(Some(&stored), "dh_wrong"));
        assert!(!verify(None, &token));
        // Empreinte nulle (chemin du hook inconnu) : jamais acceptee.
        assert!(!verify(Some(&[0u8; 32]), &token));
        assert!(!verify(Some(&[1u8; 8]), &token));
    }

    #[test]
    fn expiry_shape_is_rfc3339() {
        for ok in [
            "2099-01-01T00:00:00Z",
            "2099-01-01T00:00:00.123Z",
            "2099-01-01T10:00:00+02:00",
        ] {
            assert!(rfc3339_shape(ok), "{ok}");
        }
        for bad in [
            "",
            "2099-01-01",
            "2099-01-01 00:00:00Z",
            "2099-01-01T00:00:00",
            "2099-01-01T00:00:00.Z",
            "2099-01-01T00:00:00+0200",
            "20990101T000000Z",
        ] {
            assert!(!rfc3339_shape(bad), "{bad}");
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  CI externe "), Some("CI externe".into()));
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name("a\nb"), None);
        assert_eq!(clean_name(&"x".repeat(65)), None);
    }
}
