//! Updater (`ciphera-cli updater`) avec un systeme simule : telechargements
//! servis depuis la memoire, pg_dump, systemctl et healthcheck enregistres.
//! Aucun reseau, aucun root : tourne aussi sous Windows. Le parcours reel
//! (systemd, unite .path, curl, pg_dump) est teste par install/tests/update.sh.
//!
//! La base simulee porte un numero de schema : un `ciphera` plus recent la
//! migre en demarrant, un `ciphera` plus ancien refuse de demarrer sur un
//! schema qu'il ne connait pas (comme `sqlx::migrate!`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciphera_cli::updater::{self, Limit, Mode, Outcome, Paths, Sources, System, UpdateError};
use sha2::{Digest, Sha256};
use shared::update::{KEY_FILE, UpdateRequest, Version};

const TARGET: &str = "x86_64-unknown-linux-musl";
const BASE: &str = "https://releases.test";
const TOOLS: &str = "https://tools.test";
const NOW: u64 = 1_800_000_000;

fn v(s: &str) -> Version {
    s.parse().unwrap()
}

/// Dossier temporaire supprime a la fin du test.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ciphera-updater-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// --- release de test --------------------------------------------------------

struct Keys {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
}

fn keys() -> Keys {
    let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    Keys {
        pk: kp.pk,
        sk: kp.sk,
    }
}

