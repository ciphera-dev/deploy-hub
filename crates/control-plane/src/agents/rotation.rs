//! Rotation de la CA de l'instance (`ciphera admin rotate-ca`, root), voir
//! `docs/operations/certificats.md`.
//!
//! 1. [`start`] : nouvelle CA ecrite a cote de la courante (releve des
//!    certificats du lien et du panneau `ca/ca-next.own` et `ca/ca-next.key`
//!    root 0600, puis `ca/ca-next.crt` root, groupe du service, 0640). Le service la voit en quelques secondes
//!    (`agents::transport::TrustWatch`) : le lien accepte les certificats
//!    d'agent des deux CA, les agents sont reconnectes, recoivent les deux CA
//!    (`TrustedCas`) puis un certificat de la suivante (le signataire signe
//!    les agents avec elle). La CA emettrice du certificat de chaque agent
//!    est notee a sa connexion (`servers.cert_issuer_fingerprint`).
//! 2. [`status`] : serveurs non revoques dont l'agent ne presente pas encore
//!    un certificat de la CA suivante.
//! 3. [`finish`] (`--finish`, manuel) : refuse tant qu'il en reste, ou si
//!    la cle ou les SAN du lien ou du panneau ont change depuis le debut ;
//!    sinon certificats du lien et du panneau re-signes par la suivante
//!    (meme cle), puis la paire `ca/ca.crt` + `ca/ca.key` remplacee par la
//!    suivante (`certs::replace_pair` : jamais un certificat et une cle qui
//!    ne vont pas ensemble), puis `ca/ca-next.*` retires (certificat
//!    d'abord). L'ancienne cle n'existe plus. Une fin interrompue est
//!    reprise par `signer::load_cas` (relancer `--finish`). Le service
//!    recharge le lien, reconnecte les agents, qui ne gardent que la
//!    nouvelle CA.
//!
//! Une rotation commencee ne s'annule pas : retirer la CA suivante couperait
//! les agents deja passes a elle. On la termine (en revoquant au besoin les
//! serveurs qui ne reviendront pas).
//!
//! Le lien et le panneau ne passent a la nouvelle CA qu'a la fin : un agent
//! qui n'a pas encore recu la nouvelle CA (eteint pendant la rotation) peut
//! toujours se connecter et la recevoir.

use std::fmt;
use std::path::Path;

use serde_json::json;
use shared::ids::ServerId;
use sqlx::PgPool;

use super::certs::{self, OWN_CERTS};
use super::signer::{self, Cas};
use super::transport::{CA_CRT, CA_NEXT_CRT};

#[derive(Debug)]
pub struct RotationError(String);

impl fmt::Display for RotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RotationError {}

fn err(what: &str, e: impl fmt::Display) -> RotationError {
    RotationError(format!("{what} : {e}"))
}

/// Empreinte SHA-256 (`AB:CD:...`) du certificat d'une CA.
fn ca_fingerprint(ca: &pki::Ca) -> String {
    pki::der_from_pem(ca.cert_pem())
        .map(|der| pki::fingerprint_sha256(&der))
        .unwrap_or_default()
}

/// SHA-256 brut du certificat d'une CA (valeur de
/// `servers.cert_issuer_fingerprint`).
fn ca_sha256(ca: &pki::Ca) -> Vec<u8> {
    pki::der_from_pem(ca.cert_pem())
        .map(|der| pki::sha256(&der))
        .unwrap_or_default()
}

/// Dossiers remis a root, puis CA lues apres la reprise d'une fin de
/// rotation interrompue (vrai si elle vient d'etre terminee).
fn prepare(config_dir: &Path) -> Result<(Cas, bool), RotationError> {
    signer::harden_config_dir(config_dir).map_err(|e| err("dossier de configuration", e))?;
    signer::harden_ca(config_dir).map_err(|e| err("dossier de la CA", e))?;
    signer::load_cas_settled(config_dir).map_err(|e| err("CA de l'instance illisible", e))
}

/// Releve des certificats du lien et du panneau : `<certificat> <identite>`
/// par ligne (cle publique et SAN, [`pki::server_identity`]).
fn own_identities(config_dir: &Path) -> Result<String, RotationError> {
    let mut out = String::new();
    for (crt, _) in OWN_CERTS {
        let pem = std::fs::read_to_string(config_dir.join(crt)).map_err(|e| err(crt, e))?;
        let id = pki::server_identity(&pem)
            .ok_or_else(|| RotationError(format!("{crt} : certificat serveur illisible")))?;
        out.push_str(&format!("{crt} {id}\n"));
    }
    Ok(out)
}

