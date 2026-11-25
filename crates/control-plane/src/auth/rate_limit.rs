//! Limitation de debit des connexions, stockee dans PostgreSQL
//! (`login_attempts`, fenetre glissante de 15 minutes) :
//! - 5 echecs par couple (IP cliente, e-mail en minuscules) ; une connexion
//!   reussie remet ce compteur a zero (seuls comptent les echecs posterieurs
//!   au dernier succes du couple) ;
//! - 50 echecs par IP, tous e-mails confondus : borne l'essai d'un mot de
//!   passe sur de nombreux comptes. Un succes ne remet pas ce compteur a zero
//!   (sinon un attaquant le viderait avec son propre compte).
//!
//! Cle IP (`IpKey`) : IPv4 telle quelle, IPv6 regroupee par /64 (un client
//! IPv6 dispose couramment d'un /64 entier et changerait sinon d'adresse a
//! chaque essai). Les demandes « mot de passe oublie », jamais marquees
//! reussies, ont leur propre espace (`IpKey::forgot`) : elles ne peuvent pas
//! epuiser le plafond par IP des connexions. Les appels de deploy hooks
//! aussi (`IpKey::hook`, couple (IP, hook)).
//!
//! Le controle et l'enregistrement sont atomiques (`admit` : verrou
//! consultatif par IP), et chaque tentative est enregistree comme un echec
//! *avant* la verification du mot de passe, puis marquee reussie : une rafale
//! de requetes paralleles ne depasse pas la limite. L'e-mail n'est jamais stocke, seul
//! son SHA-256 l'est. Les lignes de plus de 24 heures sont purgees.

use std::net::{IpAddr, Ipv6Addr};

use sha2::{Digest, Sha256};
use shared::error::ApiError;
use sqlx::{PgConnection, PgPool};

use super::password::OffloadError;

/// Fenetre glissante, en secondes.
pub const WINDOW_SECS: i64 = 15 * 60;
/// Echecs tolores par couple (IP, e-mail) dans la fenetre.
pub const MAX_PER_IP_EMAIL: i64 = 5;
/// Echecs tolores par IP, tous e-mails confondus, dans la fenetre.
pub const MAX_PER_IP: i64 = 50;

/// Cle IP d'une tentative (colonne `ip`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpKey(String);

impl IpKey {
    /// Connexions, passkeys, invitations, inscriptions, reinitialisations.
    pub fn login(ip: IpAddr) -> Self {
        Self(bucket(ip))
    }

    /// Demandes « mot de passe oublie » : espace separe des connexions.
    pub fn forgot(ip: IpAddr) -> Self {
        Self(format!("forgot:{}", bucket(ip)))
    }

    /// Appels des deploy hooks : espace separe des connexions.
    pub fn hook(ip: IpAddr) -> Self {
        Self(format!("hook:{}", bucket(ip)))
    }