fn sign(keys: &Keys, data: &[u8]) -> Vec<u8> {
    minisign::sign(Some(&keys.pk), &keys.sk, data, None, None)
        .unwrap()
        .to_string()
        .into_bytes()
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn targz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    for (path, data) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        tar.append_data(&mut h, path, *data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// Outil de l'hote (executable).
fn write_tool(path: &Path) {
    std::fs::write(path, b"outil").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn binary(name: &str, version: Version) -> Vec<u8> {
    format!("{name} {version}").into_bytes()
}

/// Release `version` signee par `keys`, servie sous BASE et TOOLS.
fn publish(files: &mut HashMap<String, Vec<u8>>, keys: &Keys, version: Version) {
    publish_with(files, keys, version, &[], &[]);
}

/// Idem, avec des fichiers en plus (`systemd/…`, `apparmor/…`) et sans les
/// unites de `skip`.
fn publish_with(
    files: &mut HashMap<String, Vec<u8>>,
    keys: &Keys,
    version: Version,
    extra: &[(&str, &str)],
    skip: &[&str],
) {
    let name = format!("ciphera-{version}-{TARGET}");
    let tools = [
        ("railpack", "v0.43.0", "railpack", "railpack"),
        ("buildctl", "v0.35.1", "bin/buildctl", "buildctl"),
        ("mise", "2026.10.14", "mise/bin/mise", "mise-2026.10.14"),
    ];
    let mut manifest = String::new();
    for (tool, tool_version, member, dest) in tools {
        let archive = targz(&[(member, format!("{tool} {version}").as_bytes())]);
        // Chemin propre a chaque release de test (contenus differents).
        let path =
            format!("{tool}/releases/download/{tool_version}/{version}/{tool}-{TARGET}.tar.gz");
        manifest.push_str(&format!(
            "{tool} {tool_version} {} {path} {member} {dest}\n",
            sha256_hex(&archive)
        ));
        files.insert(format!("{TOOLS}/{path}"), archive);
    }
    let bins: Vec<(String, Vec<u8>)> = updater::BINARIES
        .iter()
        .map(|b| (format!("{name}/bin/{b}"), binary(b, version)))
        .chain(
            extra
                .iter()
                .map(|(p, d)| (format!("{name}/{p}"), d.as_bytes().to_vec())),
        )
        .chain(updater::UNITS.iter().chain(&[updater::TMPFILES]).filter(|u| !skip.contains(u)).map(|u| {
            let mut unit = format!("# {u} {version}");
            if *u == "ciphera-agent.service" {
                // Outil de l'hote exige par l'unite : nft, lance par
                // `ciphera-agent egress` (filtrage de la sortie).
                unit.push_str(
                    "\nConditionPathExists=/var/run/docker.sock\nExecStartPre=+/usr/local/bin/ciphera-agent egress load\nExecStart=/usr/local/bin/ciphera-agent\n",
                );
            }
            (format!("{name}/systemd/{u}"), unit.into_bytes())
        }))
        .chain([
            (
                format!("{name}/containers/postgres.digest"),
                format!("postgres@{version}").into_bytes(),
            ),
            (format!("{name}/tools.manifest"), manifest.into_bytes()),
            (
                format!("{name}/migration"),
                format!("{}\n", schema_of(version)).into_bytes(),
            ),
            (format!("{name}/LICENSE"), b"AGPL".to_vec()),
        ])
        .collect();
    let entries: Vec<(&str, &[u8])> = bins
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect();
    let archive = targz(&entries);
    let sums = format!("{}  {name}.tar.gz\n", sha256_hex(&archive));
    let dir = format!("{BASE}/releases/{version}");
    files.insert(
        format!("{dir}/SHA256SUMS.minisig"),
        sign(keys, sums.as_bytes()),
    );
    files.insert(format!("{dir}/SHA256SUMS"), sums.into_bytes());
    files.insert(format!("{dir}/{name}.tar.gz"), archive);
}

// --- systeme simule ---------------------------------------------------------

#[derive(Default)]
struct Mock {
    files: Mutex<HashMap<String, Vec<u8>>>,
    events: Mutex<Vec<String>>,
    healthy: bool,
    /// `ciphera` installe au moment du pg_dump.
    bin_at_dump: Mutex<Option<Vec<u8>>>,
    bin_dir: PathBuf,
    /// Duree du pg_dump simule, et nombre de mises a jour simultanees.
    dump_delay: Duration,
    active: AtomicUsize,
    max_active: AtomicUsize,
    /// Simule l'arret brutal du processus au redemarrage des services.
    crash_on_restart: bool,
    /// Base simulee.
    db: Mutex<Db>,
    /// Sauvegardes de l'updater, abimees au premier redemarrage.
    backup_dir: PathBuf,
    damage: Option<Damage>,
    /// `systemctl stop` en echec.
    fail_stop: bool,
    /// Simule `ciphera-updater-recover.service`, tire par chaque demarrage
    /// des services (`RequiredBy=`) : `--recover` lance pendant le
    /// `systemctl restart`, avec ces chemins et sources.
    recover_on_restart: Option<(Paths, Sources)>,
    /// Services qui ne se stabilisent pas apres le redemarrage (raison).
    unsettled: Option<String>,
    /// `ciphera-cli post-install` en echec.
    fail_post_install: bool,
    /// Changements d'unites demandes a systemctl.
    unit_changes: Mutex<Vec<updater::UnitChange>>,
    /// Unites desactivees par l'administrateur (`systemctl is-enabled`).
    disabled: Vec<String>,
    /// Version majeure d'`apparmor_parser` (4 par `Mock::new`).
    apparmor_major: Option<u32>,
    /// Bornes demandees pour chaque telechargement.
    limits: Mutex<Vec<(String, Limit)>>,
    /// Fichier remplace par un dossier non vide au premier redemarrage :
    /// le retour arriere ne peut pas le remettre (retour incomplet).
    block_restore_of: Option<PathBuf>,
    /// `restore.sql` observe a chaque arret des services : evenement
    /// `restore.sql <octets>` s'il existe a ce moment.
    watch_restore_sql: Option<PathBuf>,
}

/// Base simulee : schema (numero de migration) et donnees.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Db {
    schema: u32,
    rows: Vec<String>,
}

impl Default for Db {
    fn default() -> Self {
        Db {
            schema: schema_of(v("v0.1.0")),
            rows: vec!["projet".into()],
        }
    }
}

/// Schema attendu par chaque version de `ciphera`.
fn schema_of(version: Version) -> u32 {
    match version.to_string().as_str() {
        "v0.0.9" => 1,
        "v0.1.0" => 2,
        "v0.2.0" => 3,
        other => panic!("version inconnue {other}"),
    }
}

impl Db {
    /// Meme fin qu'un vrai `pg_dump` 17.6+ (voir fixtures/pg_dump_17.sql).
    fn dump(&self) -> String {
        format!(
            "--\n-- PostgreSQL database dump\n--\n\n\\restrict cle\n\nschema={}\n{}\n\n--\n{TRAILER}\n--\n\n\\unrestrict cle\n\n",
            self.schema,
            self.rows.join("\n")
        )
    }

    fn parse(text: &str) -> Db {
        let mut lines = text
            .lines()
            .filter(|l| !l.starts_with("--") && !l.starts_with('\\') && !l.trim().is_empty());
        let schema = lines
            .next()
            .and_then(|l| l.strip_prefix("schema="))
            .unwrap()
            .parse()
            .unwrap();
        Db {
            schema,
            rows: lines.map(String::from).collect(),
        }
    }
}

/// Ligne de fin d'un `pg_dump` complet.
const TRAILER: &str = "-- PostgreSQL database dump complete";

/// Sortie reelle de `pg_dump` 17.12 (image postgres epinglee par
/// deploy/containers/postgres.digest), sur une base de test sans secret.
const REAL_PG_DUMP: &str = include_str!("fixtures/pg_dump_17.sql");

fn zstd_bytes(text: &str) -> Vec<u8> {
    let mut enc = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    enc.write_all(text.as_bytes()).unwrap();
    enc.finish().unwrap()
}

fn write_dump(path: &Path, db: &Db) {
    std::fs::write(path, zstd_bytes(&db.dump())).unwrap();
}

/// Sauvegarde `<ts>.sql.zst` et sa fiche `<ts>.meta.json` (version et
/// derniere migration appliquee), comme l'updater les ecrit.
fn write_backup(dir: &Path, ts: &str, db: &Db, version: &str, migration: u32) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let dump = dir.join(format!("{ts}.sql.zst"));
    write_dump(&dump, db);
    std::fs::write(
        dir.join(format!("{ts}.meta.json")),
        format!("{{\"version\":\"{version}\",\"migration\":{migration}}}"),
    )
    .unwrap();
    dump
}

fn read_dump(path: &Path) -> Db {
    let mut text = String::new();
    zstd::stream::read::Decoder::new(std::fs::File::open(path).unwrap())
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    Db::parse(&text)
}

/// Degat simule sur la sauvegarde prise par l'updater, pendant que la
/// nouvelle version tourne.
#[derive(Clone, Copy, Debug)]
enum Damage {
    /// Trame zstd coupee.
    Truncated,
    /// Pas du zstd.
    Garbage,
    /// zstd valide, mais `pg_dump` interrompu (pas de ligne de fin).
    NoTrailer,
}

impl Mock {
    fn new(env: &Env, healthy: bool) -> Self {
        Mock {
            files: Mutex::new(env.files.clone()),
            healthy,
            bin_dir: env.paths.bin_dir.clone(),
            backup_dir: env.paths.backup_dir.clone(),
            apparmor_major: Some(4),
            ..Mock::default()
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn db(&self) -> Db {
        self.db.lock().unwrap().clone()
    }

    fn position(&self, event: &str) -> Option<usize> {
        self.events().iter().rposition(|e| e == event)
    }

    fn count(&self, prefix: &str) -> usize {
        self.events()
            .iter()
            .filter(|e| e.starts_with(prefix))
            .count()
    }
}

impl System for Mock {
    /// Sert le fichier tel quel, sans appliquer la borne : l'updater
    /// reverifie la taille du fichier obtenu.
    fn download(&self, url: &str, dest: &Path, limit: Limit) -> std::io::Result<()> {
        self.events.lock().unwrap().push(format!("download {url}"));
        self.limits.lock().unwrap().push((url.to_string(), limit));
        let data = self.files.lock().unwrap().get(url).cloned();
        match data {
            Some(d) => std::fs::write(dest, d),
            None => Err(std::io::Error::other("404")),
        }
    }

    fn pg_dump(&self, dest: &Path) -> std::io::Result<()> {
        let n = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(n, Ordering::SeqCst);
        self.events.lock().unwrap().push("pg_dump".into());
        *self.bin_at_dump.lock().unwrap() = std::fs::read(self.bin_dir.join("ciphera")).ok();
        std::thread::sleep(self.dump_delay);
        write_dump(dest, &self.db());
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    }

    fn latest_migration(&self) -> std::io::Result<u32> {
        Ok(self.db().schema)
    }

    /// Recoit la sauvegarde deja decompressee et verifiee (SQL).
    fn restore_db(&self, sql: &Path) -> std::io::Result<()> {
        self.events.lock().unwrap().push("restore_db".into());
        let text = std::fs::read_to_string(sql)?;
        assert!(updater::dump_complete(&text), "SQL non verifie : {text}");
        *self.db.lock().unwrap() = Db::parse(&text);
        Ok(())
    }

    fn stop_services(&self) -> std::io::Result<()> {
        self.events.lock().unwrap().push("stop".into());
        if let Some(sql) = &self.watch_restore_sql
            && let Ok(text) = std::fs::read_to_string(sql)
        {
            assert!(updater::dump_complete(&text), "restore.sql non verifie");
            self.events
                .lock()
                .unwrap()
                .push(format!("restore.sql {}", text.len()));
        }
        if self.fail_stop {
            return Err(std::io::Error::other("systemctl stop en echec"));
        }
        Ok(())
    }

    /// Demarre le `ciphera` installe : il migre la base vers son schema, ou
    /// refuse de demarrer sur un schema plus recent que le sien.
    fn restart_services(&self) -> std::io::Result<()> {
        self.events.lock().unwrap().push("restart".into());
        if let Some((paths, sources)) = self.recover_on_restart.clone() {
            // systemctl restart attend la fin de l'unite de reprise : elle
            // doit rendre la main aussitot (verrou tenu par cet updater).
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let sys = Mock {
                    bin_dir: paths.bin_dir.clone(),
                    ..Mock::default()
                };
                let r = updater::run(Mode::Recover, &sys, &paths, &sources);
                let _ = tx.send((format!("{r:?}"), sys.events()));
            });
            let (outcome, events) = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("la reprise attend le verrou de l'updater en cours (interblocage)");
            assert!(events.is_empty(), "la reprise a agi : {events:?}");
            self.events
                .lock()
                .unwrap()
                .push(format!("recover {outcome}"));
        }
        let bin = String::from_utf8(std::fs::read(self.bin_dir.join("ciphera"))?).unwrap();
        let version: Version = bin.rsplit(' ').next().unwrap().parse().unwrap();
        let wanted = schema_of(version);
        let started = {
            let mut db = self.db.lock().unwrap();
            if db.schema > wanted {
                false
            } else {
                if db.schema < wanted {
                    db.schema = wanted;
                    db.rows.push(format!("migration {version}"));
                }
                true
            }
        };
        let event = if started {
            format!("started {version}")
        } else {
            format!("start_failed {version}")
        };
        self.events.lock().unwrap().push(event);
        if let Some(damage) = self.damage
            && self.count("restart") == 1
        {
            for e in std::fs::read_dir(&self.backup_dir)? {
                let path = e?.path();
                if !path.to_string_lossy().ends_with(".sql.zst") {
                    continue;
                }
                let good = std::fs::read(&path)?;
                let bad = match damage {
                    Damage::Truncated => good[..good.len() / 2].to_vec(),
                    Damage::Garbage => b"pas du zstd".to_vec(),
                    Damage::NoTrailer => zstd_bytes("schema=1\nvide\n"),
                };
                std::fs::write(&path, bad)?;
            }
        }
        if let Some(path) = &self.block_restore_of
            && self.count("restart") == 1
        {
            std::fs::remove_file(path)?;
            std::fs::create_dir_all(path.join("bloque"))?;
        }
        if self.crash_on_restart && self.count("restart") == 1 {
            panic!("arret brutal simule");
        }
        Ok(())
    }

    fn wait_healthy(&self, timeout: Duration) -> bool {
        assert_eq!(timeout, Duration::from_secs(60));
        self.events.lock().unwrap().push("health".into());
        self.healthy
    }

    fn running_services(&self) -> std::io::Result<Vec<(String, String)>> {
        Ok(vec![
            ("ciphera.service".into(), "avant".into()),
            ("ciphera-proxy.service".into(), "avant".into()),
        ])
    }

    fn services_settled(
        &self,
        before: &[(String, String)],
        window: Duration,
    ) -> Result<(), String> {
        assert_eq!(window, updater::SETTLE_WINDOW);
        assert_eq!(before.len(), 2);
        self.events.lock().unwrap().push("settled".into());
        match &self.unsettled {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }

    fn enabled_units(&self, units: &[String]) -> std::io::Result<Vec<String>> {
        Ok(units
            .iter()
            .filter(|u| !self.disabled.contains(u))
            .cloned()
            .collect())
    }

    fn apply_units(&self, change: &updater::UnitChange) -> std::io::Result<()> {
        self.events.lock().unwrap().push("units".into());
        self.unit_changes.lock().unwrap().push(change.clone());
        Ok(())
    }

    fn post_install(&self, from: Version, to: Version) -> std::io::Result<()> {
        // Lance sur le nouveau binaire, deja en place.
        let bin = std::fs::read(self.bin_dir.join("ciphera-cli"))?;
        assert_eq!(bin, binary("ciphera-cli", to));
        self.events
            .lock()
            .unwrap()
            .push(format!("post_install {from} {to}"));
        if self.fail_post_install {
            return Err(std::io::Error::other("code 1"));
        }
        Ok(())
    }

    fn apparmor_parser_major(&self) -> Option<u32> {
        self.apparmor_major
    }

    fn now(&self) -> u64 {
        NOW
    }
}

// --- machine installee ------------------------------------------------------

struct Env {
    _tmp: TempDir,
    paths: Paths,
    sources: Sources,
    files: HashMap<String, Vec<u8>>,
    /// Cle des demandes (`<config_dir>/update-hmac.key`).
    key: [u8; 32],
    /// Cle de signature des releases publiees.
    signer: Keys,
}

/// Ciphera v0.1.0 installe ; releases v0.0.9, v0.1.0 et v0.2.0 publiees.
fn env() -> Env {
    let tmp = TempDir::new();
    let paths = Paths::under(&tmp.0);
    for d in [
        &paths.bin_dir,
        &paths.tools_dir,
        &paths.share_dir,
        &paths.unit_dir,
        &paths.config_dir,
        paths.request.parent().unwrap(),
        paths.tmpfiles.parent().unwrap(),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    for b in updater::BINARIES {
        std::fs::write(paths.bin_dir.join(b), binary(b, v("v0.1.0"))).unwrap();
    }
    std::fs::write(paths.tools_dir.join("railpack"), b"railpack v0.1.0").unwrap();
    std::fs::write(paths.share_dir.join("VERSION"), "v0.1.0\n").unwrap();
    // Outils de l'hote appeles par les unites (nftables, paquet du systeme).
    std::fs::create_dir_all(tmp.0.join("usr/sbin")).unwrap();
    write_tool(&tmp.0.join("usr/sbin/nft"));
    std::fs::create_dir_all(tmp.0.join("var/run")).unwrap();
    std::fs::write(tmp.0.join("var/run/docker.sock"), b"").unwrap();
    // Cle des demandes, comme `ciphera init` la pose (0600 sans le groupe
    // ciphera, introuvable ici).
    let key: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    write_key(&paths, &key);

    let good = keys();
    let mut files = HashMap::new();
    for version in ["v0.0.9", "v0.1.0", "v0.2.0"] {
        publish(&mut files, &good, v(version));
    }
    let sources = Sources {
        release_base_url: BASE.into(),
        tools_base_url: TOOLS.into(),
        pubkeys: vec![good.pk.to_base64()],
        target: TARGET.into(),
    };
    Env {
        _tmp: tmp,
        paths,
        sources,
        files,
        key,
        signer: good,
    }
}

/// Ecrit la cle des demandes de l'instance (0600).
fn write_key(paths: &Paths, key: &[u8]) {
    let path = paths.config_dir.join(KEY_FILE);
    std::fs::write(&path, key).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

impl Env {
    fn bin(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.paths.bin_dir.join(name)).unwrap()
    }

    fn installed(&self) -> String {
        std::fs::read_to_string(self.paths.share_dir.join("VERSION"))
            .unwrap()
            .trim()
            .to_string()
    }

    fn write_request(&self, req: &UpdateRequest) {
        std::fs::write(&self.paths.request, req.to_json()).unwrap();
    }

    /// Sauvegardes `*.sql.zst` (sans leurs fiches).
    fn backups(&self) -> Vec<PathBuf> {
        match std::fs::read_dir(&self.paths.backup_dir) {
            Ok(rd) => rd
                .map(|e| e.unwrap().path())
                .filter(|p| p.to_string_lossy().ends_with(".sql.zst"))
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    fn journal(&self) -> PathBuf {
        self.paths.state_dir.join("journal.json")
    }

    fn assert_untouched(&self) {
        for b in updater::BINARIES {
            assert_eq!(self.bin(b), binary(b, v("v0.1.0")), "{b} modifie");
        }
        assert_eq!(self.installed(), "v0.1.0");
        assert!(!self.paths.unit_dir.join("ciphera.service").exists());
        assert!(!self.paths.tools_dir.join("buildctl").exists());
    }
}

fn manual(version: &str) -> Mode {
    Mode::Manual {
        version: v(version),
        allow_downgrade: false,
        restore_db: None,
    }
}

fn downgrade(version: &str, restore_db: Option<PathBuf>) -> Mode {
    Mode::Manual {
        version: v(version),
        allow_downgrade: true,
        restore_db,
    }
}

// --- tests ------------------------------------------------------------------

#[test]
fn request_with_bad_hmac_ignored() {
    let env = env();
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &[9u8; 32]));
    let sys = Mock::new(&env, true);

    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Ignored(_))), "{r:?}");
    assert_eq!(sys.count("download"), 0);
    assert!(sys.events().is_empty(), "{:?}", sys.events());
    assert!(!env.paths.request.exists(), "demande consommee");
    env.assert_untouched();

    // Perimee (signee avec la bonne cle) : ignoree aussi.
    let key = env.key;
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW - 3600, &key));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Ignored(_))), "{r:?}");
    assert!(sys.events().is_empty());

    // La meme demande, bien signee et fraiche, passe.
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &key));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert_eq!(
        r.unwrap(),
        Outcome::Updated {
            from: v("v0.1.0"),
            to: v("v0.2.0")
        }
    );
}

