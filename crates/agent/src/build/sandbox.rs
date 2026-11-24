//! Bac a sable jetable de `railpack prepare` : le depot (non fiable) n'est
//! jamais analyse sur l'hote par le compte de l'agent (equivalent root par
//! le groupe docker), mais dans un conteneur Docker cree pour l'occasion :
//!
//! - image deja epinglee par digest (`deploy/containers/buildkit.digest`,
//!   point d'entree remplace par railpack) ;
//! - compte `ciphera-build` (uid:gid non root), `cap_drop ALL`,
//!   `no-new-privileges`, racine en lecture seule, `/tmp` en tmpfs ;
//! - railpack monte en lecture seule, le depot en lecture seule, le dossier
//!   du plan en ecriture ;
//! - mise epingle (`MISE_VERSION`, SHA-256 verifie par install.sh) monte en
//!   lecture seule la ou railpack le cherche : railpack ne telecharge jamais
//!   un mise non verifie ; s'il manque sur l'hote, le build echoue ;
//! - environnement fixe, sans `GITHUB_TOKEN` ni autre secret ;
//! - delai (`PREPARE_TIMEOUT`), conteneur supprime dans tous les cas.
//!
//! Reseau : `railpack prepare` resout les versions des paquets demandes
//! (node, python... choisis par le depot : `package.json`, `.nvmrc`,
//! `railpack.json`, `mise.toml`) par `mise latest` / `mise ls-remote`, donc
//! par Internet. Le conteneur sort par le pont dedie `ciphera-prepare`
//! (jamais `ciphera`, `ciphera-internal`, un reseau de projet, ni l'hote),
//! sans communication entre conteneurs. Son interface porte le nom du
//! reseau (`ciphera-prepare`) : comme celle de BuildKit (`ciphera-build`),
//! sa sortie est filtree par `ciphera-agent.service` (table nftables
//! `inet ciphera_egress` : ni l'hote, ni les metadonnees des clouds, ni les
//! plages privees, locales ou de lien local ; Internet public seulement).
//!
//! mise et ses greffons : railpack lance mise (`latest`, `ls-remote`,
//! `list --current`) avec un environnement qu'il construit lui-meme
//! (`HOME`, dossiers de mise, `PATH`, `MISE_PARANOID=1`, plus
//! `MISE_NO_CONFIG=1` ou `MISE_SAFE=1`) sans heriter du notre : une variable
//! comme `MISE_DISABLE_BACKENDS=asdf,vfox` posee sur le conteneur
//! n'atteindrait pas mise. Les greffons asdf (scripts shell) et vfox (Lua)
//! executent du code pendant ces recherches ; ce code tournerait ici, dans
//! le bac a sable (compte non root, sans capacite, racine en lecture seule,
//! tmpfs `noexec`, sortie filtree). Voir `.agents/memory/decisions.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bollard::models::{ContainerCreateBody, HostConfig, NetworkCreateRequest};
use shared::ids::DeploymentId;

use super::BuildError;
use super::argv::TOOLS_DIR;
use crate::docker::{DEPLOYMENT_LABEL, MANAGED_LABEL, PREPARE_ROLE, ROLE_LABEL};

/// Pont des conteneurs `railpack prepare` (sortie Internet seulement).
pub const PREPARE_NETWORK: &str = "ciphera-prepare";
/// Prefixe des conteneurs jetables de `railpack prepare`.
pub const PREPARE_PREFIX: &str = "ciphera-prepare-";
/// mise attendu par railpack (`core/mise/version.txt` de `RAILPACK_VERSION`),
/// installe par install.sh (`MISE_VERSION`) dans `TOOLS_DIR`.
pub const MISE_VERSION: &str = "2026.10.14";
/// Dossier de mise de railpack (`mise.InstallDir`, chemin fixe) ; railpack y
/// cherche `mise-<version>` avant de le telecharger.
pub const RAILPACK_MISE_DIR: &str = "/tmp/railpack/mise";
/// Delai de `railpack prepare` (conteneur tue et supprime au-dela).
pub const PREPARE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Railpack dans le conteneur.
pub const RAILPACK_TARGET: &str = "/opt/ciphera/railpack";
/// Depot (lecture seule) et plan (ecriture) dans le conteneur.
pub const SRC_TARGET: &str = "/src";
pub const PLAN_TARGET: &str = "/out";
const PIDS_LIMIT: i64 = 512;
const MEMORY_LIMIT: i64 = 2 << 30;
/// Option Docker du pont : aucune communication entre ses conteneurs.
pub const ICC_OPTION: &str = "com.docker.network.bridge.enable_icc";
pub use crate::docker::BRIDGE_NAME_OPTION;

