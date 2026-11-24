//! Deploiement d'un projet Compose.
//!
//! L'ensemble vient du build (`build::compose::bundle` : forme normalisee
//! par `docker compose config`, images construites a la place des sections
//! `build`), garde dans l'etat local par deploiement (rollback compris). Au
//! deploiement, `ComposePlan` le reverifie (`compose_policy`) et le reecrit :
//!
//! - chaque service est force sur le reseau du projet : le reseau `default`
//!   devient `ciphera-p-<project_id>` (externe, cree par l'agent) et toute
//!   section `networks` d'un service est remplacee par `default` seul, avec
//!   l'alias `<service>.internal` (la politique refuse deja les reseaux
//!   declares) ;
//! - etiquettes de Ciphera (gere, projet, projet Compose ; deploiement pour
//!   le service expose), `no-new-privileges` partout ;
//! - journaux bornes (`logging` : meme rotation que les autres conteneurs,
//!   `docker::LOG_*`) ;
//! - `env_file` retire (deja fusionne dans `environment` par la
//!   normalisation ; le depot n'existe plus) ;
//! - variables de Ciphera (`DeploySpec.env`) injectees dans chaque service,
//!   prioritaires sur celles du fichier : le fichier ne porte que des
//!   references `${CIPHERA_VAR_<CLE>}` (aucune valeur sur le disque) ; les
//!   valeurs passent par l'environnement du seul processus `docker compose`,
//!   sous le prefixe `ENV_PREFIX` (une cle choisie par l'utilisateur, comme
//!   `DOCKER_HOST` ou `LD_PRELOAD`, ne change jamais le comportement de la
//!   commande).
//!
//! Les services non exposes tournent dans le projet Compose stable de
//! l'environnement (nom normalise, `<projet>-<env>`) et sont relances un par
//! un dans l'ordre de `depends_on` (Compose ne recree que ceux dont la
//! configuration a change : nouvelle image construite, par exemple). Le
//! service expose tourne dans un projet Compose propre au deploiement
//! (`ciphera-d-<deployment_id>`) : le nouveau demarre a cote de l'ancien,
//! passe son healthcheck, puis les routes basculent (voir `deploy`).
//!
//! `docker compose` est lance sans shell : binaire fixe, arguments valides
//! (`argv`).

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use proto::EnvVar;
use serde_json::{Map, Value, json};
use shared::ids::{DeploymentId, ProjectId};

use crate::build::argv::{self, AllowedBin, ValidatedArg};
use crate::build::compose::ComposeFile;
use crate::build::{BuildBackend, ProgressSink, compose_policy};
use crate::docker::{
    APP_CAP_DROP, APP_PIDS_LIMIT, APP_TMP_TMPFS, AppLimits, LOG_DRIVER, LOG_MAX_FILE, LOG_MAX_SIZE,
    MANAGED_LABEL, SPEC_LABEL, internal_alias, project_network, scope_labels,
};

/// Etiquettes posees par Compose sur ses conteneurs.
pub const COMPOSE_PROJECT_LABEL: &str = "com.docker.compose.project";
pub const COMPOSE_SERVICE_LABEL: &str = "com.docker.compose.service";
pub use crate::docker::{COMPOSE_LABEL, PROJECT_LABEL};
/// Prefixe du projet Compose du service expose d'un deploiement.
pub const EXPOSED_PROJECT_PREFIX: &str = "ciphera-d-";
/// Prefixe des variables de Ciphera dans l'environnement de `docker compose`.
pub const ENV_PREFIX: &str = "CIPHERA_VAR_";

/// Plan d'un deploiement Compose.
#[derive(Clone, Debug)]
pub struct ComposePlan {
    /// Projet Compose stable (services non exposes).
    pub stable: ValidatedArg,
    /// Projet Compose du service expose de ce deploiement.
    pub exposed_project: ValidatedArg,
    pub exposed: ValidatedArg,
    /// Services non exposes, dans l'ordre de `depends_on`.
    pub order: Vec<ValidatedArg>,
    /// Fichier a donner a `docker compose` (JSON).
    pub file: Value,
}

