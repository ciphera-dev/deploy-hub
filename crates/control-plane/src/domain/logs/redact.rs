//! Masquage des valeurs secretes du projet dans les lignes de journal.
//!
//! Chaque valeur d'au moins `MIN_LEN` caracteres est remplacee par `MASK`.
//! Chaque ligne d'une valeur de plusieurs lignes est aussi une valeur
//! (l'agent envoie une ligne de journal par ligne) : ce sont les
//! « fragments » du projet, `MAX_FRAGMENTS` au plus (plafond verifie a
//! l'ecriture des variables). Les valeurs viennent d'un `SecretValues`
//! injecte : `domain::variables::VariableSecrets` (variables secretes du
//! projet et globales qui s'y appliquent).
//!
//! Formes masquees de chaque fragment : la valeur telle quelle, ses formes
//! nettoyees (octets nuls, sequences ANSI et caracteres de controle traites
//! comme dans la ligne ou le message d'erreur), et, de `ENCODED_MIN_BYTES`
//! a `ENCODED_MAX_BYTES` octets, ses formes base64 (standard et URL, avec ou
//! sans remplissage, a chacun des trois alignements possibles dans un texte
//! encode plus long) et son encodage pourcent.
//!
//! Recherche : un seul automate Aho-Corasick par jeu de secrets, construit
//! une fois (garde par le `LogHub` jusqu'a la modification des secrets).
//! Une ligne est parcourue une seule fois : temps lineaire en sa longueur,
//! quel que soit le nombre de motifs. Toutes les occurrences sont masquees,
//! meme quand elles se chevauchent ou s'emboitent : la zone masquee est la
//! reunion des occurrences de tous les motifs.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use aho_corasick::automaton::{Automaton, StateID};
use aho_corasick::nfa::contiguous::NFA;
use aho_corasick::{Anchored, MatchKind};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD};
use uuid::Uuid;
use zeroize::Zeroizing;

/// Remplacement d'une valeur secrete.
pub const MASK: &str = "••••";
/// Longueur minimale (caracteres) d'une valeur masquee : en dessous, le
/// masquage effacerait des mots ordinaires sans rien proteger.
pub const MIN_LEN: usize = 4;
/// Fragments masquables au plus par projet, globales comprises.
pub const MAX_FRAGMENTS: usize = 1000;
/// Taille (octets) a partir de laquelle les formes encodees d'un fragment
/// (base64, pourcent) sont aussi masquees.
pub const ENCODED_MIN_BYTES: usize = 8;
/// Taille (octets) au-dela de laquelle les formes encodees ne sont plus
/// ajoutees : borne la memoire de l'automate (environ 10 octets par octet
/// de motif ; au pire, 1 Mio de secrets et `MAX_FRAGMENTS` fragments de
/// cette taille, quelques dizaines de Mio par projet suivi).
pub const ENCODED_MAX_BYTES: usize = 256;
/// Avertissement renvoye a l'ecriture d'un secret trop court pour etre
/// masque dans les journaux.
pub const TOO_SHORT_TO_REDACT: &str = "too_short_to_redact";

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

/// Un secret de moins de `MIN_LEN` caracteres n'est pas masque.
pub fn too_short_to_redact(value: &str) -> bool {
    value.chars().count() < MIN_LEN
}

/// Fragments masquables de `values` : chaque valeur et chaque ligne d'une
/// valeur de plusieurs lignes, d'au moins `MIN_LEN` caracteres, sans
/// doublon.
pub fn fragments<S: AsRef<str>>(values: &[S]) -> Vec<Zeroizing<String>> {
    let mut out = Vec::new();
    for value in values {
        let value = value.as_ref();
        out.push(Zeroizing::new(value.to_string()));
        // Une valeur de plusieurs lignes (cle PEM...) arrive en plusieurs
        // lignes de journal : chacune de ses lignes est aussi masquee.
        if value.contains(['\n', '\r']) {
            out.extend(
                value
                    .split(['\n', '\r'])
                    .map(|l| Zeroizing::new(l.trim().to_string())),
            );
        }
    }
    dedup(&mut out);
    out
}

/// Retire les valeurs trop courtes et les doublons.
fn dedup(values: &mut Vec<Zeroizing<String>>) {
    values.retain(|v| !too_short_to_redact(v));
    values.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    values.dedup_by(|a, b| a.as_str() == b.as_str());
}

