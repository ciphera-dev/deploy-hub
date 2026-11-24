//! Dockerfile du depot, construit par le frontend `dockerfile.v0` de
//! BuildKit. Le chemin est resolu apres le clone (liens symboliques compris)
//! et doit rester dans le depot ; le contexte est la racine du depot (ou le
//! contexte du service, pour Compose).
//!
//! `buildctl ... build --frontend dockerfile.v0 --local context=<contexte>
//!  --local dockerfile=<dossier du Dockerfile> --opt filename=<nom>
//!  [--opt target=<etape>] --output type=docker,name=...`

use std::path::Path;

use super::argv::ValidatedArg;
use super::{
    BuildBackend, BuildError, ProgressSink, Staged, Targets, Workdir, buildctl_command, confine,
};

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

/// Arguments du frontend `dockerfile.v0` pour `buildctl`.
pub fn frontend_args(t: &DockerfileTarget) -> Vec<ValidatedArg> {
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
    args
}

/// Construit `t` et le charge sous le nom provisoire `staging`.
pub(crate) async fn build_target<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    workdir: &Workdir,
    socket: &ValidatedArg,
    t: &DockerfileTarget,
    staging: &ValidatedArg,
) -> Result<proto::ImageRef, BuildError> {
    let home = ValidatedArg::path(&workdir.home())?;
    let cmd = buildctl_command(socket, frontend_args(t), staging, &home);
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
    let staging = build_target(backend, sink, workdir, socket, &t, &targets.staging(None)).await?;
    staged.push(Staged {
        staging,
        target: targets.main_tag(),
    });
    Ok(())
}
