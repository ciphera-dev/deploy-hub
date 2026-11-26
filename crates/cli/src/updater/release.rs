//! Telechargement et verification d'une release, sans rien installer :
//! `SHA256SUMS` signe minisign, SHA-256 de l'archive, contenu lu entree par
//! entree (jamais `unpack` : aucun lien, aucun chemin hors du dossier de
//! travail), puis outils tiers du manifeste `tools.manifest` verifies par
//! SHA-256. Meme logique qu'`install.sh` (`download_and_verify`,
//! `download_build_tools`).

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use shared::update::Version;

use super::{
    APPARMOR_PROFILE, APPARMOR_PROFILE_ABI3, ARCHIVE_LIMIT, BINARIES, Limit, METADATA_LIMIT,
    REQUIRED_UNITS, Sources, System, TMPFILES, is_unit_name,
};

/// Nombre maximal d'unites systemd dans une archive.
const MAX_UNITS: usize = 32;
/// Taille maximale d'un fichier extrait d'une archive.
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;
/// Taille maximale de `SHA256SUMS`, de sa signature et du manifeste.
const MAX_SMALL_BYTES: u64 = 64 * 1024;
/// Outils tiers attendus dans le manifeste, et membre de leur archive.
const TOOLS: [(&str, &str); 3] = [
    ("railpack", "railpack"),
    ("buildctl", "bin/buildctl"),
    ("mise", "mise/bin/mise"),
];

/// Release verifiee, prete a installer.
pub(super) struct Staged {
    /// Contenu de l'archive (`bin/`, `systemd/`, `apparmor/`, `containers/`).
    pub pkg: PathBuf,
    /// Unites systemd de l'archive (`systemd/ciphera*.{service,…}`), triees.
    pub units: Vec<String>,
    /// Variantes du profil AppArmor de buildkitd livrees par l'archive
    /// (`APPARMOR_PROFILE`, `APPARMOR_PROFILE_ABI3`).
    pub apparmor: Vec<String>,
    /// Outils verifies : (fichier prepare, nom installe).
    pub tools: Vec<(PathBuf, String)>,
    /// Fichiers `containers/*.digest` de l'archive.
    pub containers: Vec<String>,
    /// Derniere migration de la base de cette release (fichier `migration`
    /// de l'archive, ecrit par `install/package.sh`).
    pub migration: u32,
}

/// Octet admis dans une URL ou un chemin du manifeste (comme `install.sh`).
pub(super) fn url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b":/._-".contains(&b)
}

fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"._-".contains(&b)
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('.') && s.bytes().all(name_byte)
}

/// Chemin relatif sur : segments non vides, sans `.` ni `..` initial.
fn is_rel_path(s: &str) -> bool {
    !s.is_empty() && s.split('/').all(is_name)
}

fn sha256_hex(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn read_small(path: &Path) -> Result<String, String> {
    let mut s = String::new();
    File::open(path)
        .and_then(|f| f.take(MAX_SMALL_BYTES + 1).read_to_string(&mut s))
        .map_err(|_| format!("{} illisible", path.display()))?;
    if s.len() as u64 > MAX_SMALL_BYTES {
        return Err(format!("{} trop long", path.display()));
    }
    Ok(s)
}

/// Telecharge `url` dans `dest`, borne par `limit` ; la taille est
/// reverifiee sur le fichier obtenu, retire au-dela.
fn get(sys: &dyn System, url: &str, dest: &Path, limit: Limit) -> Result<(), String> {
    let fetched = sys
        .download(url, dest, limit)
        .map_err(|e| format!("telechargement impossible : {url} ({e})"))
        .and_then(|()| {
            let len = fs::symlink_metadata(dest)
                .map_err(|e| format!("telechargement impossible : {url} ({e})"))?
                .len();
            if len > limit.max_bytes {
                return Err(format!(
                    "telechargement abandonne : {url} depasse la taille maximale ({} octets)",
                    limit.max_bytes
                ));
            }
            Ok(())
        });
    if fetched.is_err() {
        let _ = fs::remove_file(dest);
    }
    fetched
}

/// Signature minisign de `data` par l'une des cles de confiance (plusieurs
/// pendant une rotation). Une cle mal formee est une erreur, jamais ignoree.
fn verify_any(pubkeys: &[String], data: &[u8], sig: &str) -> Result<(), String> {
    let keys = pubkeys
        .iter()
        .map(|k| minisign_verify::PublicKey::from_base64(k))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "cle publique minisign invalide".to_string())?;
    let sig = minisign_verify::Signature::decode(sig)
        .map_err(|_| "signature minisign de SHA256SUMS invalide".to_string())?;
    if keys.iter().any(|pk| pk.verify(data, &sig, false).is_ok()) {
        Ok(())
    } else {
        Err("signature minisign de SHA256SUMS invalide (aucune cle de confiance)".into())
    }
}

