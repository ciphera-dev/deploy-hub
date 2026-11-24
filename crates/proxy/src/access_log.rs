//! Chemin ecrit dans les journaux d'acces : jamais la requete (`?...`), et
//! les segments qui portent un jeton (lien de reinitialisation, invitation)
//! remplaces par `[redacted]`.
//!
//! La recherche du jeton se fait sur une forme normalisee du chemin
//! (barres obliques fusionnees, `\` lu comme `/`, segments `.` et `..`
//! resolus, encodage `%XX` decode, casse ignoree) : un amont peut lire
//! `/invite//%74ok` comme `/invite/tok`. Si le chemin n'etait pas deja sous
//! cette forme, tout ce qui suit le prefixe est masque.

use std::borrow::Cow;

/// Prefixes (segments) suivis d'un segment jeton.
const TOKEN_PREFIXES: [&[&str]; 4] = [
    &["api", "auth", "password", "reset"],
    &["api", "auth", "invitations"],
    &["invite"],
    &["reset"],
];

const REDACTED: &str = "[redacted]";

/// Decodage `%XX` repete (double encodage compris), en minuscules.
fn decode_segment(seg: &str) -> String {
    let mut cur = seg.to_string();
    for _ in 0..4 {
        let bytes = cur.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16);
            if bytes[i] == b'%'
                && i + 2 < bytes.len()
                && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
            {
                out.push((h * 16 + l) as u8);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        let next = String::from_utf8_lossy(&out).into_owned();
        if next == cur {
            break;
        }
        cur = next;
    }
    cur.to_ascii_lowercase()
}

/// Chemin de `target` (cible de la requete) sans requete ni fragment, jetons
/// masques.
pub fn log_path(target: &str) -> Cow<'_, str> {
    let path = target.split(['?', '#']).next().unwrap_or("");
    // Segments normalises comme un amont tolerant les lirait ; un `/` ou un
    // `\` decode separe aussi des segments.
    let mut segs: Vec<String> = Vec::new();
    for raw in path.split(['/', '\\']) {
        for seg in decode_segment(raw).split(['/', '\\']) {
            match seg {
                "" | "." => {}
                ".." => {
                    segs.pop();
                }
                s => segs.push(s.to_string()),
            }
        }
    }
    for prefix in TOKEN_PREFIXES {
        let n = prefix.len();
        if segs.len() <= n || !segs.iter().zip(prefix.iter()).all(|(s, p)| s == p) {
            continue;
        }
        let head = format!("/{}/{REDACTED}", prefix.join("/"));
        let tail = &segs[n + 1..];
        if tail.is_empty() {
            return Cow::Owned(head);
        }
        // Chemin deja sous forme normale (casse ignoree) : la suite est reprise.
        let canonical = format!("/{}", segs.join("/"));
        let clean = !path.contains(['%', '\\'])
            && path.trim_end_matches('/').eq_ignore_ascii_case(&canonical)
            && tail.iter().all(|s| {
                s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            });
        return if clean {
            Cow::Owned(format!("{head}/{}", tail.join("/")))
        } else {
            Cow::Owned(format!("{head}/{REDACTED}"))
        };
    }
    Cow::Borrowed(path)
}
