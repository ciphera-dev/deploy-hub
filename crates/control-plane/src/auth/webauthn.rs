//! Passkeys (WebAuthn, webauthn-rs) : partie relying party, ceremonies et
//! credentials stockes.
//!
//! - RP ID et origine : `CIPHERA_WEBAUTHN_RP_ID` / `CIPHERA_WEBAUTHN_RP_ORIGIN`,
//!   sinon l'origine publique (domaine du panneau confirme, sinon
//!   `CIPHERA_PUBLIC_URL`) des qu'elle porte un nom de domaine, et seulement
//!   a defaut l'hote par lequel le panneau est joint (en-tete `Host`). Les
//!   navigateurs refusent un RP ID qui est une adresse IP : WebAuthn est alors
//!   indisponible (`relying_party` renvoie `None`). `localhost` est accepte.
//!   Changer de domaine du panneau change le RP ID : les passkeys enregistrees
//!   sous l'ancien domaine ne fonctionnent plus (l'interface le signale).
//! - Ceremonies : etat (defi compris) garde cote serveur, a usage unique
//!   (`take` le supprime), valable 5 minutes. Enregistrement et second facteur
//!   sont lies a la session qui les a demandes ; la connexion sans mot de
//!   passe est identifiee par un identifiant aleatoire et bornee par IP.
//! - Credentials : le `Passkey` de webauthn-rs (cle publique, compteur) est
//!   stocke en JSON et ne sort jamais du serveur.

use std::net::IpAddr;
use std::time::Duration;

use axum::http::{HeaderMap, Uri, header};
use serde::Serialize;
use serde::de::DeserializeOwned;
use shared::config::Config;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;
use webauthn_rs::prelude::{Passkey, Url, Webauthn, WebauthnBuilder, WebauthnError};

/// Duree de vie d'une ceremonie (et delai laisse a l'authentificateur), en secondes.
pub const CEREMONY_TTL_SECS: i64 = 5 * 60;
/// Ceremonies de connexion sans mot de passe en cours tolerees par adresse IP.
pub const MAX_PASSWORDLESS_PER_IP: i64 = 20;
/// Longueur maximale du nom d'une cle, en caracteres.
pub const MAX_NAME_LEN: usize = 64;
/// Nom donne a une cle enregistree sans nom.
pub const DEFAULT_NAME: &str = "Clé de sécurité";
const RP_NAME: &str = "Ciphera";

/// Relying party effective pour une requete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rp {
    pub id: String,
    pub origin: Url,
}

/// Hote presente par le client : en-tete `Host` (HTTP/1.1) ou autorite de
/// l'URI (HTTP/2).
pub fn request_host<'a>(headers: &'a HeaderMap, uri: &'a Uri) -> Option<&'a str> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .or_else(|| uri.authority().map(|a| a.as_str()))
}

/// RP ID et origine, ou `None` si WebAuthn est indisponible (RP ID qui est
/// une adresse IP, configuration ou hote invalide). `public_origin` :
/// origine publique du panneau (`PanelSwitch::public_url`), prioritaire sur
/// `host` quand elle porte un nom de domaine.
pub fn relying_party(cfg: &Config, public_origin: Option<&str>, host: Option<&str>) -> Option<Rp> {
    let (id, origin) = match (&cfg.webauthn_rp_id, &cfg.webauthn_rp_origin) {
        (id, Some(origin)) => {
            let origin = Url::parse(origin).ok()?;
            let id = match id {
                Some(id) => id.clone(),
                None => origin.host_str()?.to_string(),
            };
            (id, origin)
        }
        (Some(id), None) => {
            let origin = match cfg.panel_port {
                443 => format!("https://{id}"),
                port => format!("https://{id}:{port}"),
            };
            (id.clone(), Url::parse(&origin).ok()?)
        }
        (None, None) => match public_origin
            .and_then(|o| Url::parse(o).ok())
            .and_then(|o| Some((o.domain()?.to_string(), o.clone())))
        {
            // Origine publique nommee : elle fait foi, quel que soit `Host`.
            Some(public) => public,
            None => {
                // Nom d'hote et port seulement : rien qui puisse changer le sens de l'URL.
                let host = host?;
                if host.is_empty()
                    || host.len() > 260
                    || !host
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-.:[]".contains(&b))
                {
                    return None;
                }
                let origin = Url::parse(&format!("https://{host}")).ok()?;
                // `domain()` est vide pour une adresse IP.
                let id = origin.domain()?.to_string();
                (id, origin)
            }
        },
    };
    let id = id.to_ascii_lowercase();
    let ip_literal = id.parse::<IpAddr>().is_ok() || id.starts_with('[');
    let secure = origin.scheme() == "https" || (origin.scheme() == "http" && id == "localhost");
    if ip_literal || id.is_empty() || !secure {
        return None;
    }
    Some(Rp { id, origin })
}