fn err(m: impl Into<String>) -> String {
    m.into()
}

/// Taille maximale d'un `tmpfs` et de `shm_size`, en octets.
const MAX_TMPFS_BYTES: u64 = 256 << 20;

/// Octets d'une taille Compose : entier, ou texte `<n>[b|k|m|g][b]`.
fn parse_bytes(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => {
            let s = s.trim().to_ascii_lowercase();
            let digits = s.bytes().take_while(u8::is_ascii_digit).count();
            let n: u64 = s[..digits].parse().ok()?;
            let shift = match s[digits..].trim_end_matches('b') {
                "" => 0,
                "k" => 10,
                "m" => 20,
                "g" => 30,
                _ => return None,
            };
            n.checked_mul(1 << shift)
        }
        _ => None,
    }
}

/// Taille bornee a `MAX_TMPFS_BYTES` ; absente, nulle ou illisible : le
/// plafond (0 voudrait dire « sans limite » pour un `tmpfs`).
fn bounded_size(v: Option<&Value>) -> u64 {
    v.and_then(parse_bytes)
        .filter(|b| *b > 0)
        .map_or(MAX_TMPFS_BYTES, |b| b.min(MAX_TMPFS_BYTES))
}

/// Plus petite des valeurs strictement positives.
fn min_positive<const N: usize>(values: [Option<f64>; N]) -> Option<f64> {
    values
        .into_iter()
        .flatten()
        .filter(|v| *v > 0.0)
        .min_by(f64::total_cmp)
}

