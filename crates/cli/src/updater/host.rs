//! Systeme reel de l'updater (Linux, root) : `curl` (https seulement, comme
//! `install.sh`), `docker exec ciphera-postgres pg_dump` / `psql`,
//! `systemctl`, `systemd-tmpfiles`. Programmes et arguments fixes : seules les URL
//! (construites et verifiees par l'updater) et les chemins de travail
//! varient, jamais via un shell.
//!
//! Chaque programme est lance par son chemin absolu (cherche dans les seuls
//! dossiers systeme, jamais `/usr/local`) avec un environnement vide
//! (`PATH` systeme seulement, plus `PGPASSWORD` pour `docker exec`) : rien
//! de l'environnement de l'updater (`LD_PRELOAD`, `PATH`…) ne passe.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use shared::update::Version;

use super::{SERVICES, System, UnitChange};

/// Conteneur PostgreSQL de l'instance (lance par l'agent).
const POSTGRES_CONTAINER: &str = "ciphera-postgres";
/// Taille maximale d'un telechargement.
const MAX_DOWNLOAD_BYTES: &str = "536870912";
/// Connexion, duree d'un essai et duree totale avec les nouveaux essais
/// (secondes) : un serveur bloque ne garde pas le verrou jusqu'au delai de
/// l'unite.
const CONNECT_TIMEOUT: &str = "15";
const DOWNLOAD_TIMEOUT: &str = "300";
const RETRY_MAX_TIME: &str = "600";
/// Attente du conteneur PostgreSQL avant une restauration (demarrage de
/// la machine : Docker le relance de lui-meme).
const POSTGRES_WAIT: Duration = Duration::from_secs(180);
/// Prefixe d'une restauration : schema vide, puis la sauvegarde (format
/// texte de `pg_dump`, deja verifiee), le tout dans une seule transaction.
const RESET_SCHEMA: &[u8] =
    b"BEGIN;\nDROP SCHEMA IF EXISTS public CASCADE;\nCREATE SCHEMA public;\n";
/// Arguments de `psql` pour une restauration. Jamais `--single-transaction` :
/// psql validerait la transaction a la fin de son entree, meme tronquee.
const RESTORE_ARGS: [&str; 2] = ["--quiet", "--set=ON_ERROR_STOP=1"];

/// Entree de `psql` pour une restauration : `BEGIN`, schema vide, la
/// sauvegarde, puis `COMMIT` seulement une fois toute la sauvegarde ecrite.
fn write_restore(out: &mut impl Write, dump: &mut impl io::Read) -> io::Result<()> {
    out.write_all(RESET_SCHEMA)?;
    io::copy(dump, out)?;
    // Fin de ligne avant le `COMMIT` : la derniere instruction de la
    // sauvegarde peut ne pas en avoir.
    out.write_all(b"\nCOMMIT;\n")?;
    out.flush()
}
/// Seul `PATH` transmis aux programmes lances.
pub const CHILD_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";
/// Dossiers ou chercher les programmes (root seulement).
const SYSTEM_DIRS: [&str; 4] = ["/usr/sbin", "/usr/bin", "/sbin", "/bin"];
/// Derniere migration appliquee par le plan de controle.
const LATEST_MIGRATION_SQL: &str =
    "SELECT coalesce(max(version), 0) FROM _sqlx_migrations WHERE success";

pub struct Host {
    /// Mot de passe de la base (`/etc/ciphera/db_password`, au compte
    /// ciphera ; jamais configurable), lu par [`read_password`].
    pub db_password_file: PathBuf,
    /// Compte et base du plan de controle (configuration de l'updater).
    pub db_user: String,
    pub db_name: String,
    /// Configuration des dossiers des sockets a recreer.
    pub tmpfiles: PathBuf,
    /// `http://127.0.0.1:<port local>/api/health` : ecoute locale du
    /// panneau, ouverte meme quand un domaine du panneau ferme 8443.
    pub health_url: String,
    /// Autorite de certification de test (`--cacert`), seulement avec
    /// `CIPHERA_INSTALL_TEST=1` dans l'environnement de l'unite (root).
    pub test_ca_bundle: Option<PathBuf>,
    /// `ciphera-cli` installe (etape de post-installation).
    pub cli: PathBuf,
    /// Profil AppArmor de buildkitd (`/etc/apparmor.d/ciphera-buildkitd`) :
    /// recharge par `apparmor.service` s'il est installe.
    pub apparmor_profile: PathBuf,
}