/// Instance webauthn-rs pour cette relying party.
pub fn webauthn(rp: &Rp) -> Result<Webauthn, WebauthnError> {
    WebauthnBuilder::new(&rp.id, &rp.origin)?
        .rp_name(RP_NAME)
        .timeout(Duration::from_secs(CEREMONY_TTL_SECS as u64))
        .build()
}

// --- Ceremonies ----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Register,
    SecondFactor,
    Passwordless,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Register => "register",
            Kind::SecondFactor => "second_factor",
            Kind::Passwordless => "passwordless",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "register" => Some(Kind::Register),
            "second_factor" => Some(Kind::SecondFactor),
            "passwordless" => Some(Kind::Passwordless),
            _ => None,
        }
    }
}

/// Erreur de stockage d'une ceremonie.
#[derive(Debug)]
pub enum StoreError {
    Db(sqlx::Error),
    Json(serde_json::Error),
    /// Donnee stockee inattendue (compteur absent ou hors bornes) : refus.
    Corrupt(&'static str),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Db(e) => write!(f, "base : {e}"),
            StoreError::Json(e) => write!(f, "serialisation : {e}"),
            StoreError::Corrupt(what) => write!(f, "donnee stockee invalide : {what}"),
        }
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Db(e)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Json(e)
    }
}

/// Supprime les ceremonies expirees.
pub async fn purge_ceremonies(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM webauthn_ceremonies WHERE expires_at <= now()")
        .execute(db)
        .await?;
    Ok(())
}

/// Ceremonies de connexion sans mot de passe en cours pour cette IP.
pub async fn passwordless_in_flight(db: &PgPool, ip: &str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM webauthn_ceremonies \
         WHERE kind = 'passwordless' AND ip = $1 AND expires_at > now()",
    )
    .bind(ip)
    .fetch_one(db)
    .await
}

/// Enregistre l'etat d'une ceremonie. Une ceremonie liee a une session
/// remplace la precedente du meme type pour cette session.
pub async fn save_ceremony(
    db: &PgPool,
    kind: Kind,
    session_id: Option<Uuid>,
    ip: &str,
    state: &impl Serialize,
) -> Result<Uuid, StoreError> {
    let state = serde_json::to_value(state)?;
    let id = Uuid::new_v4();
    let mut tx = db.begin().await?;
    if let Some(session_id) = session_id {
        sqlx::query("DELETE FROM webauthn_ceremonies WHERE session_id = $1 AND kind = $2")
            .bind(session_id)
            .bind(kind.as_str())
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO webauthn_ceremonies (id, kind, session_id, ip, state, expires_at) \
         VALUES ($1, $2, $3, $4, $5, now() + make_interval(secs => $6))",
    )
    .bind(id)
    .bind(kind.as_str())
    .bind(session_id)
    .bind(ip)
    .bind(state)
    .bind(CEREMONY_TTL_SECS as f64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Type et session d'une ceremonie valide (rien n'est consomme).
pub async fn ceremony_kind(
    db: &PgPool,
    id: Uuid,
) -> Result<Option<(Kind, Option<Uuid>)>, sqlx::Error> {
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT kind, session_id FROM webauthn_ceremonies WHERE id = $1 AND expires_at > now()",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.and_then(|(kind, session)| Kind::parse(&kind).map(|k| (k, session))))
}

/// Consomme la ceremonie (usage unique) et renvoie son etat, si elle existe,
/// n'a pas expire, et correspond au type et a la session attendus.
pub async fn take_ceremony<T: DeserializeOwned>(
    conn: &mut PgConnection,
    id: Uuid,
    kind: Kind,
    session_id: Option<Uuid>,
) -> Result<Option<T>, StoreError> {
    let state: Option<serde_json::Value> = sqlx::query_scalar(
        "DELETE FROM webauthn_ceremonies \
         WHERE id = $1 AND kind = $2 AND session_id IS NOT DISTINCT FROM $3 \
         RETURNING CASE WHEN expires_at > now() THEN state END",
    )
    .bind(id)
    .bind(kind.as_str())
    .bind(session_id)
    .fetch_optional(conn)
    .await?
    .flatten();
    Ok(match state {
        Some(state) => Some(serde_json::from_value(state)?),
        None => None,
    })
}

// --- Credentials ---------------------------------------------------------

/// Passkeys d'un compte (pour les options d'assertion et l'exclusion a l'enregistrement).
pub async fn passkeys(db: &PgPool, user_id: Uuid) -> Result<Vec<Passkey>, StoreError> {
    let rows: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT passkey FROM webauthn_credentials WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()?)
}

