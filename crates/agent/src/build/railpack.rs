//! Railpack : `railpack prepare` ecrit le plan de build du depot dans un
//! conteneur jetable (voir `sandbox` : compte non root, racine en lecture
//! seule, mise epingle monte en lecture seule, pont `ciphera-prepare`), puis
//! le frontend BuildKit de Railpack (meme version, image epinglee par digest)
//! le construit par `buildctl` :
//!
//! `railpack prepare /src --plan-out /out/railpack-plan.json
//!  --info-out /out/railpack-info.json` (depot et plan montes)
//!
//! `buildctl ... build --frontend gateway.v0 --opt source=<frontend>
//!  --local context=<src> --local dockerfile=<plan>
//!  --opt build-arg:cache-key=<UUID du projet>-<UUID de l'environnement>
//!  --output type=docker,name=...`
//!
//! Le frontend prefixe chaque cache du plan par `cache-key` : les UUID ont
//! une longueur fixe, donc deux projets ne peuvent pas obtenir le meme
//! prefixe (au contraire des noms : `a` + `b-c` et `a-b` + `c`).
//!
//! `prepare` sort sur Internet : il resout les versions des paquets que le
//! depot demande (`mise latest`, `mise ls-remote`). Meme posture que les
//! builds BuildKit : Internet public seulement (ni l'hote, ni les
//! metadonnees des clouds, ni les plages privees ; voir `sandbox`).

use super::argv::ValidatedArg;
use super::sandbox::{self, PrepareConfig};
use super::{BuildBackend, BuildError, ProgressSink, Staged, Targets, Workdir, buildctl_command};

/// Version de railpack installee par `install.sh` (`RAILPACK_VERSION`).
pub const RAILPACK_VERSION: &str = "v0.43.0";
/// Frontend BuildKit de la meme version, epingle par digest (index
/// multi-architecture de ghcr.io).
pub const FRONTEND: &str = "ghcr.io/railwayapp/railpack-frontend:v0.43.0@sha256:355ef61fae6f2551103cd8b10c710cb07167dd353d561f15c69b8d7ac1c676cc";
/// Depot de `FRONTEND`, sans etiquette ni empreinte : seul frontend
/// `gateway.v0` autorise par buildkitd avec `docker/dockerfile`
/// (`infra::buildkitd_config`).
pub const FRONTEND_REPOSITORY: &str = "ghcr.io/railwayapp/railpack-frontend";
/// Message d'un build Railpack sans bac a sable.
pub const NO_SANDBOX: &str =
    "bac a sable de railpack indisponible (compte ciphera-build absent) : relancer install.sh";

/// `railpack prepare` du dossier `src` (le depot de `workdir`, ou son
/// dossier racine) dans son bac a sable : plan ecrit dans `workdir.plan()`.
pub async fn prepare<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    cfg: Option<&PrepareConfig>,
    workdir: &Workdir,
    src: &str,
) -> Result<(), BuildError> {
    let cfg = cfg.ok_or(BuildError::Unavailable(NO_SANDBOX))?;
    let io = |e: std::io::Error| BuildError::Workdir(e.to_string());
    let src = std::path::absolute(src).map_err(io)?;
    let plan = std::path::absolute(workdir.plan()).map_err(io)?;
    let container = sandbox::prepare_container(cfg, workdir.deployment(), &src, &plan)?;
    sandbox::share_with_sandbox(&src, &plan)?;
    backend.prepare(&container, sink).await
}

/// Arguments du frontend Railpack pour `buildctl`.
pub fn frontend_args(
    src: &ValidatedArg,
    plan: &ValidatedArg,
    cache_key: &ValidatedArg,
) -> Vec<ValidatedArg> {
    vec![
        ValidatedArg::lit("--frontend"),
        ValidatedArg::lit("gateway.v0"),
        ValidatedArg::lit("--opt"),
        ValidatedArg::concat(&[&ValidatedArg::lit("source="), &ValidatedArg::lit(FRONTEND)]),
        ValidatedArg::lit("--local"),
        ValidatedArg::concat(&[&ValidatedArg::lit("context="), src]),
        ValidatedArg::lit("--local"),
        ValidatedArg::concat(&[&ValidatedArg::lit("dockerfile="), plan]),
        ValidatedArg::lit("--opt"),
        ValidatedArg::concat(&[&ValidatedArg::lit("build-arg:cache-key="), cache_key]),
    ]
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn build<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    prepare_cfg: Option<&PrepareConfig>,
    workdir: &Workdir,
    src: &ValidatedArg,
    targets: &Targets,
    socket: &ValidatedArg,
    staged: &mut Vec<Staged>,
) -> Result<(), BuildError> {
    let plan = ValidatedArg::path(&workdir.plan())?;
    let home = ValidatedArg::path(&workdir.home())?;
    let cache = targets.cache_scope()?;
    prepare(backend, sink, prepare_cfg, workdir, src.as_str()).await?;
    let staging = targets.staging(None);
    let cmd = buildctl_command(
        socket,
        frontend_args(src, &plan, &cache.key),
        &staging,
        &home,
    );
    let staging = proto::ImageRef(staging.as_str().to_string());
    backend.build_and_load(cmd, &staging, sink).await?;
    staged.push(Staged {
        staging,
        target: targets.main_tag(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le frontend et le binaire installe par install.sh ont la meme version.
    #[test]
    fn frontend_matches_installed_railpack() {
        let install = include_str!("../../../../install/install.sh");
        assert!(
            install.contains(&format!("RAILPACK_VERSION='{RAILPACK_VERSION}'")),
            "RAILPACK_VERSION de install.sh differente de {RAILPACK_VERSION}"
        );
        assert!(FRONTEND.contains(&format!(":{RAILPACK_VERSION}@sha256:")));
        assert!(FRONTEND.starts_with(&format!("{FRONTEND_REPOSITORY}:")));
    }
}