/// Octets laisses tels quels par l'encodage pourcent (RFC 3986, non reserves).
fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~')
}

fn percent(value: &str, upper: bool) -> Zeroizing<String> {
    const UPPER: &[u8; 16] = b"0123456789ABCDEF";
    const LOWER: &[u8; 16] = b"0123456789abcdef";
    let hex = if upper { UPPER } else { LOWER };
    let mut out = Zeroizing::new(String::with_capacity(value.len() * 3));
    for &b in value.as_bytes() {
        if unreserved(b) {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push(char::from(hex[usize::from(b >> 4)]));
            out.push(char::from(hex[usize::from(b & 0xf)]));
        }
    }
    out
}

/// Formes base64 de `value` dans un texte encode, aux trois alignements
/// possibles (precedee de 0, 1 ou 2 octets) : seuls les caracteres qui ne
/// dependent que de la valeur sont gardes (ni ceux du debut, qui dependent
/// des octets precedents, ni le dernier quand il depend des octets
/// suivants). A l'alignement 0, la forme complete de la valeur seule est
/// gardee aussi. Le remplissage `=` ne porte aucune information.
fn base64_forms(value: &[u8], out: &mut Vec<Zeroizing<String>>) {
    for engine in [&STANDARD_NO_PAD, &URL_SAFE_NO_PAD] {
        for shift in 0..3usize {
            let mut framed = Zeroizing::new(vec![0u8; shift]);
            framed.extend_from_slice(value);
            let encoded = Zeroizing::new(engine.encode(framed.as_slice()));
            if shift == 0 {
                // Valeur seule (ou en fin de texte encode).
                out.push(Zeroizing::new(encoded.to_string()));
            }
            // 2 caracteres touches par 1 octet de tete, 3 par 2.
            let skip = if shift == 0 { 0 } else { shift + 1 };
            let cut = usize::from(framed.len() % 3 != 0);
            let end = encoded.len().saturating_sub(cut);
            out.push(Zeroizing::new(
                encoded.get(skip..end).unwrap_or("").to_string(),
            ));
        }
    }
}

/// Toutes les formes masquees d'un fragment.
fn forms(fragment: &str, out: &mut Vec<Zeroizing<String>>) {
    out.push(Zeroizing::new(fragment.to_string()));
    // Formes nettoyees : la ligne (et le message d'erreur) est nettoyee
    // avant le masquage ; une valeur qui contient un caractere de controle
    // doit l'etre de la meme facon pour y etre reconnue.
    let no_nul = Zeroizing::new(fragment.replace('\0', ""));
    out.push(Zeroizing::new(super::neutralize(&no_nul)));
    out.push(Zeroizing::new(super::strip_escapes(&no_nul)));
    if (ENCODED_MIN_BYTES..=ENCODED_MAX_BYTES).contains(&fragment.len()) {
        base64_forms(fragment.as_bytes(), out);
        out.push(percent(fragment, true));
        out.push(percent(fragment, false));
    }
}

/// Longueur du plus long motif reconnu dans l'etat final `sid` (les autres
/// motifs reconnus au meme endroit en sont des suffixes).
fn longest_match(nfa: &NFA, sid: StateID) -> usize {
    (0..nfa.match_len(sid))
        .map(|i| nfa.pattern_len(nfa.match_pattern(sid, i)))
        .max()
        .unwrap_or(0)
}

/// Automate des formes masquees.
struct Matcher {
    nfa: NFA,
    start: StateID,
    /// `longest_match` de chaque etat final, calcule une fois.
    longest: HashMap<StateID, usize>,
}

impl Matcher {
    /// `Ok(None)` : aucun motif ; `Err` : automate impossible (le
    /// masqueur masque alors toute la ligne).
    fn new(patterns: &[Zeroizing<String>]) -> Result<Option<Self>, ()> {
        if patterns.is_empty() {
            return Ok(None);
        }
        let nfa = NFA::builder()
            .match_kind(MatchKind::Standard)
            .build(patterns.iter().map(|p| p.as_bytes()))
            .map_err(|e| tracing::error!("automate de masquage impossible : {e}"))?;
        let start = nfa.start_state(Anchored::No).map_err(|_| ())?;
        // Chaque etat est le prefixe d'un motif : parcourir les motifs
        // visite tous les etats atteignables.
        let mut longest = HashMap::new();
        for pattern in patterns {
            let mut sid = start;
            for &b in pattern.as_bytes() {
                sid = nfa.next_state(Anchored::No, sid, b);
                if nfa.is_match(sid) && !longest.contains_key(&sid) {
                    longest.insert(sid, longest_match(&nfa, sid));
                }
            }
        }
        Ok(Some(Self {
            nfa,
            start,
            longest,
        }))
    }