#[test]
fn request_for_older_or_same_version_refused() {
    let env = env();
    let key = env.key;
    let sys = Mock::new(&env, true);
    for version in ["v0.1.0", "v0.0.9"] {
        env.write_request(&UpdateRequest::sign(v(version), NOW, &key));
        let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
        assert!(
            matches!(r, Err(UpdateError::Refused(_))),
            "{version} : {r:?}"
        );
        assert!(!env.paths.request.exists());

        // Meme refus en ligne de commande sans --allow-downgrade.
        let r = updater::run(manual(version), &sys, &env.paths, &env.sources);
        assert!(
            matches!(r, Err(UpdateError::Refused(_))),
            "{version} : {r:?}"
        );
    }
    assert!(sys.events().is_empty(), "{:?}", sys.events());
    env.assert_untouched();
}

#[test]
fn downgrade_requires_a_database_dump() {
    let env = env();
    let sys = Mock::new(&env, true);
    let refused = |mode: Mode, why: &str| {
        let r = updater::run(mode, &sys, &env.paths, &env.sources);
        match r {
            Err(UpdateError::Refused(m)) => assert!(m.contains(why), "{why} : {m}"),
            other => panic!("{why} : {other:?}"),
        }
        assert_eq!(sys.count("pg_dump") + sys.count("stop"), 0, "{why}");
        assert_eq!(sys.count("restore_db") + sys.count("restart"), 0, "{why}");
        env.assert_untouched();
    };

    // --allow-downgrade sans --restore-db : refuse, rien n'est fait.
    refused(downgrade("v0.0.9", None), "--restore-db");
    // Sauvegarde introuvable.
    let dir = &env.paths.backup_dir;
    refused(
        downgrade("v0.0.9", Some(dir.join("20200101T000000Z.sql.zst"))),
        "introuvable",
    );

    let old = Db {
        schema: schema_of(v("v0.0.9")),
        rows: vec!["ancien".into()],
    };
    // Hors de /var/backups/ciphera (dossier root 0700) : refusee.
    let outside = write_backup(&env._tmp.0, "20200101T000000Z", &old, "v0.0.9", 1);
    refused(downgrade("v0.0.9", Some(outside)), "/var/backups/ciphera");
    // Sans fiche : version et migrations inconnues.
    let dump = write_backup(dir, "20200101T000000Z", &old, "v0.0.9", 1);
    let meta = dir.join("20200101T000000Z.meta.json");
    std::fs::remove_file(&meta).unwrap();
    refused(downgrade("v0.0.9", Some(dump.clone())), "fiche");
    // Prise sous une autre version : un binaire plus ancien la migrerait
    // en silence (donnees perdues sans le dire).
    write_backup(dir, "20200101T000000Z", &old, "v0.1.0", 2);
    refused(downgrade("v0.0.9", Some(dump.clone())), "v0.1.0");
    // Bonne version, mais migrations differentes de celles de la release.
    write_backup(dir, "20200101T000000Z", &old, "v0.0.9", 2);
    refused(downgrade("v0.0.9", Some(dump.clone())), "migration");
    // Sauvegarde abimee : coupee, ou pg_dump interrompu.
    write_backup(dir, "20200101T000000Z", &old, "v0.0.9", 1);
    let good = std::fs::read(&dump).unwrap();
    std::fs::write(&dump, &good[..good.len() / 2]).unwrap();
    refused(downgrade("v0.0.9", Some(dump.clone())), "sauvegarde");
    std::fs::write(&dump, zstd_bytes("schema=1\nancien\n")).unwrap();
    refused(downgrade("v0.0.9", Some(dump.clone())), "sauvegarde");
    // Lien symbolique vers une sauvegarde valide : refuse (O_NOFOLLOW).
    #[cfg(unix)]
    {
        let real = write_backup(dir, "20190101T000000Z", &old, "v0.0.9", 1);
        let link = dir.join("20200102T000000Z.sql.zst");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        std::fs::copy(
            dir.join("20190101T000000Z.meta.json"),
            dir.join("20200102T000000Z.meta.json"),
        )
        .unwrap();
        refused(downgrade("v0.0.9", Some(link.clone())), "sauvegarde");
        std::fs::remove_file(&link).unwrap();
    }

    // Avec la sauvegarde prise sous v0.0.9 : base remise a ce schema avant
    // le demarrage de l'ancien binaire. La plus ancienne de six
    // sauvegardes : jamais retiree pendant qu'elle sert.
    write_backup(dir, "20200101T000000Z", &old, "v0.0.9", 1);
    for day in 1..=5 {
        let ts = format!("202601{day:02}T000000Z");
        write_backup(dir, &ts, &Db::default(), "v0.1.0", 2);
    }
    let r = updater::run(
        downgrade("v0.0.9", Some(dump.clone())),
        &sys,
        &env.paths,
        &env.sources,
    );
    assert_eq!(
        r.unwrap(),
        Outcome::Updated {
            from: v("v0.1.0"),
            to: v("v0.0.9")
        }
    );
    assert_eq!(env.installed(), "v0.0.9");
    assert_eq!(sys.db(), old);
    assert!(sys.position("restore_db") < sys.position("restart"));
    assert!(sys.position("stop") < sys.position("restore_db"));
    assert_eq!(sys.count("started v0.0.9"), 1, "{:?}", sys.events());
    assert!(dump.exists() && meta.exists(), "sauvegarde utilisee gardee");
}

/// Sauvegarde abimee pendant la mise a jour : jamais restauree (aucun
/// DROP), rien n'est touche, le journal reste pour une reprise manuelle.
#[test]
fn damaged_backup_is_never_restored() {
    for damage in [Damage::Truncated, Damage::Garbage, Damage::NoTrailer] {
        let env = env();
        let sys = Mock {
            damage: Some(damage),
            ..Mock::new(&env, false)
        };
        let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
        match &r {
            Err(UpdateError::Io(m)) => assert!(m.contains("sauvegarde"), "{damage:?} : {m}"),
            other => panic!("{damage:?} : {other:?}"),
        }
        assert_eq!(sys.count("restore_db"), 0, "{damage:?}");
        assert_eq!(sys.count("stop"), 0, "{damage:?}");
        assert_eq!(sys.db().schema, schema_of(v("v0.2.0")), "{damage:?}");
        assert!(env.journal().exists(), "{damage:?}");
    }
}

/// Arret du plan de controle en echec : la base n'est pas remplacee sous
/// un plan de controle en marche ; le journal reste.
#[test]
fn database_is_not_restored_if_the_services_did_not_stop() {
    let env = env();
    let sys = Mock {
        fail_stop: true,
        ..Mock::new(&env, false)
    };
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Io(_))), "{r:?}");
    assert_eq!(sys.count("stop"), 1);
    assert_eq!(sys.count("restore_db"), 0);
    assert_eq!(sys.db().schema, schema_of(v("v0.2.0")));
    assert!(env.journal().exists());
}

/// Journal illisible : mis de cote (`journal.json.corrupt`) avec une erreur
/// visible, sans bloquer les mises a jour suivantes.
#[test]
fn corrupt_journal_is_quarantined() {
    let env = env();
    std::fs::create_dir_all(&env.paths.state_dir).unwrap();
    std::fs::write(env.journal(), b"{ pas du json").unwrap();
    let sys = Mock::new(&env, true);
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    match &r {
        Err(UpdateError::Io(m)) => assert!(m.contains("journal.json.corrupt"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(!env.journal().exists());
    assert!(env.paths.state_dir.join("journal.json.corrupt").exists());
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn concurrent_updates_serialized_by_flock() {
    let env = Arc::new(env());
    let sys = Arc::new(Mock {
        dump_delay: Duration::from_millis(400),
        ..Mock::new(&env, true)
    });
    let runs: Vec<_> = (0..2)
        .map(|_| {
            let (env, sys) = (env.clone(), sys.clone());
            std::thread::spawn(move || {
                updater::run(manual("v0.2.0"), &*sys, &env.paths, &env.sources)
            })
        })
        .collect();
    let results: Vec<_> = runs.into_iter().map(|h| h.join().unwrap()).collect();

    assert_eq!(
        sys.max_active.load(Ordering::SeqCst),
        1,
        "jamais en parallele"
    );
    assert_eq!(sys.count("pg_dump"), 1);
    let updated = results.iter().filter(|r| r.is_ok()).count();
    let refused = results
        .iter()
        .filter(|r| matches!(r, Err(UpdateError::Refused(_))))
        .count();
    assert_eq!((updated, refused), (1, 1), "{results:?}");
    assert_eq!(env.installed(), "v0.2.0");
}

#[test]
fn bad_signature_aborts_before_any_change() {
    let env = env();
    let sums = format!("{BASE}/releases/v0.2.0/SHA256SUMS");

    // SHA256SUMS signe par une autre cle.
    let mut sys = Mock::new(&env, true);
    let other = keys();
    let data = env.files[&sums].clone();
    sys.files
        .get_mut()
        .unwrap()
        .insert(format!("{sums}.minisig"), sign(&other, &data));
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Verification(_))), "{r:?}");
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    assert!(env.backups().is_empty());
    env.assert_untouched();

    // Archive alteree d'un octet (signature et SHA256SUMS intacts).
    let mut sys = Mock::new(&env, true);
    let archive = format!("{BASE}/releases/v0.2.0/ciphera-v0.2.0-{TARGET}.tar.gz");
    sys.files.get_mut().unwrap().get_mut(&archive).unwrap()[40] ^= 0xff;
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Verification(_))), "{r:?}");
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);

    // Outil tiers altere : son SHA-256 (manifeste signe) ne correspond plus.
    let mut sys = Mock::new(&env, true);
    let tool = sys
        .files
        .get_mut()
        .unwrap()
        .keys()
        .find(|k| k.starts_with(TOOLS) && k.contains("railpack") && k.contains("/v0.2.0/"))
        .cloned()
        .unwrap();
    sys.files.get_mut().unwrap().get_mut(&tool).unwrap()[30] ^= 0xff;
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Verification(_))), "{r:?}");
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    assert!(env.backups().is_empty());
    env.assert_untouched();
}

