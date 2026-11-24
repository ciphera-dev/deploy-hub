//! Configuration de l'updater : `/etc/ciphera-updater/updater.env`, ecrit
//! par `install.sh` (dossier root 0755, fichier root 0600).
//!
//! L'updater tourne en root : il ne lit jamais `/etc/ciphera/ciphera.env`,
//! que le compte `ciphera` peut modifier (`PATH`, `LD_PRELOAD`, mode de test,
//! cle ou origine des releases y donneraient root). Seules les cles
//! ci-dessous sont lues, par l'updater lui-meme (pas d'`EnvironmentFile=`) ;
//! toute autre cle est ignoree et signalee. Le fichier et son dossier
//! doivent appartenir a root et n'etre modifiables par personne d'autre.
//!
//! | Cle                        | Defaut             |
//! |----------------------------|--------------------|
//! | `CIPHERA_PANEL_LOCAL_PORT` | `8444` (healthcheck sur 127.0.0.1) |
//! | `CIPHERA_DB_USER`          | `ciphera`          |
//! | `CIPHERA_DB_NAME`          | `ciphera`          |
//! | `CIPHERA_DATA_DIR`         | `/var/lib/ciphera` |

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use shared::config::Config;

/// Taille maximale du fichier.
const MAX_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdaterConfig {
    /// Port local du panneau (`GET http://127.0.0.1:<port>/api/health`).
    pub panel_local_port: u16,
    /// Compte et base PostgreSQL du plan de controle.
    pub db_user: String,
    pub db_name: String,
    /// Dossier de donnees du plan de controle (les demandes ne sont lues
    /// que dans `shared::update::REQUEST_DIR`).
    pub data_dir: PathBuf,
    /// Cles presentes mais ignorees.
    pub ignored: Vec<String>,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        UpdaterConfig {
            panel_local_port: Config::DEFAULT_PANEL_LOCAL_PORT,
            db_user: Config::DEFAULT_DB_USER.into(),
            db_name: Config::DEFAULT_DB_NAME.into(),
            data_dir: PathBuf::from(Config::DEFAULT_DATA_DIR),
            ignored: Vec::new(),
        }
    }
}

/// Nom de compte ou de base passe a `pg_dump`/`psql` (`--username=…`).
fn db_name_ok(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 63
        && !v.starts_with('-')
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl UpdaterConfig {
    /// Lignes `CLE=valeur` ; lignes vides et commentaires (`#`) ignores.
    pub fn parse(text: &str) -> Result<UpdaterConfig, String> {
        let mut c = UpdaterConfig::default();
        for line in text.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("ligne invalide : {line}"));
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "CIPHERA_PANEL_LOCAL_PORT" => {
                    c.panel_local_port = value
                        .parse::<u16>()
                        .ok()
                        .filter(|p| *p > 0)
                        .ok_or_else(|| format!("{key} invalide"))?;
                }
                "CIPHERA_DB_USER" | "CIPHERA_DB_NAME" => {
                    if !db_name_ok(value) {
                        return Err(format!("{key} invalide"));
                    }
                    if key == "CIPHERA_DB_USER" {
                        c.db_user = value.into();
                    } else {
                        c.db_name = value.into();
                    }
                }
                "CIPHERA_DATA_DIR" => {
                    if !value.starts_with('/') {
                        return Err(format!("{key} : chemin absolu attendu"));
                    }
                    c.data_dir = PathBuf::from(value);
                }
                _ => c.ignored.push(key.to_string()),
            }
        }
        Ok(c)
    }

    /// Lit `path` ; absent : valeurs par defaut. Refuse un fichier (ou un
    /// dossier) qui n'appartient pas a root ou que d'autres peuvent modifier,
    /// et un lien symbolique.
    pub fn load(path: &Path) -> Result<UpdaterConfig, String> {
        let file = match open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{} : {e}", path.display())),
        };
        let meta = file
            .metadata()
            .map_err(|e| format!("{} : {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} : fichier ordinaire attendu", path.display()));
        }
        root_only(path, &meta)?;
        if let Some(dir) = path.parent() {
            let dmeta = fs::metadata(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
            root_only(dir, &dmeta)?;
        }
        let mut text = String::new();
        (&file)
            .take(MAX_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|e| format!("{} : {e}", path.display()))?;
        if text.len() as u64 > MAX_BYTES {
            return Err(format!("{} : fichier trop long", path.display()));
        }
        Self::parse(&text).map_err(|e| format!("{} : {e}", path.display()))
    }
}

fn open(path: &Path) -> io::Result<File> {
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    opts.open(path)
}

/// Proprietaire root, non modifiable par le groupe ni les autres.
fn root_only(path: &Path, meta: &fs::Metadata) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(format!(
                "{} doit appartenir a root et n'etre modifiable que par root \
                 (chown root:root, chmod go-w) : ignore",
                path.display()
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (path, meta);
    Ok(())
}
