//! Demande de mise a jour de Ciphera, deposee par le plan de controle
//! (`POST /api/system/update`) et lue par l'updater (`ciphera-cli updater`,
//! root) : `<data_dir>/update-request.json`, signee par HMAC-SHA256 avec une
//! cle derivee de l'identite age de l'instance (`age.key`).
//!
//! Le HMAC prouve que la demande vient d'un detenteur de la cle d'instance,
//! pas qu'elle est sans danger : un processus `ciphera` compromis la detient.
//! L'updater refuse donc toute version inferieure ou egale a la version
//! installee (seul `--allow-downgrade`, lance par root, l'autorise) et ne
//! fait confiance qu'aux releases signees minisign.

use std::fmt;
use std::str::FromStr;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::secretbox::SecretBox;

/// Dossier des demandes, surveille par `ciphera-updater.path` : le dossier
/// de donnees par defaut du plan de controle (`CIPHERA_DATA_DIR`). Avec un
/// autre dossier de donnees, les mises a jour par le panneau sont refusees.
pub const REQUEST_DIR: &str = "/var/lib/ciphera";
/// Nom du fichier de demande dans [`REQUEST_DIR`].
pub const REQUEST_FILE: &str = "update-request.json";
/// Taille maximale du fichier de demande.
pub const MAX_REQUEST_BYTES: u64 = 4096;
/// Age maximal d'une demande (secondes) : une demande plus ancienne est
/// ignoree (rejeu d'un fichier oublie).
pub const MAX_AGE_SECS: u64 = 15 * 60;
/// Avance d'horloge toleree sur `requested_at` (secondes).
pub const MAX_SKEW_SECS: u64 = 5 * 60;
/// Usage de la cle derivee de l'identite de l'instance.
const KEY_LABEL: &[u8] = b"ciphera/update-request/v1";

/// Version `vX.Y.Z` d'une release (meme forme que `install.sh --version`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// Version `CARGO_PKG_VERSION` (`X.Y.Z`) d'un binaire.
    pub fn from_pkg(pkg: &str) -> Option<Version> {
        format!("v{pkg}").parse().ok()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidVersion;

impl fmt::Display for InvalidVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("version invalide (attendu vX.Y.Z)")
    }
}

impl std::error::Error for InvalidVersion {}

impl FromStr for Version {
    type Err = InvalidVersion;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let rest = s.strip_prefix('v').ok_or(InvalidVersion)?;
        let mut parts = rest.split('.');
        let mut next = || -> Result<u32, InvalidVersion> {
            let p = parts.next().ok_or(InvalidVersion)?;
            if p.is_empty() || p.len() > 9 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(InvalidVersion);
            }
            p.parse().map_err(|_| InvalidVersion)
        };
        let v = Version {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return Err(InvalidVersion);
        }
        Ok(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Contenu du fichier de demande.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    /// Version demandee (`vX.Y.Z`).
    pub version: String,
    /// Secondes depuis l'epoque Unix.
    pub requested_at: u64,
    /// HMAC-SHA256 hexadecimal (minuscules) de [`UpdateRequest::message`].
    pub hmac: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RequestError {
    /// JSON illisible ou champ inconnu.
    Malformed,
    /// HMAC faux : demande ignoree.
    BadHmac,
    /// Demande trop ancienne, ou datee du futur.
    Stale,
    /// Version mal formee.
    Version,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "demande illisible",
            Self::BadHmac => "HMAC de la demande invalide",
            Self::Stale => "demande perimee ou datee du futur",
            Self::Version => "version demandee invalide",
        })
    }
}

impl std::error::Error for RequestError {}

/// Cle HMAC des demandes, derivee de l'identite de l'instance.
pub fn request_key(secrets: &SecretBox) -> zeroize::Zeroizing<[u8; 32]> {
    secrets.derive_key(KEY_LABEL)
}

fn mac(key: &[u8], version: &str, requested_at: u64) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepte toute cle");
    mac.update(UpdateRequest::message(version, requested_at).as_bytes());
    mac
}