/// Rotation de cle : une release signee par n'importe quelle cle de
/// l'ensemble de confiance est acceptee ; une cle hors de l'ensemble, non.
#[test]
fn any_trusted_key_verifies_a_release() {
    let next = keys().pk.to_base64();
    let mut env = env();
    let signing = env.sources.pubkeys[0].clone();
    env.sources.pubkeys = vec![next.clone(), signing];
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");

    let mut env = self::env();
    env.sources.pubkeys = vec![next];
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Verification(_))), "{r:?}");
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    env.assert_untouched();

    let mut env = self::env();
    env.sources.pubkeys = vec![];
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Verification(_))), "{r:?}");
    env.assert_untouched();
}

#[test]
fn missing_host_tool_of_a_new_unit_aborts_before_any_change() {
    let env = env();
    let nft = env.paths.root.join("usr/sbin/nft");
    std::fs::remove_file(&nft).unwrap();
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    match &r {
        Err(UpdateError::Refused(m)) => {
            assert!(m.contains("/usr/sbin/nft") && m.contains("nftables"), "{m}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    assert!(env.backups().is_empty());
    assert!(!env.journal().exists());
    env.assert_untouched();

    // Docker arrete pendant la mise a jour (ConditionPathExists fausse a cet
    // instant) : l'unite reste activee et demarrera avec Docker, l'outil
    // est toujours exige.
    std::fs::remove_file(env.paths.root.join("var/run/docker.sock")).unwrap();
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Refused(_))), "{r:?}");
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    env.assert_untouched();

    // Outil installe : la mise a jour passe.
    write_tool(&nft);
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");
}

#[test]
fn failed_health_rolls_back() {
    let env = env();
    let sys = Mock::new(&env, false);
    let before = sys.db();
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    match &r {
        Err(UpdateError::RolledBack(m)) => {
            assert!(m.contains("60 s") && m.contains("perdues"), "{m}");
        }
        other => panic!("{other:?}"),
    }
    // La nouvelle version a migre la base, puis la sauvegarde d'avant la
    // mise a jour est restauree (services arretes) : l'ancien binaire
    // redemarre sur son schema.
    assert_eq!(sys.count("started v0.2.0"), 1, "{:?}", sys.events());
    assert_eq!(sys.db(), before);
    let stop = sys.position("stop").unwrap();
    let restore = sys.position("restore_db").unwrap();
    assert!(stop < restore && restore < sys.position("restart").unwrap());
    assert_eq!(sys.events().last().map(String::as_str), Some("health"));
    assert_eq!(sys.count("started v0.1.0"), 1, "{:?}", sys.events());
    assert_eq!(sys.count("start_failed"), 0, "{:?}", sys.events());
    // Binaires, outils, unites et version d'avant ; services relances deux fois.
    env.assert_untouched();
    assert_eq!(
        std::fs::read(env.paths.tools_dir.join("railpack")).unwrap(),
        b"railpack v0.1.0"
    );
    assert_eq!(sys.count("restart"), 2);
    // La sauvegarde de la base est gardee.
    assert_eq!(env.backups().len(), 1);
    // Aucun fichier temporaire laisse a cote des binaires.
    let stray: Vec<_> = std::fs::read_dir(&env.paths.bin_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
}

/// `ciphera-updater-recover.service` est tire par chaque demarrage des
/// services, y compris le `systemctl restart` de l'updater en cours (qui
/// tient le verrou) : la reprise rend la main aussitot, sans rien toucher,
/// et ne bloque ni la mise a jour ni son retour arriere.
#[test]
fn restart_during_update_does_not_wait_for_recovery() {
    let env = env();
    let sys = Mock {
        recover_on_restart: Some((env.paths.clone(), env.sources.clone())),
        ..Mock::new(&env, true)
    };
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert_eq!(
        r.unwrap(),
        Outcome::Updated {
            from: v("v0.1.0"),
            to: v("v0.2.0")
        }
    );
    assert_eq!(sys.count("recover Ok(Busy)"), 1, "{:?}", sys.events());

    // Healthcheck en echec : le redemarrage du retour arriere ne bloque pas
    // non plus.
    let env2 = self::env();
    let sys = Mock {
        recover_on_restart: Some((env2.paths.clone(), env2.sources.clone())),
        ..Mock::new(&env2, false)
    };
    let r = updater::run(manual("v0.2.0"), &sys, &env2.paths, &env2.sources);
    assert!(matches!(r, Err(UpdateError::RolledBack(_))), "{r:?}");
    assert_eq!(sys.count("recover Ok(Busy)"), 2, "{:?}", sys.events());
    env2.assert_untouched();

    // Sans updater actif (verrou libre), la reprise fait son travail.
    let r = updater::run(Mode::Recover, &sys, &env2.paths, &env2.sources);
    assert_eq!(r.unwrap(), Outcome::NoRequest);
}

/// Sortie reelle de pg_dump 17.12 (lignes `--`, vides et `\unrestrict`
/// apres la ligne de fin) acceptee ; coupee, refusee.
#[test]
fn real_pg_dump_passes_the_completion_check() {
    let env = env();
    let dir = &env.paths.backup_dir;
    std::fs::create_dir_all(dir).unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    assert!(REAL_PG_DUMP.contains("\n\\unrestrict "), "fixture 17.6+");
    assert!(!REAL_PG_DUMP.trim_end().ends_with(TRAILER));

    let full = write("full.sql.zst", &zstd_bytes(REAL_PG_DUMP));
    updater::verify_dump(&full).unwrap();
    // Fin d'un pg_dump anterieur a 17.6 (sans \unrestrict).
    let old: String = REAL_PG_DUMP
        .lines()
        .filter(|l| !l.starts_with("\\unrestrict "))
        .map(|l| format!("{l}\n"))
        .collect();
    updater::verify_dump(&write("old.sql.zst", &zstd_bytes(&old))).unwrap();

    // Texte coupe avant la ligne de fin, ou au milieu.
    let cut = REAL_PG_DUMP.find(TRAILER).unwrap();
    for text in [
        &REAL_PG_DUMP[..cut],
        &REAL_PG_DUMP[..REAL_PG_DUMP.len() / 2],
    ] {
        let p = write("cut.sql.zst", &zstd_bytes(text));
        assert!(updater::verify_dump(&p).is_err(), "coupe accepte : {text}");
    }
    // \unrestrict seul, sans la ligne de fin.
    let forged = format!("{}\n\\unrestrict cle\n", &REAL_PG_DUMP[..cut]);
    let p = write("forged.sql.zst", &zstd_bytes(&forged));
    assert!(updater::verify_dump(&p).is_err());
    // Trame zstd coupee.
    let bytes = zstd_bytes(REAL_PG_DUMP);
    let p = write("trunc.sql.zst", &bytes[..bytes.len() - 8]);
    assert!(updater::verify_dump(&p).is_err());
}

#[test]
fn pg_dump_created_before_replace() {
    let env = env();
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert_eq!(
        r.unwrap(),
        Outcome::Updated {
            from: v("v0.1.0"),
            to: v("v0.2.0")
        }
    );
    // Au moment du pg_dump, les anciens binaires etaient encore en place.
    assert_eq!(
        sys.bin_at_dump.lock().unwrap().as_deref(),
        Some(binary("ciphera", v("v0.1.0")).as_slice())
    );
    let events = sys.events();
    let dump = events.iter().position(|e| e == "pg_dump").unwrap();
    let restart = events.iter().position(|e| e == "restart").unwrap();
    assert!(dump < restart, "{events:?}");
    assert!(
        events[..dump].iter().all(|e| e.starts_with("download")),
        "tout est telecharge et verifie avant la sauvegarde : {events:?}"
    );

    // /var/backups/ciphera/<ts>.sql.zst, lisible par zstd.
    let backups = env.backups();
    assert_eq!(backups.len(), 1, "{backups:?}");
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.ends_with(".sql.zst"), "{name}");
    assert_eq!(name, "20270115T080000Z.sql.zst");
    assert_eq!(read_dump(&backups[0]), Db::default());
    assert_eq!(sys.count("restore_db"), 0);
    // Fiche de la sauvegarde : version et derniere migration appliquee.
    let meta: serde_json::Value = serde_json::from_slice(
        &std::fs::read(env.paths.backup_dir.join("20270115T080000Z.meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        meta,
        serde_json::json!({ "version": "v0.1.0", "migration": schema_of(v("v0.1.0")) })
    );

    // Nouvelle version en place : binaires, outils du manifeste, unites.
    for b in updater::BINARIES {
        assert_eq!(env.bin(b), binary(b, v("v0.2.0")));
    }
    assert_eq!(env.installed(), "v0.2.0");
    for (tool, expected) in [
        ("railpack", "railpack v0.2.0"),
        ("buildctl", "buildctl v0.2.0"),
        ("mise-2026.10.14", "mise v0.2.0"),
    ] {
        assert_eq!(
            std::fs::read_to_string(env.paths.tools_dir.join(tool)).unwrap(),
            expected
        );
    }
    assert_eq!(
        std::fs::read_to_string(env.paths.unit_dir.join("ciphera.service")).unwrap(),
        "# ciphera.service v0.2.0"
    );
    assert_eq!(
        std::fs::read_to_string(env.paths.share_dir.join("containers/postgres.digest")).unwrap(),
        "postgres@v0.2.0"
    );
    assert!(!env.paths.state_dir.join("journal.json").exists());
}

#[test]
fn interrupted_update_is_rolled_back_on_next_run() {
    let env = env();
    let sys = Mock {
        crash_on_restart: true,
        ..Mock::new(&env, true)
    };
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources)
    }));
    assert!(crashed.is_err());
    assert_eq!(env.bin("ciphera"), binary("ciphera", v("v0.2.0")));
    assert!(env.paths.state_dir.join("journal.json").exists());

    assert_eq!(sys.db().schema, schema_of(v("v0.2.0")), "base migree");

    // Execution suivante (sans demande) : retour a l'etat d'avant, base
    // comprise (la nouvelle version a tourne).
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert_eq!(r.unwrap(), Outcome::Recovered);
    env.assert_untouched();
    assert!(!env.paths.state_dir.join("journal.json").exists());
    assert_eq!(sys.count("restart"), 2);
    assert_eq!(sys.db(), Db::default());
    assert_eq!(sys.count("started v0.1.0"), 1, "{:?}", sys.events());
}