    /// Actions d'un utilisateur connecte limitees par compte (espace separe).
    pub fn user_action(action: &str, user: uuid::Uuid) -> Self {
        Self(format!("{action}:{user}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// IPv4 (ou IPv6 mappee) telle quelle ; IPv6 reduite a son /64.
fn bucket(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{}/64", Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
    }
}

/// Cle de l'e-mail : SHA-256 de sa forme en minuscules.
pub fn email_key(email_lower: &str) -> Vec<u8> {
    Sha256::digest(email_lower.as_bytes()).to_vec()
}

/// Issue de `admit`.
#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    /// Tentative enregistree (comptee comme un echec jusqu'a `mark_success`).
    Allowed(i64),
    /// Limite atteinte : delai avant qu'une tentative redevienne possible
    /// (valeur de `Retry-After`, en secondes). Rien n'est enregistre.
    Limited(u64),
}

/// Verifie les deux limites puis enregistre la tentative, de facon atomique :
/// un verrou consultatif de transaction par IP serialise le couple
/// « compter puis inserer » de toutes les tentatives de cette IP (il couvre
/// donc le compteur (IP, e-mail) et le plafond par IP). Le verrou n'est tenu
/// que le temps de ces deux requetes, pas pendant le calcul Argon2.
pub async fn admit(db: &PgPool, ip: &IpKey, email_hash: &[u8]) -> Result<Admission, sqlx::Error> {
    let ip = ip.as_str();
    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ciphera-login:' || $1, 0))")
        .bind(ip)
        .execute(&mut *tx)
        .await?;
    if let Some(retry_after) = limited(&mut tx, ip, email_hash).await? {
        tx.rollback().await?;
        return Ok(Admission::Limited(retry_after));
    }
    let id = sqlx::query_scalar(
        "INSERT INTO login_attempts (at, ip, email_hash, succeeded)          VALUES (clock_timestamp(), $1, $2, false) RETURNING id",
    )
    .bind(ip)
    .bind(email_hash)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Admission::Allowed(id))
}

/// `Some(secondes)` si une limite est atteinte. Horloge reelle
/// (`clock_timestamp()`) : `now()` serait fige au debut de la transaction,
/// avant l'attente du verrou.
async fn limited(
    conn: &mut PgConnection,
    ip: &str,
    email_hash: &[u8],
) -> Result<Option<u64>, sqlx::Error> {
    // Le N-ieme echec le plus recent : quand il sort de la fenetre, le compteur
    // repasse sous la limite.
    let per_pair: Option<i64> = sqlx::query_scalar(
        "SELECT ceil(extract(epoch FROM at + make_interval(secs => $3) - clock_timestamp()))::bigint          FROM login_attempts          WHERE ip = $1 AND email_hash = $2 AND NOT succeeded            AND at > clock_timestamp() - make_interval(secs => $3)            AND at > coalesce((SELECT max(at) FROM login_attempts                               WHERE ip = $1 AND email_hash = $2 AND succeeded), '-infinity')          ORDER BY at DESC OFFSET $4 LIMIT 1",
    )
    .bind(ip)
    .bind(email_hash)
    .bind(WINDOW_SECS as f64)
    .bind(MAX_PER_IP_EMAIL - 1)
    .fetch_optional(&mut *conn)
    .await?;
    let per_ip: Option<i64> = sqlx::query_scalar(
        "SELECT ceil(extract(epoch FROM at + make_interval(secs => $2) - clock_timestamp()))::bigint          FROM login_attempts          WHERE ip = $1 AND NOT succeeded AND at > clock_timestamp() - make_interval(secs => $2)          ORDER BY at DESC OFFSET $3 LIMIT 1",
    )
    .bind(ip)
    .bind(WINDOW_SECS as f64)
    .bind(MAX_PER_IP - 1)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(per_pair
        .into_iter()
        .chain(per_ip)
        .max()
        .map(|secs| secs.clamp(1, WINDOW_SECS) as u64))
}

/// Marque la tentative reussie : remet a zero le compteur du couple (IP, e-mail).
pub async fn mark_success(conn: &mut PgConnection, attempt_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE login_attempts SET succeeded = true WHERE id = $1")
        .bind(attempt_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Supprime les tentatives de plus de 24 heures.
pub async fn purge(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_attempts WHERE at < now() - interval '24 hours'")
        .execute(db)
        .await?;
    Ok(())
}

// --- Calcul Argon2 de connexion : un seul par compte, delai croissant ----

/// Echecs de mot de passe d'un compte (toutes IP) tolores dans la fenetre
/// avant que chaque tentative soit ralentie.
pub const SLOWDOWN_AFTER: i64 = 10;
/// Delai maximal ajoute a une tentative, en secondes.
pub const SLOWDOWN_MAX_SECS: u64 = 30;

/// Comptes ayant une connexion en vol (cles `email_key`).
#[derive(Clone, Default)]
pub struct LoginFlights(std::sync::Arc<std::sync::Mutex<std::collections::HashSet<Vec<u8>>>>);

/// Place de l'unique connexion en vol pour un compte ; rendue a la chute.
pub struct LoginFlight {
    flights: LoginFlights,
    key: Vec<u8>,
}

impl LoginFlights {
    /// `None` si une connexion sur ce compte (cle `email_key`) est deja en cours.
    pub fn try_begin(&self, email_hash: &[u8]) -> Option<LoginFlight> {
        self.set().insert(email_hash.to_vec()).then(|| LoginFlight {
            flights: self.clone(),
            key: email_hash.to_vec(),
        })
    }

    fn set(&self) -> std::sync::MutexGuard<'_, std::collections::HashSet<Vec<u8>>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Drop for LoginFlight {
    fn drop(&mut self) {
        self.flights.set().remove(&self.key);
    }
}

/// Delai impose a une tentative quand le compte a deja `failures` echecs de
/// mot de passe dans la fenetre (celui de la tentative en cours compris) :
/// 1 s au 11e, puis une seconde de plus par echec, jusqu'a 30 s. Jamais de blocage.
pub fn login_delay(failures: i64) -> std::time::Duration {
    let over = (failures - SLOWDOWN_AFTER).clamp(0, SLOWDOWN_MAX_SECS as i64);
    std::time::Duration::from_secs(over as u64)
}

/// Echecs de mot de passe recents du compte (toutes IP), depuis son dernier
/// succes. Les demandes « mot de passe oublie » et les deploy hooks ont leur
/// propre espace d'IP et ne comptent pas.
pub async fn recent_login_failures(db: &PgPool, email_hash: &[u8]) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM login_attempts          WHERE email_hash = $1 AND NOT succeeded            AND ip NOT LIKE 'forgot:%' AND ip NOT LIKE 'hook:%'            AND at > clock_timestamp() - make_interval(secs => $2)            AND at > coalesce((SELECT max(at) FROM login_attempts                               WHERE email_hash = $1 AND succeeded), '-infinity')",
    )
    .bind(email_hash)
    .bind(WINDOW_SECS as f64)
    .fetch_one(db)
    .await
}

// --- Second facteur et re-authentification ------------------------------
//
// Table `mfa_attempts`. Meme schema que la connexion : la tentative est
// enregistree comme un echec avant la verification (verrou consultatif par
// compte), puis marquee reussie. Les limites dependent du facteur :
// - code TOTP (10^6 valeurs) : 5 echecs TOTP par compte dans la fenetre de
//   15 minutes (remis a zero par une verification TOTP reussie) et
//   `MAX_MFA_FAILURES_PER_DAY` echecs TOTP par compte sur 24 heures, sans
//   remise a zero par un succes : seule une reinitialisation du mot de passe
//   (`clear_mfa`) les efface. Seules les tentatives TOTP sont bloquees ;
// - code de secours et passkey (ne se devinent pas) : 5 echecs par session et
//   `MAX_MFA_FAILURES_PER_IP` par IP (regroupee) dans la fenetre ; les echecs
//   TOTP du compte ne les bloquent pas, le titulaire garde ces recours ;
// - mot de passe redemande (re-authentification d'une session complete) :
//   5 echecs par session dans la fenetre, sans lien avec les autres sessions.

/// Echecs tolores par compte (TOTP) ou par session (autres), dans la fenetre.
pub const MAX_MFA_FAILURES: i64 = 5;
/// Echecs tolores par IP pour les codes de secours et les passkeys, dans la fenetre.
pub const MAX_MFA_FAILURES_PER_IP: i64 = 20;
/// Fenetre du plafond journalier, en secondes (24 heures, comme la purge).
pub const MFA_DAY_SECS: i64 = 24 * 3600;
/// Echecs TOTP tolores par compte sur `MFA_DAY_SECS` (verrouillage progressif,
/// ASVS V2.2.1) : sans ce plafond, qui detient le mot de passe essaierait
/// 480 codes TOTP par jour, soit environ 3 % de chances par mois de tomber
/// sur le bon ; avec, moins de 0,2 % par mois.
pub const MAX_MFA_FAILURES_PER_DAY: i64 = 20;

/// Second facteur ou verification tentee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MfaFactor {
    /// Code TOTP : limite par compte, plafond journalier.
    Totp,
    /// Code de secours : limite par session et par IP.
    Recovery,
    /// Passkey : limite par session et par IP.
    Passkey,
    /// Mot de passe redemande dans une session complete : limite par session.
    Reauth,
}

