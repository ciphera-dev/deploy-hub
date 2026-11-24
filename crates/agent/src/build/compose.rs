//! Projet Compose : chaque service qui a une section `build` est construit
//! par BuildKit (frontend `dockerfile.v0`) et etiquete
//! `ciphera/<projet>-<env>/<service>:<deployment_id>`. `docker compose` ne
//! sert ici qu'a normaliser le fichier (`config`). Le build produit
//! l'ensemble a deployer (`bundle`), lance par `deploy::compose`.
//!
//! Ordre :
//! 1. fichier resolu dans le depot (liens symboliques compris), 1 Mio au plus ;
//! 2. `compose_policy::check` sur le fichier du depot, puis les fichiers
//!    qu'il fait lire a Compose (`env_file`, `.env`) verifies dans le depot ;
//! 3. `docker compose --project-directory <dossier> -f <fichier>
//!    -p <projet>-<env> config --format json` (environnement vide :
//!    interpolation depuis le `.env` du depot seulement) ;
//! 4. `compose_policy::check` sur la forme normalisee ; contextes et
//!    Dockerfiles resolus dans le depot ; build de chaque service.

use std::path::{Path, PathBuf};
use std::process::Command;

use proto::ImageRef;
use serde_json::Value;

use super::argv::{self, AllowedBin, ValidatedArg};
use super::compose_policy::{self, PolicyViolation, env_file_paths, lexical};
use super::dockerfile::{self, DockerfileTarget, split_file};
use super::{BuildBackend, BuildError, ProgressSink, Staged, Targets, Workdir, confine};

/// Taille maximale du fichier Compose lu.
pub const MAX_COMPOSE_BYTES: u64 = 1 << 20;

/// Document Compose (forme du depot ou normalisee) et ses dossiers : `root`
/// (racine du depot) et `base` (dossier du projet Compose), canoniques.
#[derive(Clone, Debug)]
pub struct ComposeFile {
    pub(crate) doc: Value,
    /// Sortie de `docker compose config` (reseau `default` et noms de
    /// volumes poses par Compose acceptes).
    pub(crate) normalized: bool,
    pub(crate) root: PathBuf,
    pub(crate) base: PathBuf,
}

fn parse_error(reason: &'static str) -> PolicyViolation {
    PolicyViolation {
        path: "(fichier)".into(),
        reason,
    }
}

impl ComposeFile {
    /// Fichier du depot (YAML ; cles de fusion `<<` appliquees).
    pub fn from_yaml(text: &str, root: &Path, base: &Path) -> Result<Self, PolicyViolation> {
        let mut yaml: serde_norway::Value =
            serde_norway::from_str(text).map_err(|_| parse_error("YAML illisible"))?;
        yaml.apply_merge()
            .map_err(|_| parse_error("cle de fusion YAML invalide"))?;
        let doc =
            serde_json::to_value(&yaml).map_err(|_| parse_error("YAML non pris en charge"))?;
        Ok(Self {
            doc,
            normalized: false,
            root: root.to_path_buf(),
            base: base.to_path_buf(),
        })
    }

    /// Sortie JSON de `docker compose config --format json`.
    pub fn from_normalized_json(
        bytes: &[u8],
        root: &Path,
        base: &Path,
    ) -> Result<Self, PolicyViolation> {
        let doc = serde_json::from_slice(bytes)
            .map_err(|_| parse_error("sortie de Compose illisible"))?;
        Ok(Self {
            doc,
            normalized: true,
            root: root.to_path_buf(),
            base: base.to_path_buf(),
        })
    }

    fn services(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.doc
            .get("services")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
    }
}

/// `docker compose --project-directory <base> -f <file> -p <name> config
/// --format json`, HOME dans le dossier du build.
pub fn config_command(
    base: &ValidatedArg,
    file: &ValidatedArg,
    project: &ValidatedArg,
    home: &ValidatedArg,
) -> Command {
    let mut cmd = argv::command(
        AllowedBin::DockerCompose,
        &[
            ValidatedArg::lit("--project-directory"),
            base.clone(),
            ValidatedArg::lit("-f"),
            file.clone(),
            ValidatedArg::lit("-p"),
            project.clone(),
            ValidatedArg::lit("config"),
            ValidatedArg::lit("--format"),
            ValidatedArg::lit("json"),
        ],
    );
    cmd.env("HOME", home.as_str());
    cmd
}

/// Fichiers lus par Compose a la normalisation (`env_file` des services,
/// `.env` du projet) : dans le depot, liens symboliques compris. Un fichier
/// absent n'est pas lu (Compose echoue s'il est exige).
fn check_read_files(file: &ComposeFile, src: &str) -> Result<(), BuildError> {
    let mut paths = vec![file.base.join(".env")];
    for (_, svc) in file.services() {
        if let Some(v) = svc.get("env_file") {
            for p in env_file_paths(v).unwrap_or_default() {
                paths.push(lexical(&file.base, p));
            }
        }
    }
    for p in paths {
        if std::fs::symlink_metadata(&p).is_err() {
            continue;
        }
        confine(src, &file.root, &p)?;
    }
    Ok(())
}

