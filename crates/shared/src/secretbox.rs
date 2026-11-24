//! Coffre de secrets : chiffrement age (identite X25519). Le plan de
//! controle y chiffre les valeurs sensibles stockees en base (cles de
//! deploiement, jetons...), le proxy ses certificats et son compte ACME.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use age::x25519::Identity;
use zeroize::Zeroizing;

#[derive(Debug)]
pub struct SecretError(&'static str);

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "coffre de secrets : {}", self.0)
    }
}

impl std::error::Error for SecretError {}

pub struct SecretBox {
    identity: Identity,
}

impl fmt::Debug for SecretBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretBox").finish_non_exhaustive()
    }
}

impl SecretBox {
    /// Charge l'identite age ecrite par `ciphera init` (`age.key`). Les lignes
    /// vides et les commentaires (`#`) sont ignores.
    pub fn load(identity_path: &Path) -> Result<SecretBox, SecretError> {
        let content = Zeroizing::new(
            std::fs::read_to_string(identity_path)
                .map_err(|_| SecretError("fichier d'identite illisible"))?,
        );
        let line = content
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .ok_or(SecretError("fichier d'identite vide"))?;
        let identity =
            Identity::from_str(line).map_err(|_| SecretError("identite age invalide"))?;
        Ok(SecretBox { identity })
    }

    pub fn from_identity(identity: Identity) -> SecretBox {
        SecretBox { identity }
    }

    /// Nouvelle identite aleatoire et le contenu du fichier d'identite a
    /// ecrire (mode 0600) ; ce contenu est efface de la memoire a sa
    /// destruction.
    pub fn generate() -> (SecretBox, Zeroizing<String>) {
        use age::secrecy::ExposeSecret;
        let identity = Identity::generate();
        let line = Zeroizing::new(format!("{}\n", identity.to_string().expose_secret()));
        (SecretBox { identity }, line)
    }

    /// Chiffre pour l'identite de l'instance.
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
        age::encrypt(&self.identity.to_public(), plaintext)
            .map_err(|_| SecretError("chiffrement impossible"))
    }

    /// Cle de 32 octets propre a l'usage `label`, derivee de l'identite de
    /// l'instance (HMAC-SHA256 de `label` sous la cle secrete age) : la meme
    /// identite donne toujours la meme cle, un autre `label` une autre cle.
    #[cfg(feature = "update")]
    pub fn derive_key(&self, label: &[u8]) -> Zeroizing<[u8; 32]> {
        use age::secrecy::ExposeSecret;
        use hmac::{Hmac, KeyInit, Mac};
        let secret = self.identity.to_string();
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret.expose_secret().as_bytes())
            .expect("HMAC accepte toute cle");
        mac.update(label);
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&mac.finalize().into_bytes());
        key
    }

    /// Dechiffre ; le clair est efface de la memoire a sa destruction.
    pub fn open(&self, sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, SecretError> {
        age::decrypt(&self.identity, sealed)
            .map(Zeroizing::new)
            .map_err(|_| SecretError("dechiffrement impossible"))
    }
}

/// Debut du clair d'un chiffre lie (`seal_bound`).
const BOUND_MAGIC: &[u8] = b"ciphera-bound/1\n";

impl SecretBox {
    /// Chiffre `plaintext` lie a `label` (la ligne qui le stocke, par
    /// exemple `table|id|cle`) : [`open_bound`](Self::open_bound) ne l'ouvre
    /// que pour ce meme `label`. Un chiffre copie dans une autre ligne
    /// devient illisible (equivalent des donnees associees d'un AEAD, que le
    /// format age n'offre pas). Clair : `BOUND_MAGIC`, longueur du label
    /// (u32 gros-boutiste), label, valeur.
    pub fn seal_bound(&self, label: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
        let len = u32::try_from(label.len()).map_err(|_| SecretError("etiquette trop longue"))?;
        let mut framed = Zeroizing::new(Vec::with_capacity(
            BOUND_MAGIC.len() + 4 + label.len() + plaintext.len(),
        ));
        framed.extend_from_slice(BOUND_MAGIC);
        framed.extend_from_slice(&len.to_be_bytes());
        framed.extend_from_slice(label);
        framed.extend_from_slice(plaintext);
        self.seal(&framed)
    }

    /// Dechiffre un chiffre lie a `label` ; refuse un autre label et un
    /// chiffre non lie (`seal`).
    pub fn open_bound(
        &self,
        label: &[u8],
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SecretError> {
        let clear = self.open(sealed)?;
        let rest = clear
            .strip_prefix(BOUND_MAGIC)
            .ok_or(SecretError("chiffre non lie a sa ligne"))?;
        let (len, rest) = rest
            .split_first_chunk::<4>()
            .ok_or(SecretError("chiffre lie tronque"))?;
        let len = u32::from_be_bytes(*len) as usize;
        if rest.len() < len || &rest[..len] != label {
            return Err(SecretError("chiffre lie a une autre ligne"));
        }
        Ok(Zeroizing::new(rest[len..].to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_ciphertext_opens_only_for_its_label() {
        let (sb, _) = SecretBox::generate();
        let sealed = sb.seal_bound(b"project|env-1|KEY", b"valeur").unwrap();
        assert_eq!(
            sb.open_bound(b"project|env-1|KEY", &sealed)
                .unwrap()
                .as_slice(),
            b"valeur"
        );
        assert!(sb.open_bound(b"project|env-2|KEY", &sealed).is_err());
        assert!(sb.open_bound(b"project|env-1|KEY2", &sealed).is_err());
        assert!(sb.open_bound(b"project|env-1|KE", &sealed).is_err());
        // Chiffre non lie (format d'avant) : refuse.
        let legacy = sb.seal(b"valeur").unwrap();
        assert!(sb.open_bound(b"project|env-1|KEY", &legacy).is_err());
        // Valeur vide et label vide.
        let empty = sb.seal_bound(b"", b"").unwrap();
        assert_eq!(sb.open_bound(b"", &empty).unwrap().as_slice(), b"");
    }
}
