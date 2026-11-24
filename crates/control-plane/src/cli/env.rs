//! Environnement des commandes `ciphera admin` : celui du service.
//!
//! L'operateur lance `sudo ciphera admin ...` depuis un shell qui n'a ni les
//! `Environment=` de `ciphera.service` (socket de PostgreSQL, fichier du mot
//! de passe) ni `/etc/ciphera/ciphera.env`. La configuration est donc
//! reconstruite comme systemd la construit pour le service, par ordre de
//! priorite croissante :
//!
//! 1. les valeurs de l'unite (`UNIT_ENV`, verifiees contre
//!    `deploy/systemd/ciphera.service` par un test) ;
//! 2. `/etc/ciphera/ciphera.env` (surcharges de l'administrateur), lu comme
//!    root avec des controles : fichier ordinaire, jamais un lien, taille
//!    bornee, proprietaire root ou `ciphera`, non modifiable par le groupe
//!    ni les autres. Ce fichier est modifiable par le compte `ciphera` : les
//!    cles qui designent un fichier ou un dossier (`*_FILE`, `*_DIR` de
//!    configuration et de donnees) y sont ignorees, sinon root lirait pour
//!    lui un fichier choisi par ce compte ;
//! 3. l'environnement du processus (choix explicite de l'operateur).

use std::io;
use std::path::Path;

/// Fichier de surcharges locales lu par `ciphera.service`.
pub const ENV_FILE: &str = "/etc/ciphera/ciphera.env";

/// `Environment=` de `deploy/systemd/ciphera.service`.
pub const UNIT_ENV: [(&str, &str); 8] = [
    ("CIPHERA_CONFIG_DIR", "/etc/ciphera"),
    ("CIPHERA_DATA_DIR", "/var/lib/ciphera"),
    ("CIPHERA_PANEL_PORT", "8443"),
    ("CIPHERA_DB_SOCKET_DIR", "/run/ciphera/postgres"),
    ("CIPHERA_DB_PORT", "5432"),
    ("CIPHERA_DB_USER", "ciphera"),
    ("CIPHERA_DB_NAME", "ciphera"),
    ("CIPHERA_DB_PASSWORD_FILE", "/etc/ciphera/db_password"),
];

/// Cles de `ciphera.env` ignorees par les commandes root (chemins lus).
const PATH_KEYS: [&str; 4] = [
    "CIPHERA_CONFIG_DIR",
    "CIPHERA_DATA_DIR",
    "CIPHERA_DB_PASSWORD_FILE",
    "CIPHERA_DATABASE_URL_FILE",
];

/// Taille maximale de `ciphera.env`.
const MAX_ENV_FILE_BYTES: u64 = 64 * 1024;

/// Lignes `CLE=valeur` d'un fichier d'environnement systemd : commentaires
/// (`#`, `;`) et lignes vides ignores, guillemets simples ou doubles
/// entourant toute la valeur retires.
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| {
            let v = v.trim();
            let unquoted = [('"', '"'), ('\'', '\'')]
                .iter()
                .find_map(|(a, b)| v.strip_prefix(*a).and_then(|r| r.strip_suffix(*b)))
                .unwrap_or(v);
            (k.trim().to_string(), unquoted.to_string())
        })
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// Recherche des variables pour une commande d'administration : unite, puis
/// `ciphera.env` (sans les cles de chemin), puis le processus.
pub fn lookup(
    process: impl Fn(&str) -> Option<String>,
    env_file: &[(String, String)],
) -> impl Fn(&str) -> Option<String> {
    let file: Vec<(String, String)> = env_file
        .iter()
        .filter(|(k, _)| !PATH_KEYS.contains(&k.as_str()))
        .cloned()
        .collect();
    move |name: &str| {
        process(name)
            .or_else(|| {
                file.iter()
                    .rev()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            })
            .or_else(|| {
                UNIT_ENV
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            })
    }
}