/// Verifie les limites du facteur, puis enregistre la tentative. `ip` n'est
/// utilise que pour les codes de secours et les passkeys.
pub async fn admit_mfa(
    db: &PgPool,
    user_id: uuid::Uuid,
    session_id: uuid::Uuid,
    factor: MfaFactor,
    ip: Option<&IpKey>,
) -> Result<Admission, sqlx::Error> {
    let totp = factor == MfaFactor::Totp;
    let ip = match factor {
        MfaFactor::Recovery | MfaFactor::Passkey => ip.map(|k| k.as_str().to_string()),
        MfaFactor::Totp | MfaFactor::Reauth => None,
    };
    let mut tx = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('ciphera-mfa:' || $1::text, 0))")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    // TOTP : le N-ieme echec TOTP le plus recent du compte (depuis la derniere
    // verification TOTP reussie), et le N-ieme sur la journee (sans remise a
    // zero). Autres : le N-ieme echec de la session depuis son dernier succes,
    // et le N-ieme echec de l'IP.
    let retry_after: Option<i64> = sqlx::query_scalar(
        "WITH day_failures AS (              SELECT at FROM mfa_attempts              WHERE $7 AND user_id = $1 AND totp AND NOT succeeded                AND at > clock_timestamp() - make_interval(secs => $5)          ), per_user AS (              SELECT at FROM day_failures              WHERE at > clock_timestamp() - make_interval(secs => $3)                AND at > coalesce((SELECT max(at) FROM mfa_attempts                                   WHERE user_id = $1 AND totp AND succeeded), '-infinity')              ORDER BY at DESC OFFSET $4 LIMIT 1          ), per_day AS (              SELECT at FROM day_failures ORDER BY at DESC OFFSET $6 LIMIT 1          ), per_session AS (              SELECT at FROM mfa_attempts              WHERE NOT $7 AND session_id = $2 AND NOT totp AND NOT succeeded                AND at > clock_timestamp() - make_interval(secs => $3)                AND at > coalesce((SELECT max(at) FROM mfa_attempts                                   WHERE session_id = $2 AND NOT totp AND succeeded), '-infinity')              ORDER BY at DESC OFFSET $4 LIMIT 1          ), per_ip AS (              SELECT at FROM mfa_attempts              WHERE $8::text IS NOT NULL AND ip = $8 AND NOT totp AND NOT succeeded                AND at > clock_timestamp() - make_interval(secs => $3)              ORDER BY at DESC OFFSET $9 LIMIT 1          )          SELECT max(secs)::bigint FROM (              SELECT ceil(extract(epoch FROM at + make_interval(secs => $3) - clock_timestamp())) AS secs              FROM (SELECT at FROM per_user UNION ALL SELECT at FROM per_session                    UNION ALL SELECT at FROM per_ip) hit              UNION ALL              SELECT ceil(extract(epoch FROM at + make_interval(secs => $5) - clock_timestamp()))              FROM per_day          ) waits",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(WINDOW_SECS as f64)
    .bind(MAX_MFA_FAILURES - 1)
    .bind(MFA_DAY_SECS as f64)
    .bind(MAX_MFA_FAILURES_PER_DAY - 1)
    .bind(totp)
    .bind(ip.as_deref())
    .bind(MAX_MFA_FAILURES_PER_IP - 1)
    .fetch_one(&mut *tx)
    .await?;
    if let Some(secs) = retry_after {
        tx.rollback().await?;
        return Ok(Admission::Limited(secs.clamp(1, MFA_DAY_SECS) as u64));
    }
    let id = sqlx::query_scalar(
        "INSERT INTO mfa_attempts (at, user_id, session_id, succeeded, totp, ip)          VALUES (clock_timestamp(), $1,                  (SELECT id FROM sessions WHERE id = $2 FOR KEY SHARE), false, $3, $4)          RETURNING id",
    )
    .bind(user_id)
    .bind(session_id)
    .bind(totp)
    .bind(ip.as_deref())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Admission::Allowed(id))
}

