//! Dockerfile du depot, construit par le frontend `dockerfile.v0` de
//! BuildKit. Le chemin est resolu apres le clone (liens symboliques compris)
//! et doit rester dans le depot ; le contexte est la racine du depot (ou le
//! contexte du service, pour Compose).
//!
//! `buildctl ... build --frontend dockerfile.v0 --local context=<contexte>
//!  --local dockerfile=<dossier du Dockerfile> --opt filename=<nom>
//!  [--opt target=<etape>]
//!  --opt build-arg:BUILDKIT_CACHE_MOUNT_NS=<UUID projet>-<UUID environnement>
//!  --opt build-arg:cache-key=<UUID projet>-<UUID environnement>
//!  --output type=docker,name=...`
//!
//! Cloisonnement des caches : chaque `RUN --mount=type=cache` (avec ou sans
//! `id`) est range sous les UUID du projet et de l'environnement.
//! `cache-key` suit le Dockerfile s'il est confie au frontend Railpack : ses
//! caches restent alors sous ces memes UUID. La directive `syntax` ne peut demander que
//! `docker/dockerfile` 1.4 ou plus, sans empreinte (`check_syntax`) ;
//! buildkitd n'accepte de son cote que ce depot et le frontend Railpack
//! epingle (`infra::buildkitd_config`).

use std::path::Path;

use super::argv::ValidatedArg;
use super::{
    BuildBackend, BuildError, CacheScope, ProgressSink, Staged, Targets, Workdir, buildctl_command,
    confine,
};

/// Plus grand Dockerfile accepte (lu pour sa directive `syntax`).
const MAX_DOCKERFILE_BYTES: u64 = 16 * 1024 * 1024;
/// Seul frontend qu'un Dockerfile peut demander par sa directive `syntax`
/// (nom normalise), en plus du frontend `dockerfile.v0` integre.
pub const DOCKERFILE_FRONTEND: &str = "docker.io/docker/dockerfile";
/// Plus ancienne version `1.x` acceptee de `docker/dockerfile` : une
/// version ancienne pourrait ignorer `BUILDKIT_CACHE_MOUNT_NS`.
pub const MIN_DOCKERFILE_MINOR: u32 = 4;
/// Longueur maximale de la valeur reprise dans un message.
const MAX_SHOWN: usize = 200;

/// Dockerfile resolu : contexte, dossier et nom du fichier.
#[derive(Clone, Debug)]
pub struct DockerfileTarget {
    pub context: ValidatedArg,
    pub dir: ValidatedArg,
    pub filename: ValidatedArg,
    pub target: Option<ValidatedArg>,
}

/// Dossier et nom de fichier d'un chemin deja confine (`a/b/Dockerfile`).
pub(crate) fn split_file(path: &ValidatedArg) -> Result<(ValidatedArg, ValidatedArg), BuildError> {
    let (dir, file) = path
        .as_str()
        .rsplit_once('/')
        .ok_or_else(|| BuildError::Path("Dockerfile sans dossier".into()))?;
    Ok((ValidatedArg::path(dir)?, ValidatedArg::path(file)?))
}

/// Dockerfile `user_path` (relatif a `src`, le depot ou son dossier
/// racine), contexte a `src`.
pub fn resolve(
    src: &ValidatedArg,
    user_path: &ValidatedArg,
) -> Result<DockerfileTarget, BuildError> {
    let src = src.as_str();
    let root_canon = std::fs::canonicalize(src)
        .map_err(|e| BuildError::Path(format!("depot introuvable : {e}")))?;
    let (file, canon) = confine(src, &root_canon, &Path::new(src).join(user_path.as_str()))?;
    if !canon.is_file() {
        return Err(BuildError::Path(
            "le Dockerfile n'est pas un fichier".into(),
        ));
    }
    let (dir, filename) = split_file(&file)?;
    Ok(DockerfileTarget {
        context: ValidatedArg::path(src)?,
        dir,
        filename,
        target: None,
    })
}

/// Valeurs de la directive `syntax` d'un Dockerfile, sous toutes les formes
/// que lit BuildKit (`# syntax=`, `// syntax=`, objet JSON `{"syntax": …}`,
/// apres un BOM ou une ligne `#!`). Lecture plus large que celle de
/// BuildKit (lignes vides, espaces en tete, majuscules, toutes les
/// occurrences) : un doute fait refuser, jamais accepter.
fn syntax_directives(text: &str) -> Vec<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) {
        for (k, v) in &map {
            if k.trim().eq_ignore_ascii_case("syntax") {
                out.push(v.as_str().unwrap_or_default().trim().to_string());
            }
        }
    }
    for line in text.split(['\n', '\r']) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(body) = line.strip_prefix("//").or_else(|| line.strip_prefix('#')) else {
            // Premiere instruction : BuildKit ne lit plus de directive.
            break;
        };
        if let Some((key, value)) = body.split_once('=')
            && key.trim().eq_ignore_ascii_case("syntax")
        {
            let first = value.split_whitespace().next().unwrap_or_default();
            out.push(first.to_string());
        }
    }
    out
}

/// Nom normalise d'une reference d'image sans etiquette ni empreinte
/// (`docker/dockerfile` -> `docker.io/docker/dockerfile`).
fn normalize_repository(name: &str) -> String {
    match name.split_once('/') {
        Some((domain, rest))
            if domain.contains('.') || domain.contains(':') || domain == "localhost" =>
        {
            let domain = if domain == "index.docker.io" {
                "docker.io"
            } else {
                domain
            };
            format!("{domain}/{rest}")
        }
        Some(_) => format!("docker.io/{name}"),
        None => format!("docker.io/library/{name}"),
    }
}

