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
        }
    }
}

fn select() -> String {
    format!(
        "SELECT h.id, h.name, u.name AS created_by, \
                to_char(h.created_at AT TIME ZONE 'UTC', {TS}) AS created_at, \
                to_char(h.last_used_at AT TIME ZONE 'UTC', {TS}) AS last_used_at \
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
) -> sqlx::Result<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deploy_hooks (id, project_id, name, token_hash, created_by) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(project)
    .bind(name)
    .bind(hash_token(token))
    .bind(created_by)
    .execute(conn)
    .await?;
    Ok(id)
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

/// Projet et empreinte du hook `id`.
pub async fn lookup(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<(Uuid, Vec<u8>)>> {
    sqlx::query_as("SELECT project_id, token_hash FROM deploy_hooks WHERE id = $1")
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
    fn names_are_cleaned() {
        assert_eq!(clean_name("  CI externe "), Some("CI externe".into()));
        assert_eq!(clean_name(""), None);
        assert_eq!(clean_name("a\nb"), None);
        assert_eq!(clean_name(&"x".repeat(65)), None);
    }
}
