//! Derniere version publiee : `releases/latest.json`, signe minisign
//! (`releases/latest.json.minisig`) par `install/sign-latest.sh`, lu par le
//! plan de controle (verification des mises a jour du panneau) et par
//! `ciphera-cli updater --latest`.
//!
//! Le commentaire de confiance de la signature (couvert par la signature
//! globale minisign) porte la version et une echeance :
//! `ciphera-latest version=vX.Y.Z issued=<secondes Unix> expires=<issued +
//! 120 jours>`. Un fichier n'est utilise que si :
//! - la signature (et la signature globale, donc le commentaire) vient d'une
//!   cle de confiance integree (`install/ciphera.pub`, plusieurs pendant une
//!   rotation ; jamais les cles retirees) ;
//! - le commentaire a exactement cette forme ;
//! - la version du JSON est celle du commentaire.
//!
//! Sinon il est ignore. Une echeance passee ne bloque rien : elle signale
//! seulement des metadonnees trop anciennes ([`SignedLatest::expired`]).
//! `refresh-latest.yml` re-signe le fichier chaque mois avec une nouvelle
//! echeance.

use std::fmt;

use serde::Deserialize;

use crate::update::Version;

/// Taille maximale de `latest.json` et de sa signature.
pub const MAX_LATEST_BYTES: usize = 4096;
/// Duree de validite d'une signature de `latest.json` (120 jours).
pub const VALIDITY_SECS: u64 = 120 * 24 * 3600;
/// Premier mot du commentaire de confiance.
const COMMENT_TAG: &str = "ciphera-latest";

/// Cles publiques minisign de confiance des releases (celles injectees dans
/// `install.sh` publie ; plusieurs pendant une rotation).
const PUBKEY_FILE: &str = include_str!("../../../install/ciphera.pub");

/// Cles integrees (`install/ciphera.pub` : chaque ligne qui n'est ni vide
/// ni un commentaire minisign).
pub fn embedded_pubkeys() -> Vec<String> {
    PUBKEY_FILE
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
        .map(str::to_string)
        .collect()
}

/// `latest.json` verifie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedLatest {
    pub version: Version,
    /// Champ `changelog_url` du JSON, non filtre (a valider par l'appelant).
    pub changelog_url: Option<String>,
    /// Date de signature (secondes Unix).
    pub issued: u64,
    /// Echeance (secondes Unix).
    pub expires: u64,
}

impl SignedLatest {
    /// Echeance passee a `now` (secondes Unix) : metadonnees trop anciennes
    /// pour garantir que la version annoncee est la derniere.
    pub fn expired(&self, now: u64) -> bool {
        now >= self.expires
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LatestError {
    /// Cle de confiance mal formee.
    Key,
    /// Signature illisible, ou d'aucune cle de confiance.
    Signature,
    /// Commentaire de confiance absent ou mal forme.
    Comment,
    /// JSON illisible, trop long ou version mal formee.
    Json,
    /// Version du JSON differente de celle du commentaire signe.
    VersionMismatch,
}

impl fmt::Display for LatestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Key => "latest.json : cle publique minisign invalide",
            Self::Signature => {
                "latest.json : signature minisign invalide (aucune cle de confiance)"
            }
            Self::Comment => "latest.json : commentaire de confiance mal forme",
            Self::Json => "latest.json : contenu invalide",
            Self::VersionMismatch => {
                "latest.json : version differente de celle du commentaire signe"
            }
        })
    }
}

impl std::error::Error for LatestError {}

#[derive(Deserialize)]
struct LatestJson {
    version: String,
    #[serde(default)]
    changelog_url: Option<String>,
}

/// Entier decimal sans zero en tete (forme canonique), 20 chiffres au plus.
fn parse_secs(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = s.parse().ok()?;
    (n.to_string() == s).then_some(n)
}

/// Version sous sa forme canonique seulement (`v1.2.3`, pas `v01.2.3`).
fn parse_version(s: &str) -> Option<Version> {
    let v: Version = s.parse().ok()?;
    (v.to_string() == s).then_some(v)
}