/// Reglages du bac a sable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareConfig {
    /// Image epinglee par digest.
    pub image: String,
    /// `(uid, gid)` du compte `ciphera-build`.
    pub user: (u32, u32),
    /// Railpack sur l'hote.
    pub railpack: PathBuf,
    /// mise epingle sur l'hote.
    pub mise: PathBuf,
}

impl PrepareConfig {
    /// Outils installes par install.sh dans `TOOLS_DIR`.
    pub fn production(image: String, user: (u32, u32)) -> Self {
        Self {
            image,
            user,
            railpack: PathBuf::from(format!("{TOOLS_DIR}/railpack")),
            mise: PathBuf::from(format!("{TOOLS_DIR}/mise-{MISE_VERSION}")),
        }
    }
}

/// Conteneur jetable pret a creer.
#[derive(Clone, Debug)]
pub struct PrepareContainer {
    pub name: String,
    pub image: String,
    pub body: ContainerCreateBody,
}

/// Chemin de l'hote monte dans le conteneur : absolu, sans separateur de la
/// syntaxe `source:cible:options` (Linux ; ailleurs, seuls les tests
/// simules l'utilisent).
fn bind_source(path: &Path) -> Result<String, BuildError> {
    let s = path
        .to_str()
        .ok_or_else(|| BuildError::Path("chemin non UTF-8".into()))?;
    #[cfg(unix)]
    let ok = crate::docker::valid_bind_path(s);
    #[cfg(not(unix))]
    let ok = !s.is_empty() && !s.bytes().any(|b| b == b',' || b.is_ascii_control());
    if ok {
        Ok(s.to_string())
    } else {
        Err(BuildError::Path(format!("montage refuse : {s}")))
    }
}

/// Outil epingle present sur l'hote (fichier regulier) ; sinon le build
/// echoue, sans repli sur un telechargement.
fn require_tool(path: &Path, what: &str) -> Result<(), BuildError> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => Ok(()),
        _ => Err(BuildError::Tool {
            tool: "railpack",
            message: format!(
                "{what} epingle absent ({}) : relancer install.sh",
                path.display()
            ),
        }),
    }
}

