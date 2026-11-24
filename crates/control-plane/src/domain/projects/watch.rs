//! Chemins surveilles : motifs glob relatifs a la racine du depot.
//!
//! Syntaxe (volontairement reduite, sans dependance) :
//! - `*` : n'importe quelle suite de caracteres sans `/` ;
//! - `?` : un caractere autre que `/` ;
//! - `**` : un segment entier, qui couvre zero, un ou plusieurs dossiers
//!   (`apps/**`, `**/*.rs`, `a/**/b`) ;
//! - un motif sans joker designe un fichier ou un dossier (`apps/web`
//!   couvre `apps/web/x.ts`) ; un `/` final equivaut a `/**`.
//!
//! La correspondance est lineaire par segment (aucun retour arriere
//! exponentiel) : programmation dynamique sur les segments, puis sur les
//! caracteres d'un segment.

use shared::validate::Invalid;

/// Motifs par projet.
pub const MAX_PATTERNS: usize = 20;
/// Longueur maximale d'un motif.
pub const MAX_PATTERN_LEN: usize = 200;

fn pattern_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '*' | '?' | '@' | '+')
}

/// Motif accepte : caracteres en liste blanche, relatif, sans `..`.
pub fn validate(pattern: &str) -> Result<(), Invalid> {
    if pattern.is_empty()
        || pattern.len() > MAX_PATTERN_LEN
        || !pattern.chars().all(pattern_char)
        || pattern.starts_with('/')
        || pattern.contains("//")
        || pattern.split('/').any(|s| s == ".." || s == ".")
    {
        return Err(
            "Chemin surveillé invalide : motif relatif au dépôt (lettres, chiffres, « . _ - / @ + », jokers « * », « ** » et « ? »).",
        );
    }
    Ok(())
}

/// `true` si `name` (un segment) correspond a `pattern` (un segment avec
/// `*` et `?`).
fn segment_matches(pattern: &[u8], name: &[u8]) -> bool {
    // dp[j] : le prefixe courant du motif couvre name[..j].
    let mut dp = vec![false; name.len() + 1];
    dp[0] = true;
    for &p in pattern {
        let mut next = vec![false; name.len() + 1];
        match p {
            b'*' => {
                let mut reachable = false;
                for j in 0..=name.len() {
                    reachable |= dp[j];
                    next[j] = reachable;
                }
            }
            _ => {
                for j in 0..name.len() {
                    next[j + 1] = dp[j] && (p == b'?' || p == name[j]);
                }
            }
        }
        dp = next;
    }
    dp[name.len()]
}

/// `true` si `path` (fichier relatif a la racine du depot) correspond a
/// `pattern`.
pub fn matches(pattern: &str, path: &str) -> bool {
    let trimmed = pattern.trim_end_matches('/');
    // Motif sans joker, ou dossier (`/` final) : lui-meme ou son contenu
    // (`**` couvre aussi zero segment).
    let owned;
    let pattern = if trimmed.len() < pattern.len() || !trimmed.contains(['*', '?']) {
        owned = format!("{trimmed}/**");
        owned.as_str()
    } else {
        trimmed
    };
    let pat: Vec<&str> = pattern.split('/').collect();
    let name: Vec<&str> = path.split('/').collect();
    // dp[j] : les segments deja lus du motif couvrent name[..j].
    let mut dp = vec![false; name.len() + 1];
    dp[0] = true;
    for seg in &pat {
        let mut next = vec![false; name.len() + 1];
        if *seg == "**" {
            let mut reachable = false;
            for j in 0..=name.len() {
                reachable |= dp[j];
                next[j] = reachable;
            }
        } else {
            for j in 0..name.len() {
                next[j + 1] = dp[j] && segment_matches(seg.as_bytes(), name[j].as_bytes());
            }
        }
        dp = next;
    }
    dp[name.len()]
}

/// `true` si l'un des fichiers correspond a l'un des motifs ; liste de
/// motifs vide : toujours `true`.
pub fn any_match<'a>(patterns: &[String], files: impl IntoIterator<Item = &'a str>) -> bool {
    if patterns.is_empty() {
        return true;
    }
    files
        .into_iter()
        .any(|f| patterns.iter().any(|p| matches(p, f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_star_spans_directories() {
        assert!(matches("apps/web/**", "apps/web/src/index.ts"));
        assert!(matches("apps/web/**", "apps/web/package.json"));
        assert!(!matches("apps/web/**", "apps/api/main.rs"));
        assert!(!matches("apps/web/**", "apps/website/x"));
        assert!(matches("**/*.rs", "main.rs"));
        assert!(matches("**/*.rs", "crates/a/src/lib.rs"));
        assert!(!matches("**/*.rs", "crates/a/src/lib.ts"));
        assert!(matches("a/**/b", "a/b"));
        assert!(matches("a/**/b", "a/x/y/b"));
        assert!(matches("**", "anything/at/all"));
    }

    #[test]
    fn single_star_and_question_mark_stay_in_a_segment() {
        assert!(matches("src/*.ts", "src/a.ts"));
        assert!(!matches("src/*.ts", "src/sub/a.ts"));
        assert!(matches("Dockerfile.?", "Dockerfile.a"));
        assert!(!matches("Dockerfile.?", "Dockerfile.ab"));
        assert!(matches("*", "README.md"));
        assert!(!matches("*", "docs/README.md"));
    }

    #[test]
    fn literal_patterns_cover_files_and_directories() {
        assert!(matches("apps/web", "apps/web/x.ts"));
        assert!(matches("apps/web/", "apps/web/x.ts"));
        assert!(matches("package.json", "package.json"));
        assert!(!matches("apps/web", "apps/webapp/x.ts"));
        assert!(!matches("package.json", "sub/package.json"));
    }

    #[test]
    fn no_catastrophic_backtracking() {
        let path = format!("{}b", "a".repeat(150));
        assert!(!matches(&"*a".repeat(60), &path));
        let deep = vec!["x"; 200].join("/");
        let stars = vec!["**"; 60].join("/");
        assert!(matches(&format!("{stars}/y"), &format!("{deep}/y")));
        assert!(!matches(&format!("{stars}/y"), &deep));
    }

    #[test]
    fn empty_list_always_matches() {
        assert!(any_match(&[], ["x"]));
        assert!(!any_match(&["a/**".into()], ["b/c"]));
        assert!(any_match(&["a/**".into(), "b/**".into()], ["z", "b/c"]));
    }

    #[test]
    fn validation_is_a_whitelist() {
        for ok in ["apps/web/**", "**/*.rs", "Dockerfile", "a-b_c.d/@x+y/?"] {
            assert!(validate(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "/abs", "../x", "a/../b", "./a", "a b", "[abc]", "{a,b}", "!a", "a//b", "a\\b",
        ] {
            assert!(validate(bad).is_err(), "{bad}");
        }
        assert!(validate(&"a".repeat(MAX_PATTERN_LEN + 1)).is_err());
    }
}
