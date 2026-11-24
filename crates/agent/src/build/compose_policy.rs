//! Politique des fichiers Compose, appliquee avant tout `docker compose`
//! (sur le fichier du depot) puis sur sa forme normalisee
//! (`docker compose config`). Liste blanche : une cle inconnue est refusee.
//!
//! Refuses (une app deployee n'a aucun privilege de l'hote) : `privileged`,
//! `network_mode`, `pid`, `ipc`, `cap_add`, `devices`, `security_opt` autre
//! que `no-new-privileges`, volumes `bind` (seuls les volumes nommes et
//! `tmpfs` sont acceptes, donc jamais le socket Docker), `ports` publies
//! (c'est le proxy qui expose), reseaux declares (Ciphera place chaque
//! service sur le reseau du projet), volumes externes ou avec pilote,
//! `include`, `extends` vers un autre fichier, `env_file` et contexte ou
//! Dockerfile de build hors du depot, entitlements et reseau du build.
//!
//! Les chemins sont verifies ici sans toucher au disque (forme lexicale) ;
//! les liens symboliques sont verifies par `compose` (`confine`).

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::compose::ComposeFile;

/// Cle refusee, avec son emplacement (`services.web.privileged`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyViolation {
    pub path: String,
    pub reason: &'static str,
}

impl fmt::Display for PolicyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Compose : {} refuse ({})", self.path, self.reason)
    }
}

impl std::error::Error for PolicyViolation {}

type Checked = Result<(), PolicyViolation>;

fn deny(path: impl Into<String>, reason: &'static str) -> PolicyViolation {
    PolicyViolation {
        path: path.into(),
        reason,
    }
}

const HOST_PRIVILEGE: &str = "privilege de l'hote";
const UNSUPPORTED: &str = "cle non prise en charge";
const OUTSIDE: &str = "chemin hors du depot ou invalide";

/// Cles acceptees d'un service (avec leurs regles propres plus bas).
const SERVICE_KEYS: &[&str] = &[
    "image",
    "build",
    "command",
    "entrypoint",
    "environment",
    "env_file",
    "depends_on",
    "healthcheck",
    "restart",
    "volumes",
    "working_dir",
    "user",
    "labels",
    "deploy",
    "expose",
    "init",
    "stop_signal",
    "stop_grace_period",
    "tmpfs",
    "ulimits",
    "read_only",
    "shm_size",
    "mem_limit",
    "memswap_limit",
    "mem_reservation",
    "cpus",
    "cpu_shares",
    "pids_limit",
    "platform",
    "pull_policy",
    "profiles",
    "scale",
    "stdin_open",
    "tty",
    "hostname",
    "domainname",
    "extra_hosts",
    "dns",
    "dns_search",
    "links",
    "cap_drop",
    "security_opt",
    "networks",
    "extends",
    "annotations",
    "attach",
];

/// Cles refusees d'un service, avec leur raison.
const SERVICE_DENIED: &[(&str, &str)] = &[
    ("privileged", HOST_PRIVILEGE),
    ("network_mode", "reseau de l'hote ou d'un autre conteneur"),
    ("pid", "espace de noms PID de l'hote"),
    ("ipc", "espace de noms IPC de l'hote"),
    ("cap_add", "capacites ajoutees"),
    ("devices", "peripheriques de l'hote"),
    ("device_cgroup_rules", "peripheriques de l'hote"),
    ("gpus", "peripheriques de l'hote"),
    (
        "ports",
        "port publie sur l'hote (le proxy expose le service)",
    ),
    ("userns_mode", HOST_PRIVILEGE),
    ("uts", HOST_PRIVILEGE),
    ("cgroup", HOST_PRIVILEGE),
    ("cgroup_parent", HOST_PRIVILEGE),
    ("sysctls", HOST_PRIVILEGE),
    ("runtime", HOST_PRIVILEGE),
    ("isolation", HOST_PRIVILEGE),
    ("volumes_from", "volumes d'un autre conteneur"),
    ("external_links", "conteneur hors du projet"),
    ("container_name", "nom de conteneur fixe"),
    ("use_api_socket", "socket Docker"),
    ("provider", "programme de l'hote"),
    ("post_start", "commande privilegiee"),
    ("pre_stop", "commande privilegiee"),
    ("label_file", "fichier lu sur l'hote"),
    ("configs", "fichier lu sur l'hote"),
    ("secrets", "fichier lu sur l'hote"),
    ("logging", "pilote de journaux de l'hote"),
];

