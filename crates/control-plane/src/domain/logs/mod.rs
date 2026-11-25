//! Logs en direct (build et execution).
//!
//! - `redact` : masquage des valeurs secretes du projet (`••••`) ;
//! - `hub` : diffusion des lignes des agents aux clients, un suivi par cible
//!   chez l'agent tant qu'il reste un lecteur, tampon borne par client ;
//! - `store` : lignes des deploiements enregistrees en base (plafonnees),
//!   pour l'historique et `logs.txt`.
//!
//! Aucune ligne ne quitte le plan de controle sans masquage : chaque ligne
//! est masquee a la reception (diffusion et enregistrement), et les lignes
//! relues en base le sont de nouveau (secrets ajoutes depuis).

pub mod hub;
pub mod redact;
pub mod store;

use proto::LogStream;
use serde::Serialize;

/// Conteneur affiche pour les lignes d'un deploiement (build et etapes).
pub const BUILD_CONTAINER: &str = "build";
/// Conteneur affiche pour les messages du plan de controle (lignes perdues).
pub const SYSTEM_CONTAINER: &str = "ciphera";
/// Longueur maximale d'une ligne (octets), au-dela : tronquee. La coupe se
/// fait toujours apres le masquage de la ligne entiere.
pub const MAX_LINE_BYTES: usize = 16 * 1024;

/// Longueur maximale d'une ligne brute a la reception (octets), avant tout
/// masquage : celle que l'agent reassemble. La coupe a `MAX_LINE_BYTES` se
/// fait apres le masquage (couper avant pourrait laisser le debut d'un
/// secret) ; ce plafond borne seulement ce qu'un agent peut faire garder en
/// memoire en attendant le masquage.
pub const MAX_RECEIVED_LINE_BYTES: usize = 64 * 1024;

/// Coupe une ligne brute recue a `MAX_RECEIVED_LINE_BYTES`, sur une
/// frontiere de caractere.
pub fn cap_received(mut line: String) -> String {
    if line.len() > MAX_RECEIVED_LINE_BYTES {
        let mut end = MAX_RECEIVED_LINE_BYTES;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    line
}

/// Ligne envoyee au panneau : `{ ts, level, container, message }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LogEvent {
    /// RFC 3339 UTC a la milliseconde.
    pub ts: String,
    /// `info` (sortie standard), `error` (sortie d'erreur), `warn` (message
    /// du plan de controle).
    pub level: &'static str,
    pub container: String,
    pub message: String,
    /// Horodatage source (millisecondes), pour ecarter les doublons.
    #[serde(skip)]
    pub ts_ms: i64,
}

impl LogEvent {
    /// Ligne deja masquee.
    pub fn line(ts_ms: i64, stream: LogStream, container: &str, message: String) -> Self {
        Self {
            ts: format_ts(ts_ms),
            level: level(stream),
            container: container.to_string(),
            message: truncate(message),
            ts_ms,
        }
    }

    /// Marqueur « N lignes perdues » d'un client trop lent.
    pub fn lost(count: u64) -> Self {
        let ts_ms = jiff::Timestamp::now().as_millisecond();
        Self {
            ts: format_ts(ts_ms),
            level: "warn",
            container: SYSTEM_CONTAINER.to_string(),
            message: format!("{count} lignes perdues"),
            ts_ms,
        }
    }

    /// Marqueur du plan de controle (niveau `warn`).
    pub fn notice(message: &str) -> Self {
        let ts_ms = jiff::Timestamp::now().as_millisecond();
        Self {
            ts: format_ts(ts_ms),
            level: "warn",
            container: SYSTEM_CONTAINER.to_string(),
            message: message.to_string(),
            ts_ms,
        }
    }

    /// Ligne de `logs.txt` : `<ts> [INFO|ERR|WARN] <conteneur> | <message>`.
    pub fn text_line(&self) -> String {
        let tag = match self.level {
            "error" => "ERR",
            "warn" => "WARN",
            _ => "INFO",
        };
        format!(
            "{} [{tag}] {} | {}\n",
            self.ts,
            neutralize(&self.container),
            neutralize(&self.message)
        )
    }
}

/// Neutralise pour un fichier texte : les sequences ANSI (CSI, OSC et
/// echappements a deux octets) sont retirees, les autres caracteres de
/// controle C0/C1 (dont retour a la ligne) deviennent une espace, sauf `\t`.
pub fn neutralize(text: &str) -> String {
    strip_escapes(text)
        .chars()
        .map(|c| if c != '\t' && c.is_control() { ' ' } else { c })
        .collect()
}

/// Retire les sequences ANSI (CSI, OSC et echappements a deux octets) et
/// tout ESC isole ; les autres caracteres (retours a la ligne compris) sont
/// gardes.
pub fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.peek().copied() {
                // CSI : parametres et intermediaires, puis un octet final.
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&n) {
                            break;
                        }
                    }
                }
                // OSC : jusqu'a BEL ou ST (ESC \).
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            chars.next_if_eq(&'\\');
                            break;
                        }
                    }
                }
                Some(n) if ('\x20'..='\x7e').contains(&n) => {
                    chars.next();
                }
                _ => {}
            },
            c => out.push(c),
        }
    }
    out
}

pub fn level(stream: LogStream) -> &'static str {
    match stream {
        LogStream::Stdout => "info",
        LogStream::Stderr => "error",
    }
}

/// RFC 3339 UTC a la milliseconde (`2025-11-22T08:26:40.123Z`).
pub fn format_ts(ts_ms: i64) -> String {
    match jiff::Timestamp::from_millisecond(ts_ms) {
        Ok(t) => t.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        Err(_) => "1970-01-01T00:00:00.000Z".to_string(),
    }
}

/// Coupe la ligne a `MAX_LINE_BYTES`, sur une frontiere de caractere.
pub fn truncate(mut line: String) -> String {
    if line.len() > MAX_LINE_BYTES {
        let mut end = MAX_LINE_BYTES;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_and_levels() {
        assert_eq!(format_ts(1_763_800_000_123), "2025-11-22T08:26:40.123Z");
        assert_eq!(format_ts(0), "1970-01-01T00:00:00.000Z");
        let e = LogEvent::line(5, LogStream::Stderr, "web", "boom".into());
        assert_eq!(e.level, "error");
        assert_eq!(e.text_line(), "1970-01-01T00:00:00.005Z [ERR] web | boom\n");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"ts": "1970-01-01T00:00:00.005Z", "level": "error", "container": "web", "message": "boom"})
        );
        assert_eq!(LogEvent::lost(3).message, "3 lignes perdues");
    }

    #[test]
    fn logs_txt_strips_ansi() {
        let e = LogEvent::line(
            5,
            LogStream::Stdout,
            "web",
            "\x1b[31mrouge\x1b[0m\ta\x1b]0;titre\x07b\nFAUX\r\x01\u{85}fin\x1b[".into(),
        );
        assert_eq!(
            e.text_line(),
            "1970-01-01T00:00:00.005Z [INFO] web | rouge\ta".to_string() + "b FAUX   fin\n"
        );
    }

    #[test]
    fn truncate_keeps_char_boundaries() {
        let s = truncate("é".repeat(MAX_LINE_BYTES));
        assert_eq!(s.len(), MAX_LINE_BYTES);
        let s = truncate(format!("a{}", "é".repeat(MAX_LINE_BYTES)));
        assert_eq!(s.len(), MAX_LINE_BYTES - 1);
        assert_eq!(truncate("court".into()), "court");
    }
}