/// Contenu de `path` (absent : `None`), lu avec les controles du module.
pub fn read_env_file(path: &Path) -> io::Result<Option<String>> {
    use std::io::Read;
    let refused = |why: &str| io::Error::other(format!("{} : {why}", path.display()));
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    let file = match opts.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(refused("illisible (lien symbolique ?)")),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(refused("fichier ordinaire attendu"));
    }
    if meta.len() > MAX_ENV_FILE_BYTES {
        return Err(refused("fichier trop long"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let service = std::fs::read_to_string("/etc/passwd")
            .ok()
            .and_then(|t| uid_of(&t, "ciphera"));
        if meta.uid() != 0 && Some(meta.uid()) != service {
            return Err(refused("proprietaire inattendu (root ou ciphera)"));
        }
        if meta.mode() & 0o022 != 0 {
            return Err(refused("modifiable par le groupe ou les autres"));
        }
    }
    let mut text = String::new();
    (&file).take(MAX_ENV_FILE_BYTES).read_to_string(&mut text)?;
    Ok(Some(text))
}

/// Configuration d'une commande d'administration : celle du service.
pub fn admin_config() -> Result<shared::config::Config, Box<dyn std::error::Error>> {
    let file = read_env_file(Path::new(ENV_FILE))?
        .map(|text| parse_env_file(&text))
        .unwrap_or_default();
    Ok(shared::config::Config::from_lookup(lookup(
        |name| std::env::var(name).ok(),
        &file,
    ))?)
}

/// Uid de `name` dans un texte au format `/etc/passwd`.
#[cfg_attr(not(unix), allow(dead_code))]
fn uid_of(passwd: &str, name: &str) -> Option<u32> {
    passwd.lines().find_map(|l| {
        let mut f = l.split(':');
        (f.next()? == name).then_some(())?;
        f.nth(1)?.parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn unit_values_match_the_shipped_service() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/systemd/ciphera.service");
        let text = std::fs::read_to_string(path).unwrap();
        let unit: Vec<(String, String)> = text
            .lines()
            .map(str::trim)
            .filter_map(|l| l.strip_prefix("Environment="))
            .filter_map(|v| v.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let ours: Vec<(String, String)> = UNIT_ENV
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(unit, ours);
    }

    #[test]
    fn defaults_reach_postgres_by_the_service_socket() {
        let get = lookup(none, &[]);
        assert_eq!(
            get("CIPHERA_DB_SOCKET_DIR").as_deref(),
            Some("/run/ciphera/postgres")
        );
        assert_eq!(
            get("CIPHERA_DB_PASSWORD_FILE").as_deref(),
            Some("/etc/ciphera/db_password")
        );
        assert_eq!(get("CIPHERA_PUBLIC_URL"), None);
    }

    #[test]
    fn env_file_overrides_the_unit_and_the_process_overrides_both() {
        let file = parse_env_file(
            "# local\nCIPHERA_DB_SOCKET_DIR=/var/run/postgresql\n\nCIPHERA_PUBLIC_URL=\"https://panel.example.com\"\n",
        );
        let get = lookup(none, &file);
        assert_eq!(
            get("CIPHERA_DB_SOCKET_DIR").as_deref(),
            Some("/var/run/postgresql")
        );
        assert_eq!(
            get("CIPHERA_PUBLIC_URL").as_deref(),
            Some("https://panel.example.com")
        );
        let get = lookup(
            |k| (k == "CIPHERA_DB_SOCKET_DIR").then(|| "/tmp/pg".to_string()),
            &file,
        );
        assert_eq!(get("CIPHERA_DB_SOCKET_DIR").as_deref(), Some("/tmp/pg"));
    }

    #[test]
    fn path_keys_from_the_env_file_are_ignored() {
        let file = parse_env_file(
            "CIPHERA_DB_PASSWORD_FILE=/etc/shadow\nCIPHERA_DATABASE_URL_FILE=/root/x\nCIPHERA_CONFIG_DIR=/root\nCIPHERA_DATA_DIR=/root\n",
        );
        let get = lookup(none, &file);
        assert_eq!(
            get("CIPHERA_DB_PASSWORD_FILE").as_deref(),
            Some("/etc/ciphera/db_password")
        );
        assert_eq!(get("CIPHERA_DATABASE_URL_FILE"), None);
        assert_eq!(get("CIPHERA_CONFIG_DIR").as_deref(), Some("/etc/ciphera"));
        assert_eq!(get("CIPHERA_DATA_DIR").as_deref(), Some("/var/lib/ciphera"));
    }

    #[test]
    fn parse_keeps_only_assignments() {
        assert_eq!(
            parse_env_file(" ; c\nA = 'x y'\nnot an assignment\n=v\nB=\n"),
            vec![
                ("A".to_string(), "x y".to_string()),
                ("B".to_string(), String::new())
            ]
        );
    }

    #[test]
    fn missing_env_file_is_not_an_error() {
        let dir = std::env::temp_dir().join(format!("ciphera-env-{}", uuid::Uuid::new_v4()));
        assert!(read_env_file(&dir.join("ciphera.env")).unwrap().is_none());
    }

    #[test]
    fn uid_is_read_from_passwd() {
        let passwd =
            "root:x:0:0::/root:/bin/sh\nciphera:x:998:998::/var/lib/ciphera:/usr/sbin/nologin\n";
        assert_eq!(uid_of(passwd, "ciphera"), Some(998));
        assert_eq!(uid_of(passwd, "nobody"), None);
    }

    #[cfg(unix)]
    #[test]
    fn env_file_writable_by_others_or_symlinked_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ciphera-env-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ciphera.env");
        std::fs::write(&file, "CIPHERA_DB_PORT=1\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
        // Proprietaire : l'utilisateur des tests (root en CI conteneur, sinon
        // refuse aussi pour le proprietaire) ; le mode suffit a refuser.
        assert!(read_env_file(&file).is_err());
        let link = dir.join("link.env");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(read_env_file(&link).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