    /// Zones a masquer (debut, fin), triees et disjointes : reunion des
    /// occurrences qui se chevauchent ; deux occurrences seulement
    /// contigues restent deux zones.
    fn zones(&self, text: &str) -> Vec<(usize, usize)> {
        let mut zones: Vec<(usize, usize)> = Vec::new();
        let mut sid = self.start;
        for (i, &b) in text.as_bytes().iter().enumerate() {
            sid = self.nfa.next_state(Anchored::No, sid, b);
            if !self.nfa.is_match(sid) {
                continue;
            }
            let end = i + 1;
            let len = match self.longest.get(&sid) {
                Some(len) => *len,
                None => longest_match(&self.nfa, sid),
            };
            let mut start = end - len;
            // Fins croissantes : la nouvelle zone absorbe celles qu'elle
            // chevauche.
            while let Some(&(s, e)) = zones.last() {
                if start < e {
                    start = start.min(s);
                    zones.pop();
                } else {
                    break;
                }
            }
            zones.push((start, end));
        }
        zones
    }
}

/// Valeurs a masquer d'un projet (automate construit une fois).
#[derive(Default)]
pub struct Redactor {
    matcher: Option<Matcher>,
    /// Automate impossible (inatteignable sous les plafonds d'ecriture) :
    /// echec ferme, chaque ligne est entierement masquee.
    broken: bool,
    fragments: usize,
    /// Tests : appele au debut de chaque masquage de ligne.
    #[cfg(test)]
    pub(crate) gate: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for Redactor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Redactor({} fragment(s))", self.fragments)
    }
}

impl Redactor {
    pub fn new(values: Vec<Zeroizing<String>>) -> Self {
        let fragments = fragments(&values);
        drop(values);
        let mut patterns = Vec::new();
        for fragment in &fragments {
            forms(fragment, &mut patterns);
        }
        dedup(&mut patterns);
        let (matcher, broken) = match Matcher::new(&patterns) {
            Ok(matcher) => (matcher, false),
            Err(()) => (None, true),
        };
        Self {
            matcher,
            broken,
            fragments: fragments.len(),
            #[cfg(test)]
            gate: None,
        }
    }

    /// Ligne de journal masquee : octets nuls et sequences ANSI/controle
    /// retires AVANT le masquage (un secret entrecoupe de codes ANSI doit
    /// etre reconnu). Point d'entree unique du direct, de l'enregistrement
    /// et de la relecture : aucun chemin ne masque une ligne brute.
    pub fn redact_line(&self, line: &str) -> String {
        #[cfg(test)]
        if let Some(gate) = &self.gate {
            gate();
        }
        let line: String = line.chars().filter(|c| *c != '\0').collect();
        self.redact(super::neutralize(&line))
    }

    /// Message d'erreur masque : octets nuls et sequences ANSI retires
    /// AVANT le masquage, retours a la ligne gardes (le message est nettoye
    /// ensuite).
    pub fn redact_message(&self, message: &str) -> String {
        let message = message.replace('\0', "");
        self.redact(super::strip_escapes(&message))
    }