/// Proprietes d'une unite (`systemctl show`).
#[derive(Debug, PartialEq, Eq)]
struct UnitState {
    active: String,
    invocation: String,
    restarts: String,
}

/// Sortie de `systemctl show -p ActiveState -p InvocationID -p NRestarts`
/// (`cle=valeur`, une par ligne).
fn parse_show(text: &str) -> UnitState {
    let mut st = UnitState {
        active: String::new(),
        invocation: String::new(),
        restarts: String::new(),
    };
    for line in text.lines() {
        match line.split_once('=') {
            Some(("ActiveState", v)) => st.active = v.trim().to_string(),
            Some(("InvocationID", v)) => st.invocation = v.trim().to_string(),
            Some(("NRestarts", v)) => st.restarts = v.trim().to_string(),
            _ => {}
        }
    }
    st
}

fn unit_state(unit: &str) -> io::Result<UnitState> {
    let out = clean_command("systemctl")
        .args([
            "show",
            "-p",
            "ActiveState",
            "-p",
            "InvocationID",
            "-p",
            "NRestarts",
            unit,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "systemctl show {unit} : {}",
            out.status
        )));
    }
    Ok(parse_show(&String::from_utf8_lossy(&out.stdout)))
}

/// Verdict d'une observation : chaque service de `before` actif, sur une
/// invocation nouvelle (`first` : celle vue au debut de la fenetre), sans
/// redemarrage depuis.
fn settled_check(
    before: &[(String, String)],
    first: &[UnitState],
    now: &[UnitState],
) -> Result<(), String> {
    for (((unit, old), f), n) in before.iter().zip(first).zip(now) {
        if n.active != "active" {
            return Err(format!("{unit} n'est plus actif ({})", n.active));
        }
        if n.invocation == *old {
            return Err(format!("{unit} n'a pas ete relance"));
        }
        if n.invocation != f.invocation || n.restarts != f.restarts {
            return Err(format!("{unit} redemarre en boucle"));
        }
    }
    Ok(())
}

/// Taille maximale du fichier du mot de passe de la base.
const MAX_PASSWORD_BYTES: u64 = 4096;
/// Compte du plan de controle (proprietaire de `db_password`).
#[cfg_attr(not(unix), allow(dead_code))]
const SERVICE_ACCOUNT: &str = "ciphera";

/// Proprietaire accepte pour `db_password` : root, le proprietaire du
/// dossier, ou le compte de service quand le dossier est a root (dossier
/// cree par root avant l'installation, fichiers remis au compte ciphera).
#[cfg_attr(not(unix), allow(dead_code))]
fn trusted_owner(file_uid: u32, dir_uid: u32, service_uid: Option<u32>) -> bool {
    file_uid == 0 || file_uid == dir_uid || (dir_uid == 0 && service_uid == Some(file_uid))
}

/// uid d'un compte dans le texte de `/etc/passwd` (uid 0 refuse).
#[cfg_attr(not(unix), allow(dead_code))]
fn uid_in_passwd(passwd: &str, name: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut f = line.split(':');
        (f.next()? == name).then_some(())?;
        let uid: u32 = f.nth(1)?.parse().ok()?;
        (uid != 0).then_some(uid)
    })
}