fn as_f64(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Option `size=` d'un `tmpfs` en forme courte (`/chemin[:opt,opt]`), bornee
/// a `MAX_TMPFS_BYTES` et imposee si absente.
fn bound_tmpfs_entry(entry: &str) -> String {
    let (path, opts) = entry.split_once(':').unwrap_or((entry, ""));
    let mut size = MAX_TMPFS_BYTES;
    let mut kept: Vec<String> = Vec::new();
    for o in opts.split(',').filter(|o| !o.is_empty()) {
        match o.strip_prefix("size=") {
            Some(v) => {
                size = bounded_size(Some(&Value::String(v.to_string())));
            }
            None => kept.push(o.to_string()),
        }
    }
    kept.push(format!("size={size}"));
    format!("{path}:{}", kept.join(","))
}

/// Memes limites que les conteneurs d'application simples, par service :
/// processus (`APP_PIDS_LIMIT`), capacites `APP_CAP_DROP` retirees,
/// memoire et CPU (la valeur demandee si elle est plus basse que le plafond
/// du noeud), repliques (une pour le service expose), `tmpfs` et `shm_size`
/// bornes. Chaque limite n'a qu'une valeur, au niveau du service : Compose
/// refuse la meme limite en `deploy.resources.limits` avec une autre valeur.
fn bound_service(
    name: &str,
    svc: &mut Map<String, Value>,
    limits: AppLimits,
    exposed: bool,
) -> Result<(), String> {
    // Repliques : au plus 4 deja garanti par la politique ; une seule pour
    // le service expose (le proxy route vers un conteneur par deploiement).
    let scale = svc.get("scale").and_then(compose_policy::replica_count);
    let deployed = svc
        .get("deploy")
        .and_then(|d| d.get("replicas"))
        .and_then(compose_policy::replica_count);
    if exposed && scale.into_iter().chain(deployed).any(|n| n != 1) {
        return Err(format!(
            "service expose {name} : une seule replique (scale, deploy.replicas)"
        ));
    }

    let mut deployed_limits = Map::new();
    if let Some(Value::Object(deploy)) = svc.get_mut("deploy")
        && let Some(Value::Object(resources)) = deploy.get_mut("resources")
        && let Some(Value::Object(l)) = resources.remove("limits")
    {
        deployed_limits = l;
    }
    // -1 (illimite) et 0 : le plafond.
    let pids = min_positive([
        as_f64(svc.get("pids_limit")),
        as_f64(deployed_limits.get("pids")),
        Some(APP_PIDS_LIMIT as f64),
    ])
    .map_or(APP_PIDS_LIMIT, |p| p as i64);
    svc.insert("pids_limit".into(), json!(pids));

    let cap_mem = limits.memory;
    let memory = [svc.get("mem_limit"), deployed_limits.get("memory")]
        .into_iter()
        .flatten()
        .filter_map(parse_bytes)
        .filter(|m| *m > 0)
        .fold(cap_mem, u64::min);
    svc.insert("mem_limit".into(), json!(memory));
    // Sans swap en plus de la memoire.
    svc.insert("memswap_limit".into(), json!(memory));
    // Une reservation au-dessus de la limite fait refuser la creation.
    let over = |v: &Value| parse_bytes(v).is_some_and(|r| r > memory);
    if svc.get("mem_reservation").is_some_and(over) {
        svc.insert("mem_reservation".into(), json!(memory));
    }
    if let Some(Value::Object(deploy)) = svc.get_mut("deploy")
        && let Some(Value::Object(resources)) = deploy.get_mut("resources")
        && let Some(Value::Object(reserved)) = resources.get_mut("reservations")
        && reserved.get("memory").is_some_and(over)
    {
        reserved.insert("memory".into(), json!(memory));
    }

    let cap_cpus = limits.nano_cpus as f64 / 1e9;
    let cpus = min_positive([
        as_f64(svc.get("cpus")),
        as_f64(deployed_limits.get("cpus")),
        Some(cap_cpus),
    ])
    .unwrap_or(cap_cpus)
    .min(cap_cpus);
    svc.insert("cpus".into(), json!(cpus));

    // `deploy` ne garde que ce qui n'a pas ete deplace au niveau du service.
    if let Some(Value::Object(deploy)) = svc.get_mut("deploy") {
        let empty = deploy
            .get("resources")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty);
        if empty {
            deploy.remove("resources");
        }
    }
    if svc
        .get("deploy")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        svc.remove("deploy");
    }

    let drop = svc
        .entry("cap_drop")
        .or_insert_with(|| Value::Array(Vec::new()));
    if drop.is_null() {
        *drop = Value::Array(Vec::new());
    }
    let drop = drop
        .as_array_mut()
        .ok_or_else(|| format!("service {name} : cap_drop invalide"))?;
    for cap in APP_CAP_DROP {
        if !drop
            .iter()
            .any(|c| c.as_str().is_some_and(|c| c.eq_ignore_ascii_case(cap)))
        {
            drop.push(cap.into());
        }
    }

    if let Some(shm) = svc.get("shm_size") {
        let bytes = bounded_size(Some(shm));
        svc.insert("shm_size".into(), json!(bytes));
    }
    match svc.get("tmpfs") {
        None | Some(Value::Null) => {}
        Some(Value::String(one)) => {
            let bounded = bound_tmpfs_entry(one);
            svc.insert("tmpfs".into(), json!([bounded]));
        }
        Some(Value::Array(items)) => {
            let bounded = items
                .iter()
                .map(|i| {
                    i.as_str()
                        .map(|s| Value::String(bound_tmpfs_entry(s)))
                        .ok_or_else(|| format!("service {name} : tmpfs invalide"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            svc.insert("tmpfs".into(), Value::Array(bounded));
        }
        Some(_) => return Err(format!("service {name} : tmpfs invalide")),
    }
    if let Some(Value::Array(volumes)) = svc.get_mut("volumes") {
        for v in volumes {
            let Some(vol) = v.as_object_mut() else {
                continue;
            };
            if vol.get("type").and_then(Value::as_str) != Some("tmpfs") {
                continue;
            }
            let tmpfs = vol
                .entry("tmpfs")
                .or_insert_with(|| Value::Object(Map::new()));
            if tmpfs.is_null() {
                *tmpfs = Value::Object(Map::new());
            }
            let tmpfs = tmpfs
                .as_object_mut()
                .ok_or_else(|| format!("service {name} : tmpfs invalide"))?;
            let size = bounded_size(tmpfs.get("size"));
            tmpfs.insert("size".into(), json!(size));
        }
    }
    Ok(())
}

/// Dependances d'un service (`depends_on` en liste ou en table).
fn depends_on(svc: &Value) -> Result<Vec<String>, String> {
    match svc.get("depends_on") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or_else(|| err("depends_on invalide"))
            })
            .collect(),
        Some(Value::Object(map)) => Ok(map.keys().cloned().collect()),
        Some(_) => Err(err("depends_on invalide")),
    }
}

