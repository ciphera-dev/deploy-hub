//! Noms des images internes de Ciphera, communs au plan de controle et a
//! l'agent.
//!
//! Les images construites ou etiquetees par Ciphera sont nommees sous le
//! registre `ciphera.invalid` : le domaine `.invalid` ne se resout jamais
//! (RFC 2606), donc Docker ne peut pas les telecharger d'un registre public.
//! Une reference donnee par un utilisateur (image d'un projet, `image` d'un
//! service Compose) ne doit jamais designer une de ces images, ni une image
//! des anciens prefixes `ciphera/` et `ciphera-staging/`.

/// Registre des images internes (jamais resolu).
pub const INTERNAL_REGISTRY: &str = "ciphera.invalid";

/// Prefixe des images internes (`<INTERNAL_REGISTRY>/`).
pub const INTERNAL_PREFIX: &str = "ciphera.invalid/";

/// Prefixe des noms provisoires du chargement d'un build
/// (`ciphera.invalid/staging/<deploiement>[-<service>]:build`).
pub const STAGING_PREFIX: &str = "ciphera.invalid/staging/";

/// Anciens prefixes des images internes (deploiements anterieurs : leur
/// image garde le nom enregistre).
pub const LEGACY_PREFIX: &str = "ciphera/";
pub const LEGACY_STAGING_PREFIX: &str = "ciphera-staging/";

/// Formes du registre de Docker Hub, implicite quand la reference n'a pas
/// de domaine.
const DOCKER_HUB_PREFIXES: [&str; 3] = ["docker.io/", "index.docker.io/", "registry-1.docker.io/"];

/// Prefixes reserves, une fois la reference normalisee.
const RESERVED_PREFIXES: [&str; 4] = [
    INTERNAL_PREFIX,
    "ciphera.invalid:",
    LEGACY_PREFIX,
    LEGACY_STAGING_PREFIX,
];

/// Reference d'image normalisee pour la comparer aux prefixes reserves : en
/// minuscules, sans le registre de Docker Hub (`docker.io/`,
/// `index.docker.io/`, `registry-1.docker.io/`, autant de fois qu'il
/// apparait en tete).
pub fn normalize_ref(reference: &str) -> String {
    let mut out = reference.trim().to_ascii_lowercase();
    while let Some(prefix) = DOCKER_HUB_PREFIXES.iter().find(|p| out.starts_with(*p)) {
        out.drain(..prefix.len());
    }
    out
}

/// Reference designant une image interne de Ciphera (registre
/// `ciphera.invalid`, ou anciens prefixes `ciphera/` et `ciphera-staging/`,
/// avec ou sans le registre de Docker Hub).
pub fn is_internal_ref(reference: &str) -> bool {
    let normalized = normalize_ref(reference);
    RESERVED_PREFIXES.iter().any(|p| normalized.starts_with(p))
}

/// Depot de l'image d'un environnement : `ciphera.invalid/<projet>-<env>`.
pub fn repository(project: &str, environment: &str) -> String {
    format!("{INTERNAL_PREFIX}{project}-{environment}")
}

/// Image d'un deploiement : `ciphera.invalid/<projet>-<env>:<deploiement>`.
pub fn deployment_image(
    project: &str,
    environment: &str,
    deployment: impl std::fmt::Display,
) -> String {
    format!("{}:{deployment}", repository(project, environment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_prefixes_are_removed() {
        assert_eq!(normalize_ref("docker.io/ciphera/web:1"), "ciphera/web:1");
        assert_eq!(normalize_ref("index.docker.io/ciphera/web"), "ciphera/web");
        assert_eq!(normalize_ref("registry-1.docker.io/a/b"), "a/b");
        assert_eq!(normalize_ref("Docker.IO/docker.io/Ciphera/x"), "ciphera/x");
        assert_eq!(normalize_ref("ghcr.io/org/app:1"), "ghcr.io/org/app:1");
    }

    #[test]
    fn internal_refs_in_every_form() {
        for internal in [
            "ciphera.invalid/web-production:1",
            "ciphera.invalid/staging/x:build",
            "ciphera.invalid:5000/web",
            "CIPHERA.INVALID/web",
            "ciphera/web-production:1",
            "ciphera-staging/x:build",
            "docker.io/ciphera/web",
            "index.docker.io/ciphera-staging/x",
            "registry-1.docker.io/ciphera.invalid/x",
            "docker.io/docker.io/ciphera/x",
        ] {
            assert!(is_internal_ref(internal), "{internal}");
        }
        for user in [
            "nginx",
            "nginx:1.27",
            "ghcr.io/ciphera/web:1",
            "docker.io/library/nginx",
            "cipheraweb/app",
            "ciphera-web/app",
            "ciphera.invalid.example.com/x",
            "acme/ciphera",
        ] {
            assert!(!is_internal_ref(user), "{user}");
        }
    }

    #[test]
    fn deployment_image_under_the_internal_registry() {
        assert_eq!(
            deployment_image("web", "production", "d1"),
            "ciphera.invalid/web-production:d1"
        );
        assert!(is_internal_ref(&deployment_image("a", "b", 1)));
    }
}