/// Avertissement d'un `latest.json` dont l'echeance est passee (meme sens
/// que le message du panneau).
pub(super) const LATEST_EXPIRED: &str = "impossible de garantir que Ciphera est a jour \
    (metadonnees de version de plus de 120 jours)";

/// Derniere version publiee (`--latest`) : `releases/latest.json` et
/// `latest.json.minisig`, verifies par une cle de confiance
/// ([`shared::latest::verify_latest`] : signature, commentaire de confiance,
/// version identique) avant tout usage ; sinon refus. Echeance passee :
/// avertissement seulement. La version obtenue suit ensuite le meme chemin
/// que `--version` (anti-retrogradation, minisign de `SHA256SUMS`).
pub(super) fn fetch_latest(
    sys: &dyn System,
    dir: &Path,
    sources: &Sources,
) -> Result<Version, String> {
    let base = format!("{}/releases", sources.release_base_url);
    for f in ["latest.json", "latest.json.minisig"] {
        get(sys, &format!("{base}/{f}"), &dir.join(f), METADATA_LIMIT)?;
    }
    let json = read_small(&dir.join("latest.json"))?;
    let sig = read_small(&dir.join("latest.json.minisig"))?;
    let latest = shared::latest::verify_latest(&sources.pubkeys, json.as_bytes(), &sig)
        .map_err(|e| e.to_string())?;
    if latest.expired(sys.now()) {
        eprintln!(
            "attention : {LATEST_EXPIRED} : latest.json signe le {}, echeance depassee ; \
             mise a jour poursuivie",
            super::utc_stamp(latest.issued)
        );
    }
    eprintln!("derniere version publiee : {}", latest.version);
    Ok(latest.version)
}

pub(super) fn fetch(
    sys: &dyn System,
    staging: &Path,
    sources: &Sources,
    version: Version,
) -> Result<Staged, String> {
    let name = format!("ciphera-{version}-{}", sources.target);
    let archive = format!("{name}.tar.gz");
    let base = format!("{}/releases/{version}", sources.release_base_url);
    let dl = staging.join("download");
    fs::create_dir_all(&dl).map_err(|e| e.to_string())?;

    // Signature minisign de SHA256SUMS.
    get(
        sys,
        &format!("{base}/SHA256SUMS"),
        &dl.join("SHA256SUMS"),
        METADATA_LIMIT,
    )?;
    get(
        sys,
        &format!("{base}/SHA256SUMS.minisig"),
        &dl.join("SHA256SUMS.minisig"),
        METADATA_LIMIT,
    )?;
    let sums = read_small(&dl.join("SHA256SUMS"))?;
    let sig = read_small(&dl.join("SHA256SUMS.minisig"))?;
    verify_any(&sources.pubkeys, sums.as_bytes(), &sig)?;

    // Une seule ligne pour l'archive choisie.
    let wanted: Vec<&str> = sums
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (sum, file) = (f.next()?, f.next()?);
            (f.next().is_none() && (file == archive || file.strip_prefix('*') == Some(&archive)))
                .then_some(sum)
        })
        .collect();
    let [sum] = wanted[..] else {
        return Err(format!(
            "SHA256SUMS ne contient pas exactement une ligne pour {archive}"
        ));
    };
    if !is_sha256(sum) {
        return Err("empreinte SHA-256 mal formee".into());
    }
    let archive_path = dl.join(&archive);
    get(
        sys,
        &format!("{base}/{archive}"),
        &archive_path,
        ARCHIVE_LIMIT,
    )?;
    if sha256_hex(&archive_path).map_err(|e| e.to_string())? != sum {
        return Err(format!("SHA-256 de {archive} incorrect"));
    }

    // Contenu de l'archive.
    let pkg = staging.join("pkg");
    let Extracted {
        containers,
        units,
        apparmor,
    } = extract_release(&archive_path, &name, &pkg)?;

    let migration = read_small(&pkg.join("migration"))?;
    let migration = migration
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|_| {
            migration.trim().len() <= 9 && migration.trim().bytes().all(|b| b.is_ascii_digit())
        })
        .ok_or_else(|| "archive : fichier migration invalide".to_string())?;

    // Outils tiers du manifeste de la nouvelle release.
    let manifest = read_small(&pkg.join("tools.manifest"))?;
    let tools_dir = staging.join("tools");
    fs::create_dir_all(&tools_dir).map_err(|e| e.to_string())?;
    let mut tools = Vec::new();
    for tool in parse_manifest(&manifest)? {
        let file = tool.path.rsplit('/').next().unwrap_or(tool.name);
        let downloaded = dl.join(format!("tool-{}", tool.name));
        get(
            sys,
            &format!("{}/{}", sources.tools_base_url, tool.path),
            &downloaded,
            ARCHIVE_LIMIT,
        )?;
        if sha256_hex(&downloaded).map_err(|e| e.to_string())? != tool.sha256 {
            return Err(format!("SHA-256 de {file} incorrect"));
        }
        let staged = tools_dir.join(&tool.dest);
        extract_member(&downloaded, tool.member, &staged)
            .map_err(|e| format!("archive {file} : {e}"))?;
        tools.push((staged, tool.dest));
    }
    Ok(Staged {
        pkg,
        units,
        apparmor,
        tools,
        containers,
        migration,
    })
}