/// Ordre de demarrage (dependances d'abord ; a egalite, ordre alphabetique).
/// Dependance inconnue ou cycle : refuse.
pub fn start_order(services: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut deps: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for (name, svc) in services {
        let d: BTreeSet<String> = depends_on(svc)?.into_iter().collect();
        if let Some(unknown) = d.iter().find(|x| !services.contains_key(x.as_str())) {
            return Err(format!("{name} depend d'un service inconnu : {unknown}"));
        }
        deps.insert(name.as_str(), d);
    }
    let mut order = Vec::with_capacity(deps.len());
    while !deps.is_empty() {
        let ready: Vec<&str> = deps
            .iter()
            .filter(|(_, d)| d.is_empty())
            .map(|(n, _)| *n)
            .collect();
        if ready.is_empty() {
            return Err(err("cycle dans depends_on"));
        }
        for name in ready {
            deps.remove(name);
            for d in deps.values_mut() {
                d.remove(name);
            }
            order.push(name.to_string());
        }
    }
    Ok(order)
}

impl ComposePlan {
    /// Plan du deploiement `deployment` du projet `project`, service expose
    /// `exposed`, depuis l'ensemble Compose `bundle`.
    pub fn new(
        bundle: &Value,
        exposed: &str,
        project: ProjectId,
        deployment: DeploymentId,
    ) -> Result<Self, String> {
        Self::with_limits(bundle, exposed, project, deployment, AppLimits::node())
    }

    /// Comme `new`, avec les plafonds de ressources `limits` (ceux du noeud
    /// en production).
    pub fn with_limits(
        bundle: &Value,
        exposed: &str,
        project: ProjectId,
        deployment: DeploymentId,
        limits: AppLimits,
    ) -> Result<Self, String> {
        let stable_name = bundle
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| err("ensemble Compose sans nom de projet"))?;
        let stable = ValidatedArg::name(stable_name).map_err(|e| e.to_string())?;
        let exposed_arg = ValidatedArg::compose_service(exposed).map_err(|e| e.to_string())?;
        let exposed_project = ValidatedArg::name(&format!("{EXPOSED_PROJECT_PREFIX}{deployment}"))
            .map_err(|e| e.to_string())?;
        // Meme politique qu'au build (ensemble modifie entre-temps ?).
        let checked = ComposeFile {
            doc: bundle.clone(),
            normalized: true,
            root: PathBuf::from("/nonexistent"),
            base: PathBuf::from("/nonexistent"),
        };
        compose_policy::check(&checked).map_err(|v| v.to_string())?;

