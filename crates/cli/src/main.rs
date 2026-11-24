//! `ciphera-cli` : outils d'administration du serveur.
//!
//! ```text
//! ciphera-cli updater                     demande du plan de controle (unite ciphera-updater.service)
//! ciphera-cli updater --recover           reprise seule, au demarrage (ciphera-updater-recover.service)
//! ciphera-cli updater --version vX.Y.Z    mise a jour vers une version superieure
//! ciphera-cli updater --version vX.Y.Z --allow-downgrade --restore-db <sauvegarde.sql.zst>
//!                                         retour a une version anterieure et a sa base
//! ciphera-cli post-install --from vX.Y.Z --to vX.Y.Z
//!                                         etape lancee par l'updater sur le nouveau binaire,
//!                                         fichiers en place, avant le redemarrage
//! ```
//!
//! L'updater exige root. Sans demande, il ne fait que reprendre une mise a
//! jour interrompue (retour a la version precedente). Sa configuration est
//! dans `/etc/ciphera-updater/updater.env` (root 0600) : port local du
//! panneau, compte et base PostgreSQL, dossier de donnees.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use ciphera_cli::updater::Mode;
#[cfg(unix)]
use ciphera_cli::updater::config::UpdaterConfig;
#[cfg(unix)]
use ciphera_cli::updater::{self, Outcome, Paths, Sources, UpdateError, host::Host};
use shared::update::Version;

const USAGE: &str = "usage : ciphera-cli updater [--recover | --version vX.Y.Z \
                     [--allow-downgrade --restore-db <sauvegarde.sql.zst>]]\n        \
                     ciphera-cli post-install --from vX.Y.Z --to vX.Y.Z";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("updater") => match parse_updater(&args[1..]) {
            Ok(mode) => run_updater(mode),
            Err(e) => {
                eprintln!("erreur : {e}\n{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("post-install") => match parse_post_install(&args[1..]) {
            Ok((from, to)) => {
                // Etape lancee par l'updater (root) sur le binaire de la
                // version installee, fichiers en place, services pas encore
                // relances : ce que cette version doit ajuster sur la machine
                // au-dela des fichiers de l'archive (unites et profil
                // AppArmor sont traites par l'updater). Un code non nul
                // ramene la version precedente. Rien a faire en v0.1.
                eprintln!("post-installation {from} -> {to} : rien a faire");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("erreur : {e}\n{USAGE}");
                ExitCode::from(2)
            }
        },
        Some("-h" | "--help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            println!("ciphera-cli {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `post-install --from <ancienne> --to <nouvelle>`.
fn parse_post_install(args: &[String]) -> Result<(Version, Version), String> {
    let (mut from, mut to) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let slot = match a.as_str() {
            "--from" => &mut from,
            "--to" => &mut to,
            _ => return Err("option inconnue".into()),
        };
        let v = it.next().ok_or("--from et --to attendent une version")?;
        *slot = Some(
            v.parse::<Version>()
                .map_err(|_| "version invalide (attendu vX.Y.Z)")?,
        );
    }
    match (from, to) {
        (Some(f), Some(t)) => Ok((f, t)),
        _ => Err("--from et --to sont exiges".into()),
    }
}

fn parse_updater(args: &[String]) -> Result<Mode, String> {
    let mut version = None;
    let mut allow_downgrade = false;
    let mut restore_db: Option<PathBuf> = None;
    let mut recover = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" => {
                let v = it.next().ok_or("--version attend une valeur")?;
                version = Some(
                    v.parse::<Version>()
                        .map_err(|_| "version invalide (attendu vX.Y.Z)")?,
                );
            }
            "--allow-downgrade" => allow_downgrade = true,
            "--restore-db" => {
                let p = it.next().ok_or("--restore-db attend une sauvegarde")?;
                restore_db = Some(PathBuf::from(p));
            }
            "--recover" => recover = true,
            _ => return Err("option inconnue".into()),
        }
    }
    if recover {
        if version.is_some() || allow_downgrade || restore_db.is_some() {
            return Err("--recover s'utilise seul".into());
        }
        return Ok(Mode::Recover);
    }
    if allow_downgrade && restore_db.is_none() {
        return Err(
            "--allow-downgrade exige --restore-db <sauvegarde.sql.zst> : une version \
             anterieure ne demarre pas sur la base migree par la version installee"
                .into(),
        );
    }
    if restore_db.is_some() && !allow_downgrade {
        return Err("--restore-db s'utilise avec --allow-downgrade".into());
    }
    match version {
        Some(version) => Ok(Mode::Manual {
            version,
            allow_downgrade,
            restore_db,
        }),
        None if allow_downgrade => Err("--allow-downgrade exige --version".into()),
        None => Ok(Mode::Request),
    }
}

