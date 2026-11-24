//! Coffre de secrets de l'instance : chiffrement age (identite X25519) des
//! valeurs sensibles stockees en base. Le meme coffre sert au proxy pour ses
//! certificats (`shared::secretbox`).

pub use shared::secretbox::{SecretBox, SecretError};
