//! Validation en liste blanche des entrees d'un projet, commune au plan de
//! controle (API) et a l'agent (arguments de `git`, `railpack`, `buildctl`
//! ou `docker compose`, tableau d'arguments fixe, jamais un shell) : jeux de
//! caracteres etroits, et rien qui puisse passer pour une option (`-x`).

pub const MAX_NAME: usize = 63;
pub const MAX_PATH: usize = 255;
pub const MAX_BRANCH: usize = 255;
pub const MAX_IMAGE: usize = 255;
const GITHUB_PREFIX: &str = "https://github.com/";

/// Message destine a l'utilisateur (422 `validation_failed`).
pub type Invalid = &'static str;

/// `^[a-z0-9][a-z0-9-]{0,62}$`
pub fn name(value: &str) -> Result<(), Invalid> {
    let mut chars = value.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !first_ok
        || value.len() > MAX_NAME
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(
            "Le nom doit commencer par une lettre minuscule ou un chiffre, puis contenir des minuscules, des chiffres ou des tirets (63 caractères au plus).",
        );
    }
    Ok(())
}

/// Prefixe des noms de conteneurs, reseaux et projets Compose de Ciphera.
pub const RESERVED_PREFIX: &str = "ciphera";

/// Nom de projet : `name`, sans le prefixe reserve `ciphera` (les noms de
/// conteneurs et de projets Compose en derivent : aucun ne doit pouvoir
/// passer pour un conteneur de Ciphera).
pub fn project_name(value: &str) -> Result<(), Invalid> {
    name(value)?;
    if value.starts_with(RESERVED_PREFIX) {
        return Err("Les noms commençant par « ciphera » sont réservés.");
    }
    Ok(())
}

fn is_ref_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')
}

/// `^[A-Za-z0-9._/-]{1,255}$`, sans `..`, ni `//`, ni debut par `/` ou `-`.
pub fn path(value: &str) -> Result<(), Invalid> {
    if value.is_empty()
        || value.len() > MAX_PATH
        || !value.chars().all(is_ref_char)
        || value.contains("..")
        || value.contains("//")
        || value.starts_with(['/', '-'])
    {
        return Err(
            "Chemin invalide : relatif au dépôt, avec lettres, chiffres, « . », « _ », « - » et « / » seulement.",
        );
    }
    Ok(())
}

/// Nom de service Compose : `^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$`.
pub fn compose_service(value: &str) -> Result<(), Invalid> {
    let mut chars = value.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    if !first_ok
        || value.len() > MAX_NAME
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("Nom de service Compose invalide.");
    }
    Ok(())
}

/// Reference Git (`git check-ref-format`, en plus strict) : jeu de caracteres
/// etroit, pas d'option (`-`), pas de `..`, `//`, `/.`, `@{`, ni de `.lock`.
pub fn branch(value: &str) -> Result<(), Invalid> {
    if value.is_empty()
        || value.len() > MAX_BRANCH
        || !value.chars().all(is_ref_char)
        || value.starts_with(['-', '/', '.'])
        || value.ends_with(['/', '.'])
        || value.ends_with(".lock")
        || value.contains("..")
        || value.contains("//")
        || value.contains("/.")
        || value.contains(".lock/")
    {
        return Err("Nom de branche invalide.");
    }
    Ok(())
}

/// Reference d'image donnee par un utilisateur :
/// `^[A-Za-z0-9][A-Za-z0-9._/:@+-]{0,254}$` sans `..`. Ni espace, ni
/// metacaractere de shell, ni guillemet, ni image interne de Ciphera
/// (`crate::images::is_internal_ref`).
pub fn image(value: &str) -> Result<(), Invalid> {
    let mut chars = value.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    if !first_ok
        || value.len() > MAX_IMAGE
        || !chars.all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | ':' | '@' | '+' | '-')
        })
        || value.contains("..")
        || value.contains("//")
    {
        return Err("Référence d'image invalide (exemple : ghcr.io/org/app:1.2).");
    }
    if crate::images::is_internal_ref(value) {
        return Err(
            "Les images « ciphera.invalid/… », « ciphera/… » et « ciphera-staging/… » sont réservées à Ciphera.",
        );
    }
    Ok(())
}

/// Depot GitHub : URL `https://github.com/proprietaire/nom[.git]` ou
/// `proprietaire/nom`. Renvoie `proprietaire/nom`.
pub fn repository(value: &str) -> Result<String, Invalid> {
    const INVALID: Invalid =
        "Dépôt invalide : URL https://github.com/propriétaire/nom ou « propriétaire/nom ».";
    let rest = value.trim();
    let rest = rest.strip_prefix(GITHUB_PREFIX).unwrap_or(rest);
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, repo) = rest.split_once('/').ok_or(INVALID)?;
    let owner_ok = !owner.is_empty()
        && owner.len() <= 39
        && owner.starts_with(|c: char| c.is_ascii_alphanumeric())
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let repo_ok = !repo.is_empty()
        && repo.len() <= 100
        && !matches!(repo, "." | "..")
        && repo.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !owner_ok || !repo_ok {
        return Err(INVALID);
    }
    Ok(format!("{owner}/{repo}"))
}