/// Evenement du journal d'audit de la rotation (`ca.rotate.start`,
/// `ca.rotate.cli`), au mieux : un echec est seulement signale.
pub async fn audit(db: &PgPool, action: &str, fingerprint: &str) {
    let audited = sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, NULL, $2, 'ca', $3)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(action)
    .bind(json!({ "fingerprint": fingerprint }))
    .execute(db)
    .await;
    if let Err(e) = audited {
        tracing::warn!("rotation de la CA non journalisee ({action}) : {e}");
        eprintln!("attention : rotation de la CA non journalisee ({action}) : {e}");
    }
}

/// Issue de [`start`].
#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    /// CA suivante creee (empreinte de son certificat).
    Started(String),
    /// Rotation deja en cours (empreinte de la CA suivante) : rien n'a
    /// change.
    InProgress(String),
}

/// Debut de rotation : CA suivante generee et ecrite (voir le module).
/// Le journal d'audit (`ca.rotate.start`, [`audit`]) est a la charge de
/// l'appelant (la commande ouvre la base).
pub fn start(config_dir: &Path) -> Result<Start, RotationError> {
    let (cas, _) = prepare(config_dir)?;
    if let Some(next) = &cas.next {
        // Rotation commencee sans releve : il est fait maintenant.
        if signer::read_next_own(config_dir)
            .map_err(|e| err(signer::CA_NEXT_OWN, e))?
            .is_none()
        {
            signer::write_next_own(config_dir, &own_identities(config_dir)?)
                .map_err(|e| err(signer::CA_NEXT_OWN, e))?;
        }
        return Ok(Start::InProgress(ca_fingerprint(next)));
    }
    let own = own_identities(config_dir)?;
    let name = format!(
        "Ciphera CA {}",
        time_label(certs::now_unix()).unwrap_or_default()
    );
    let next = pki::Ca::generate(name.trim()).map_err(|e| err("generation de la CA", e))?;
    signer::write_next_ca(config_dir, &next, &own)
        .map_err(|e| err("ecriture de la CA suivante", e))?;
    Ok(Start::Started(ca_fingerprint(&next)))
}

/// Date et heure `AAAA-MM-JJ HH:MM:SSZ` (UTC) d'un instant Unix : nom de la
/// CA, distinct d'une rotation a l'autre.
fn time_label(at: i64) -> Option<String> {
    jiff::Timestamp::from_second(at)
        .ok()
        .map(|t| t.strftime("%Y-%m-%d %H:%M:%SZ").to_string())
}

/// Serveur dont l'agent ne presente pas encore un certificat de la CA
/// suivante.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: ServerId,
    pub name: String,
}

/// Etat de la rotation ([`status`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Empreinte de la CA courante.
    pub current: String,
    /// Empreinte de la CA suivante, pendant une rotation.
    pub next: Option<String>,
    /// Serveurs non revoques encore sur une autre CA (rotation seulement).
    pub remaining: Vec<Pending>,
}

/// Serveurs non revoques dont le certificat presente n'est pas emis par la
/// CA de SHA-256 `issuer` (jamais connectes depuis : compris).
async fn remaining(db: &PgPool, issuer: &[u8]) -> Result<Vec<Pending>, RotationError> {
    let rows: Vec<(ServerId, String)> = sqlx::query_as(
        "SELECT id, name FROM servers WHERE revoked_at IS NULL \
         AND cert_issuer_fingerprint IS DISTINCT FROM $1 ORDER BY name, id",
    )
    .bind(issuer)
    .fetch_all(db)
    .await
    .map_err(|e| err("base de donnees", e))?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| Pending { id, name })
        .collect())
}

/// Etat de la rotation : CA courante, suivante, serveurs restants.
pub async fn status(db: &PgPool, config_dir: &Path) -> Result<Status, RotationError> {
    let (cas, _) = prepare(config_dir)?;
    let (next, remaining) = match &cas.next {
        Some(next) => (
            Some(ca_fingerprint(next)),
            remaining(db, &ca_sha256(next)).await?,
        ),
        None => (None, Vec::new()),
    };
    Ok(Status {
        current: ca_fingerprint(&cas.current),
        next,
        remaining,
    })
}

