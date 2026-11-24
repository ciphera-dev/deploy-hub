//! Seconds facteurs d'un compte (TOTP, passkeys) : verrou de modification et
//! regle « derniere methode » de la 2FA imposee par l'organisation.

use sqlx::PgConnection;
use uuid::Uuid;

/// Serialise, jusqu'a la fin de la transaction, les modifications des seconds
/// facteurs du compte (ajout, suppression, codes de secours) : deux
/// suppressions simultanees ne peuvent pas retirer chacune « l'avant-derniere »
/// methode.
pub async fn lock(conn: &mut PgConnection, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-factors:' || $1::text, 0))",
    )
    .bind(user_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Seconds facteurs du compte, lus sous le verrou.
pub struct Factors {
    /// 2FA imposee par l'organisation.
    pub org_requires: bool,
    /// TOTP confirmee.
    pub totp: bool,
    /// Nombre de passkeys.
    pub passkeys: i64,
}

pub async fn read(conn: &mut PgConnection, user_id: Uuid) -> Result<Factors, sqlx::Error> {
    let (org_requires, totp, passkeys): (bool, bool, i64) = sqlx::query_as(
        "SELECT coalesce((SELECT mfa_required FROM instance LIMIT 1), false), \
                EXISTS (SELECT 1 FROM user_totp WHERE user_id = $1 AND confirmed_at IS NOT NULL), \
                (SELECT count(*) FROM webauthn_credentials WHERE user_id = $1)",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await?;
    Ok(Factors {
        org_requires,
        totp,
        passkeys,
    })
}