/// Arret brutal pendant les renommages : au demarrage suivant,
/// `ciphera-updater-recover.service` (mode `Recover`, avant les services)
/// remet les fichiers d'avant et la base (le drapeau `started` est ecrit,
/// et rendu durable, avant le premier renommage), sans toucher aux services.
#[test]
fn boot_recovery_replays_a_journal_interrupted_mid_replace() {
    let env = env();
    let sys = Mock {
        crash_on_restart: true,
        ..Mock::new(&env, true)
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources)
    }));
    let journal = env.journal();
    let j: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    assert_eq!(j["started"], true, "drapeau ecrit avant les renommages");
    // Etat d'un arret au milieu des renommages : la moitie des binaires
    // encore anciens, leurs remplacants prepares a cote.
    for b in &updater::BINARIES[2..] {
        let staged = env.paths.bin_dir.join(format!(".{b}.ciphera-new"));
        std::fs::write(staged, binary(b, v("v0.2.0"))).unwrap();
        std::fs::write(env.paths.bin_dir.join(b), binary(b, v("v0.1.0"))).unwrap();
    }
    std::fs::write(env.paths.share_dir.join("VERSION"), "v0.1.0\n").unwrap();
    let before = sys.events().len();

    let r = updater::run(Mode::Recover, &sys, &env.paths, &env.sources);
    assert_eq!(r.unwrap(), Outcome::Recovered);
    env.assert_untouched();
    assert!(!journal.exists());
    let stray: Vec<_> = std::fs::read_dir(&env.paths.bin_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(stray.is_empty(), "{stray:?}");
    // Base restauree ; ni arret ni redemarrage (pas encore demarres au boot).
    assert_eq!(&sys.events()[before..], &["restore_db".to_string()]);
    assert_eq!(sys.db(), Db::default());

    // Sans journal : rien a faire.
    let r = updater::run(Mode::Recover, &sys, &env.paths, &env.sources);
    assert_eq!(r.unwrap(), Outcome::NoRequest);
    assert_eq!(sys.events().len(), before + 1);
}

/// Arret brutal avant le drapeau `started` (fichiers seulement prepares a
/// cote) : la reprise retire les fichiers prepares, sans toucher a la base.
#[test]
fn boot_recovery_before_started_leaves_the_database() {
    let env = env();
    let sys = Mock {
        crash_on_restart: true,
        ..Mock::new(&env, true)
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources)
    }));
    let journal = env.journal();
    let mut j: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    j["started"] = serde_json::Value::Bool(false);
    std::fs::write(&journal, serde_json::to_vec(&j).unwrap()).unwrap();
    for b in updater::BINARIES {
        let staged = env.paths.bin_dir.join(format!(".{b}.ciphera-new"));
        std::fs::write(staged, binary(b, v("v0.2.0"))).unwrap();
        std::fs::write(env.paths.bin_dir.join(b), binary(b, v("v0.1.0"))).unwrap();
    }
    std::fs::write(env.paths.share_dir.join("VERSION"), "v0.1.0\n").unwrap();
    let migrated = sys.db();
    let before = sys.events().len();
    let r = updater::run(Mode::Recover, &sys, &env.paths, &env.sources);
    assert_eq!(r.unwrap(), Outcome::Recovered);
    env.assert_untouched();
    assert_eq!(&sys.events()[before..], &[] as &[String]);
    assert_eq!(sys.db(), migrated);
}

/// Au demarrage, une nouvelle version qui avait deja tourne : base
/// restauree aussi, services laisses a systemd (ordre Before=).
#[test]
fn boot_recovery_restores_the_database_when_the_new_version_ran() {
    let env = env();
    let sys = Mock {
        crash_on_restart: true,
        ..Mock::new(&env, true)
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources)
    }));
    let before = sys.events().len();
    let r = updater::run(Mode::Recover, &sys, &env.paths, &env.sources);
    assert_eq!(r.unwrap(), Outcome::Recovered);
    env.assert_untouched();
    assert_eq!(sys.db(), Db::default());
    assert_eq!(&sys.events()[before..], &["restore_db".to_string()]);
}

#[test]
fn only_the_last_five_backups_are_kept() {
    let env = env();
    std::fs::create_dir_all(&env.paths.backup_dir).unwrap();
    for day in 1..=6 {
        let ts = format!("202601{day:02}T000000Z");
        write_backup(&env.paths.backup_dir, &ts, &Db::default(), "v0.1.0", 2);
    }
    std::fs::write(env.paths.backup_dir.join("notes.txt"), "a garder").unwrap();
    let sys = Mock::new(&env, true);
    updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&env.paths.backup_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut want: Vec<String> = ["03", "04", "05", "06"]
        .iter()
        .flat_map(|d| {
            [
                format!("202601{d}T000000Z.meta.json"),
                format!("202601{d}T000000Z.sql.zst"),
            ]
        })
        .collect();
    want.extend([
        "20270115T080000Z.meta.json".to_string(),
        "20270115T080000Z.sql.zst".to_string(),
        "notes.txt".to_string(),
    ]);
    assert_eq!(names, want);
}

/// Une FIFO ou un lien a la place de la demande ne bloque pas l'updater et
/// n'est pas suivi.
#[cfg(unix)]
#[test]
fn request_must_be_a_regular_file() {
    let env = env();
    let sys = Arc::new(Mock::new(&env, true));
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &env.paths.request,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (paths, sources, worker) = (env.paths.clone(), env.sources.clone(), sys.clone());
    std::thread::spawn(move || {
        let _ = tx.send(updater::run(Mode::Request, &*worker, &paths, &sources));
    });
    let r = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("FIFO : l'updater ne doit pas bloquer");
    assert!(matches!(r, Err(UpdateError::Ignored(_))), "{r:?}");
    assert!(!env.paths.request.exists());

    let key = env.key;
    let elsewhere = env._tmp.0.join("ailleurs.json");
    let req = UpdateRequest::sign(v("v0.2.0"), NOW, &key);
    std::fs::write(&elsewhere, req.to_json()).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &env.paths.request).unwrap();
    let r = updater::run(Mode::Request, &*sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Ignored(_))), "{r:?}");
    assert!(sys.events().is_empty());
    env.assert_untouched();
}

/// Environnement simule : `get` de [`Sources::from_env`].
fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.to_string())
    }
}

const TEST_PUBKEY: &str = "RWTpqw9Q/wWqBSNQaYthtbYw0wq6wcvcDgqOZn0bpg07oQkfBitVXECh";

#[test]
fn test_sources_refused_outside_test_mode() {
    let s = Sources::from_env(env_of(&[]), TARGET).unwrap();
    assert_eq!(s.release_base_url, "https://get.ciphera.ch");
    assert_eq!(s.tools_base_url, "https://github.com");
    assert_eq!(s.pubkeys, updater::embedded_pubkeys());
    assert!(!s.pubkeys.is_empty());
    assert!(
        s.pubkeys
            .iter()
            .all(|k| k.starts_with("RW") && k.len() == 56)
    );

    for env in [
        &[("CIPHERA_RELEASE_BASE_URL", "https://evil.test")][..],
        &[("CIPHERA_TOOLS_BASE_URL", "https://evil.test")][..],
    ] {
        assert!(Sources::from_env(env_of(env), TARGET).is_err(), "{env:?}");
    }
}

/// Build de test : surcharges acceptees avec `CIPHERA_INSTALL_TEST=1`
/// seulement.
#[cfg(feature = "install-test")]
#[test]
fn test_sources_accepted_in_test_mode() {
    assert!(
        Sources::from_env(
            env_of(&[("CIPHERA_TEST_MINISIGN_PUBKEY", TEST_PUBKEY)]),
            TARGET
        )
        .is_err()
    );
    let s = Sources::from_env(
        env_of(&[
            ("CIPHERA_INSTALL_TEST", "1"),
            ("CIPHERA_RELEASE_BASE_URL", "https://localhost:9443/good"),
            ("CIPHERA_TEST_MINISIGN_PUBKEY", TEST_PUBKEY),
        ]),
        TARGET,
    )
    .unwrap();
    assert_eq!(s.release_base_url, "https://localhost:9443/good");
    assert_eq!(s.pubkeys, [TEST_PUBKEY]);
    assert!(
        Sources::from_env(
            env_of(&[
                ("CIPHERA_INSTALL_TEST", "1"),
                ("CIPHERA_RELEASE_BASE_URL", "http://localhost:9443"),
            ]),
            TARGET,
        )
        .is_err()
    );
}