/// Etiquette acceptee de `docker/dockerfile` : aucune, `latest`, `1`,
/// `1.N` ou `1.N.M` avec `N >= MIN_DOCKERFILE_MINOR`.
fn allowed_tag(tag: Option<&str>) -> bool {
    let Some(tag) = tag else {
        return true;
    };
    if tag == "latest" || tag == "1" {
        return true;
    }
    let digits = |p: &str| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit());
    let mut parts = tag.split('.');
    let (Some("1"), Some(minor)) = (parts.next(), parts.next()) else {
        return false;
    };
    let patch_ok = match parts.next() {
        None => true,
        Some(patch) => digits(patch) && parts.next().is_none(),
    };
    patch_ok
        && digits(minor)
        && minor
            .parse::<u32>()
            .is_ok_and(|m| m >= MIN_DOCKERFILE_MINOR)
}

fn check_frontend(value: &str) -> Result<(), BuildError> {
    let refused = || {
        BuildError::Frontend(format!(
            "directive syntax={:?} : seul docker/dockerfile (1, ou 1.{MIN_DOCKERFILE_MINOR} et plus, \
             sans empreinte) est accepte ; retirer la ligne pour utiliser le frontend integre",
            super::truncate(value, MAX_SHOWN)
        ))
    };
    // Une empreinte l'emporte sur l'etiquette : elle designerait n'importe
    // quelle version.
    if value.is_empty() || value.contains('@') {
        return Err(refused());
    }
    let last_slash = value.rfind('/').map_or(0, |i| i + 1);
    let (name, tag) = match value[last_slash..].rfind(':') {
        Some(i) => (&value[..last_slash + i], Some(&value[last_slash + i + 1..])),
        None => (value, None),
    };
    if normalize_repository(name) != DOCKERFILE_FRONTEND || !allowed_tag(tag) {
        return Err(refused());
    }
    Ok(())
}

/// Refuse un Dockerfile dont la directive `syntax` demande un autre
/// frontend que `docker/dockerfile` (1.4 ou plus, sans empreinte) : un
/// frontend arbitraire choisirait lui-meme ses identifiants de cache.
pub fn check_syntax(text: &str) -> Result<(), BuildError> {
    for value in syntax_directives(text) {
        check_frontend(&value)?;
    }
    Ok(())
}

/// Lit le Dockerfile de `t` (deja confine au depot) et verifie sa directive
/// `syntax`.
fn check_dockerfile(t: &DockerfileTarget) -> Result<(), BuildError> {
    let path = Path::new(t.dir.as_str()).join(t.filename.as_str());
    let unreadable = |e: std::io::Error| BuildError::Path(format!("Dockerfile illisible : {e}"));
    let meta = std::fs::metadata(&path).map_err(unreadable)?;
    if !meta.is_file() || meta.len() > MAX_DOCKERFILE_BYTES {
        return Err(BuildError::Path(
            "Dockerfile absent ou trop grand (16 Mio au plus)".into(),
        ));
    }
    let bytes = std::fs::read(&path).map_err(unreadable)?;
    check_syntax(&String::from_utf8_lossy(&bytes))
}

/// Arguments du frontend `dockerfile.v0` pour `buildctl`, caches ranges
/// sous `cache`.
pub fn frontend_args(t: &DockerfileTarget, cache: &CacheScope) -> Vec<ValidatedArg> {
    let mut args = vec![
        ValidatedArg::lit("--frontend"),
        ValidatedArg::lit("dockerfile.v0"),
        ValidatedArg::lit("--local"),
        ValidatedArg::concat(&[&ValidatedArg::lit("context="), &t.context]),
        ValidatedArg::lit("--local"),
        ValidatedArg::concat(&[&ValidatedArg::lit("dockerfile="), &t.dir]),
        ValidatedArg::lit("--opt"),
        ValidatedArg::concat(&[&ValidatedArg::lit("filename="), &t.filename]),
    ];
    if let Some(target) = &t.target {
        args.push(ValidatedArg::lit("--opt"));
        args.push(ValidatedArg::concat(&[
            &ValidatedArg::lit("target="),
            target,
        ]));
    }
    args.extend([
        ValidatedArg::lit("--opt"),
        ValidatedArg::concat(&[
            &ValidatedArg::lit("build-arg:BUILDKIT_CACHE_MOUNT_NS="),
            &cache.key,
        ]),
        ValidatedArg::lit("--opt"),
        ValidatedArg::concat(&[&ValidatedArg::lit("build-arg:cache-key="), &cache.key]),
    ]);
    args
}

/// Construit `t` (caches sous `cache`) et le charge sous le nom provisoire
/// `staging`. Directive `syntax` verifiee avant tout appel a BuildKit.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn build_target<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    workdir: &Workdir,
    socket: &ValidatedArg,
    t: &DockerfileTarget,
    cache: &CacheScope,
    staging: &ValidatedArg,
) -> Result<proto::ImageRef, BuildError> {
    check_dockerfile(t)?;
    let home = ValidatedArg::path(&workdir.home())?;
    let cmd = buildctl_command(socket, frontend_args(t, cache), staging, &home);
    let staging = proto::ImageRef(staging.as_str().to_string());
    backend.build_and_load(cmd, &staging, sink).await?;
    Ok(staging)
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
    staged: &mut Vec<Staged>,
) -> Result<(), BuildError> {
    let t = resolve(src, user_path)?;
    let cache = targets.cache_scope()?;
    let staging = build_target(
        backend,
        sink,
        workdir,
        socket,
        &t,
        &cache,
        &targets.staging(None),
    )
    .await?;
    staged.push(Staged {
        staging,
        target: targets.main_tag(),
    });
    Ok(())
}