/// Fichier d'archive retenu : `rel` (relatif au dossier de la release).
fn wanted_release_file(rel: &str) -> bool {
    match rel.split_once('/') {
        None => rel == "tools.manifest" || rel == "migration",
        Some(("bin", b)) => BINARIES.contains(&b),
        Some(("systemd", u)) => is_unit_name(u) || u == TMPFILES,
        Some(("apparmor", a)) => a == APPARMOR_PROFILE || a == APPARMOR_PROFILE_ABI3,
        Some(("containers", c)) => is_name(c) && c.ends_with(".digest"),
        _ => false,
    }
}

/// Fichiers retenus d'une archive extraite.
struct Extracted {
    containers: Vec<String>,
    units: Vec<String>,
    apparmor: Vec<String>,
}

/// Extrait les fichiers utiles de l'archive dans `out`. Toute entree hors de
/// `<name>/`, tout lien, tout fichier exige manquant et plus de
/// [`MAX_UNITS`] unites font echouer.
fn extract_release(archive: &Path, name: &str, out: &Path) -> Result<Extracted, String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    let mut seen = HashSet::new();
    let prefix = format!("{name}/");
    for entry in tar.entries().map_err(|_| "archive illisible".to_string())? {
        let entry = entry.map_err(|_| "archive illisible".to_string())?;
        let path = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|_| "archive : nom de fichier invalide".to_string())?;
        let path = path.strip_prefix("./").unwrap_or(&path);
        let rel = match path.strip_prefix(&prefix) {
            Some(rel) => rel.trim_end_matches('/'),
            None if path.trim_end_matches('/') == name => continue,
            None => return Err(format!("archive : entree hors de {name}/ ({path})")),
        };
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err(format!("archive : entree non ordinaire refusee ({rel})"));
        }
        if !is_rel_path(rel) {
            return Err(format!("archive : chemin refuse ({rel})"));
        }
        if !wanted_release_file(rel) {
            continue;
        }
        if !seen.insert(rel.to_string()) {
            return Err(format!("archive : {rel} en double"));
        }
        write_entry(entry, &out.join(rel)).map_err(|e| format!("archive : {rel} ({e})"))?;
    }
    let required = BINARIES
        .iter()
        .map(|b| format!("bin/{b}"))
        .chain(
            REQUIRED_UNITS
                .iter()
                .chain(&[TMPFILES])
                .map(|u| format!("systemd/{u}")),
        )
        .chain(["tools.manifest".to_string(), "migration".to_string()]);
    for r in required {
        if !seen.contains(&r) {
            return Err(format!("archive incomplete ({r})"));
        }
    }
    let mut containers: Vec<String> = seen
        .iter()
        .filter_map(|r| r.strip_prefix("containers/").map(str::to_string))
        .collect();
    containers.sort();
    let mut units: Vec<String> = seen
        .iter()
        .filter_map(|r| r.strip_prefix("systemd/"))
        .filter(|u| *u != TMPFILES)
        .map(str::to_string)
        .collect();
    if units.len() > MAX_UNITS {
        return Err(format!("archive : plus de {MAX_UNITS} unites systemd"));
    }
    units.sort();
    Ok(Extracted {
        containers,
        units,
        apparmor: [APPARMOR_PROFILE, APPARMOR_PROFILE_ABI3]
            .into_iter()
            .filter(|a| seen.contains(&format!("apparmor/{a}")))
            .map(str::to_string)
            .collect(),
    })
}

fn write_entry(entry: impl Read, dest: &Path) -> io::Result<()> {
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)?;
    let copied = io::copy(&mut entry.take(MAX_ENTRY_BYTES + 1), &mut out)?;
    if copied > MAX_ENTRY_BYTES {
        return Err(io::Error::other("fichier trop gros"));
    }
    out.sync_all()
}