/// Binaire livre (sans la fonctionnalite `install-test`) : ni
/// `CIPHERA_INSTALL_TEST` ni `CIPHERA_TEST_*` ne sont lus. Les origines de
/// test restent refusees, la cle de test est ignoree (cles integrees).
#[cfg(not(feature = "install-test"))]
#[test]
fn test_vars_ignored_without_feature() {
    for env in [
        &[
            ("CIPHERA_INSTALL_TEST", "1"),
            ("CIPHERA_RELEASE_BASE_URL", "https://localhost:9443/good"),
        ][..],
        &[
            ("CIPHERA_INSTALL_TEST", "1"),
            ("CIPHERA_TOOLS_BASE_URL", "https://localhost:9443/tools"),
        ][..],
    ] {
        let e = Sources::from_env(env_of(env), TARGET).unwrap_err();
        assert!(!e.contains("CIPHERA_INSTALL_TEST"), "{e}");
    }
    for env in [
        &[("CIPHERA_TEST_MINISIGN_PUBKEY", TEST_PUBKEY)][..],
        &[
            ("CIPHERA_INSTALL_TEST", "1"),
            ("CIPHERA_TEST_MINISIGN_PUBKEY", TEST_PUBKEY),
        ][..],
        &[
            ("CIPHERA_INSTALL_TEST", "1"),
            ("CIPHERA_TEST_MINISIGN_PUBKEY", "invalide"),
        ][..],
    ] {
        let s = Sources::from_env(env_of(env), TARGET).unwrap();
        assert_eq!(s.pubkeys, updater::embedded_pubkeys(), "{env:?}");
        assert_eq!(s.release_base_url, "https://get.ciphera.ch");
    }
}

/// La configuration de l'updater (`/etc/ciphera-updater/updater.env`, root)
/// ne retient que ses cles : PATH, LD_PRELOAD, mode de test, cle ou origine
/// des releases n'y ont aucun effet.
#[test]
fn updater_config_keeps_only_its_keys() {
    use ciphera_cli::updater::config::UpdaterConfig;
    let text = "\
# commentaire
PATH=/var/lib/ciphera/piege
LD_PRELOAD=/var/lib/ciphera/piege.so
CIPHERA_INSTALL_TEST=1
CIPHERA_TEST_MINISIGN_PUBKEY=RWTpqw9Q/wWqBSNQaYthtbYw0wq6wcvcDgqOZn0bpg07oQkfBitVXECh
CIPHERA_RELEASE_BASE_URL=https://evil.test
CIPHERA_DB_PASSWORD_FILE=/etc/shadow
CIPHERA_PANEL_LOCAL_PORT=9444
CIPHERA_DB_USER=app
CIPHERA_DB_NAME=appdb
CIPHERA_DATA_DIR=/srv/ciphera
";
    let c = UpdaterConfig::parse(text).unwrap();
    assert_eq!(c.panel_local_port, 9444);
    assert_eq!(c.db_user, "app");
    assert_eq!(c.db_name, "appdb");
    assert_eq!(c.data_dir, PathBuf::from("/srv/ciphera"));
    assert_eq!(
        c.ignored,
        [
            "PATH",
            "LD_PRELOAD",
            "CIPHERA_INSTALL_TEST",
            "CIPHERA_TEST_MINISIGN_PUBKEY",
            "CIPHERA_RELEASE_BASE_URL",
            "CIPHERA_DB_PASSWORD_FILE",
        ]
    );
    let d = UpdaterConfig::parse("").unwrap();
    assert_eq!(
        (d.panel_local_port, d.db_user.as_str(), d.db_name.as_str()),
        (8444, "ciphera", "ciphera")
    );
    assert_eq!(d.data_dir, PathBuf::from("/var/lib/ciphera"));
    for bad in [
        "CIPHERA_PANEL_LOCAL_PORT=0",
        "CIPHERA_PANEL_LOCAL_PORT=abc",
        "CIPHERA_DB_USER=-x",
        "CIPHERA_DB_NAME=a b",
        "CIPHERA_DATA_DIR=relatif",
        "ligne sans egal",
    ] {
        assert!(UpdaterConfig::parse(bad).is_err(), "{bad}");
    }
}

/// Fichier de configuration modifiable par un autre compte que root, ou
/// dans un dossier qui l'est : refuse.
#[cfg(unix)]
#[test]
fn updater_config_must_be_root_owned() {
    use ciphera_cli::updater::config::UpdaterConfig;
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new();
    let dir = tmp.0.join("etc-ciphera-updater");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let file = dir.join("updater.env");
    // Absent : valeurs par defaut.
    assert_eq!(UpdaterConfig::load(&file).unwrap().panel_local_port, 8444);
    std::fs::write(&file, "CIPHERA_PANEL_LOCAL_PORT=9444\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(UpdaterConfig::load(&file).is_err(), "ecrivable par tous");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    let root = rustix::process::geteuid().is_root();
    assert_eq!(UpdaterConfig::load(&file).is_ok(), root, "root seulement");
    if root {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(UpdaterConfig::load(&file).is_err(), "dossier ecrivable");
    }
}

/// Verrou en 0600 (meme laisse plus ouvert par une execution manuelle
/// precedente) et son dossier cree avec un mode explicite (0751, celui de
/// `ciphera.tmpfiles.conf`), quel que soit le umask du shell root.
#[cfg(unix)]
#[test]
fn lock_file_and_its_dir_get_explicit_modes() {
    use std::os::unix::fs::PermissionsExt;
    let env = env();
    let sys = Mock::new(&env, true);
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;
    updater::run(Mode::Recover, &sys, &env.paths, &env.sources).unwrap();
    assert_eq!(mode(env.paths.lock.parent().unwrap()), 0o751);
    assert_eq!(mode(&env.paths.lock), 0o600);

    std::fs::set_permissions(&env.paths.lock, std::fs::Permissions::from_mode(0o644)).unwrap();
    updater::run(Mode::Recover, &sys, &env.paths, &env.sources).unwrap();
    assert_eq!(
        mode(&env.paths.lock),
        0o600,
        "verrou lisible par d'autres comptes"
    );
}

// --- unites de l'archive et sante complete (C-I1, C-I2) ---------------------

const TIMER: &str = "[Timer]\nOnCalendar=daily\n\n[Install]\nWantedBy=timers.target\n";
const OLD_UNIT: &str =
    "[Service]\nExecStart=/usr/local/bin/ciphera\n\n[Install]\nWantedBy=multi-user.target\n";
const PATH_UNIT: &str = "[Path]\nPathChanged=/run/ciphera\n\n[Install]\nWantedBy=paths.target\n";
const SOCKET_UNIT: &str = "[Socket]\nListenStream=443\n\n[Install]\nWantedBy=sockets.target\n";

/// Registre des unites installees par Ciphera v0.1.0 (`units.list`).
fn record_v010() -> String {
    let mut units: Vec<&str> = updater::UNITS.to_vec();
    units.push("ciphera-old.service");
    units.sort();
    units.iter().map(|u| format!("{u}\n")).collect()
}

fn record(env: &Env) -> Option<String> {
    std::fs::read_to_string(env.paths.share_dir.join("units.list")).ok()
}

/// v0.2.0 republiee avec une unite nouvelle (`ciphera-backup.timer`) et le
/// profil AppArmor ; v0.1.0 installee avec une unite que v0.2.0 retire
/// (`ciphera-old.service`) et le profil AppArmor.
fn env_with_unit_changes() -> (Env, Keys) {
    let mut env = env();
    let k = keys();
    env.sources.pubkeys = vec![k.pk.to_base64()];
    for version in ["v0.0.9", "v0.1.0"] {
        publish(&mut env.files, &k, v(version));
    }
    publish_with(
        &mut env.files,
        &k,
        v("v0.2.0"),
        &[
            ("systemd/ciphera-backup.timer", TIMER),
            ("apparmor/ciphera-buildkitd", "profil v0.2.0"),
            ("apparmor/ciphera-buildkitd.abi3", "profil v0.2.0 abi3"),
        ],
        &[],
    );
    std::fs::write(env.paths.unit_dir.join("ciphera-old.service"), OLD_UNIT).unwrap();
    std::fs::write(env.paths.share_dir.join("units.list"), record_v010()).unwrap();
    std::fs::create_dir_all(&env.paths.apparmor_dir).unwrap();
    std::fs::write(
        env.paths.apparmor_dir.join(updater::APPARMOR_PROFILE),
        "profil v0.1.0",
    )
    .unwrap();
    (env, k)
}

#[test]
fn archive_units_are_installed_enabled_and_dropped_ones_removed() {
    let (env, _k) = env_with_unit_changes();
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");

    let units = &env.paths.unit_dir;
    assert_eq!(
        std::fs::read_to_string(units.join("ciphera-backup.timer")).unwrap(),
        TIMER
    );
    assert!(
        !units.join("ciphera-old.service").exists(),
        "unite retiree supprimee apres le succes"
    );
    assert_eq!(
        std::fs::read_to_string(env.paths.apparmor_dir.join(updater::APPARMOR_PROFILE)).unwrap(),
        "profil v0.2.0"
    );
    let changes = sys.unit_changes.lock().unwrap().clone();
    let first = &changes[0];
    assert_eq!(first.disable, vec!["ciphera-old.service".to_string()]);
    assert!(first.stop);
    assert_eq!(first.enable, vec!["ciphera-backup.timer".to_string()]);
    assert_eq!(first.start, vec!["ciphera-backup.timer".to_string()]);
    // Relecture apres la suppression de l'unite retiree.
    assert_eq!(changes.last().unwrap(), &updater::UnitChange::default());
    // Registre : les unites de l'archive.
    let rec = record(&env).unwrap();
    assert!(rec.lines().any(|l| l == "ciphera-backup.timer"), "{rec}");
    assert!(!rec.lines().any(|l| l == "ciphera-old.service"), "{rec}");

    // Ordre : unites, post-installation, redemarrage, sante, stabilite.
    let pos = |e: &str| sys.events().iter().position(|x| x == e).unwrap();
    assert!(pos("units") < pos("post_install v0.1.0 v0.2.0"));
    assert!(pos("post_install v0.1.0 v0.2.0") < pos("restart"));
    assert!(pos("health") < pos("settled"));
}

#[test]
fn apparmor_profile_is_not_installed_where_it_was_not() {
    let (env, _k) = env_with_unit_changes();
    std::fs::remove_file(env.paths.apparmor_dir.join(updater::APPARMOR_PROFILE)).unwrap();
    let sys = Mock::new(&env, true);
    updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources).unwrap();
    assert!(
        !env.paths
            .apparmor_dir
            .join(updater::APPARMOR_PROFILE)
            .exists()
    );
}

