//! Table `servers` : agents autorises a se connecter (empreinte de leur
//! certificat client), et enregistrement du serveur local.

use std::fmt;
use std::path::Path;

use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use shared::ids::ServerId;
use sqlx::PgPool;

/// Copie publique du certificat de l'agent local, ecrite par `ciphera init`
/// (relative au dossier de configuration).
pub const LOCAL_AGENT_CERT: &str = "agents/local.crt";

#[derive(Debug)]
pub enum RegisterError {
    /// Certificat illisible ou sans SAN d'agent.
    BadCert,
    Db(sqlx::Error),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadCert => f.write_str("certificat d'agent illisible ou sans identifiant"),
            Self::Db(_) => f.write_str("base de donnees indisponible"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Vrai si `id` est enregistre avec cette empreinte (actuelle, ou celle du
/// certificat renouvele pas encore presente) et n'est pas revoque.
pub async fn is_authorized(
    db: &PgPool,
    id: ServerId,
    fingerprint: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM servers \
         WHERE id = $1 AND (cert_fingerprint = $2 OR next_cert_fingerprint = $2) \
         AND revoked_at IS NULL)",
    )
    .bind(id)
    .bind(fingerprint)
    .fetch_one(db)
    .await
}

/// Note le certificat renouvele de `id` (accepte en plus de l'actuel),
/// seulement si `current` est l'actuel et que le serveur n'est pas
/// revoque. Vrai si la ligne a change.
pub async fn set_next_cert(
    db: &PgPool,
    id: ServerId,
    current: &str,
    next: &str,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE servers SET next_cert_fingerprint = $3 \
         WHERE id = $1 AND cert_fingerprint = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(current)
    .bind(next)
    .execute(db)
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// Premiere connexion avec le certificat renouvele : il remplace l'ancien
/// en une instruction (l'ancien n'est plus accepte). Vrai si remplace.
pub async fn promote_next_cert(
    db: &PgPool,
    id: ServerId,
    fingerprint: &str,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE servers SET cert_fingerprint = next_cert_fingerprint, \
         next_cert_fingerprint = NULL \
         WHERE id = $1 AND next_cert_fingerprint = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(fingerprint)
    .execute(db)
    .await?
    .rows_affected();
    Ok(n == 1)
}

/// Note la CA emettrice (`issuer` : SHA-256 de son certificat DER) du
/// certificat presente par `id`, s'il est toujours son certificat actuel.
pub async fn set_issuer(
    db: &PgPool,
    id: ServerId,
    fingerprint: &str,
    issuer: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE servers SET cert_issuer_fingerprint = $3 \
         WHERE id = $1 AND cert_fingerprint = $2 AND revoked_at IS NULL \
         AND cert_issuer_fingerprint IS DISTINCT FROM $3",
    )
    .bind(id)
    .bind(fingerprint)
    .bind(issuer)
    .execute(db)
    .await?;
    Ok(())
}

/// Enregistre le serveur local d'apres le certificat PEM de son agent.
/// Idempotent : une ligne existante (meme revoquee) n'est jamais modifiee.
pub async fn register_local(db: &PgPool, cert_pem: &str) -> Result<ServerId, RegisterError> {
    let der =
        CertificateDer::from_pem_slice(cert_pem.as_bytes()).map_err(|_| RegisterError::BadCert)?;
    let id = pki::agent_id_from_cert(&der).ok_or(RegisterError::BadCert)?;
    let fingerprint = pki::fingerprint_sha256(&der);
    let inserted = sqlx::query(
        "INSERT INTO servers (id, name, kind, cert_fingerprint) \
         VALUES ($1, 'local', 'local', $2) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&fingerprint)
    .execute(db)
    .await
    .map_err(RegisterError::Db)?
    .rows_affected();
    if inserted == 0
        && !is_authorized(db, id, &fingerprint)
            .await
            .map_err(RegisterError::Db)?
    {
        tracing::warn!(
            "serveur local {id} : deja enregistre autrement (autre certificat, ou revoque) ; \
             la ligne existante est conservee"
        );
    }
    Ok(id)
}

/// Enregistre le serveur local depuis `<config_dir>/agents/local.crt`.
/// `Ok(None)` si le fichier n'existe pas (instance initialisee sans agent).
pub async fn register_local_from_dir(
    db: &PgPool,
    config_dir: &Path,
) -> Result<Option<ServerId>, RegisterError> {
    match std::fs::read_to_string(config_dir.join(LOCAL_AGENT_CERT)) {
        Ok(pem) => register_local(db, &pem).await.map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(RegisterError::BadCert),
    }
}
