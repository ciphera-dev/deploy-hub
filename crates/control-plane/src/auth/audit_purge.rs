//! Purge quotidienne du journal d'audit : les echecs de connexion (mot de
//! passe, second facteur, passkey) de plus de 90 jours sont supprimes. Ils
//! peuvent etre provoques sans compte : sans purge, la table grossirait sans
//! fin. Les autres evenements sont gardes.

use std::time::Duration;

use sqlx::PgPool;
use tokio::sync::watch;

/// Actions d'audit des echecs de connexion.
pub const FAILED_LOGIN_ACTIONS: [&str; 4] = [
    "auth.login.failure",
    "auth.mfa.failure",
    "auth.recovery_code.failure",
    "auth.webauthn.failure",
];
/// Conservation des echecs de connexion, en jours.
pub const RETENTION_DAYS: i32 = 90;
/// Intervalle entre deux purges.
pub const EVERY: Duration = Duration::from_secs(24 * 3600);

/// Supprime les echecs de connexion de plus de `RETENTION_DAYS` jours ;
/// renvoie le nombre de lignes supprimees.
pub async fn purge(db: &PgPool) -> sqlx::Result<u64> {
    let actions: Vec<String> = FAILED_LOGIN_ACTIONS.iter().map(|a| a.to_string()).collect();
    Ok(sqlx::query(
        "DELETE FROM audit_events \
         WHERE action = ANY($1) AND at < now() - make_interval(days => $2)",
    )
    .bind(&actions)
    .bind(RETENTION_DAYS)
    .execute(db)
    .await?
    .rows_affected())
}

/// Purge au demarrage puis toutes les 24 heures, jusqu'a l'arret.
pub async fn run(db: PgPool, mut stop: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = stop.wait_for(|s| *s) => return,
        }
        match purge(&db).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(lignes = n, "echecs de connexion anciens retires de l'audit"),
            Err(e) => tracing::warn!(error = %e, "purge du journal d'audit"),
        }
    }
}