/// apparmor_parser 3 (Debian 12, Ubuntu 22.04), ou version illisible : la
/// variante AppArmor 3 remplace le profil installe, sous le meme nom.
#[test]
fn apparmor_profile_variant_follows_the_parser_version() {
    for major in [Some(3), None] {
        let (env, _k) = env_with_unit_changes();
        let mut sys = Mock::new(&env, true);
        sys.apparmor_major = major;
        let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
        assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");
        assert_eq!(
            std::fs::read_to_string(env.paths.apparmor_dir.join(updater::APPARMOR_PROFILE))
                .unwrap(),
            "profil v0.2.0 abi3",
            "{major:?}"
        );
        assert!(
            !env.paths
                .apparmor_dir
                .join(updater::APPARMOR_PROFILE_ABI3)
                .exists()
        );
    }
}

#[test]
fn apparmor_parser_version_is_parsed() {
    assert_eq!(
        updater::parse_apparmor_parser_major("AppArmor parser version 4.0.1\nCopyright\n"),
        Some(4)
    );
    assert_eq!(
        updater::parse_apparmor_parser_major("AppArmor parser version 3.0.8"),
        Some(3)
    );
    assert_eq!(updater::parse_apparmor_parser_major("autre chose"), None);
    assert_eq!(updater::apparmor_variant(Some(4)), "ciphera-buildkitd");
    assert_eq!(updater::apparmor_variant(Some(5)), "ciphera-buildkitd");
    assert_eq!(updater::apparmor_variant(Some(3)), "ciphera-buildkitd.abi3");
    assert_eq!(updater::apparmor_variant(None), "ciphera-buildkitd.abi3");
}

/// Retour arriere : unite nouvelle desactivee et retiree, unite retiree
/// reactivee et relancee, profil precedent remis.
fn assert_units_rolled_back(env: &Env, sys: &Mock) {
    let units = &env.paths.unit_dir;
    assert!(!units.join("ciphera-backup.timer").exists());
    assert_eq!(
        std::fs::read_to_string(units.join("ciphera-old.service")).unwrap(),
        OLD_UNIT
    );
    assert_eq!(
        std::fs::read_to_string(env.paths.apparmor_dir.join(updater::APPARMOR_PROFILE)).unwrap(),
        "profil v0.1.0"
    );
    let changes = sys.unit_changes.lock().unwrap().clone();
    let n = changes.len();
    assert!(n >= 3, "{changes:?}");
    assert_eq!(
        changes[n - 2].disable,
        vec!["ciphera-backup.timer".to_string()]
    );
    assert!(changes[n - 2].stop);
    assert_eq!(
        changes[n - 1].enable,
        vec!["ciphera-old.service".to_string()]
    );
    assert_eq!(
        changes[n - 1].start,
        vec!["ciphera-old.service".to_string()]
    );
    assert_eq!(env.installed(), "v0.1.0");
    assert_eq!(env.bin("ciphera"), binary("ciphera", v("v0.1.0")));
    assert_eq!(record(env).as_deref(), Some(record_v010().as_str()));
}

/// Hote installe avant le registre (`units.list` absent) : seules les
/// unites de la liste v0.1 sont gerees ; une unite `ciphera-*` ecrite par
/// l'administrateur n'est ni desactivee ni retiree, une unite existante
/// qu'il a desactivee n'est pas reactivee.
#[test]
fn without_a_record_only_v010_units_are_managed() {
    let (mut env, k) = env_with_unit_changes();
    std::fs::remove_file(env.paths.share_dir.join("units.list")).unwrap();
    publish_with(
        &mut env.files,
        &k,
        v("v0.2.0"),
        &[
            ("systemd/ciphera-backup.timer", TIMER),
            ("systemd/ciphera-updater.path", PATH_UNIT),
        ],
        &["ciphera-proxy.socket", "ciphera-updater.path"],
    );
    let units = env.paths.unit_dir.clone();
    std::fs::write(units.join("ciphera-proxy.socket"), SOCKET_UNIT).unwrap();
    std::fs::write(units.join("ciphera-updater.path"), PATH_UNIT).unwrap();
    let mut sys = Mock::new(&env, true);
    sys.disabled = vec!["ciphera-updater.path".into()];
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");

    assert_eq!(
        std::fs::read_to_string(units.join("ciphera-old.service")).unwrap(),
        OLD_UNIT,
        "unite de l'administrateur gardee"
    );
    assert!(!units.join("ciphera-proxy.socket").exists());
    let changes = sys.unit_changes.lock().unwrap().clone();
    assert_eq!(changes[0].disable, vec!["ciphera-proxy.socket".to_string()]);
    assert_eq!(changes[0].enable, vec!["ciphera-backup.timer".to_string()]);
    for c in &changes {
        assert!(
            !c.disable.contains(&"ciphera-old.service".to_string()),
            "{c:?}"
        );
        assert!(
            !c.enable.contains(&"ciphera-updater.path".to_string()),
            "{c:?}"
        );
    }
    let rec = record(&env).unwrap();
    assert!(rec.lines().any(|l| l == "ciphera-updater.path"), "{rec}");
    assert!(!rec.lines().any(|l| l == "ciphera-proxy.socket"), "{rec}");
    assert!(!rec.lines().any(|l| l == "ciphera-old.service"), "{rec}");
}

/// Mise a jour qui reussit : une unite existante (conservee par l'archive)
/// n'est jamais reactivee, qu'elle soit activee ou non.
#[test]
fn existing_units_keep_their_enabled_state() {
    let (mut env, k) = env_with_unit_changes();
    publish_with(
        &mut env.files,
        &k,
        v("v0.2.0"),
        &[("systemd/ciphera-updater.path", PATH_UNIT)],
        &["ciphera-updater.path"],
    );
    std::fs::write(env.paths.unit_dir.join("ciphera-updater.path"), PATH_UNIT).unwrap();
    let mut sys = Mock::new(&env, true);
    sys.disabled = vec!["ciphera-updater.path".into()];
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");
    for c in sys.unit_changes.lock().unwrap().iter() {
        assert!(c.enable.is_empty(), "{c:?}");
        assert!(c.start.is_empty(), "{c:?}");
    }
}

/// Retour arriere : une unite retiree que l'administrateur avait
/// desactivee reste desactivee ; le registre precedent est remis.
#[test]
fn rollback_keeps_a_disabled_removed_unit_disabled() {
    let (env, _k) = env_with_unit_changes();
    let mut sys = Mock::new(&env, false);
    sys.disabled = vec!["ciphera-old.service".into()];
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::RolledBack(_))), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(env.paths.unit_dir.join("ciphera-old.service")).unwrap(),
        OLD_UNIT
    );
    for c in sys.unit_changes.lock().unwrap().iter() {
        assert!(
            !c.enable.contains(&"ciphera-old.service".to_string()),
            "{c:?}"
        );
        assert!(
            !c.start.contains(&"ciphera-old.service".to_string()),
            "{c:?}"
        );
    }
    assert_eq!(record(&env).as_deref(), Some(record_v010().as_str()));
}

#[test]
fn unhealthy_update_reverts_unit_changes() {
    let (env, _k) = env_with_unit_changes();
    let sys = Mock::new(&env, false);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::RolledBack(_))), "{r:?}");
    assert_units_rolled_back(&env, &sys);
}

/// Proxy qui plante en boucle apres le redemarrage : le panneau repond,
/// mais la mise a jour n'est pas declaree reussie.
#[test]
fn crash_looping_proxy_rolls_back() {
    let (env, _k) = env_with_unit_changes();
    let mut sys = Mock::new(&env, true);
    sys.unsettled = Some("ciphera-proxy.service redemarre en boucle".into());
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::RolledBack(m)) => {
            assert!(
                m.contains("ciphera-proxy.service redemarre en boucle"),
                "{m}"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.count("restore_db"), 1);
    assert_units_rolled_back(&env, &sys);
    assert!(!env.journal().exists());
}

#[test]
fn failing_post_install_rolls_back_before_restart() {
    let (env, _k) = env_with_unit_changes();
    let mut sys = Mock::new(&env, true);
    sys.fail_post_install = true;
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::RolledBack(m)) => assert!(m.contains("post-installation"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(
        sys.position("post_install v0.1.0 v0.2.0").unwrap() < sys.position("restart").unwrap(),
        "{:?}",
        sys.events()
    );
    assert_eq!(sys.count("settled"), 0);
    assert_units_rolled_back(&env, &sys);
}

#[test]
fn archive_without_a_required_unit_is_refused() {
    let mut env = env();
    let k = keys();
    env.sources.pubkeys = vec![k.pk.to_base64()];
    publish_with(
        &mut env.files,
        &k,
        v("v0.2.0"),
        &[],
        &["ciphera-updater.path"],
    );
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(
        format!("{r:?}").contains("systemd/ciphera-updater.path"),
        "{r:?}"
    );
    env.assert_untouched();
}

/// Cle des demandes : le fichier `update-hmac.key`, jamais l'identite age.
/// Un `age.key` illisible (contenu et droits invalides) ne change rien.
#[test]
fn hmac_key_file_used_not_age() {
    let env = env();
    let age_key = env.paths.config_dir.join("age.key");
    std::fs::write(&age_key, b"pas une identite age\n").unwrap();
    let sys = Mock::new(&env, true);

    // Signee avec une autre cle : ignoree.
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &[3u8; 32]));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Ignored(_))), "{r:?}");
    env.assert_untouched();

    // Signee avec update-hmac.key : acceptee, age.key jamais lu ni modifie.
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &env.key));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert_eq!(
        r.unwrap(),
        Outcome::Updated {
            from: v("v0.1.0"),
            to: v("v0.2.0")
        }
    );
    assert_eq!(std::fs::read(&age_key).unwrap(), b"pas une identite age\n");

    // Sans age.key du tout : meme chose (l'updater n'en depend plus).
    let env = self::env();
    let sys = Mock::new(&env, true);
    assert!(!env.paths.config_dir.join("age.key").exists());
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &env.key));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");

    // Ni le code de l'updater ni la CLI n'ouvrent l'identite age.
    for (name, src) in [
        ("updater.rs", include_str!("../src/updater.rs")),
        ("main.rs", include_str!("../src/main.rs")),
    ] {
        assert!(!src.contains("SecretBox"), "{name} : SecretBox");
        assert!(!src.contains("join(\"age.key\")"), "{name} : age.key");
    }
}