/// Spec du conteneur de `railpack prepare` pour le depot `src` et le
/// dossier du plan `plan` (chemins de l'hote).
pub fn prepare_container(
    cfg: &PrepareConfig,
    deployment: DeploymentId,
    src: &Path,
    plan: &Path,
) -> Result<PrepareContainer, BuildError> {
    require_tool(&cfg.railpack, "railpack")?;
    require_tool(&cfg.mise, &format!("mise {MISE_VERSION}"))?;
    if !crate::docker::valid_image(&cfg.image) || !cfg.image.contains("@sha256:") {
        return Err(BuildError::Image(format!(
            "image du bac a sable non epinglee : {}",
            cfg.image
        )));
    }
    let (uid, gid) = cfg.user;
    if uid == 0 || gid == 0 {
        return Err(BuildError::Invalid(super::argv::InvalidArg {
            kind: "compte",
            reason: "root refuse dans le bac a sable",
        }));
    }
    let binds = vec![
        format!("{}:{RAILPACK_TARGET}:ro", bind_source(&cfg.railpack)?),
        format!(
            "{}:{RAILPACK_MISE_DIR}/mise-{MISE_VERSION}:ro",
            bind_source(&cfg.mise)?
        ),
        format!("{}:{SRC_TARGET}:ro,z", bind_source(src)?),
        format!("{}:{PLAN_TARGET}:rw,z", bind_source(plan)?),
    ];
    // `/tmp` au conteneur ; le dossier de mise de railpack (verrous, cache
    // des versions) au compte du conteneur. Le binaire de mise y est monte
    // en lecture seule par-dessus.
    let tmpfs = HashMap::from([
        (
            "/tmp".to_string(),
            "rw,nosuid,nodev,noexec,size=64m,mode=1777".to_string(),
        ),
        (
            RAILPACK_MISE_DIR.to_string(),
            format!("rw,nosuid,nodev,noexec,size=256m,mode=0700,uid={uid},gid={gid}"),
        ),
    ]);
    // Etiquettes reservees (refusees dans un fichier Compose) : le
    // balayage au demarrage reconnait le bac a sable par elles seules.
    let labels = HashMap::from([
        (MANAGED_LABEL.to_string(), "true".to_string()),
        (DEPLOYMENT_LABEL.to_string(), deployment.to_string()),
        (ROLE_LABEL.to_string(), PREPARE_ROLE.to_string()),
    ]);
    let body = ContainerCreateBody {
        image: Some(cfg.image.clone()),
        entrypoint: Some(vec![RAILPACK_TARGET.to_string()]),
        cmd: Some(
            [
                "prepare",
                SRC_TARGET,
                "--plan-out",
                "/out/railpack-plan.json",
                "--info-out",
                "/out/railpack-info.json",
            ]
            .map(String::from)
            .to_vec(),
        ),
        // Remplace celles de l'image (HOME, TMPDIR, XDG_RUNTIME_DIR,
        // BUILDKIT_HOST de son compte 1000) ; aucun secret.
        env: Some(
            [
                "PATH=/usr/bin:/bin",
                "HOME=/tmp",
                "TMPDIR=/tmp",
                "XDG_RUNTIME_DIR=/tmp",
                "BUILDKIT_HOST=",
                "LC_ALL=C",
            ]
            .map(String::from)
            .to_vec(),
        ),
        user: Some(format!("{uid}:{gid}")),
        working_dir: Some("/tmp".to_string()),
        labels: Some(labels),
        host_config: Some(HostConfig {
            network_mode: Some(PREPARE_NETWORK.to_string()),
            binds: Some(binds),
            tmpfs: Some(tmpfs),
            readonly_rootfs: Some(true),
            cap_drop: Some(vec!["ALL".to_string()]),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            pids_limit: Some(PIDS_LIMIT),
            memory: Some(MEMORY_LIMIT),
            privileged: Some(false),
            publish_all_ports: Some(false),
            log_config: Some(crate::docker::log_config()),
            ..Default::default()
        }),
        ..Default::default()
    };
    Ok(PrepareContainer {
        name: format!("{PREPARE_PREFIX}{deployment}"),
        image: cfg.image.clone(),
        body,
    })
}

/// Requete de creation du pont `ciphera-prepare` : non interne (sortie
/// Internet filtree), gere par Ciphera, sans communication entre
/// conteneurs, interface nommee `ciphera-prepare`.
pub fn prepare_network_request() -> NetworkCreateRequest {
    NetworkCreateRequest {
        name: PREPARE_NETWORK.to_string(),
        driver: Some("bridge".to_string()),
        internal: Some(false),
        attachable: Some(false),
        labels: Some(HashMap::from([(
            MANAGED_LABEL.to_string(),
            "true".to_string(),
        )])),
        options: Some(HashMap::from([
            (ICC_OPTION.to_string(), "false".to_string()),
            (BRIDGE_NAME_OPTION.to_string(), PREPARE_NETWORK.to_string()),
        ])),
        ..Default::default()
    }
}