fn run_updater(mode: Mode) -> ExitCode {
    #[cfg(unix)]
    if !rustix::process::geteuid().is_root() {
        eprintln!("erreur : l'updater exige root (sudo ciphera-cli updater ...)");
        return ExitCode::FAILURE;
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        eprintln!("erreur : l'updater ne fonctionne que sur Linux");
        ExitCode::FAILURE
    }
    #[cfg(unix)]
    {
        let Some(target) = updater::host_target() else {
            eprintln!("erreur : architecture non prise en charge");
            return ExitCode::FAILURE;
        };
        let sources = match Sources::from_env(|n| std::env::var(n).ok(), target) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("erreur : {e}");
                return ExitCode::FAILURE;
            }
        };
        let paths = Paths::system();
        // Configuration root seulement (/etc/ciphera-updater/updater.env),
        // jamais /etc/ciphera/ciphera.env (modifiable par le compte ciphera).
        let cfg = match UpdaterConfig::load(&paths.updater_config) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("erreur : configuration de l'updater : {e}");
                return ExitCode::FAILURE;
            }
        };
        if !cfg.ignored.is_empty() {
            eprintln!(
                "attention : {} : cles ignorees ({})",
                paths.updater_config.display(),
                cfg.ignored.join(", ")
            );
        }
        if cfg.data_dir != Path::new(shared::update::REQUEST_DIR) {
            eprintln!(
                "attention : CIPHERA_DATA_DIR={} : les demandes du panneau ne sont lues \
                 que dans {} (mises a jour par le panneau indisponibles)",
                cfg.data_dir.display(),
                shared::update::REQUEST_DIR
            );
        }
        // Autorite de test pour curl : environnement de l'unite (root), en
        // mode test seulement (comme les autres surcharges de Sources).
        let test_ca_bundle = (std::env::var("CIPHERA_INSTALL_TEST").as_deref() == Ok("1"))
            .then(|| std::env::var_os("CURL_CA_BUNDLE"))
            .flatten()
            .map(PathBuf::from);
        let host = Host {
            db_password_file: paths.config_dir.join("db_password"),
            db_user: cfg.db_user,
            db_name: cfg.db_name,
            tmpfiles: paths.tmpfiles.clone(),
            health_url: format!("http://127.0.0.1:{}/api/health", cfg.panel_local_port),
            test_ca_bundle,
            cli: paths.bin_dir.join("ciphera-cli"),
            apparmor_profile: paths.apparmor_dir.join(updater::APPARMOR_PROFILE),
        };
        report(updater::run(mode, &host, &paths, &sources), &paths.request)
    }
}

#[cfg(unix)]
fn report(result: Result<Outcome, UpdateError>, request: &Path) -> ExitCode {
    match result {
        Ok(Outcome::NoRequest) => {
            eprintln!("aucune demande de mise a jour ({})", request.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::Recovered) => {
            eprintln!("mise a jour interrompue annulee : version precedente en place");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Busy) => {
            eprintln!("mise a jour en cours : reprise laissee a l'updater actif");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Updated { from, to }) => {
            eprintln!("Ciphera mis a jour : {from} -> {to}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Mode, String> {
        parse_updater(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn downgrade_needs_a_database_dump() {
        let e = parse(&["--version", "v0.0.9", "--allow-downgrade"]).unwrap_err();
        assert!(e.contains("--restore-db"), "{e}");
        assert!(parse(&["--version", "v0.0.9", "--restore-db", "a.sql.zst"]).is_err());
        assert!(matches!(
            parse(&[
                "--version",
                "v0.0.9",
                "--allow-downgrade",
                "--restore-db",
                "/var/backups/ciphera/a.sql.zst"
            ]),
            Ok(Mode::Manual {
                allow_downgrade: true,
                restore_db: Some(_),
                ..
            })
        ));
        assert!(matches!(parse(&["--recover"]), Ok(Mode::Recover)));
        assert!(parse(&["--recover", "--version", "v1.0.0"]).is_err());
        assert!(matches!(parse(&[]), Ok(Mode::Request)));
    }
}