/// Cle absente (installation anterieure) : creee par l'updater, meme sans
/// demande ; une demande signee avec elle passe ensuite.
#[test]
fn missing_hmac_key_created_by_updater() {
    let env = env();
    let path = env.paths.config_dir.join(KEY_FILE);
    std::fs::remove_file(&path).unwrap();
    #[cfg(unix)]
    let gid = {
        use std::os::unix::fs::MetadataExt;
        let gid = std::fs::metadata(&env.paths.config_dir).unwrap().gid();
        std::fs::create_dir_all(env._tmp.0.join("etc")).unwrap();
        std::fs::write(
            env._tmp.0.join("etc/group"),
            format!("root:x:0:\nciphera:x:{gid}:\n"),
        )
        .unwrap();
        gid
    };
    let sys = Mock::new(&env, true);

    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert_eq!(r.unwrap(), Outcome::NoRequest);
    let key = std::fs::read(&path).unwrap();
    assert_eq!(key.len(), 32);
    assert_ne!(key, env.key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(meta.mode() & 0o7777, 0o640, "root:ciphera 0640");
        assert_eq!(meta.gid(), gid);
    }

    // Jamais remplacee ensuite ; une demande signee avec elle passe.
    let key: [u8; 32] = key.try_into().unwrap();
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &key));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");
    assert_eq!(std::fs::read(&path).unwrap(), key);

    // Cle illisible (mauvaise taille) : jamais remplacee, demande refusee.
    std::fs::write(&path, b"courte").unwrap();
    env.write_request(&UpdateRequest::sign(v("v0.2.0"), NOW, &key));
    let r = updater::run(Mode::Request, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Io(_))), "{r:?}");
    assert!(!env.paths.request.exists(), "demande consommee");
    assert_eq!(std::fs::read(&path).unwrap(), b"courte");
}

/// Telechargements bornes : metadonnees a 1 Mio, archives a 512 Mio ; un
/// fichier au-dela est abandonne avant toute modification.
#[test]
fn oversized_metadata_download_aborts() {
    let env = env();
    let sys = Mock::new(&env, true);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    assert!(matches!(r, Ok(Outcome::Updated { .. })), "{r:?}");
    let limits = sys.limits.lock().unwrap().clone();
    assert_eq!(limits.len(), 6, "{limits:?}");
    for (url, limit) in &limits {
        let expected = if url.contains("/SHA256SUMS") {
            updater::METADATA_LIMIT
        } else {
            updater::ARCHIVE_LIMIT
        };
        assert_eq!(*limit, expected, "{url}");
    }
    assert_eq!(updater::METADATA_LIMIT.max_bytes, 1024 * 1024);
    assert_eq!(updater::ARCHIVE_LIMIT.max_bytes, 512 * 1024 * 1024);
    assert!(updater::METADATA_LIMIT.max_time <= Duration::from_secs(5 * 60));
    assert!(updater::ARCHIVE_LIMIT.max_time <= Duration::from_secs(15 * 60));

    let env = self::env();
    let sys = Mock::new(&env, true);
    let sums = format!("{BASE}/releases/v0.2.0/SHA256SUMS");
    sys.files
        .lock()
        .unwrap()
        .insert(sums, vec![b'a'; 1024 * 1024 + 1]);
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::Verification(m)) => assert!(m.contains("taille maximale"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.count("pg_dump"), 0);
    assert!(env.backups().is_empty());
    env.assert_untouched();
}

/// Retour arriere incomplet (un fichier ne peut pas etre remis) : la
/// sauvegarde decompressee `restore.sql` est quand meme effacee, le journal
/// garde pour la prochaine execution.
#[test]
fn restore_sql_removed_after_partial_rollback() {
    let env = env();
    let sys = Mock {
        block_restore_of: Some(env.paths.bin_dir.join("ciphera-proxy")),
        watch_restore_sql: Some(env.paths.state_dir.join("restore.sql")),
        ..Mock::new(&env, false)
    };
    let r = updater::run(manual("v0.2.0"), &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::Io(m)) => assert!(m.contains("incomplet"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(env.journal().exists(), "journal garde");
    assert_eq!(sys.count("restore_db"), 0, "base non restauree");
    // restore.sql existait (verifie, non vide) a l'arret des services du
    // retour arriere, puis a ete efface.
    let seen: Vec<String> = sys
        .events()
        .into_iter()
        .filter(|e| e.starts_with("restore.sql "))
        .collect();
    assert_eq!(seen.len(), 1, "{:?}", sys.events());
    assert_ne!(seen[0], "restore.sql 0");
    assert!(
        sys.position(&seen[0]).unwrap() > sys.position("restart").unwrap(),
        "observe pendant le retour arriere"
    );
    assert!(
        !env.paths.state_dir.join("restore.sql").exists(),
        "restore.sql laisse sur disque"
    );
}

// --- latest.json signe (`--latest`) -----------------------------------------

/// Commentaire de confiance de `latest.json` (comme install/sign-latest.sh).
fn latest_comment(version: &str, issued: u64) -> String {
    let expires = issued + shared::latest::VALIDITY_SECS;
    format!("ciphera-latest version={version} issued={issued} expires={expires}")
}

/// Publie `latest.json` (`json`) et sa signature par `keys` avec `comment`.
fn publish_latest(sys: &Mock, keys: &Keys, json: &str, comment: &str) {
    let sig = minisign::sign(
        Some(&keys.pk),
        &keys.sk,
        json.as_bytes(),
        Some(comment),
        None,
    )
    .unwrap()
    .to_string()
    .into_bytes();
    let mut files = sys.files.lock().unwrap();
    files.insert(
        format!("{BASE}/releases/latest.json"),
        json.as_bytes().to_vec(),
    );
    files.insert(format!("{BASE}/releases/latest.json.minisig"), sig);
}

#[test]
fn latest_signed_by_a_trusted_key_updates() {
    let env = env();
    let sys = Mock::new(&env, true);
    publish_latest(
        &sys,
        &env.signer,
        "{\"version\":\"v0.2.0\"}\n",
        &latest_comment("v0.2.0", NOW - 3600),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    assert!(
        matches!(r, Ok(Outcome::Updated { to, .. }) if to == v("v0.2.0")),
        "{r:?}"
    );
    assert_eq!(env.installed(), "v0.2.0");
    // Metadonnees : bornes de SHA256SUMS ; dossier de travail retire.
    for (url, limit) in sys.limits.lock().unwrap().iter() {
        if url.contains("latest.json") {
            assert_eq!(*limit, updater::METADATA_LIMIT, "{url}");
        }
    }
    let left: Vec<_> = std::fs::read_dir(&env.paths.state_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("latest-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn latest_signature_required() {
    let json = "{\"version\":\"v0.2.0\"}\n";
    let comment = latest_comment("v0.2.0", NOW - 3600);

    // Signature absente.
    let env = env();
    let sys = Mock::new(&env, true);
    sys.files.lock().unwrap().insert(
        format!("{BASE}/releases/latest.json"),
        json.as_bytes().to_vec(),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Io(_))), "{r:?}");
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    env.assert_untouched();

    // Signee par une cle hors de l'ensemble de confiance.
    let sys = Mock::new(&env, true);
    publish_latest(&sys, &keys(), json, &comment);
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::Verification(m)) => assert!(m.contains("signature"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.count("download"), 2, "rien d'autre telecharge");
    env.assert_untouched();

    // Contenu modifie apres la signature (version annoncee changee).
    let sys = Mock::new(&env, true);
    publish_latest(&sys, &env.signer, json, &comment);
    sys.files.lock().unwrap().insert(
        format!("{BASE}/releases/latest.json"),
        b"{\"version\":\"v0.2.1\"}\n".to_vec(),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Verification(_))), "{r:?}");
    env.assert_untouched();

    // Commentaire de confiance par defaut de minisign (sans version ni
    // echeance) : refuse.
    let sys = Mock::new(&env, true);
    publish_latest(
        &sys,
        &env.signer,
        json,
        "timestamp:1800000000\tfile:latest.json",
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::Verification(m)) => assert!(m.contains("commentaire"), "{m}"),
        other => panic!("{other:?}"),
    }
    env.assert_untouched();
    assert!(env.backups().is_empty());

    // Date de signature a plus d'un jour dans le futur : refusee.
    let sys = Mock::new(&env, true);
    publish_latest(
        &sys,
        &env.signer,
        json,
        &latest_comment("v0.2.0", NOW + 2 * 24 * 3600),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::Verification(m)) => assert!(m.contains("futur"), "{m}"),
        other => panic!("{other:?}"),
    }
    env.assert_untouched();

    // Site injoignable : erreur de telechargement, pas de verification.
    let sys = Mock::new(&env, true);
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Io(_))), "{r:?}");
    env.assert_untouched();
}

#[test]
fn latest_version_mismatch_refused() {
    let env = env();
    let sys = Mock::new(&env, true);
    // Signature valide, mais la version du JSON n'est pas celle du
    // commentaire signe.
    publish_latest(
        &sys,
        &env.signer,
        "{\"version\":\"v0.2.0\"}\n",
        &latest_comment("v0.3.0", NOW - 3600),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    match r {
        Err(UpdateError::Verification(m)) => assert!(m.contains("version differente"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(sys.count("pg_dump") + sys.count("restart"), 0);
    env.assert_untouched();

    // latest.json signe et valide, mais pour une version anterieure :
    // l'anti-retrogradation reste en place.
    let sys = Mock::new(&env, true);
    publish_latest(
        &sys,
        &env.signer,
        "{\"version\":\"v0.0.9\"}\n",
        &latest_comment("v0.0.9", NOW - 3600),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    assert!(matches!(r, Err(UpdateError::Refused(_))), "{r:?}");
    env.assert_untouched();
}

#[test]
fn expired_latest_warns_not_blocks() {
    let env = env();
    let sys = Mock::new(&env, true);
    // Signe il y a 200 jours : echeance (120 jours) depassee.
    publish_latest(
        &sys,
        &env.signer,
        "{\"version\":\"v0.2.0\"}\n",
        &latest_comment("v0.2.0", NOW - 200 * 24 * 3600),
    );
    let r = updater::run(Mode::Latest, &sys, &env.paths, &env.sources);
    assert!(
        matches!(r, Ok(Outcome::Updated { to, .. }) if to == v("v0.2.0")),
        "{r:?}"
    );
    assert_eq!(env.installed(), "v0.2.0");
}