/// Ouvre le depot en lecture et le dossier du plan en ecriture au compte du
/// conteneur (« autres » : le depot est clone avec le masque 0077 de
/// l'agent). Le dossier du build reste 0700 : rien n'est expose sur l'hote.
/// Les liens symboliques ne sont jamais suivis.
#[cfg(unix)]
pub fn share_with_sandbox(src: &Path, plan: &Path) -> Result<(), BuildError> {
    use std::os::unix::fs::PermissionsExt;
    let io = |e: std::io::Error| BuildError::Workdir(e.to_string());
    let mut stack = vec![src.to_path_buf()];
    while let Some(path) = stack.pop() {
        let meta = std::fs::symlink_metadata(&path).map_err(io)?;
        let mode = meta.permissions().mode();
        if meta.is_dir() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode | 0o005))
                .map_err(io)?;
            for entry in std::fs::read_dir(&path).map_err(io)? {
                stack.push(entry.map_err(io)?.path());
            }
        } else if meta.is_file() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode | 0o004))
                .map_err(io)?;
        }
    }
    std::fs::set_permissions(plan, std::fs::Permissions::from_mode(0o703)).map_err(io)
}

#[cfg(not(unix))]
pub fn share_with_sandbox(_src: &Path, _plan: &Path) -> Result<(), BuildError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "docker.io/moby/buildkit:v0.35.1-rootless@sha256:7d02292556e66feea6d2d89b7ec143503b57e4f59a4efc6b7ece8547e4d3e5be";

    /// Outils simules sous `target/` (supprimes a la fin).
    struct Tools(PathBuf);

    impl Tools {
        fn new() -> Self {
            let dir = std::path::absolute(format!("target/sandbox-tests/{}", DeploymentId::new()))
                .unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("railpack"), "").unwrap();
            std::fs::write(dir.join(format!("mise-{MISE_VERSION}")), "").unwrap();
            Self(dir)
        }

        fn config(&self) -> PrepareConfig {
            PrepareConfig {
                image: IMAGE.into(),
                user: (10003, 10004),
                railpack: self.0.join("railpack"),
                mise: self.0.join(format!("mise-{MISE_VERSION}")),
            }
        }

        fn container(&self, cfg: &PrepareConfig) -> Result<PrepareContainer, BuildError> {
            prepare_container(
                cfg,
                DeploymentId::new(),
                &self.0.join("src"),
                &self.0.join("plan"),
            )
        }
    }

    impl Drop for Tools {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn prepare_container_is_locked_down() {
        let tools = Tools::new();
        let cfg = tools.config();
        let c = tools.container(&cfg).unwrap();
        assert!(c.name.starts_with(PREPARE_PREFIX));
        assert_eq!(c.image, IMAGE);
        let body = &c.body;
        // Reconnu au balayage par ses etiquettes, jamais par son nom.
        let labels = body.labels.as_ref().unwrap();
        assert_eq!(
            labels[crate::docker::ROLE_LABEL],
            crate::docker::PREPARE_ROLE
        );
        assert_eq!(labels[MANAGED_LABEL], "true");
        let host = body.host_config.as_ref().unwrap();
        assert_eq!(host.network_mode.as_deref(), Some(PREPARE_NETWORK));
        for forbidden in ["host", "ciphera", "ciphera-internal", "bridge"] {
            assert_ne!(host.network_mode.as_deref(), Some(forbidden));
        }
        assert_eq!(body.user.as_deref(), Some("10003:10004"));
        assert_eq!(host.cap_drop, Some(vec!["ALL".to_string()]));
        assert!(host.cap_add.is_none());
        assert_eq!(
            host.security_opt,
            Some(vec!["no-new-privileges:true".to_string()])
        );
        assert_eq!(host.readonly_rootfs, Some(true));
        assert_eq!(host.privileged, Some(false));
        assert!(host.restart_policy.is_none(), "conteneur jetable");
        assert!(host.port_bindings.is_none());
        let tmpfs = host.tmpfs.as_ref().unwrap();
        assert!(tmpfs.contains_key("/tmp"));
        assert!(tmpfs[RAILPACK_MISE_DIR].contains("uid=10003,gid=10004"));
        // Un mise telecharge par railpack ne pourrait pas s'executer : seul
        // le binaire epingle (monte par-dessus) tourne.
        for dir in ["/tmp", RAILPACK_MISE_DIR] {
            assert!(tmpfs[dir].split(',').any(|o| o == "noexec"), "{dir}");
        }
        assert_eq!(
            body.entrypoint,
            Some(vec![RAILPACK_TARGET.to_string()]),
            "railpack remplace le point d'entree de l'image"
        );
        assert_eq!(body.cmd.as_ref().unwrap()[..2], ["prepare", SRC_TARGET]);
        assert_eq!(
            body.labels
                .as_ref()
                .unwrap()
                .get(MANAGED_LABEL)
                .map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn prepare_mounts_are_read_only_except_the_plan() {
        let tools = Tools::new();
        let cfg = tools.config();
        let c = tools.container(&cfg).unwrap();
        let binds = c.body.host_config.unwrap().binds.unwrap();
        let mount = |target: &str| {
            binds
                .iter()
                .find(|b| b.contains(&format!(":{target}:")))
                .unwrap_or_else(|| panic!("{target} absent : {binds:?}"))
                .clone()
        };
        let mise = mount(&format!("{RAILPACK_MISE_DIR}/mise-{MISE_VERSION}"));
        assert!(mise.starts_with(cfg.mise.to_str().unwrap()), "{mise}");
        assert!(
            mise.ends_with(":ro"),
            "mise monte en lecture seule : {mise}"
        );
        assert!(mount(RAILPACK_TARGET).ends_with(":ro"));
        assert!(mount(SRC_TARGET).contains(":ro"));
        assert!(mount(PLAN_TARGET).contains(":rw"));
        assert_eq!(binds.len(), 4, "{binds:?}");
        assert!(!binds.iter().any(|b| b.contains("docker.sock")));
    }

    #[test]
    fn prepare_env_carries_no_secret() {
        let tools = Tools::new();
        let c = tools.container(&tools.config()).unwrap();
        let env = c.body.env.unwrap();
        for var in &env {
            let name = var.split('=').next().unwrap();
            assert!(
                !["GITHUB_TOKEN", "GH_TOKEN", "CIPHERA_GIT_TOKEN"].contains(&name)
                    && !name.contains("TOKEN")
                    && !name.contains("SECRET")
                    && !name.contains("PASSWORD"),
                "{var}"
            );
        }
        assert!(env.iter().any(|v| v == "HOME=/tmp"));
        assert!(env.iter().any(|v| v == "TMPDIR=/tmp"));
    }

    #[test]
    fn missing_pinned_mise_root_or_unpinned_image_is_refused() {
        let tools = Tools::new();
        let mut cfg = tools.config();
        cfg.mise = tools.0.join("mise-absent");
        let err = tools.container(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("mise 2026.10.14 epingle absent"),
            "{err}"
        );

        let mut cfg = tools.config();
        cfg.user = (0, 0);
        assert!(matches!(tools.container(&cfg), Err(BuildError::Invalid(_))));

        let mut cfg = tools.config();
        cfg.image = "docker.io/moby/buildkit:latest".into();
        assert!(matches!(tools.container(&cfg), Err(BuildError::Image(_))));
    }

    #[test]
    fn prepare_network_is_a_managed_bridge_without_icc() {
        let req = prepare_network_request();
        assert_eq!(req.name, PREPARE_NETWORK);
        assert_eq!(req.driver.as_deref(), Some("bridge"));
        assert_eq!(req.internal, Some(false));
        assert_eq!(req.labels.unwrap()[MANAGED_LABEL], "true");
        let options = req.options.unwrap();
        assert_eq!(options[ICC_OPTION], "false");
        // Interface nommee comme le reseau : filtrage de la sortie
        // (ciphera-agent.service).
        assert_eq!(
            options[BRIDGE_NAME_OPTION], PREPARE_NETWORK,
            "nom d'interface de 15 caracteres au plus"
        );
        assert!(PREPARE_NETWORK.len() <= 15);
    }

    /// Version de mise epinglee = celle d'install.sh.
    #[test]
    fn mise_version_matches_install_sh() {
        let install = include_str!("../../../../install/install.sh");
        assert!(install.contains(&format!("MISE_VERSION='{MISE_VERSION}'")));
    }
}
