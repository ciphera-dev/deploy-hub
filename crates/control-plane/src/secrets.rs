//! Coffre de secrets de l'instance : chiffrement age (identite X25519) des
//! valeurs sensibles stockees en base. Le meme coffre sert au proxy pour ses
//! certificats (`shared::secretbox`).

pub use shared::secretbox::{SecretBox, SecretError};

/// Etiquettes de liaison (`SecretBox::seal_bound`) des secrets stockes en
/// base : un chiffre copie dans une autre ligne ou une autre colonne devient
/// illisible. Format `table|colonne|id de la ligne`.
pub mod labels {
    use uuid::Uuid;

    /// Mot de passe SMTP (ligne unique `id = true`).
    pub const SMTP_PASSWORD: &[u8] = b"smtp_settings|password_sealed|true";
    /// Cle privee de la GitHub App (ligne unique `id = true`).
    pub const GITHUB_PRIVATE_KEY: &[u8] = b"github_app|private_key_sealed|true";
    /// Secret de webhook de la GitHub App (ligne unique `id = true`).
    pub const GITHUB_WEBHOOK_SECRET: &[u8] = b"github_app|webhook_secret_sealed|true";

    /// Graine TOTP du compte, confirmee ou en attente d'enrolement.
    pub fn totp_seed(user_id: Uuid) -> Vec<u8> {
        format!("user_totp|secret_sealed|{user_id}").into_bytes()
    }
}