/// Service construit : nom et Dockerfile resolu.
struct ServiceBuild {
    name: ValidatedArg,
    target: DockerfileTarget,
}

fn service_builds(file: &ComposeFile, src: &str) -> Result<Vec<ServiceBuild>, BuildError> {
    let mut out = Vec::new();
    for (name, svc) in file.services() {
        let Some(build) = svc.get("build") else {
            continue;
        };
        let name = ValidatedArg::name(name).map_err(|_| {
            BuildError::Compose(format!(
                "service {name:?} : nom de service construit hors de [a-z0-9-]"
            ))
        })?;
        let (context, dockerfile, target) = match build {
            Value::String(c) => (c.as_str(), "Dockerfile", None),
            Value::Object(b) => (
                b.get("context").and_then(Value::as_str).unwrap_or("."),
                b.get("dockerfile")
                    .and_then(Value::as_str)
                    .unwrap_or("Dockerfile"),
                b.get("target").and_then(Value::as_str),
            ),
            _ => return Err(BuildError::Compose("section build invalide".into())),
        };
        let (context, ctx_canon) = confine(src, &file.root, &lexical(&file.base, context))?;
        if !ctx_canon.is_dir() {
            return Err(BuildError::Path(
                "contexte de build : pas un dossier".into(),
            ));
        }
        let (df, df_canon) = confine(src, &file.root, &lexical(&ctx_canon, dockerfile))?;
        if !df_canon.is_file() {
            return Err(BuildError::Path(
                "le Dockerfile n'est pas un fichier".into(),
            ));
        }
        let (dir, filename) = split_file(&df)?;
        let target = target
            .map(|t| ValidatedArg::name(&t.to_ascii_lowercase()))
            .transpose()?;
        out.push(ServiceBuild {
            name,
            target: DockerfileTarget {
                context,
                dir,
                filename,
                target,
            },
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn build<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    workdir: &Workdir,
    src: &ValidatedArg,
    targets: &Targets,
    socket: &ValidatedArg,
    user_path: &ValidatedArg,
    service: &str,
    staged: &mut Vec<Staged>,
) -> Result<String, BuildError> {
    let src = src.as_str();
    let home = ValidatedArg::path(&workdir.home())?;
    let root = std::fs::canonicalize(src)
        .map_err(|e| BuildError::Path(format!("depot introuvable : {e}")))?;
    let (file_arg, file_canon) = confine(src, &root, &Path::new(src).join(user_path.as_str()))?;
    let meta = std::fs::metadata(&file_canon).map_err(|e| BuildError::Compose(e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_COMPOSE_BYTES {
        return Err(BuildError::Compose(
            "fichier absent ou trop grand (1 Mio au plus)".into(),
        ));
    }
    let (base_arg, _) = split_file(&file_arg)?;
    let base = file_canon
        .parent()
        .ok_or_else(|| BuildError::Path("dossier du fichier Compose".into()))?
        .to_path_buf();
    let text =
        std::fs::read_to_string(&file_canon).map_err(|e| BuildError::Compose(e.to_string()))?;

    let raw = ComposeFile::from_yaml(&text, &root, &base)?;
    compose_policy::check(&raw)?;
    check_read_files(&raw, src)?;

    let project = targets.cache_key();
    let json = backend
        .exec(
            AllowedBin::DockerCompose,
            config_command(&base_arg, &file_arg, &project, &home),
            sink,
            true,
        )
        .await?;
    let normalized = ComposeFile::from_normalized_json(&json, &root, &base)?;
    compose_policy::check(&normalized)?;
    if !normalized.services().any(|(name, _)| name == service) {
        return Err(BuildError::Compose(format!(
            "service expose {service:?} absent du fichier"
        )));
    }
    let mut built = Vec::new();
    for s in service_builds(&normalized, src)? {
        let staging = targets.staging(Some(&s.name));
        let staging =
            dockerfile::build_target(backend, sink, workdir, socket, &s.target, &staging).await?;
        let target = targets.service_tag(&s.name);
        built.push((s.name.as_str().to_string(), target.clone()));
        staged.push(Staged { staging, target });
    }
    Ok(bundle(&normalized, &built))
}

/// Ensemble a deployer : forme normalisee, chaque section `build` remplacee
/// par l'image construite du service, `env_file` retire (deja fusionne dans
/// `environment` ; le depot n'existera plus au deploiement).
pub fn bundle(normalized: &ComposeFile, built: &[(String, ImageRef)]) -> String {
    let mut doc = normalized.doc.clone();
    if let Some(services) = doc.get_mut("services").and_then(Value::as_object_mut) {
        for (name, svc) in services.iter_mut() {
            let Some(svc) = svc.as_object_mut() else {
                continue;
            };
            svc.remove("env_file");
            if svc.remove("build").is_some()
                && let Some((_, image)) = built.iter().find(|(n, _)| n == name)
            {
                svc.insert("image".into(), Value::String(image.0.clone()));
            }
        }
    }
    doc.to_string()
}