/// Lit le mot de passe de la base dans `path`, dont le dossier
/// (`/etc/ciphera`) appartient au compte ciphera : root ne doit pas y lire
/// un autre fichier que ce compte y aurait mis a la place. Ouverture sans
/// suivre de lien (`O_NOFOLLOW`), puis controles sur le descripteur ouvert
/// (`fstat`) : fichier ordinaire, un seul lien (pas de lien dur vers un
/// fichier d'un autre compte), proprietaire root, celui du dossier ou, dossier
/// a root, le compte ciphera ([`trusted_owner`]) ; ni
/// groupe ni autres n'y ont acces, 4 Kio au plus.
pub fn read_password(path: &Path) -> io::Result<String> {
    use std::io::Read;
    let refused = |why: &str| io::Error::other(format!("{} : {why}", path.display()));
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
        opts.custom_flags(flags.bits() as i32);
    }
    let file = opts.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(refused("fichier ordinaire attendu"));
    }
    if meta.len() > MAX_PASSWORD_BYTES {
        return Err(refused("fichier trop long"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let dir_owner = match path.parent() {
            Some(dir) => fs::metadata(dir)?.uid(),
            None => 0,
        };
        if meta.nlink() != 1 {
            return Err(refused("lien dur refuse"));
        }
        // Dossier a root (`/etc/ciphera` cree avant l'installation) :
        // `ciphera init` y laisse un fichier au compte ciphera.
        let service = (dir_owner == 0)
            .then(|| fs::read_to_string("/etc/passwd").ok())
            .flatten()
            .and_then(|text| uid_in_passwd(&text, SERVICE_ACCOUNT));
        if !trusted_owner(meta.uid(), dir_owner, service) {
            return Err(refused("proprietaire inattendu"));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(refused("accessible au groupe ou aux autres (chmod 0600)"));
        }
    }
    let mut text = String::new();
    (&file).take(MAX_PASSWORD_BYTES).read_to_string(&mut text)?;
    let password = text.trim();
    if password.is_empty() || password.chars().any(char::is_control) {
        return Err(refused("mot de passe vide ou invalide"));
    }
    Ok(password.to_string())
}

/// Chemin absolu d'un programme systeme.
pub fn program(name: &str) -> PathBuf {
    SYSTEM_DIRS
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| Path::new("/usr/bin").join(name))
}

/// `name` par son chemin absolu, environnement vide sauf `PATH` systeme.
pub fn clean_command(name: &str) -> Command {
    let mut cmd = Command::new(program(name));
    cmd.env_clear().env("PATH", CHILD_PATH);
    cmd
}

impl Host {
    fn password(&self) -> io::Result<String> {
        read_password(&self.db_password_file)
    }

    /// `docker exec [-i] -e PGPASSWORD ciphera-postgres <outil> --username
    /// --dbname --no-password` ; mot de passe passe par l'environnement de
    /// `docker` puis de l'exec (`-e PGPASSWORD` sans valeur) : jamais en
    /// argument.
    fn pg_command(&self, interactive: bool, tool: &str, password: &str) -> Command {
        let mut cmd = clean_command("docker");
        cmd.arg("exec");
        if interactive {
            cmd.arg("-i");
        }
        cmd.args(["-e", "PGPASSWORD", POSTGRES_CONTAINER, tool])
            .arg(format!("--username={}", self.db_user))
            .arg(format!("--dbname={}", self.db_name))
            .env("PGPASSWORD", password);
        // pg_isready ne demande jamais de mot de passe (et ignore l'option).
        if tool != "pg_isready" {
            cmd.arg("--no-password");
        }
        cmd
    }

    /// Attend que PostgreSQL accepte les connexions (conteneur relance par
    /// Docker au demarrage de la machine).
    fn wait_postgres(&self, password: &str) -> io::Result<()> {
        let deadline = Instant::now() + POSTGRES_WAIT;
        loop {
            let ready = self
                .pg_command(false, "pg_isready", password)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other(format!(
                    "{POSTGRES_CONTAINER} ne repond pas apres {} s",
                    POSTGRES_WAIT.as_secs()
                )));
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    }
}

fn status(what: &str, s: io::Result<std::process::ExitStatus>) -> io::Result<()> {
    match s {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(io::Error::other(format!("{what} : {s}"))),
        Err(e) => Err(io::Error::other(format!("{what} : {e}"))),
    }
}