/// Commentaire `ciphera-latest version=<v> issued=<n> expires=<n>`, exact :
/// champs dans cet ordre, separes par une espace, rien d'autre ; echeance
/// apres la signature et au plus [`VALIDITY_SECS`] plus tard.
fn parse_comment(comment: &str) -> Result<(Version, u64, u64), LatestError> {
    let mut parts = comment.split(' ');
    let mut field = |name: &str| -> Result<&str, LatestError> {
        parts
            .next()
            .and_then(|p| p.strip_prefix(name))
            .ok_or(LatestError::Comment)
    };
    if !field(COMMENT_TAG)?.is_empty() {
        return Err(LatestError::Comment);
    }
    let version = parse_version(field("version=")?).ok_or(LatestError::Comment)?;
    let issued = parse_secs(field("issued=")?).ok_or(LatestError::Comment)?;
    let expires = parse_secs(field("expires=")?).ok_or(LatestError::Comment)?;
    if parts.next().is_some() || expires <= issued || expires - issued > VALIDITY_SECS {
        return Err(LatestError::Comment);
    }
    Ok((version, issued, expires))
}

/// Verifie `json` (contenu exact de `latest.json`) avec `sig` (contenu de
/// `latest.json.minisig`) et l'une des cles `pubkeys` (base64, `RW…`). Une
/// cle mal formee est une erreur, jamais ignoree.
pub fn verify_latest(
    pubkeys: &[String],
    json: &[u8],
    sig: &str,
) -> Result<SignedLatest, LatestError> {
    let keys = pubkeys
        .iter()
        .map(|k| minisign_verify::PublicKey::from_base64(k))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| LatestError::Key)?;
    if json.len() > MAX_LATEST_BYTES || sig.len() > MAX_LATEST_BYTES {
        return Err(LatestError::Json);
    }
    let sig = minisign_verify::Signature::decode(sig).map_err(|_| LatestError::Signature)?;
    // Signature du fichier et signature globale (signature + commentaire de
    // confiance) : `verify` controle les deux.
    if !keys.iter().any(|pk| pk.verify(json, &sig, false).is_ok()) {
        return Err(LatestError::Signature);
    }
    let (signed_version, issued, expires) = parse_comment(sig.trusted_comment())?;
    let parsed: LatestJson = serde_json::from_slice(json).map_err(|_| LatestError::Json)?;
    let version = parse_version(&parsed.version).ok_or(LatestError::Json)?;
    if version != signed_version {
        return Err(LatestError::VersionMismatch);
    }
    Ok(SignedLatest {
        version,
        changelog_url: parsed.changelog_url,
        issued,
        expires,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_is_strict() {
        let ok = "ciphera-latest version=v1.2.3 issued=100 expires=10368100";
        assert_eq!(
            parse_comment(ok),
            Ok(("v1.2.3".parse().unwrap(), 100, 10_368_100))
        );
        for bad in [
            "",
            "ciphera-latest",
            "ciphera-latestx version=v1.2.3 issued=100 expires=200",
            "ciphera-latest version=v1.2.3 issued=100 expires=200 extra",
            "ciphera-latest version=v1.2.3  issued=100 expires=200",
            "ciphera-latest version=v1.2.3 expires=200 issued=100",
            "ciphera-latest version=1.2.3 issued=100 expires=200",
            "ciphera-latest version=v01.2.3 issued=100 expires=200",
            "ciphera-latest version=v1.2.3 issued=0100 expires=200",
            "ciphera-latest version=v1.2.3 issued=+100 expires=200",
            "ciphera-latest version=v1.2.3 issued=100 expires=100",
            "ciphera-latest version=v1.2.3 issued=200 expires=100",
            "ciphera-latest version=v1.2.3 issued=100 expires=10368101",
            "ciphera-latest version=v1.2.3 issued=100 expires=200\t",
            "timestamp:100\tfile:latest.json",
        ] {
            assert_eq!(parse_comment(bad), Err(LatestError::Comment), "{bad:?}");
        }
    }

    #[test]
    fn embedded_keys_are_minisign_keys() {
        let keys = embedded_pubkeys();
        assert!(!keys.is_empty());
        for k in &keys {
            assert!(minisign_verify::PublicKey::from_base64(k).is_ok(), "{k}");
        }
    }
}