        let mut file = bundle.clone();
        let doc = file
            .as_object_mut()
            .ok_or_else(|| err("ensemble Compose invalide"))?;
        doc.remove("name");
        doc.insert(
            "networks".into(),
            json!({ "default": { "name": project_network(project), "external": true } }),
        );
        let services = doc
            .get_mut("services")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| err("ensemble Compose sans services"))?;
        if !services.contains_key(exposed) {
            return Err(format!("service expose {exposed:?} absent de l'ensemble"));
        }
        let order = start_order(services)?;
        for (name, svc) in services.iter_mut() {
            let svc = svc
                .as_object_mut()
                .ok_or_else(|| format!("service {name} invalide"))?;
            if svc.contains_key("build") || !svc.get("image").is_some_and(Value::is_string) {
                return Err(format!("service {name} : image construite absente"));
            }
            // Reseau `default` seul, avec l'alias `<service>.internal` (le
            // nom nu reste l'alias pose par Compose).
            svc.remove("networks");
            if let Some(alias) = internal_alias(name) {
                svc.insert(
                    "networks".into(),
                    json!({ "default": { "aliases": [alias] } }),
                );
            }
            svc.remove("env_file");
            // Rotation des journaux (la politique refuse `logging`).
            svc.insert(
                "logging".into(),
                json!({
                    "driver": LOG_DRIVER,
                    "options": { "max-size": LOG_MAX_SIZE, "max-file": LOG_MAX_FILE },
                }),
            );
            let labels = svc
                .entry("labels")
                .or_insert_with(|| Value::Object(Map::new()));
            if labels.is_null() {
                *labels = Value::Object(Map::new());
            }
            let labels = labels
                .as_object_mut()
                .ok_or_else(|| format!("service {name} : etiquettes invalides"))?;
            // Toutes les etiquettes reservees, vides si sans objet (le
            // service non expose n'a pas de deploiement) : aucune ne peut
            // venir de l'image.
            labels.insert(MANAGED_LABEL.into(), "true".into());
            labels.insert(SPEC_LABEL.into(), "".into());
            let owner = (name == exposed).then_some(deployment);
            for (key, value) in scope_labels(owner, stable_name, Some(project)) {
                labels.insert(key.into(), value.into());
            }
            let opts = svc
                .entry("security_opt")
                .or_insert_with(|| Value::Array(Vec::new()));
            let opts = opts
                .as_array_mut()
                .ok_or_else(|| format!("service {name} : security_opt invalide"))?;
            if !opts.iter().any(|o| {
                o.as_str()
                    .is_some_and(|o| o.starts_with("no-new-privileges"))
            }) {
                opts.push("no-new-privileges:true".into());
            }
            bound_service(name, svc, limits, name == exposed)?;
        }
        let order = order
            .into_iter()
            .filter(|s| s != exposed)
            .map(|s| ValidatedArg::compose_service(&s).map_err(|e| e.to_string()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            stable,
            exposed_project,
            exposed: exposed_arg,
            order,
            file,
        })
    }
}

impl ComposePlan {
    /// Reglages de securite du service expose (les autres gardent le `user`
    /// et le `read_only` de leur fichier Compose) : utilisateur `run_as`
    /// (`uid[:gid]`, deja valide) et racine en lecture seule avec un `tmpfs`
    /// sur `/tmp` (`APP_TMP_TMPFS`, sauf si le fichier en declare un).
    pub fn secure_exposed(
        &mut self,
        run_as: Option<&str>,
        read_only_rootfs: bool,
    ) -> Result<(), String> {
        if run_as.is_none() && !read_only_rootfs {
            return Ok(());
        }
        if run_as.is_some_and(|u| !proto::valid_run_as(u)) {
            return Err("utilisateur (run_as) invalide".into());
        }
        let name = self.exposed.as_str().to_string();
        let svc = self
            .file
            .get_mut("services")
            .and_then(|s| s.get_mut(&name))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("service expose {name:?} absent du plan"))?;
        if let Some(user) = run_as {
            svc.insert("user".into(), json!(user));
        }
        if read_only_rootfs {
            svc.insert("read_only".into(), json!(true));
            let tmp = format!("/tmp:{APP_TMP_TMPFS}");
            let declares_tmp = |v: &Value| {
                v.as_str()
                    .is_some_and(|s| s.split(':').next() == Some("/tmp"))
            };
            let mut entries = match svc.remove("tmpfs") {
                Some(Value::Array(a)) => a,
                Some(Value::String(s)) => vec![Value::String(s)],
                _ => Vec::new(),
            };
            let volume_tmp = svc
                .get("volumes")
                .and_then(Value::as_array)
                .is_some_and(|vs| {
                    vs.iter()
                        .any(|v| v.get("target").and_then(Value::as_str) == Some("/tmp"))
                });
            if !volume_tmp && !entries.iter().any(declares_tmp) {
                entries.push(Value::String(tmp));
            }
            if !entries.is_empty() {
                svc.insert("tmpfs".into(), Value::Array(entries));
            }
        }
        Ok(())
    }
}