    /// Ligne masquee, sans nettoyage des sequences ANSI : privee, les
    /// chemins de journaux passent par `redact_line` (ou `redact_message`).
    /// Les octets nuls sont retires d'abord, au meme endroit
    /// pour le direct, l'enregistrement et le telechargement : un octet nul
    /// glisse dans une valeur ne la soustrait pas au masquage. La coupe a la
    /// longueur maximale vient toujours apres.
    fn redact(&self, line: String) -> String {
        let mut line = line;
        if line.contains('\0') {
            line.retain(|c| c != '\0');
        }
        if self.broken {
            return MASK.to_string();
        }
        let Some(matcher) = &self.matcher else {
            return line;
        };
        let zones = matcher.zones(&line);
        if zones.is_empty() {
            return line;
        }
        let mut out = String::with_capacity(line.len());
        let mut at = 0;
        for (start, end) in zones {
            // Motifs et ligne en UTF-8 : une occurrence commence et finit
            // sur une frontiere de caractere.
            out.push_str(&line[at..start]);
            out.push_str(MASK);
            at = end;
        }
        out.push_str(&line[at..]);
        out
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

    #[test]
    fn overlapping_and_nested_secrets_all_masked() {
        // Deux secrets qui se chevauchent : la reunion est masquee.
        let r = redactor(&["abcdefgh", "efghijkl"]);
        assert_eq!(r.redact("xx abcdefghijkl yy".into()), "xx •••• yy");
        assert_eq!(r.redact("xx efghijkl yy".into()), "xx •••• yy");
        // Secret contenu dans un autre : le plus long en entier, le plus
        // court seul aussi.
        let r = redactor(&["inner-secret", "outer-inner-secret-x"]);
        assert_eq!(r.redact("a outer-inner-secret-x b".into()), "a •••• b");
        assert_eq!(
            r.redact("a outer-inner-secret-y b".into()),
            "a outer-••••-y b"
        );
        // Chaine de chevauchements, avec un secret qui commence avant le
        // precedent.
        let r = redactor(&["1234567", "5678abc", "345678ab", "bcdefg"]);
        assert_eq!(r.redact("<1234567890>".into()), "<••••890>");
        assert_eq!(r.redact("<12345678abcdefgh>".into()), "<••••h>");
        // Aucun fragment de 4 caracteres d'un secret ne reste.
        let secrets = ["k3y-AAAA-BBBB-CCCC", "BBBB-CCCC-DDDD-z9"];
        let r = redactor(&secrets);
        let out = r.redact("v=k3y-AAAA-BBBB-CCCC-DDDD-z9;".into());
        assert_eq!(out, "v=••••;");
    }

    #[test]
    fn control_character_inside_secret_still_masked() {
        for c in ['\x0b', '\x7f', '\u{85}', '\x1b', '\x07', '\0'] {
            let secret = format!("abcd{c}efgh-s3cr3t");
            let r = redactor(&[&secret]);
            let line = format!("k={secret} fin");
            assert_eq!(r.redact_line(&line), "k=•••• fin", "caractere {:?}", c);
            // Message d'erreur : meme valeur reconnue.
            assert_eq!(r.redact_message(&line), "k=•••• fin", "caractere {:?}", c);
        }
    }

    #[test]
    fn error_message_with_ansi_inside_secret_is_masked() {
        let r = redactor(&["s3cr3t-value"]);
        let message = "boom s3cr\x1b[31m3t-va\x1b[0mlue\nligne 2 \x1b]0;t\x07s3cr3t-value";
        let masked = r.redact_message(message);
        assert_eq!(masked, "boom ••••\nligne 2 ••••");
        // Nettoyage ensuite, comme le plan de controle l'enregistre.
        assert_eq!(
            crate::domain::deployments::clean_message(&masked),
            "boom •••• ligne 2 ••••"
        );
    }

    #[test]
    fn base64_form_redacted() {
        use base64::engine::general_purpose::{STANDARD, URL_SAFE};
        let secret = "s3cr3t-value>>?~+/!";
        let r = redactor(&[secret]);
        // Valeur seule, base64 standard et URL, avec et sans remplissage.
        for encoded in [
            STANDARD.encode(secret),
            URL_SAFE.encode(secret),
            STANDARD_NO_PAD.encode(secret),
            URL_SAFE_NO_PAD.encode(secret),
        ] {
            let out = r.redact_line(&format!("x {encoded} y"));
            assert!(out.starts_with("x ••••"), "{out}");
            assert!(
                out.trim_start_matches("x ••••").trim_start_matches('=') == " y",
                "{out}"
            );
        }
        // Valeur au milieu d'un texte encode (en-tete Basic), aux trois
        // alignements : il ne reste au plus que 3 caracteres de chaque cote.
        for user in ["u:", "us:", "use:"] {
            let encoded = STANDARD.encode(format!("{user}{secret}"));
            let out = r.redact_line(&format!("Authorization: Basic {encoded}"));
            let rest = out.trim_start_matches("Authorization: Basic ");
            let (head, tail) = rest.split_once(MASK).expect(&out);
            assert!(head.len() <= user.len() + 2, "{out}");
            assert!(tail.trim_end_matches('=').len() <= 1, "{out}");
        }
        // Valeur suivie d'autres octets (alignement 0), pour des longueurs
        // de reste 0, 1 et 2 modulo 3 : `echo "$SECRET" | base64`, et
        // l'en-tete Basic avec le secret comme nom d'utilisateur.
        for secret in [
            "s3cr3t-value>>?~+/",
            "s3cr3t-value>>?~+/!",
            "s3cr3t-value>>?~+/!!",
        ] {
            let r = redactor(&[secret]);
            for (encoded, header) in [
                (
                    STANDARD.encode(format!(
                        "{secret}
"
                    )),
                    "",
                ),
                (
                    STANDARD.encode(format!("{secret}:")),
                    "Authorization: Basic ",
                ),
                (URL_SAFE.encode(format!("{secret}:x")), ""),
            ] {
                let out = r.redact_line(&format!("{header}{encoded}"));
                let rest = out.strip_prefix(header).expect(&out);
                let (head, tail) = rest.split_once(MASK).expect(&out);
                assert!(head.is_empty(), "{out}");
                // Au plus le caractere partage avec l'octet suivant, puis
                // ceux des 1 ou 2 octets qui suivent la valeur.
                assert!(tail.trim_end_matches('=').len() <= 4, "{secret} : {out}");
            }
        }
        // Encodage pourcent (majuscules et minuscules).
        let secret = "p@ss w0rd/é!";
        let r = redactor(&[secret]);
        assert_eq!(
            r.redact_line("u=p%40ss%20w0rd%2F%C3%A9%21&x=1"),
            "u=••••&x=1"
        );
        assert_eq!(
            r.redact_line("u=p%40ss%20w0rd%2f%c3%a9%21&x=1"),
            "u=••••&x=1"
        );
        // Moins de 8 octets : pas de forme encodee (faux positifs).
        let r = redactor(&["abcdefg"]);
        assert_eq!(
            r.redact_line(&STANDARD.encode("abcdefg")),
            STANDARD.encode("abcdefg")
        );
    }

    #[test]
    fn short_secret_warning() {
        assert!(too_short_to_redact("abc"));
        assert!(too_short_to_redact("éà"));
        assert!(!too_short_to_redact("abcd"));
        let r = redactor(&["abc"]);
        assert_eq!(r.redact("abc".into()), "abc");
    }

    #[test]
    fn fragments_counts_values_and_lines() {
        let values = [
            "one-secret",
            "-----BEGIN-----\nAAAAAAAA\nxy\n-----END-----",
            "one-secret",
            "ab",
        ];
        // 2 valeurs distinctes de 4 caracteres ou plus, et 3 lignes.
        assert_eq!(fragments(&values).len(), 5);
    }

    #[test]
    fn redaction_linear_time() {
        // 1 000 motifs aleatoires (et leurs formes encodees) x lignes de
        // 16 Kio : un seul parcours par ligne.
        let values: Vec<String> = (0..MAX_FRAGMENTS)
            .map(|i| {
                format!(
                    "{:032x}",
                    (i as u128 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c834)
                )
            })
            .collect();
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        let r = redactor(&refs);
        let mut line = "z".repeat(crate::domain::logs::MAX_LINE_BYTES - 64);
        line.push_str(&values[500]);
        let started = std::time::Instant::now();
        for _ in 0..20 {
            let out = r.redact_line(&line);
            assert!(out.ends_with(MASK));
        }
        let per_line = started.elapsed() / 20;
        assert!(
            per_line < std::time::Duration::from_millis(100),
            "{per_line:?} par ligne"
        );

        // Pire cas des motifs emboites (a^4 ... a^1003) sur une ligne de a :
        // toujours un seul parcours, sans enumerer les occurrences.
        let values: Vec<String> = (MIN_LEN..MIN_LEN + MAX_FRAGMENTS)
            .map(|n| "a".repeat(n))
            .collect();
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        let r = redactor(&refs);
        let line = "a".repeat(crate::domain::logs::MAX_LINE_BYTES);
        let started = std::time::Instant::now();
        for _ in 0..20 {
            assert_eq!(r.redact_line(&line), MASK);
        }
        let per_line = started.elapsed() / 20;
        assert!(
            per_line < std::time::Duration::from_millis(100),
            "{per_line:?} par ligne"
        );
    }
}