/// Le compte a au moins une passkey.
pub async fn has_any(db: &PgPool, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM webauthn_credentials WHERE user_id = $1)")
        .bind(user_id)
        .fetch_one(db)
        .await
}

/// Ce que l'interface affiche d'une cle (ni cle publique ni identifiant du credential).
pub struct Listed {
    pub id: Uuid,
    pub name: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

pub async fn list(db: &PgPool, user_id: Uuid) -> Result<Vec<Listed>, sqlx::Error> {
    let rows: Vec<(Uuid, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, name, extract(epoch FROM created_at)::bigint, \
                extract(epoch FROM last_used_at)::bigint \
         FROM webauthn_credentials WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, created_at, last_used_at)| Listed {
            id,
            name,
            created_at,
            last_used_at,
        })
        .collect())
}

/// Enregistre une passkey ; `None` si ce credential existe deja sur l'instance.
pub async fn insert(
    conn: &mut PgConnection,
    user_id: Uuid,
    passkey: &Passkey,
    name: &str,
) -> Result<Option<Uuid>, StoreError> {
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO webauthn_credentials (id, user_id, credential_id, passkey, name) \
         VALUES ($1, $2, $3, $4, $5) ON CONFLICT (credential_id) DO NOTHING",
    )
    .bind(id)
    .bind(user_id)
    .bind(passkey.cred_id().as_slice())
    .bind(serde_json::to_value(passkey)?)
    .bind(name)
    .execute(conn)
    .await?
    .rows_affected();
    Ok((inserted == 1).then_some(id))
}

/// Passkey verrouillee (jusqu'a la fin de la transaction) pour une assertion.
pub struct Locked {
    pub id: Uuid,
    pub passkey: Passkey,
    /// Compteur de signatures stocke.
    pub counter: u32,
}

/// Passkey de ce compte pour cet identifiant de credential, verrouillee.
pub async fn lock(
    conn: &mut PgConnection,
    user_id: Uuid,
    credential_id: &[u8],
) -> Result<Option<Locked>, StoreError> {
    let row: Option<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT id, passkey FROM webauthn_credentials \
         WHERE user_id = $1 AND credential_id = $2 FOR UPDATE",
    )
    .bind(user_id)
    .bind(credential_id)
    .fetch_optional(conn)
    .await?;
    let Some((id, json)) = row else {
        return Ok(None);
    };
    // Compteur illisible : echec ferme (jamais remis a zero).
    let counter = json["cred"]["counter"]
        .as_u64()
        .and_then(|c| u32::try_from(c).ok())
        .ok_or(StoreError::Corrupt("compteur de signatures"))?;
    Ok(Some(Locked {
        id,
        passkey: serde_json::from_value(json)?,
        counter,
    }))
}

/// Compteur de signatures acceptable : strictement croissant des qu'un des
/// deux est non nul (un compteur toujours nul est celui des passkeys synchronisees).
pub fn counter_ok(stored: u32, presented: u32) -> bool {
    (stored == 0 && presented == 0) || presented > stored
}

/// Enregistre l'utilisation (compteur, drapeaux de sauvegarde, date).
pub async fn record_use(
    conn: &mut PgConnection,
    id: Uuid,
    passkey: &Passkey,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE webauthn_credentials SET passkey = $2, last_used_at = now() WHERE id = $1")
        .bind(id)
        .bind(serde_json::to_value(passkey)?)
        .execute(conn)
        .await?;
    Ok(())
}

/// La cle `id` appartient a ce compte.
pub async fn owns(conn: &mut PgConnection, id: Uuid, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM webauthn_credentials WHERE id = $1 AND user_id = $2)",
    )
    .bind(id)
    .bind(user_id)
    .fetch_one(conn)
    .await
}

/// Supprime la cle `id` de ce compte ; `false` si elle n'existe pas (ou est a un autre compte).
pub async fn delete(conn: &mut PgConnection, id: Uuid, user_id: Uuid) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query("DELETE FROM webauthn_credentials WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user_id)
            .execute(conn)
            .await?
            .rows_affected()
            == 1,
    )
}