impl ComposePlan {
    /// Variables `keys` (cles deja validees) dans chaque service : reference
    /// `${CIPHERA_VAR_<CLE>}`, qui remplace une valeur du fichier de meme nom.
    pub fn inject_env<'a>(
        &mut self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), String> {
        let keys: Vec<&str> = keys.into_iter().collect();
        if keys.is_empty() {
            return Ok(());
        }
        let services = self
            .file
            .get_mut("services")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| err("ensemble Compose sans services"))?;
        for (name, svc) in services.iter_mut() {
            let svc = svc
                .as_object_mut()
                .ok_or_else(|| format!("service {name} invalide"))?;
            let mut env = match svc.remove("environment") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(m)) => m,
                // Forme liste : `CLE=valeur` ou `CLE` seule.
                Some(Value::Array(items)) => {
                    let mut m = Map::new();
                    for item in items {
                        let item = item
                            .as_str()
                            .ok_or_else(|| format!("service {name} : environment invalide"))?;
                        match item.split_once('=') {
                            Some((k, v)) => m.insert(k.to_string(), Value::String(v.to_string())),
                            None => m.insert(item.to_string(), Value::Null),
                        };
                    }
                    m
                }
                Some(_) => return Err(format!("service {name} : environment invalide")),
            };
            for key in &keys {
                env.insert(
                    (*key).to_string(),
                    Value::String(format!("${{{ENV_PREFIX}{key}}}")),
                );
            }
            svc.insert("environment".into(), Value::Object(env));
        }
        Ok(())
    }
}

/// `docker compose -p <projet> -f <fichier> up -d --no-deps --no-build
/// --quiet-pull <service>`, HOME dans le dossier du deploiement, et les
/// valeurs de `env` sous `ENV_PREFIX` (jamais dans un fichier ni un journal :
/// `argv::describe` ne montre pas l'environnement).
pub fn up_command(
    project: &ValidatedArg,
    file: &ValidatedArg,
    service: &ValidatedArg,
    home: &ValidatedArg,
    env: &[EnvVar],
) -> Command {
    let mut cmd = argv::command(
        AllowedBin::DockerCompose,
        &[
            ValidatedArg::lit("-p"),
            project.clone(),
            ValidatedArg::lit("-f"),
            file.clone(),
            ValidatedArg::lit("up"),
            ValidatedArg::lit("-d"),
            ValidatedArg::lit("--no-deps"),
            ValidatedArg::lit("--no-build"),
            ValidatedArg::lit("--quiet-pull"),
            service.clone(),
        ],
    );
    cmd.env("HOME", home.as_str());
    for v in env {
        cmd.env(format!("{ENV_PREFIX}{}", v.key), v.value.expose());
    }
    cmd
}

/// Lancement d'un service Compose. `CliCompose` en production ; simule dans
/// les tests.
pub trait ComposeRunner: Send + Sync + 'static {
    /// `docker compose ... up` d'un service, variables `env` dans
    /// l'environnement du processus ; sorties vers `sink`.
    fn up(
        &self,
        project: &ValidatedArg,
        file: &ValidatedArg,
        service: &ValidatedArg,
        home: &ValidatedArg,
        env: &[EnvVar],
        sink: &ProgressSink,
    ) -> impl Future<Output = Result<(), String>> + Send;
}

/// `docker compose` en sous-processus, par l'executeur du build (delai,
/// journaux, processus tue si la tache est annulee).
pub struct CliCompose<B>(pub Arc<B>);

impl<B: BuildBackend + 'static> ComposeRunner for CliCompose<B> {
    async fn up(
        &self,
        project: &ValidatedArg,
        file: &ValidatedArg,
        service: &ValidatedArg,
        home: &ValidatedArg,
        env: &[EnvVar],
        sink: &ProgressSink,
    ) -> Result<(), String> {
        self.0
            .exec(
                AllowedBin::DockerCompose,
                up_command(project, file, service, home, env),
                sink,
                false,
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
