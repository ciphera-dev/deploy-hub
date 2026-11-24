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

    /// Ligne de journal masquee : octets nuls et sequences ANSI/controle
    /// retires AVANT le masquage (un secret entrecoupe de codes ANSI doit
    /// etre reconnu). Point d'entree unique du direct, de l'enregistrement
    /// et de la relecture : aucun chemin ne masque une ligne brute.
    pub fn redact_line(&self, line: &str) -> String {
        let line: String = line.chars().filter(|c| *c != '\0').collect();
        self.redact(super::neutralize(&line))
    }

    /// Ligne masquee. Les octets nuls sont retires d'abord, au meme endroit
    /// pour le direct, l'enregistrement et le telechargement : un octet nul
    /// glisse dans une valeur ne la soustrait pas au masquage. La coupe a la
    /// longueur maximale vient toujours apres.
    pub fn redact(&self, line: String) -> String {
        let mut line = line;
        if line.contains('\0') {
            line.retain(|c| c != '\0');
        }
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
    fn nul_inside_secret_redacted_live_and_stored() {
        let r = redactor(&["s3cr3t-value"]);
        let line = "x s3cr\u{0}3t-va\u{0}lue y".to_string();
        // Direct : masquage puis coupe, comme le hub.
        let live = crate::domain::logs::LogEvent::line(
            0,
            proto::LogStream::Stdout,
            "web",
            r.redact(line.clone()),
        );
        assert_eq!(live.message, "x •••• y");
        // Enregistre : meme resultat, aucun octet nul.
        let stored = r.redact(line);
        assert_eq!(stored, "x •••• y");
        assert!(!stored.contains('\0'));
    }

    #[test]
    fn secret_across_16k_boundary_not_leaked() {
        let secret = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd";
        assert_eq!(secret.len(), 40);
        let r = redactor(&[secret]);
        // Valeur a cheval sur l'octet 16 384, a plusieurs decalages.
        for shift in 0..40 {
            let head = "a".repeat(crate::domain::logs::MAX_LINE_BYTES - shift);
            let line = format!("{head}{secret}{}", "b".repeat(100));
            let event = crate::domain::logs::LogEvent::line(
                0,
                proto::LogStream::Stdout,
                "web",
                r.redact(line),
            );
            assert!(event.message.len() <= crate::domain::logs::MAX_LINE_BYTES);
            for i in 0..=secret.len() - 4 {
                assert!(
                    !event.message.contains(&secret[i..i + 4]),
                    "fragment {i} (decalage {shift})"
                );
            }
        }
        assert_eq!(crate::domain::logs::MAX_LINE_BYTES, 16 * 1024);
    }

    #[test]
    fn debug_never_shows_values() {
        let r = redactor(&["topsecretvalue"]);
        assert!(!format!("{r:?}").contains("topsecret"));
    }
}