/// Tentative enregistree par `admit` (connexion) ou `admit_mfa`.
#[derive(Clone, Copy, Debug)]
pub enum Attempt {
    Login(i64),
    Mfa(i64),
}

/// Reponse a l'echec d'un calcul Argon2 fait apres l'admission de `attempt`.
/// « Serveur occupe » (503) n'est pas un echec du client : la tentative est
/// retiree, sinon une saturation volontaire ferait aussi refuser (429) les
/// vrais utilisateurs qui reessaient.
pub async fn offload_error(
    db: &PgPool,
    attempt: Attempt,
    error: OffloadError,
    context: &str,
) -> ApiError {
    if matches!(error, OffloadError::Busy) {
        let query = match attempt {
            Attempt::Login(id) => sqlx::query("DELETE FROM login_attempts WHERE id = $1").bind(id),
            Attempt::Mfa(id) => sqlx::query("DELETE FROM mfa_attempts WHERE id = $1").bind(id),
        };
        if let Err(e) = query.execute(db).await {
            tracing::warn!(error = %e, "tentative non retiree apres un refus « occupe »");
        }
    }
    error.into_api(context)
}

/// Oublie les echecs de second facteur du compte (mot de passe reinitialise :
/// l'ancien detenteur du mot de passe est dehors).
pub async fn clear_mfa(conn: &mut PgConnection, user_id: uuid::Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM mfa_attempts WHERE user_id = $1")
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Marque la tentative reussie : remet a zero le compteur court du facteur
/// (compte pour la TOTP, session pour les autres) ; jamais le plafond journalier.
pub async fn mark_mfa_success(conn: &mut PgConnection, attempt_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mfa_attempts SET succeeded = true WHERE id = $1")
        .bind(attempt_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Supprime les tentatives de second facteur de plus de 24 heures.
pub async fn purge_mfa(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM mfa_attempts WHERE at < now() - interval '24 hours'")
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ip: &str) -> String {
        IpKey::login(ip.parse().unwrap()).as_str().to_string()
    }

    #[test]
    fn ipv4_is_kept_and_mapped_ipv4_is_canonical() {
        assert_eq!(key("203.0.113.7"), "203.0.113.7");
        assert_eq!(key("::ffff:203.0.113.7"), "203.0.113.7");
    }

    #[test]
    fn ipv6_is_bucketed_by_slash_64() {
        assert_eq!(key("2001:db8:1:2::1"), "2001:db8:1:2::/64");
        assert_eq!(key("2001:db8:1:2:ffff:ffff:ffff:ffff"), "2001:db8:1:2::/64");
        assert_ne!(key("2001:db8:1:3::1"), key("2001:db8:1:2::1"));
        assert_eq!(key("::1"), "::/64");
    }

    #[test]
    fn one_login_in_flight_per_account() {
        let flights = LoginFlights::default();
        let a = email_key("flight-a@example.com");
        let b = email_key("flight-b@example.com");
        let first = flights.try_begin(&a).expect("premiere connexion");
        assert!(flights.try_begin(&a).is_none());
        assert!(flights.try_begin(&b).is_some());
        drop(first);
        assert!(flights.try_begin(&a).is_some());
    }

    /// La place du compte reste prise tant que le calcul tourne, meme si la
    /// requete qui l'a lance est abandonnee.
    #[tokio::test]
    async fn flight_outlives_a_cancelled_request() {
        let flights = LoginFlights::default();
        let key = email_key("flight-cancel@example.com");
        let flight = flights.try_begin(&key).expect("place");
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let task = tokio::spawn(crate::auth::password::offload(move || {
            let _flight = flight;
            wait.recv().ok();
        }));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        task.abort();
        let _ = task.await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(flights.try_begin(&key).is_none(), "calcul encore en cours");
        release.send(()).unwrap();
        let mut freed = false;
        for _ in 0..40 {
            if flights.try_begin(&key).is_some() {
                freed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(freed, "place rendue a la fin du calcul");
    }

    #[test]
    fn login_delay_grows_from_one_to_thirty_seconds() {
        let secs = |n| login_delay(n).as_secs();
        assert_eq!(secs(0), 0);
        assert_eq!(secs(10), 0);
        assert_eq!(secs(11), 1);
        assert_eq!(secs(20), 10);
        assert_eq!(secs(40), 30);
        assert_eq!(secs(10_000), 30);
    }

    #[test]
    fn forgot_attempts_have_their_own_namespace() {
        let ip = "203.0.113.7".parse().unwrap();
        assert_eq!(IpKey::forgot(ip).as_str(), "forgot:203.0.113.7");
        assert_ne!(IpKey::forgot(ip), IpKey::login(ip));
        let v6 = "2001:db8::5".parse().unwrap();
        assert_eq!(IpKey::forgot(v6).as_str(), "forgot:2001:db8::/64");
        assert_eq!(IpKey::hook(ip).as_str(), "hook:203.0.113.7");
        assert_ne!(IpKey::hook(ip), IpKey::login(ip));
    }
}