fn systemctl(args: &[&str]) -> io::Result<()> {
    status(
        &format!("systemctl {}", args.join(" ")),
        clean_command("systemctl")
            .args(args)
            .stdin(Stdio::null())
            .status(),
    )
}

impl System for Host {
    fn download(&self, url: &str, dest: &Path) -> io::Result<()> {
        if !url.starts_with("https://") {
            return Err(io::Error::other("URL https exigee"));
        }
        let mut cmd = clean_command("curl");
        cmd.args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--retry",
            "3",
            "--retry-max-time",
            RETRY_MAX_TIME,
            "--connect-timeout",
            CONNECT_TIMEOUT,
            "--max-time",
            DOWNLOAD_TIMEOUT,
            "--max-filesize",
            MAX_DOWNLOAD_BYTES,
        ]);
        if let Some(ca) = &self.test_ca_bundle {
            cmd.arg("--cacert").arg(ca);
        }
        let s = cmd
            .arg("-o")
            .arg(dest)
            .arg(url)
            .stdin(Stdio::null())
            .status();
        status("curl", s)
    }

    fn pg_dump(&self, dest: &Path) -> io::Result<()> {
        let password = self.password()?;
        let mut child = self
            .pg_command(false, "pg_dump", &password)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut out = child.stdout.take().expect("sortie redirigee");
        let file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(dest)?;
        let encoded = zstd::stream::copy_encode(&mut out, &file, 3);
        drop(out);
        let exit = child.wait();
        encoded?;
        status("pg_dump", exit)?;
        file.sync_all()
    }

    fn latest_migration(&self) -> io::Result<u32> {
        let password = self.password()?;
        let out = self
            .pg_command(false, "psql", &password)
            .args(["--quiet", "--no-align", "--tuples-only", "--command"])
            .arg(LATEST_MIGRATION_SQL)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other(format!(
                "lecture de la derniere migration : {}",
                out.status
            )));
        }
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .map_err(|_| io::Error::other("derniere migration illisible"))
    }

    fn restore_db(&self, sql: &Path) -> io::Result<()> {
        let password = self.password()?;
        let mut input = fs::File::open(sql)?;
        self.wait_postgres(&password)?;
        // Une seule transaction, validee par le seul `COMMIT` ecrit apres
        // toute la sauvegarde (`write_restore`) : erreur SQL (ON_ERROR_STOP),
        // erreur de lecture de notre cote ou client docker tue, psql se
        // termine sans `COMMIT` et PostgreSQL annule tout.
        let mut child = self
            .pg_command(true, "psql", &password)
            .args(RESTORE_ARGS)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()?;
        let mut stdin = child.stdin.take().expect("entree redirigee");
        if let Err(e) = write_restore(&mut stdin, &mut input) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        drop(stdin);
        status("psql (restauration de la base)", child.wait())
    }

    fn stop_services(&self) -> io::Result<()> {
        systemctl(&["stop", "ciphera.service"])
    }

    fn restart_services(&self) -> io::Result<()> {
        systemctl(&["daemon-reload"])?;
        // Dossiers des sockets de la nouvelle version ; un echec n'empeche
        // pas le redemarrage (le healthcheck tranche).
        if let Err(e) = status(
            "systemd-tmpfiles",
            clean_command("systemd-tmpfiles")
                .arg("--create")
                .arg(&self.tmpfiles)
                .stdin(Stdio::null())
                .status(),
        ) {
            eprintln!("attention : {e}");
        }
        // L'agent (sans Docker, inactif) et le proxy ne sont relances que
        // s'ils tournaient ; le plan de controle l'est toujours.
        systemctl(&[
            "try-restart",
            "ciphera-agent.service",
            "ciphera-proxy.service",
        ])?;
        // Profil AppArmor de la nouvelle version : recharge par
        // apparmor.service (hors du bac a sable de l'updater).
        if self.apparmor_profile.is_file()
            && let Err(e) = systemctl(&["try-reload-or-restart", "apparmor.service"])
        {
            eprintln!("attention : {e}");
        }
        systemctl(&["restart", "ciphera.service"])
    }

    fn running_services(&self) -> io::Result<Vec<(String, String)>> {
        let mut out = Vec::new();
        for unit in SERVICES {
            let st = unit_state(unit)?;
            if st.active == "active" {
                out.push((unit.to_string(), st.invocation));
            }
        }
        Ok(out)
    }

    fn services_settled(
        &self,
        before: &[(String, String)],
        window: Duration,
    ) -> Result<(), String> {
        let observe = || -> Result<Vec<UnitState>, String> {
            before
                .iter()
                .map(|(u, _)| unit_state(u).map_err(|e| e.to_string()))
                .collect()
        };
        let first = observe()?;
        settled_check(before, &first, &first)?;
        let deadline = Instant::now() + window;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_secs(1));
            settled_check(before, &first, &observe()?)?;
        }
        Ok(())
    }

    fn enabled_units(&self, units: &[String]) -> io::Result<Vec<String>> {
        let mut enabled = Vec::new();
        for u in units {
            let out = clean_command("systemctl")
                .args(["is-enabled", u.as_str()])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .map_err(|e| io::Error::other(format!("systemctl is-enabled {u} : {e}")))?;
            // Code non nul pour `disabled`, `masked`… : seule la sortie compte.
            if String::from_utf8_lossy(&out.stdout).trim() == "enabled" {
                enabled.push(u.clone());
            }
        }
        Ok(enabled)
    }

    fn apply_units(&self, change: &UnitChange) -> io::Result<()> {
        if !change.disable.is_empty() {
            let mut args = vec!["disable"];
            if change.stop {
                args.push("--now");
            }
            args.extend(change.disable.iter().map(String::as_str));
            // Unite deja absente ou deja desactivee : pas bloquant.
            if let Err(e) = systemctl(&args) {
                eprintln!("attention : {e}");
            }
        }
        systemctl(&["daemon-reload"])?;
        if !change.enable.is_empty() {
            let mut args = vec!["reenable"];
            args.extend(change.enable.iter().map(String::as_str));
            systemctl(&args)?;
        }
        if !change.start.is_empty() {
            let mut args = vec!["start"];
            args.extend(change.start.iter().map(String::as_str));
            systemctl(&args)?;
        }
        Ok(())
    }

    fn apparmor_parser_major(&self) -> Option<u32> {
        let out = clean_command("apparmor_parser")
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        super::parse_apparmor_parser_major(&String::from_utf8_lossy(&out.stdout))
    }

    fn post_install(&self, from: Version, to: Version) -> io::Result<()> {
        let mut cmd = Command::new(&self.cli);
        cmd.env_clear().env("PATH", CHILD_PATH);
        status(
            "ciphera-cli post-install",
            cmd.args(["post-install", "--from"])
                .arg(from.to_string())
                .arg("--to")
                .arg(to.to_string())
                .stdin(Stdio::null())
                .status(),
        )
    }

    fn wait_healthy(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let ok = clean_command("curl")
                .args(["-fsS", "-o", "/dev/null", "--max-time", "5"])
                .arg(&self.health_url)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                return true;
            }
            if Instant::now() + Duration::from_secs(2) >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;

    fn st(active: &str, invocation: &str, restarts: &str) -> UnitState {
        UnitState {
            active: active.into(),
            invocation: invocation.into(),
            restarts: restarts.into(),
        }
    }

    #[test]
    fn show_output_is_parsed() {
        assert_eq!(
            parse_show(
                "ActiveState=active
InvocationID=abc
NRestarts=2
"
            ),
            st("active", "abc", "2")
        );
    }

    /// Proxy qui plante en boucle : nouvelle invocation ou compteur qui
    /// monte pendant la fenetre, ou etat `activating` entre deux essais.
    #[test]
    fn crash_looping_service_is_not_settled() {
        let before = vec![("ciphera-proxy.service".to_string(), "old".to_string())];
        let first = [st("active", "n1", "0")];
        assert!(settled_check(&before, &first, &first).is_ok());
        for now in [
            st("activating", "n1", "0"),
            st("active", "n2", "1"),
            st("active", "n1", "1"),
            st("failed", "n1", "0"),
        ] {
            assert!(settled_check(&before, &first, &[now]).is_err());
        }
        // Pas relance du tout : ancienne invocation.
        let same = [st("active", "old", "0")];
        assert!(settled_check(&before, &same, &same).is_err());
    }

    /// La transaction n'est validee que par le `COMMIT` ecrit apres toute la
    /// sauvegarde : une entree coupee (client docker tue) se termine sans
    /// `COMMIT` et PostgreSQL annule tout. `--single-transaction`, lui,
    /// validerait a la fin de l'entree, meme tronquee.
    #[test]
    fn restore_commits_only_after_the_whole_dump() {
        let mut out = Vec::new();
        write_restore(&mut out, &mut &b"CREATE TABLE t ();"[..]).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("BEGIN;\nDROP SCHEMA"), "{text}");
        assert!(text.ends_with("CREATE TABLE t ();\nCOMMIT;\n"), "{text}");
        assert!(!RESTORE_ARGS.contains(&"--single-transaction"));
        assert!(RESTORE_ARGS.contains(&"--set=ON_ERROR_STOP=1"));
    }

    /// `/etc/ciphera` cree par root avant l'installation : le fichier au
    /// compte ciphera est accepte ; jamais celui d'un autre compte.
    #[test]
    fn password_owner_rules() {
        let accounts =
            "root:x:0:0::/root:/bin/sh\nciphera:x:998:998::/var/lib/ciphera:/usr/sbin/nologin\n";
        let service = uid_in_passwd(accounts, SERVICE_ACCOUNT);
        assert_eq!(service, Some(998));
        assert!(trusted_owner(0, 0, service));
        assert!(trusted_owner(998, 998, None), "dossier au compte ciphera");
        assert!(
            trusted_owner(998, 0, service),
            "dossier a root, fichier a ciphera"
        );
        assert!(!trusted_owner(1000, 0, service), "autre compte");
        assert!(
            !trusted_owner(998, 1000, service),
            "dossier d'un autre compte"
        );
        assert!(!trusted_owner(998, 0, None), "compte ciphera absent");
        assert_eq!(uid_in_passwd("ciphera:x:0:0::/:/bin/sh\n", "ciphera"), None);
        assert_eq!(uid_in_passwd(accounts, "absent"), None);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Rien de l'environnement de l'updater ne passe aux programmes lances.
    #[test]
    fn children_get_an_empty_environment() {
        assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
        let out = clean_command("env").output().unwrap();
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            format!("PATH={CHILD_PATH}\n")
        );
        assert!(program("env").is_absolute());
        assert!(!program("env").starts_with("/usr/local"));
    }

    /// Le mot de passe n'est lu que dans un fichier ordinaire 0600, ni lien
    /// symbolique ni lien dur (dossier modifiable par le compte ciphera).
    #[test]
    fn password_file_is_never_followed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ciphera-pw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let pw = dir.join("db_password");
        let other = dir.join("autre");
        fs::write(&other, "secret d'un autre fichier\n").unwrap();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();

        std::os::unix::fs::symlink(&other, &pw).unwrap();
        assert!(read_password(&pw).is_err(), "lien symbolique suivi");
        fs::remove_file(&pw).unwrap();

        fs::hard_link(&other, &pw).unwrap();
        let e = read_password(&pw).unwrap_err().to_string();
        assert!(e.contains("lien dur"), "{e}");
        fs::remove_file(&pw).unwrap();

        fs::write(&pw, "mot-de-passe\n").unwrap();
        fs::set_permissions(&pw, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_password(&pw).is_err(), "lisible par le groupe");
        fs::set_permissions(&pw, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_password(&pw).unwrap(), "mot-de-passe");

        fs::create_dir(dir.join("d")).unwrap();
        assert!(read_password(&dir.join("d")).is_err(), "dossier accepte");
        let _ = fs::remove_dir_all(&dir);
    }
}
