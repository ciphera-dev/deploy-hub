//! Connexion a la base au demarrage. PostgreSQL tourne dans un conteneur
//! lance par `ciphera-agent` (socket Unix `/run/ciphera/postgres`) : au
//! demarrage de la machine il peut ne pas etre pret. Le serveur attend et
//! reessaie (1 s, doublee jusqu'a 30 s) au lieu de s'arreter et d'etre
//! relance en boucle par systemd.

use std::future::Future;
use std::time::Duration;

/// Premier delai entre deux tentatives ; double a chaque echec.
pub const RETRY_FIRST: Duration = Duration::from_secs(1);
/// Delai maximal entre deux tentatives.
pub const RETRY_MAX: Duration = Duration::from_secs(30);
/// A partir de cette tentative, les echecs sont journalises en `error`
/// (environ 3 minutes d'attente) : la base ne demarre visiblement pas.
pub const ESCALATE_AFTER: u32 = 10;

/// Niveau de journal de l'echec numero `attempt` (a partir de 1).
pub fn level_for(attempt: u32) -> tracing::Level {
    if attempt >= ESCALATE_AFTER {
        tracing::Level::ERROR
    } else {
        tracing::Level::WARN
    }
}

/// Cause d'un echec de connexion, sans rien qui vienne de la configuration
/// ni du message du pilote (qui peut citer les options de connexion, donc le
/// mot de passe) : genre d'erreur d'E/S, code SQLSTATE ou nature de l'erreur.
pub fn sanitized_cause(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Io(io) => format!("io:{:?}", io.kind()),
        sqlx::Error::Database(db) => sqlstate(db.code().as_deref()),
        sqlx::Error::PoolTimedOut => "pool-timeout".into(),
        sqlx::Error::Tls(_) => "tls".into(),
        sqlx::Error::Protocol(_) => "protocol".into(),
        sqlx::Error::Configuration(_) => "configuration".into(),
        _ => "other".into(),
    }
}

/// `sqlstate:<code>` si le code a la forme SQLSTATE (5 caracteres
/// alphanumeriques) ; sinon rien de ce qu'a envoye le serveur.
pub fn sqlstate(code: Option<&str>) -> String {
    match code {
        Some(c) if c.len() == 5 && c.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            format!("sqlstate:{c}")
        }
        _ => "database".into(),
    }
}

/// Appelle `attempt` jusqu'a ce qu'il reussisse. Chaque echec est journalise
/// avec son numero et la cause donnee par `cause` (jamais l'erreur brute) ;
/// `warn` d'abord, `error` a partir de `ESCALATE_AFTER` tentatives.
pub async fn retry_until_ready<T, E, F, Fut, C>(mut attempt: F, cause: C) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    C: Fn(&E) -> String,
{
    let mut delay = RETRY_FIRST;
    let mut n: u32 = 0;
    loop {
        n = n.saturating_add(1);
        match attempt().await {
            Ok(value) => return value,
            Err(e) => {
                let cause = cause(&e);
                let secs = delay.as_secs();
                if level_for(n) == tracing::Level::ERROR {
                    tracing::error!(
                        attempt = n,
                        cause = %cause,
                        "base de donnees toujours indisponible, nouvel essai dans {secs} s"
                    );
                } else {
                    tracing::warn!(
                        attempt = n,
                        cause = %cause,
                        "base de donnees indisponible, nouvel essai dans {secs} s"
                    );
                }
            }
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RETRY_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test(start_paused = true)]
    async fn waits_with_backoff_until_the_database_answers() {
        let calls = Cell::new(0u32);
        let start = tokio::time::Instant::now();
        let value = retry_until_ready(
            || {
                calls.set(calls.get() + 1);
                let n = calls.get();
                async move { if n < 8 { Err("pas pret") } else { Ok(n) } }
            },
            |e| e.to_string(),
        )
        .await;
        assert_eq!(value, 8);
        // 1 + 2 + 4 + 8 + 16 + 30 + 30 secondes d'attente.
        assert_eq!(start.elapsed(), Duration::from_secs(91));
    }

    #[test]
    fn causes_are_sanitized() {
        use std::io;
        // Ni message, ni options de connexion : seulement le genre d'erreur.
        let io_err = sqlx::Error::Io(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            "postgres://ciphera:hunter2@localhost/ciphera",
        ));
        assert_eq!(sanitized_cause(&io_err), "io:ConnectionRefused");
        let config = sqlx::Error::Configuration("password=hunter2 host=/run/x".into());
        assert_eq!(sanitized_cause(&config), "configuration");
        assert_eq!(sanitized_cause(&sqlx::Error::PoolTimedOut), "pool-timeout");
        for e in [&io_err, &config] {
            assert!(!sanitized_cause(e).contains("hunter2"));
        }
        assert_eq!(sqlstate(Some("28P01")), "sqlstate:28P01");
        assert_eq!(sqlstate(Some("57P03")), "sqlstate:57P03");
        // Code mal forme (jamais renvoye tel quel) ou absent.
        assert_eq!(sqlstate(Some("password=hunter2")), "database");
        assert_eq!(sqlstate(None), "database");
    }

    #[test]
    fn failures_escalate_to_error_after_a_while() {
        assert_eq!(level_for(1), tracing::Level::WARN);
        assert_eq!(level_for(ESCALATE_AFTER - 1), tracing::Level::WARN);
        assert_eq!(level_for(ESCALATE_AFTER), tracing::Level::ERROR);
        assert_eq!(level_for(ESCALATE_AFTER + 50), tracing::Level::ERROR);
    }
}