/// URL de depot acceptee par l'agent en v0.1 :
/// `https://github.com/<proprietaire>/<nom>[.git]`, exactement. Ni `ext::`,
/// ni `file://`, ni `ssh://`, ni option (`-`), ni identifiants, ni requete.
pub fn github_url(value: &str) -> Result<(), Invalid> {
    const INVALID: Invalid = "URL de dépôt invalide : https://github.com/propriétaire/nom attendu.";
    let rest = value.strip_prefix(GITHUB_PREFIX).ok_or(INVALID)?;
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    match repository(rest) {
        Ok(normalized) if normalized == rest => Ok(()),
        _ => Err(INVALID),
    }
}

/// Empreinte de commit Git : 40 (SHA-1) ou 64 (SHA-256) chiffres
/// hexadecimaux en minuscules.
pub fn commit_sha(value: &str) -> Result<(), Invalid> {
    if matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err("Empreinte de commit invalide (40 ou 64 caractères hexadécimaux).")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a", "0", "ciphera-web", &"a".repeat(63)] {
            assert!(name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "A",
            "a_b",
            "a b",
            "a;rm -rf /",
            "../x",
            "é",
            &"a".repeat(64),
        ] {
            assert!(name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn project_names_starting_with_ciphera_are_reserved() {
        for ok in ["web", "acme-ciphera", "my-ciphera-app", "cipher"] {
            assert!(project_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "ciphera",
            "ciphera-web",
            "ciphera-prepare",
            "cipheralab",
            "A",
        ] {
            assert!(project_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn repositories() {
        assert_eq!(repository("https://github.com/o/r").unwrap(), "o/r");
        assert_eq!(repository("https://github.com/o/r.git").unwrap(), "o/r");
        assert_eq!(repository("https://github.com/o/r/").unwrap(), "o/r");
        assert_eq!(repository(" o/r.js ").unwrap(), "o/r.js");
        for bad in [
            "",
            "o",
            "o/",
            "/r",
            "o/r/x",
            "http://github.com/o/r",
            "https://gitlab.com/o/r",
            "https://github.com/o/r?x",
            "https://github.com/../r",
            "o/..",
            "-o/r",
            "o/r;id",
            "o/r r",
            "git@github.com:o/r",
        ] {
            assert!(repository(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn refs_and_paths() {
        for ok in ["main", "feat/charts", "release-1.2", "v1_0"] {
            assert!(branch(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "-x", "/a", ".a", "a/", "a.", "a..b", "a//b", "a/.b", "x.lock", "a b", "a~1", "a^",
            "a:b", "a@{1}", "a*", "a\\b", "a?",
        ] {
            assert!(branch(bad).is_err(), "{bad}");
        }
        for ok in [
            "Dockerfile",
            "docker/Dockerfile.prod",
            "compose.yaml",
            "./Dockerfile",
        ] {
            assert!(path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "../x",
            "a/../b",
            "/etc/passwd",
            "-f",
            "a b",
            "a;b",
            "a//b",
            &"a".repeat(256),
        ] {
            assert!(path(bad).is_err(), "{bad}");
        }
        assert!(compose_service("web-1").is_ok());
        assert!(compose_service("a;b").is_err() && compose_service("-a").is_err());
    }

    #[test]
    fn github_urls_and_commits() {
        for ok in [
            "https://github.com/o/r",
            "https://github.com/o/r.git",
            "https://github.com/acme-1/web.app",
        ] {
            assert!(github_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "o/r",
            "https://github.com/o/r/",
            "https://github.com/o/r?x",
            "https://user:pw@github.com/o/r",
            "http://github.com/o/r",
            "ext::sh -c id",
            "file:///etc",
            "ssh://git@github.com/o/r",
            "--upload-pack=x",
            " https://github.com/o/r",
            "https://github.com/o/r.git.git",
        ] {
            assert!(github_url(bad).is_err(), "{bad}");
        }
        assert!(commit_sha(&"a".repeat(40)).is_ok());
        assert!(commit_sha(&"0".repeat(64)).is_ok());
        for bad in ["", "abc", &"A".repeat(40), &"g".repeat(40), &"a".repeat(41)] {
            assert!(commit_sha(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn images() {
        for ok in [
            "nginx",
            "nginx:1.27",
            "ghcr.io/org/app:1.2",
            "org/app@sha256:abcdef",
        ] {
            assert!(image(ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "-x", "a b", "a;b", "a$(id)", "a`id`", "a|b", "a&b", "a\"b", "a'b", "a\nb", "a..b",
            "a\\b",
        ] {
            assert!(image(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn internal_images_are_refused() {
        for internal in [
            "ciphera.invalid/web-production:1",
            "ciphera/web-production:1",
            "ciphera-staging/x:build",
            "docker.io/ciphera/web",
            "index.docker.io/ciphera-staging/x:build",
            "registry-1.docker.io/ciphera.invalid/x",
        ] {
            assert!(image(internal).is_err(), "{internal}");
        }
        assert!(image("ghcr.io/ciphera/web:1").is_ok());
    }
}