/// Nom saisi, nettoye : `None` s'il est trop long ou contient des caracteres de controle.
pub fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.chars().count() > MAX_NAME_LEN || name.chars().any(char::is_control) {
        return None;
    }
    Some(if name.is_empty() {
        DEFAULT_NAME.to_string()
    } else {
        name.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(rp_id: Option<&str>, origin: Option<&str>) -> Config {
        let rp_id = rp_id.map(str::to_string);
        let origin = origin.map(str::to_string);
        Config::from_lookup(move |name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@db/c".to_string()),
            "CIPHERA_WEBAUTHN_RP_ID" => rp_id.clone(),
            "CIPHERA_WEBAUTHN_RP_ORIGIN" => origin.clone(),
            _ => None,
        })
        .unwrap()
    }

    fn rp(id: &str, origin: &str) -> Option<Rp> {
        Some(Rp {
            id: id.to_string(),
            origin: Url::parse(origin).unwrap(),
        })
    }

    #[test]
    fn default_relying_party_is_the_request_host() {
        let c = cfg(None, None);
        assert_eq!(
            relying_party(&c, None, Some("Panel.Example.com:8443")),
            rp("panel.example.com", "https://panel.example.com:8443")
        );
        assert_eq!(
            relying_party(&c, None, Some("localhost:8443")),
            rp("localhost", "https://localhost:8443")
        );
        for host in [
            None,
            Some(""),
            Some("203.0.113.7"),
            Some("203.0.113.7:8443"),
            Some("[2001:db8::1]:8443"),
            Some("evil.com/x"),
            Some("a@b.com"),
            Some("a b"),
        ] {
            assert_eq!(relying_party(&c, None, host), None, "{host:?}");
        }
    }

    #[test]
    fn configured_relying_party_wins_over_host() {
        let c = cfg(Some("panel.example.com"), None);
        assert_eq!(
            relying_party(&c, None, Some("203.0.113.7:8443")),
            rp("panel.example.com", "https://panel.example.com:8443")
        );
        let c = cfg(None, Some("https://panel.example.com"));
        assert_eq!(
            relying_party(&c, None, None),
            rp("panel.example.com", "https://panel.example.com")
        );
        let c = cfg(Some("example.com"), Some("https://panel.example.com"));
        assert_eq!(
            relying_party(&c, None, None),
            rp("example.com", "https://panel.example.com")
        );
        assert_eq!(
            relying_party(&cfg(Some("203.0.113.7"), None), None, None),
            None
        );
        assert_eq!(relying_party(&cfg(Some("::1"), None), None, None), None);
        assert_eq!(
            relying_party(&cfg(None, Some("https://203.0.113.7:8443")), None, None),
            None
        );
        assert_eq!(
            relying_party(&cfg(None, Some("http://panel.example.com")), None, None),
            None,
            "http seulement pour localhost"
        );
        assert_eq!(
            relying_party(&cfg(None, Some("not a url")), None, None),
            None
        );
        // Variables dediees : prioritaires sur l'origine publique.
        assert_eq!(
            relying_party(
                &cfg(Some("panel.example.com"), None),
                Some("https://other.example.org"),
                None
            ),
            rp("panel.example.com", "https://panel.example.com:8443")
        );
    }

    #[test]
    fn public_origin_wins_over_host_unless_it_is_an_ip() {
        let c = cfg(None, None);
        assert_eq!(
            relying_party(
                &c,
                Some("https://panel.example.com"),
                Some("evil.example.org")
            ),
            rp("panel.example.com", "https://panel.example.com")
        );
        assert_eq!(
            relying_party(&c, Some("https://203.0.113.7:8443"), Some("localhost:8443")),
            rp("localhost", "https://localhost:8443")
        );
        assert_eq!(
            relying_party(
                &c,
                Some("https://203.0.113.7:8443"),
                Some("203.0.113.7:8443")
            ),
            None
        );
    }

    #[test]
    fn counter_must_increase_unless_both_are_zero() {
        assert!(counter_ok(0, 0));
        assert!(counter_ok(0, 1));
        assert!(counter_ok(5, 6));
        assert!(!counter_ok(5, 5));
        assert!(!counter_ok(5, 4));
        assert!(!counter_ok(5, 0));
    }

    #[test]
    fn names_are_trimmed_bounded_and_defaulted() {
        assert_eq!(clean_name("  YubiKey  ").as_deref(), Some("YubiKey"));
        assert_eq!(clean_name("").as_deref(), Some(DEFAULT_NAME));
        assert_eq!(
            clean_name(&"é".repeat(64)).map(|n| n.chars().count()),
            Some(64)
        );
        assert_eq!(clean_name(&"é".repeat(65)), None);
        assert_eq!(clean_name("a\u{0}b"), None);
    }
}