const BUILD_DENIED: &[(&str, &str)] = &[
    (
        "entitlements",
        "entitlement BuildKit (security.insecure, network.host)",
    ),
    ("network", "reseau du build"),
    ("privileged", HOST_PRIVILEGE),
    ("additional_contexts", "contexte hors du depot"),
    ("ssh", "agent SSH de l'hote"),
    ("secrets", "fichier lu sur l'hote"),
    ("cache_from", "cache hors de BuildKit"),
    ("cache_to", "cache hors de BuildKit"),
    ("extra_hosts", "reseau du build"),
    ("args", "arguments de build non pris en charge en v0.1"),
];

/// Applique la politique a `file` (forme du depot ou normalisee).
pub fn check(file: &ComposeFile) -> Checked {
    let doc = file
        .doc
        .as_object()
        .ok_or_else(|| deny("(racine)", "document Compose attendu"))?;
    let project = doc.get("name").and_then(Value::as_str);
    for (key, value) in doc {
        match key.as_str() {
            "name" | "version" => {}
            k if k.starts_with("x-") => {}
            "services" => services(file, value)?,
            "volumes" => top_volumes(file, project, value)?,
            "networks" if file.normalized => top_networks(project, value)?,
            "networks" => {
                return Err(deny(
                    "networks",
                    "reseau declare (chaque service est place sur le reseau du projet)",
                ));
            }
            "include" => return Err(deny("include", "fichier inclus")),
            other => return Err(deny(other, UNSUPPORTED)),
        }
    }
    Ok(())
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, PolicyViolation> {
    value.as_object().ok_or_else(|| deny(path, "objet attendu"))
}

/// Valeur neutre : absente, `false`, liste ou objet vides.
fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

fn services(file: &ComposeFile, value: &Value) -> Checked {
    for (name, svc) in object(value, "services")? {
        let at = format!("services.{name}");
        let svc = object(svc, &at)?;
        for (key, v) in svc {
            let path = format!("{at}.{key}");
            if let Some((_, reason)) = SERVICE_DENIED.iter().find(|(k, _)| k == key) {
                if is_empty(v) {
                    continue;
                }
                return Err(deny(path, reason));
            }
            if key.starts_with("x-") {
                continue;
            }
            if !SERVICE_KEYS.contains(&key.as_str()) {
                return Err(deny(path, UNSUPPORTED));
            }
            match key.as_str() {
                "image" => image(v, &path)?,
                "build" => build(file, v, &path)?,
                "volumes" => service_volumes(v, &path)?,
                "env_file" => env_file(file, v, &path)?,
                "security_opt" => security_opt(v, &path)?,
                "networks" => service_networks(file, v, &path)?,
                "extends" => extends(v, &path)?,
                "deploy" => deploy(v, &path)?,
                "scale" => replicas(v, &path)?,
                "labels" => labels(v, &path)?,
                _ => {}
            }
        }
    }
    Ok(())
}

fn image(value: &Value, path: &str) -> Checked {
    match value.as_str() {
        Some(i) if shared::validate::image(i).is_ok() => Ok(()),
        _ => Err(deny(path, "reference d'image invalide")),
    }
}

/// Chemin d'un fichier ou dossier du depot : sans `$` ni caractere de shell,
/// et dans le depot une fois resolu depuis `base` (forme lexicale).
fn inside(file: &ComposeFile, base: &Path, value: &str) -> Option<PathBuf> {
    let allowed = !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'));
    if !allowed {
        return None;
    }
    let joined = lexical(base, value);
    joined.starts_with(&file.root).then_some(joined)
}

/// `base` + `value` sans toucher au disque : `.` ignore, `..` remonte.
pub(crate) fn lexical(base: &Path, value: &str) -> PathBuf {
    let candidate = if Path::new(value).has_root() {
        PathBuf::from(value)
    } else {
        base.join(value)
    };
    let mut out = PathBuf::new();
    for c in candidate.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn build(file: &ComposeFile, value: &Value, path: &str) -> Checked {
    let (context, dockerfile) = match value {
        Value::String(ctx) => (Some(ctx.as_str()), None),
        Value::Object(b) => {
            for (key, v) in b {
                let at = format!("{path}.{key}");
                if let Some((_, reason)) = BUILD_DENIED.iter().find(|(k, _)| k == key) {
                    if is_empty(v) {
                        continue;
                    }
                    return Err(deny(at, reason));
                }
                if !matches!(key.as_str(), "context" | "dockerfile" | "target") {
                    return Err(deny(at, UNSUPPORTED));
                }
                if !v.is_string() {
                    return Err(deny(at, "texte attendu"));
                }
            }
            (
                b.get("context").and_then(Value::as_str),
                b.get("dockerfile").and_then(Value::as_str),
            )
        }
        _ => return Err(deny(path, "objet attendu")),
    };
    let ctx = inside(file, &file.base, context.unwrap_or("."))
        .ok_or_else(|| deny(format!("{path}.context"), OUTSIDE))?;
    if let Some(df) = dockerfile {
        inside(file, &ctx, df).ok_or_else(|| deny(format!("{path}.dockerfile"), OUTSIDE))?;
    }
    Ok(())
}

fn named_volume(name: &str) -> bool {
    let mut b = name.bytes();
    b.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'))
}

const BIND: &str = "montage d'un chemin de l'hote (seuls les volumes nommes sont acceptes)";

fn service_volumes(value: &Value, path: &str) -> Checked {
    let items = value
        .as_array()
        .ok_or_else(|| deny(path, "liste attendue"))?;
    for (i, item) in items.iter().enumerate() {
        let at = format!("{path}[{i}]");
        match item {
            Value::String(short) => {
                let parts: Vec<&str> = short.split(':').collect();
                // Une seule partie : volume anonyme (chemin du conteneur).
                if parts.len() >= 2 && !named_volume(parts[0]) {
                    return Err(deny(at, BIND));
                }
            }
            Value::Object(long) => {
                match long.get("type").and_then(Value::as_str) {
                    Some("volume") => {}
                    Some("tmpfs") => continue,
                    _ => return Err(deny(at, BIND)),
                }
                if let Some(source) = long.get("source") {
                    match source.as_str() {
                        Some(s) if named_volume(s) => {}
                        _ => return Err(deny(at, BIND)),
                    }
                }
            }
            _ => return Err(deny(at, "volume invalide")),
        }
    }
    Ok(())
}

fn env_file(file: &ComposeFile, value: &Value, path: &str) -> Checked {
    for (i, p) in env_file_paths(value)
        .ok_or_else(|| deny(path, "liste de fichiers attendue"))?
        .into_iter()
        .enumerate()
    {
        inside(file, &file.base, p).ok_or_else(|| deny(format!("{path}[{i}]"), OUTSIDE))?;
    }
    Ok(())
}

/// Chemins d'un `env_file` (texte, liste de textes ou de `{path, ...}`).
pub(crate) fn env_file_paths(value: &Value) -> Option<Vec<&str>> {
    match value {
        Value::String(p) => Some(vec![p.as_str()]),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(p) => Some(p.as_str()),
                Value::Object(o) => o.get("path").and_then(Value::as_str),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// Prefixe des etiquettes posees par Ciphera (projet, environnement,
/// deploiement) : un fichier Compose ne peut pas les forger.
const RESERVED_LABEL_PREFIX: &str = "dev.ciphera.";

fn reserved_label(key: &str) -> bool {
    key.trim_start()
        .to_ascii_lowercase()
        .starts_with(RESERVED_LABEL_PREFIX)
}

/// Etiquettes d'un service, en table ou en liste `cle=valeur`.
fn labels(value: &Value, path: &str) -> Checked {
    const RESERVED: &str = "etiquette reservee a Ciphera (dev.ciphera.*)";
    let reserved = match value {
        Value::Object(map) => map.keys().any(|k| reserved_label(k)),
        Value::Array(items) => {
            let mut reserved = false;
            for item in items {
                let entry = item.as_str().ok_or_else(|| deny(path, "texte attendu"))?;
                reserved |= reserved_label(entry.split('=').next().unwrap_or(entry));
            }
            reserved
        }
        Value::Null => false,
        _ => return Err(deny(path, "table ou liste attendue")),
    };
    if reserved {
        return Err(deny(path, RESERVED));
    }
    Ok(())
}

fn security_opt(value: &Value, path: &str) -> Checked {
    let items = value
        .as_array()
        .ok_or_else(|| deny(path, "liste attendue"))?;
    for item in items {
        match item.as_str() {
            Some("no-new-privileges" | "no-new-privileges:true" | "no-new-privileges=true") => {}
            _ => return Err(deny(path, "option de securite assouplie")),
        }
    }
    Ok(())
}

fn service_networks(file: &ComposeFile, value: &Value, path: &str) -> Checked {
    // Forme normalisee : le seul reseau `default` cree par Compose (Ciphera
    // le remplace par le reseau du projet au deploiement).
    let only_default = match value {
        Value::Object(o) => o.len() == 1 && o.get("default").is_some_and(is_empty),
        Value::Array(a) => a.len() == 1 && a[0] == "default",
        _ => false,
    };
    if file.normalized && only_default {
        return Ok(());
    }
    Err(deny(
        path,
        "reseau declare (chaque service est place sur le reseau du projet)",
    ))
}

fn extends(value: &Value, path: &str) -> Checked {
    match value {
        Value::String(_) => Ok(()),
        Value::Object(o) if o.keys().all(|k| k == "service") => Ok(()),
        Value::Object(o) if o.contains_key("file") => {
            Err(deny(format!("{path}.file"), "service d'un autre fichier"))
        }
        _ => Err(deny(path, UNSUPPORTED)),
    }
}

/// Repliques au plus par service (`scale`, `deploy.replicas`) ; le service
/// expose en a exactement une (verifie au plan de deploiement).
pub const MAX_REPLICAS: u64 = 4;

/// Nombre de repliques d'un service : entier ou texte entier, au plus
/// `MAX_REPLICAS`.
pub(crate) fn replica_count(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn replicas(value: &Value, path: &str) -> Checked {
    if value.is_null() {
        return Ok(());
    }
    match replica_count(value) {
        Some(n) if n <= MAX_REPLICAS => Ok(()),
        Some(_) => Err(deny(path, "trop de repliques (4 au plus par service)")),
        None => Err(deny(path, "nombre de repliques invalide")),
    }
}

fn deploy(value: &Value, path: &str) -> Checked {
    let allowed = |v: &Value, at: &str, keys: &[&str]| -> Checked {
        for k in object(v, at)?.keys() {
            if !keys.contains(&k.as_str()) {
                return Err(deny(format!("{at}.{k}"), UNSUPPORTED));
            }
        }
        Ok(())
    };
    allowed(value, path, &["resources", "replicas", "restart_policy"])?;
    let d = object(value, path)?;
    if let Some(r) = d.get("resources") {
        let at = format!("{path}.resources");
        allowed(r, &at, &["limits", "reservations"])?;
        let r = object(r, &at)?;
        if let Some(l) = r.get("limits") {
            allowed(l, &format!("{at}.limits"), &["cpus", "memory", "pids"])?;
        }
        if let Some(l) = r.get("reservations") {
            allowed(l, &format!("{at}.reservations"), &["cpus", "memory"])?;
        }
    }
    if let Some(n) = d.get("replicas") {
        replicas(n, &format!("{path}.replicas"))?;
    }
    if let Some(p) = d.get("restart_policy") {
        allowed(
            p,
            &format!("{path}.restart_policy"),
            &["condition", "delay", "max_attempts", "window"],
        )?;
    }
    Ok(())
}

/// Volumes du projet : nommes, sans pilote ni options (un pilote `local`
/// avec `o: bind` monterait un chemin de l'hote), jamais externes. Forme
/// normalisee : `name` vaut `<projet>_<cle>` (pose par Compose).
fn top_volumes(file: &ComposeFile, project: Option<&str>, value: &Value) -> Checked {
    for (key, v) in object(value, "volumes")? {
        let at = format!("volumes.{key}");
        if !named_volume(key) {
            return Err(deny(at, "nom de volume invalide"));
        }
        if v.is_null() {
            continue;
        }
        for (k, val) in object(v, &at)? {
            match k.as_str() {
                "labels" => {}
                "external" if is_empty(val) => {}
                "name"
                    if file.normalized
                        && project.is_some_and(|p| val.as_str() == Some(&format!("{p}_{key}"))) => {
                }
                "external" => return Err(deny(format!("{at}.external"), "volume externe")),
                "driver" | "driver_opts" => {
                    return Err(deny(
                        format!("{at}.{k}"),
                        "pilote de volume (chemin de l'hote)",
                    ));
                }
                "name" => return Err(deny(format!("{at}.name"), "volume hors du projet")),
                other => return Err(deny(format!("{at}.{other}"), UNSUPPORTED)),
            }
        }
    }
    Ok(())
}

/// Forme normalisee : seulement le reseau `default` cree par Compose.
fn top_networks(project: Option<&str>, value: &Value) -> Checked {
    for (key, v) in object(value, "networks")? {
        let at = format!("networks.{key}");
        if key != "default" {
            return Err(deny(at, "reseau declare"));
        }
        if v.is_null() {
            continue;
        }
        for (k, val) in object(v, &at)? {
            let ok = match k.as_str() {
                "name" => project.is_some_and(|p| val.as_str() == Some(&format!("{p}_default"))),
                "ipam" | "external" | "labels" => is_empty(val) || k == "labels",
                "driver" => val.as_str() == Some("bridge"),
                _ => false,
            };
            if !ok {
                return Err(deny(format!("{at}.{k}"), "reseau modifie"));
            }
        }
    }
    Ok(())
}