impl UpdateRequest {
    /// Message signe : contexte, version et date, une valeur par ligne.
    pub fn message(version: &str, requested_at: u64) -> String {
        format!("ciphera-update-request\n{version}\n{requested_at}")
    }

    pub fn sign(version: Version, requested_at: u64, key: &[u8]) -> UpdateRequest {
        let version = version.to_string();
        let tag = mac(key, &version, requested_at).finalize().into_bytes();
        UpdateRequest {
            hmac: tag.iter().map(|b| format!("{b:02x}")).collect(),
            version,
            requested_at,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("serialisation d'une structure simple")
    }

    pub fn from_json(bytes: &[u8]) -> Result<UpdateRequest, RequestError> {
        serde_json::from_slice(bytes).map_err(|_| RequestError::Malformed)
    }

    /// Verifie le HMAC (comparaison en temps constant), puis la fraicheur
    /// par rapport a `now`, et renvoie la version demandee.
    pub fn verify(&self, key: &[u8], now: u64) -> Result<Version, RequestError> {
        let tag = decode_hex(&self.hmac).ok_or(RequestError::BadHmac)?;
        mac(key, &self.version, self.requested_at)
            .verify_slice(&tag)
            .map_err(|_| RequestError::BadHmac)?;
        if self.requested_at > now.saturating_add(MAX_SKEW_SECS)
            || now.saturating_sub(self.requested_at) > MAX_AGE_SECS
        {
            return Err(RequestError::Stale);
        }
        self.version.parse().map_err(|_| RequestError::Version)
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() != 64 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn versions_parse_and_compare_numerically() {
        assert_eq!(v("v0.1.0").to_string(), "v0.1.0");
        assert!(v("v0.10.0") > v("v0.9.9"));
        assert!(v("v1.0.0") > v("v0.99.99"));
        assert_eq!(Version::from_pkg("0.1.0"), Some(v("v0.1.0")));
        for bad in [
            "",
            "0.1.0",
            "v0.1",
            "v0.1.0.1",
            "v0.1.x",
            "v-1.0.0",
            "v0..1",
            "v0.1.0 ",
            "v+1.0.0",
            "v1234567890.0.0",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn signed_request_verifies_and_rejects_tampering() {
        let key = [7u8; 32];
        let req = UpdateRequest::sign(v("v0.2.0"), 1_000, &key);
        let back = UpdateRequest::from_json(req.to_json().as_bytes()).unwrap();
        assert_eq!(back.verify(&key, 1_010), Ok(v("v0.2.0")));
        assert_eq!(back.verify(&[8u8; 32], 1_010), Err(RequestError::BadHmac));

        let mut other = back.clone();
        other.version = "v0.3.0".into();
        assert_eq!(other.verify(&key, 1_010), Err(RequestError::BadHmac));
        let mut other = back.clone();
        other.requested_at += 1;
        assert_eq!(other.verify(&key, 1_010), Err(RequestError::BadHmac));
        let mut other = back.clone();
        other.hmac = "zz".into();
        assert_eq!(other.verify(&key, 1_010), Err(RequestError::BadHmac));

        assert_eq!(
            back.verify(&key, 1_000 + MAX_AGE_SECS + 1),
            Err(RequestError::Stale)
        );
        assert_eq!(
            UpdateRequest::sign(v("v0.2.0"), 1_000 + MAX_SKEW_SECS + 1, &key).verify(&key, 1_000),
            Err(RequestError::Stale)
        );
        assert_eq!(
            UpdateRequest::from_json(br#"{"version":"v0.2.0","requested_at":1,"hmac":"","x":1}"#),
            Err(RequestError::Malformed)
        );
    }

    #[test]
    fn request_dir_is_the_default_data_dir() {
        assert_eq!(REQUEST_DIR, crate::config::Config::DEFAULT_DATA_DIR);
    }

    #[test]
    fn request_key_depends_on_the_instance_identity() {
        let a = SecretBox::from_identity(age::x25519::Identity::generate());
        let b = SecretBox::from_identity(age::x25519::Identity::generate());
        assert_eq!(*request_key(&a), *request_key(&a));
        assert_ne!(*request_key(&a), *request_key(&b));
    }
}
