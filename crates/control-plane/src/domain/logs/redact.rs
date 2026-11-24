//! Masquage des valeurs secretes du projet dans les lignes de journal.
//!
//! Chaque valeur d'au moins `MIN_LEN` caracteres est remplacee par `MASK`,
//! la plus longue d'abord (une valeur qui en contient une autre est masquee
//! en entier). Chaque ligne d'une valeur de plusieurs lignes est aussi une
//! valeur (l'agent envoie une ligne de journal par ligne). Les valeurs viennent d'un `SecretValues` injecte :
//! `domain::variables::VariableSecrets` (variables secretes du projet et
//! globales qui s'y appliquent).

use std::future::Future;
use std::pin::Pin;

use uuid::Uuid;
use zeroize::Zeroizing;

/// Remplacement d'une valeur secrete.
pub const MASK: &str = "••••";
/// Longueur minimale (caracteres) d'une valeur masquee : en dessous, le
/// masquage effacerait des mots ordinaires sans rien proteger.
pub const MIN_LEN: usize = 4;

/// Valeurs secretes illisibles (base indisponible) : les lignes du projet
/// sont retenues, jamais envoyees sans masquage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecretsUnavailable;

impl std::fmt::Display for SecretsUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("masquage indisponible")
    }
}

impl std::error::Error for SecretsUnavailable {}

/// Valeurs secretes d'un projet, ou l'impossibilite de les lire.
pub type SecretsResult = Result<Vec<Zeroizing<String>>, SecretsUnavailable>;

/// Source des valeurs secretes d'un projet (en clair, effacees apres usage).
pub trait SecretValues: Send + Sync {
    fn values(&self, project: Uuid) -> Pin<Box<dyn Future<Output = SecretsResult> + Send + '_>>;
}

/// Aucun secret (tests).
#[derive(Debug, Default)]
pub struct NoSecrets;

impl SecretValues for NoSecrets {
    fn values(&self, _project: Uuid) -> Pin<Box<dyn Future<Output = SecretsResult> + Send + '_>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// Valeurs a masquer d'un projet, de la plus longue a la plus courte.
#[derive(Default)]
pub struct Redactor {
    values: Vec<Zeroizing<String>>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} valeur(s))", self.values.len())
    }
}

impl Redactor {
    pub fn new(values: Vec<Zeroizing<String>>) -> Self {
        // Une valeur de plusieurs lignes (cle PEM...) arrive en plusieurs
        // lignes de journal : chacune de ses lignes est aussi masquee.
        let mut lines = Vec::new();
        for value in &values {
            if value.contains(['\n', '\r']) {
                lines.extend(
                    value
                        .split(['\n', '\r'])
                        .map(|l| Zeroizing::new(l.trim().to_string())),
                );
            }
        }
        let mut values: Vec<_> = values
            .into_iter()
            .chain(lines)
            .filter(|v| v.chars().count() >= MIN_LEN)
            .collect();
        values.sort_by(|a, b| {
            b.len()
                .cmp(&a.len())
                .then_with(|| a.as_str().cmp(b.as_str()))
        });
        values.dedup_by(|a, b| a.as_str() == b.as_str());
        Self { values }
    }

    /// Ligne masquee.
    pub fn redact(&self, line: String) -> String {
        let mut line = line;
        for value in &self.values {
            if line.contains(value.as_str()) {
                line = line.replace(value.as_str(), MASK);
            }
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor(values: &[&str]) -> Redactor {
        Redactor::new(
            values
                .iter()
                .map(|v| Zeroizing::new(v.to_string()))
                .collect(),
        )
    }

    #[test]
    fn longest_value_first_and_short_values_kept() {
        let r = redactor(&["pass", "password123", "abc", "", "pass"]);
        assert_eq!(
            r.redact("password123 pass abc".into()),
            "•••• •••• abc".to_string()
        );
        assert_eq!(r.redact("rien".into()), "rien");
    }

    #[test]
    fn every_occurrence_is_masked() {
        let r = redactor(&["s3cr3t"]);
        assert_eq!(r.redact("s3cr3ts3cr3t=s3cr3t".into()), "••••••••=••••");
        // Quatre caracteres (et non octets) suffisent.
        let r = redactor(&["éàçù"]);
        assert_eq!(r.redact("x éàçù".into()), "x ••••");
    }

    #[test]
    fn multi_line_value_is_masked_line_by_line_and_whole() {
        let pem = "-----BEGIN KEY-----\nAAAAbbbbCCCC\r\n  DDDDeeee\nxy\n-----END KEY-----";
        let r = redactor(&[pem]);
        // Valeur entiere dans un message de plusieurs lignes.
        assert_eq!(r.redact(format!("cle={pem} fin")), "cle=•••• fin");
        // Chaque ligne de la valeur, envoyee seule par l'agent.
        assert_eq!(r.redact("AAAAbbbbCCCC".into()), "••••");
        assert_eq!(r.redact("    DDDDeeee".into()), "    ••••");
        assert_eq!(r.redact("-----END KEY-----".into()), "••••");
        // Morceau de moins de 4 caracteres : garde.
        assert_eq!(r.redact("xy".into()), "xy");
    }

    #[test]
    fn debug_never_shows_values() {
        let r = redactor(&["topsecretvalue"]);
        assert!(!format!("{r:?}").contains("topsecret"));
    }
}
