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
    LOG_DRIVER, LOG_MAX_FILE, LOG_MAX_SIZE, MANAGED_LABEL, SPEC_LABEL, internal_alias,
    project_network, scope_labels,
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