/// Extrait le seul fichier `member` d'une archive tar.gz dans `dest`.
fn extract_member(archive: &Path, member: &str, dest: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    for entry in tar.entries().map_err(|_| "illisible".to_string())? {
        let entry = entry.map_err(|_| "illisible".to_string())?;
        let path = entry.path_bytes();
        let path = path.strip_prefix(b"./").unwrap_or(&path);
        if path != member.as_bytes() {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(format!("{member} n'est pas un fichier ordinaire"));
        }
        return write_entry(entry, dest).map_err(|e| e.to_string());
    }
    Err(format!("{member} absent"))
}

struct Tool<'a> {
    name: &'static str,
    sha256: &'a str,
    path: &'a str,
    member: &'static str,
    dest: String,
}

/// `tools.manifest` (`install.sh --tools-manifest <cible>`) : une ligne par
/// outil, `<nom> <version> <sha256> <chemin> <membre> <nom installe>`.
fn parse_manifest(text: &str) -> Result<Vec<Tool<'_>>, String> {
    let bad = |l: &str| format!("tools.manifest : ligne invalide ({l})");
    let mut tools = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        let [name, version, sha256, path, member, dest] = f[..] else {
            return Err(bad(line));
        };
        let Some(&(name, expected_member)) = TOOLS.iter().find(|(n, _)| *n == name) else {
            return Err(bad(line));
        };
        if !is_name(version)
            || !is_sha256(sha256)
            || !is_rel_path(path)
            || !path.bytes().all(url_byte)
            || member != expected_member
            || !is_name(dest)
            || !dest.starts_with(name)
            || tools.iter().any(|t: &Tool| t.name == name)
        {
            return Err(bad(line));
        }
        tools.push(Tool {
            name,
            sha256,
            path,
            member: expected_member,
            dest: dest.to_string(),
        });
    }
    if tools.len() != TOOLS.len() {
        return Err("tools.manifest incomplet".into());
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUM: &str = "248a8f60f36854aabed6124113631078a802de3461660855fc4de9c26065c1fb";

    fn line(name: &str, member: &str, dest: &str) -> String {
        format!("{name} v1 {SUM} org/{name}/releases/download/v1/{name}.tar.gz {member} {dest}\n")
    }

    #[test]
    fn manifest_is_strict() {
        let ok = line("railpack", "railpack", "railpack")
            + &line("buildctl", "bin/buildctl", "buildctl")
            + &line("mise", "mise/bin/mise", "mise-2026.10.14");
        assert_eq!(parse_manifest(&ok).unwrap().len(), 3);
        // Outil en moins, inconnu, en double, membre ou destination detournes.
        let two = line("railpack", "railpack", "railpack")
            + &line("buildctl", "bin/buildctl", "buildctl");
        assert!(parse_manifest(&two).is_err());
        for bad in [
            line("curl", "curl", "curl"),
            line("railpack", "railpack", "railpack"),
            line("mise", "../etc/passwd", "mise"),
            line("mise", "mise/bin/mise", "../ciphera"),
            line("mise", "mise/bin/mise", "ciphera"),
            line("mise", "mise/bin/mise", ".mise"),
            "mise v1 abc org/x mise/bin/mise mise\n".to_string(),
            format!("mise v1 {SUM} ../x mise/bin/mise mise\n"),
        ] {
            let text = two.clone() + &bad;
            assert!(parse_manifest(&text).is_err(), "{bad}");
        }
    }

    #[test]
    fn only_expected_release_files_are_kept() {
        assert!(wanted_release_file("bin/ciphera"));
        assert!(wanted_release_file("systemd/ciphera.service"));
        assert!(wanted_release_file("containers/postgres.digest"));
        assert!(wanted_release_file("tools.manifest"));
        assert!(!wanted_release_file("bin/sh"));
        assert!(!wanted_release_file("systemd/evil.service"));
        assert!(wanted_release_file("systemd/ciphera-backup.timer"));
        assert!(wanted_release_file("apparmor/ciphera-buildkitd"));
        assert!(wanted_release_file("apparmor/ciphera-buildkitd.abi3"));
        assert!(!wanted_release_file("apparmor/other"));
        assert!(!wanted_release_file("systemd/ciphera.conf"));
        assert!(!wanted_release_file("systemd/ciphera-.service"));
        assert!(!wanted_release_file("systemd/ciphera--x.service"));
        assert!(!wanted_release_file("systemd/Ciphera.service"));
        assert!(!wanted_release_file("systemd/ciphera.mount"));
        assert!(!wanted_release_file("containers/x.sh"));
        assert!(!wanted_release_file("LICENSE"));
        assert!(!is_rel_path("bin/../x"));
        assert!(!is_rel_path("/etc/passwd"));
    }
}