/// Issue de [`finish`].
#[derive(Debug)]
pub struct Finished {
    /// Empreinte de la nouvelle CA courante.
    pub fingerprint: String,
    /// Certificats du plan de controle re-signes par elle.
    pub server_certs: Vec<&'static str>,
    /// Fin interrompue terminee a la reprise (rien d'autre a faire).
    pub resumed: bool,
}

/// Fin de rotation (voir le module). Refusee sans rotation en cours, ou
/// tant qu'un serveur non revoque presente un certificat d'une autre CA
/// que la suivante (a reconnecter, ou a revoquer : `ciphera admin
/// revoke-agent`), ou si la cle ou les SAN du lien ou du panneau ont change
/// depuis le debut. Relancee apres une interruption, elle reprend.
pub async fn finish(db: &PgPool, config_dir: &Path) -> Result<Finished, RotationError> {
    let (cas, resumed) = prepare(config_dir)?;
    let Some(next) = &cas.next else {
        if resumed {
            let fingerprint = ca_fingerprint(&cas.current);
            audit(db, "ca.rotate.cli", &fingerprint).await;
            return Ok(Finished {
                fingerprint,
                server_certs: Vec::new(),
                resumed: true,
            });
        }
        return Err(RotationError(
            "aucune rotation en cours (ciphera admin rotate-ca pour la commencer)".into(),
        ));
    };
    let left = remaining(db, &ca_sha256(next)).await?;
    if !left.is_empty() {
        let names: Vec<String> = left
            .iter()
            .map(|p| format!("{} ({})", p.name, p.id))
            .collect();
        return Err(RotationError(format!(
            "{} serveur(s) presentent encore un certificat de l'ancienne CA : {} ; attendre \
             leur reconnexion (ciphera admin rotate-ca --status) ou les revoquer \
             (ciphera admin revoke-agent <id>)",
            left.len(),
            names.join(", ")
        )));
    }
    // Lien et panneau inchanges depuis le debut (meme cle, memes SAN) : root
    // ne re-signe pas un contenu change entre-temps dans `tls/`.
    let recorded = signer::read_next_own(config_dir)
        .map_err(|e| err(signer::CA_NEXT_OWN, e))?
        .ok_or_else(|| {
            RotationError(format!(
                "{} absent : relancer ciphera admin rotate-ca (il le refait), puis --finish",
                signer::CA_NEXT_OWN
            ))
        })?;
    if own_identities(config_dir)? != recorded {
        return Err(RotationError(
            "la cle ou les SAN du lien ou du panneau ont change depuis le debut de la \
             rotation : rien n'est re-signe (verifier tls/link.crt et tls/panel.crt)"
                .into(),
        ));
    }
    // 1. Lien et panneau re-signes par la suivante (meme cle) : tous les
    //    agents s'y fient deja.
    let now = certs::now_unix();
    let mut server_certs = Vec::new();
    for pair in OWN_CERTS {
        certs::renew_server_with(config_dir, next, &cas.trusted(), pair, now, true)
            .map_err(|e| err(pair.0, e))?;
        server_certs.push(pair.0);
    }
    // 2. La suivante devient la courante (certificat et cle ensemble).
    let key = next.key_pem();
    certs::replace_pair(
        config_dir,
        (CA_CRT, certs::CA_KEY),
        next.cert_pem().as_bytes(),
        key.as_bytes(),
    )
    .map_err(|e| err("installation de la nouvelle CA", e))?;
    // 3. Fichiers de la suivante retires.
    signer::remove_next_ca(config_dir).map_err(|e| err(CA_NEXT_CRT, e))?;
    let fingerprint = ca_fingerprint(next);
    audit(db, "ca.rotate.cli", &fingerprint).await;
    Ok(Finished {
        fingerprint,
        server_certs,
        resumed: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_name_carries_the_utc_date_and_time() {
        assert_eq!(time_label(0).as_deref(), Some("1970-01-01 00:00:00Z"));
        assert_eq!(
            time_label(1_764_028_800 + 3_723).as_deref(),
            Some("2025-11-25 01:02:03Z")
        );
        assert_eq!(
            time_label(951_782_400).as_deref(),
            Some("2000-02-29 00:00:00Z")
        );
    }
}
