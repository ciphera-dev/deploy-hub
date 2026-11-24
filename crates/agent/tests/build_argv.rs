//! Pipeline de build sans shell : arguments valides en liste blanche,
//! binaires fixes, jeton Git hors des arguments et des journaux, rapport
//! d'echec par etape, dossier de build supprime, politique Compose.
//! Les outils et Docker sont simules (`FakeBackend`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use ciphera_agent::build::argv::{self, AllowedBin, ValidatedArg};
use ciphera_agent::build::compose::ComposeFile;
use ciphera_agent::build::sandbox::{MISE_VERSION, PrepareConfig, PrepareContainer};
use ciphera_agent::build::{
    self, BuildBackend, BuildConfig, BuildError, ProgressSink, clone, compose_policy, dockerfile,
};
use proto::{
    AgentToControl, BuildMethod, BuildSource, BuildSpec, GitToken, ImageRef, LogStream, Stage,
    StageStatus,
};
use shared::ids::{DeploymentId, EnvironmentId, ProjectId};
use tokio::sync::mpsc;

const TOKEN: &str = "ghs_TopSecretInstallationToken123";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const SANDBOX_IMAGE: &str = "docker.io/moby/buildkit:v0.35.1-rootless@sha256:7d02292556e66feea6d2d89b7ec143503b57e4f59a4efc6b7ece8547e4d3e5be";

// --- outils simules ---------------------------------------------------------

/// Une commande lancee : programme et arguments (jamais l'environnement).
#[derive(Clone, Debug)]
struct Call {
    program: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
}

impl Call {
    fn of(cmd: &Command) -> Self {
        Self {
            program: cmd.get_program().to_string_lossy().into_owned(),
            args: cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            envs: cmd
                .get_envs()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.map(|v| v.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
        }
    }
}

#[derive(Default)]
struct FakeBackend {
    calls: Mutex<Vec<Call>>,
    tags: Mutex<Vec<(String, String)>>,
    loaded: Mutex<Vec<String>>,
    removed: Mutex<Vec<String>>,
    /// Fichiers ecrits par le faux `git clone` (chemin relatif, contenu).
    repo: Vec<(&'static str, &'static str)>,
    /// Liens symboliques du faux depot (lien, cible), Unix seulement.
    #[cfg_attr(not(unix), allow(dead_code))]
    links: Vec<(&'static str, &'static str)>,
    /// Sortie du faux `docker compose config`.
    compose_json: Option<String>,
    /// Le faux buildctl echoue.
    fail_build: bool,
    /// Un outil ecrit le jeton dans son journal.
    echo_token: bool,
    unavailable: bool,
    /// Conteneurs `railpack prepare` demandes.
    prepared: Mutex<Vec<PrepareContainer>>,
}

impl BuildBackend for FakeBackend {
    fn buildkit_available(&self) -> bool {
        !self.unavailable
    }

    async fn exec(
        &self,
        _bin: AllowedBin,
        cmd: Command,
        sink: &ProgressSink,
        _capture: bool,
    ) -> Result<Vec<u8>, BuildError> {
        let call = Call::of(&cmd);
        self.calls.lock().unwrap().push(call.clone());
        if self.echo_token {
            sink.log(LogStream::Stderr, &format!("remote: token {TOKEN}"))
                .await;
        }
        if call.args.iter().any(|a| a == "clone") {
            let dest = PathBuf::from(call.args.last().unwrap());
            std::fs::create_dir_all(&dest).unwrap();
            for (path, content) in &self.repo {
                let p = dest.join(path);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, content).unwrap();
            }
            #[cfg(unix)]
            for (link, target) in &self.links {
                std::os::unix::fs::symlink(target, dest.join(link)).unwrap();
            }
            return Ok(Vec::new());
        }
        if call.args.iter().any(|a| a == "rev-parse") {
            return Ok(format!("{SHA}\n").into_bytes());
        }
        if call.args.iter().any(|a| a == "config") {
            return Ok(self.compose_json.clone().unwrap_or_default().into_bytes());
        }
        Ok(Vec::new())
    }

    async fn build_and_load(
        &self,
        cmd: Command,
        staging: &ImageRef,
        _sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        self.calls.lock().unwrap().push(Call::of(&cmd));
        if self.fail_build {
            return Err(BuildError::Tool {
                tool: "buildctl",
                message: "echec du build (exit status: 1)".into(),
            });
        }
        self.loaded.lock().unwrap().push(staging.0.clone());
        Ok(())
    }

    async fn pull(&self, image: &ImageRef, _sink: &ProgressSink) -> Result<(), BuildError> {
        self.loaded.lock().unwrap().push(image.0.clone());
        Ok(())
    }

    async fn tag(&self, source: &ImageRef, target: &ImageRef) -> Result<(), BuildError> {
        self.tags
            .lock()
            .unwrap()
            .push((source.0.clone(), target.0.clone()));
        Ok(())
    }

    async fn prepare(
        &self,
        container: &PrepareContainer,
        _sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        self.prepared.lock().unwrap().push(container.clone());
        Ok(())
    }

    async fn remove_image(&self, image: &ImageRef) -> Result<(), BuildError> {
        self.removed.lock().unwrap().push(image.0.clone());
        Ok(())
    }
}

/// Dossier des builds du test : chemin relatif sous `target/` (accepte par
/// `ValidatedArg::path` sur toutes les plateformes), supprime a la fin.
struct BuildsDir(String);

impl BuildsDir {
    fn new() -> Self {
        let dir = Self(format!("target/build-tests/{}", DeploymentId::new()));
        // Outils epingles simules (railpack, mise) a cote des builds.
        let tools = dir.tools();
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join("railpack"), "").unwrap();
        std::fs::write(tools.join(format!("mise-{MISE_VERSION}")), "").unwrap();
        dir
    }

    fn tools(&self) -> PathBuf {
        std::path::absolute(format!("{}-tools", self.0)).unwrap()
    }

    fn config(&self) -> BuildConfig {
        BuildConfig {
            builds_dir: self.0.clone(),
            buildkit_socket: "/run/ciphera-agent/buildkit/buildkitd.sock".into(),
            askpass: PathBuf::from("/usr/local/bin/ciphera-agent"),
            prepare: Some(PrepareConfig {
                image: SANDBOX_IMAGE.into(),
                user: (10003, 10003),
                railpack: self.tools().join("railpack"),
                mise: self.tools().join(format!("mise-{MISE_VERSION}")),
            }),
        }
    }
}

impl Drop for BuildsDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_dir_all(self.tools());
    }
}

fn spec(build: BuildMethod) -> BuildSpec {
    BuildSpec {
        deployment_id: DeploymentId::new(),
        project: "web".into(),
        environment: "production".into(),
        source: BuildSource::Git {
            url: "https://github.com/acme/web".into(),
            branch: "main".into(),
            commit_sha: SHA.into(),
        },
        build,
        git_token: Some(GitToken::new(TOKEN.into())),
        root_directory: None,
        project_id: Some(ProjectId::new()),
        environment_id: Some(EnvironmentId::new()),
    }
}

/// Lance le pipeline et renvoie son resultat et les messages emis.
async fn run(
    spec: &BuildSpec,
    backend: &FakeBackend,
    dir: &BuildsDir,
) -> (Result<build::BuiltImages, BuildError>, Vec<AgentToControl>) {
    let (tx, mut rx) = mpsc::channel(1024);
    let sink = ProgressSink::new(spec.deployment_id, tx);
    let result = build::run(spec, &dir.config(), backend, &sink).await;
    drop(sink);
    let mut messages = Vec::new();
    while let Some(m) = rx.recv().await {
        messages.push(m);
    }
    (result, messages)
}

fn stages(messages: &[AgentToControl]) -> Vec<(Stage, StageStatus)> {
    messages
        .iter()
        .filter_map(|m| match m {
            AgentToControl::DeployProgress { stage, status, .. } => Some((*stage, status.clone())),
            _ => None,
        })
        .collect()
}

fn dockerfile_repo() -> Vec<(&'static str, &'static str)> {
    vec![("Dockerfile", "FROM busybox\n")]
}

// --- tests -------------------------------------------------------------------

#[test]
fn validated_arg_rejects_shell_metachars() {
    let bad = [
        "a;b",
        "a|b",
        "$(id)",
        "a$(id)",
        "`id`",
        "a&b",
        "a>b",
        "a b",
        "a\nb",
        "a'b",
        "a\"b",
        "../etc",
        "a/../../b",
        "..",
        "-rf",
        "",
    ];
    for value in bad {
        assert!(ValidatedArg::path(value).is_err(), "path {value:?}");
        assert!(ValidatedArg::name(value).is_err(), "name {value:?}");
        assert!(ValidatedArg::git_url(value).is_err(), "git_url {value:?}");
        assert!(ValidatedArg::branch(value).is_err(), "branch {value:?}");
    }
    assert!(ValidatedArg::path("/var/lib/ciphera-agent/builds/x/src").is_ok());
    assert!(ValidatedArg::path("docker/Dockerfile.prod").is_ok());
    assert!(ValidatedArg::name("web-1").is_ok());
    assert!(ValidatedArg::branch("feat/charts").is_ok());
    // Le message d'erreur ne reprend pas la valeur.
    let e = ValidatedArg::git_url("https://user:secret@github.com/o/r").unwrap_err();
    assert!(!e.to_string().contains("secret"));
}

#[test]
fn git_url_rejects_ext_file_ssh_and_option_injection() {
    for bad in [
        "ext::sh -c id",
        "ext::sh -c id%20>&2",
        "file:///etc",
        "file:///etc/passwd",
        "ssh://git@github.com/acme/web",
        "git@github.com:acme/web.git",
        "--upload-pack=x",
        "-uhttps://github.com/acme/web",
        "http://github.com/acme/web",
        "https://gitlab.com/acme/web",
        "https://github.com.evil.com/acme/web",
        "https://x-access-token:t@github.com/acme/web",
        "https://github.com/acme/web?x=1",
        "https://github.com/acme/web#x",
        "https://github.com/acme/../web",
    ] {
        assert!(ValidatedArg::git_url(bad).is_err(), "{bad}");
    }
    for ok in [
        "https://github.com/acme/web",
        "https://github.com/acme/web.git",
    ] {
        assert!(ValidatedArg::git_url(ok).is_ok(), "{ok}");
    }
}

#[tokio::test]
async fn command_never_uses_shell() {
    const SHELLS: [&str; 6] = ["sh", "bash", "dash", "zsh", "ash", "busybox"];
    let check = |program: &str| {
        assert!(program.starts_with('/'), "programme non absolu : {program}");
        let base = program.rsplit('/').next().unwrap();
        assert!(!SHELLS.contains(&base), "shell : {program}");
    };
    for bin in AllowedBin::ALL {
        let cmd = argv::command(bin, &[]);
        check(&cmd.get_program().to_string_lossy());
        assert!(cmd.get_args().all(|a| a != "-c"), "{bin:?}");
    }

    // Toutes les commandes emises par le pipeline, pour chaque methode.
    let compose = "services:\n  web:\n    build: .\n";
    let normalized = r#"{"name":"web-production","services":{"web":{"build":{"context":".","dockerfile":"Dockerfile"},"networks":{"default":null}}},"networks":{"default":{"name":"web-production_default"}}}"#;
    for (method, repo, json) in [
        (BuildMethod::Railpack, vec![("package.json", "{}")], None),
        (
            BuildMethod::Dockerfile {
                path: "Dockerfile".into(),
            },
            dockerfile_repo(),
            None,
        ),
        (
            BuildMethod::Compose {
                path: "compose.yaml".into(),
                service: "web".into(),
            },
            vec![("compose.yaml", compose), ("Dockerfile", "FROM busybox\n")],
            Some(normalized.to_string()),
        ),
    ] {
        let dir = BuildsDir::new();
        let backend = FakeBackend {
            repo,
            compose_json: json,
            ..Default::default()
        };
        let (result, _) = run(&spec(method.clone()), &backend, &dir).await;
        result.unwrap_or_else(|e| panic!("{method:?} : {e}"));
        let calls = backend.calls.lock().unwrap();
        assert!(calls.len() >= 3, "{calls:?}");
        for call in calls.iter() {
            check(&call.program);
            assert!(
                [
                    AllowedBin::Git,
                    AllowedBin::DockerCompose,
                    AllowedBin::Buildctl
                ]
                .iter()
                .any(|b| b.program() == call.program),
                "binaire hors liste : {}",
                call.program
            );
        }
    }
}

#[tokio::test]
async fn git_token_not_in_argv_or_logs() {
    // Commande de clone : jeton seulement dans l'environnement de git.
    let checkout = clone::Checkout::new("https://github.com/acme/web", "main", SHA).unwrap();
    let dest = ValidatedArg::path("/var/lib/ciphera-agent/builds/x/src").unwrap();
    let home = ValidatedArg::path("/var/lib/ciphera-agent/builds/x/home").unwrap();
    let token = GitToken::new(TOKEN.into());
    let askpass = Path::new("/usr/local/bin/ciphera-agent");
    let cmd = clone::clone_command(&checkout, &dest, &home, askpass, Some(&token));
    let call = Call::of(&cmd);
    assert_eq!(
        call.args,
        [
            "-c",
            "protocol.file.allow=never",
            "-c",
            "core.hooksPath=/dev/null",
            "clone",
            "--depth",
            "1",
            "--no-recurse-submodules",
            "--branch",
            "main",
            "https://github.com/acme/web",
            "/var/lib/ciphera-agent/builds/x/src",
        ]
    );
    assert!(call.args.iter().all(|a| !a.contains(TOKEN)));
    assert!(!argv::describe(&cmd).contains(TOKEN));
    let env = |k: &str| {
        call.envs
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(env("GIT_ASKPASS"), Some("/usr/local/bin/ciphera-agent"));
    assert_eq!(env(clone::TOKEN_ENV), Some(TOKEN));
    assert_eq!(env("GIT_TERMINAL_PROMPT"), Some("0"));
    assert_eq!(env("GIT_ALLOW_PROTOCOL"), Some("https"));
    assert_eq!(env("GIT_CONFIG_NOSYSTEM"), Some("1"));
    assert_eq!(env("HOME"), Some("/var/lib/ciphera-agent/builds/x/home"));
    // Le jeton n'est passe qu'a git, a aucun autre outil.
    let carriers: Vec<_> = call
        .envs
        .iter()
        .filter(|(_, v)| v.contains(TOKEN))
        .collect();
    assert_eq!(carriers.len(), 1, "{carriers:?}");
    // Sans jeton : pas d'askpass.
    let anon = Call::of(&clone::clone_command(
        &checkout, &dest, &home, askpass, None,
    ));
    assert!(anon.envs.iter().all(|(k, _)| k != "GIT_ASKPASS"));

    // Pipeline complet : aucun argument, aucune ligne de journal, aucun
    // message ne contient le jeton, meme si un outil l'affiche.
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: dockerfile_repo(),
        echo_token: true,
        fail_build: true,
        ..Default::default()
    };
    let spec = spec(BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    });
    assert!(!format!("{spec:?}").contains(TOKEN));
    let (result, messages) = run(&spec, &backend, &dir).await;
    assert!(result.is_err());
    assert!(!messages.is_empty());
    for m in &messages {
        assert!(!format!("{m:?}").contains(TOKEN), "{m:?}");
    }
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, AgentToControl::LogLine(l) if l.line.contains("***")))
    );
    for call in backend.calls.lock().unwrap().iter() {
        assert!(call.args.iter().all(|a| !a.contains(TOKEN)), "{call:?}");
        if call.program != AllowedBin::Git.program() {
            assert!(
                call.envs.iter().all(|(_, v)| !v.contains(TOKEN)),
                "{call:?}"
            );
        }
    }
}

#[tokio::test]
async fn build_failure_reports_stage_build_failed_and_no_current_tag() {
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: dockerfile_repo(),
        fail_build: true,
        ..Default::default()
    };
    let spec = spec(BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    });
    let (result, messages) = run(&spec, &backend, &dir).await;
    assert!(matches!(result, Err(BuildError::Tool { .. })), "{result:?}");
    let reported = stages(&messages);
    assert_eq!(
        reported[..3],
        [
            (Stage::Clone, StageStatus::Started),
            (Stage::Clone, StageStatus::Succeeded),
            (Stage::Build, StageStatus::Started),
        ]
    );
    assert!(
        matches!(reported.last(), Some((Stage::Build, StageStatus::Failed { message })) if message.contains("buildctl")),
        "{reported:?}"
    );
    assert!(
        reported.iter().all(|(s, _)| *s != Stage::Image),
        "{reported:?}"
    );
    // Aucune image etiquetee (ni la version, ni une etiquette courante).
    assert!(backend.tags.lock().unwrap().is_empty());
    assert!(backend.loaded.lock().unwrap().is_empty());

    // Succes : l'image n'est etiquetee qu'apres son chargement, sous
    // ciphera/<projet>-<env>:<deployment_id> seulement, puis le nom
    // provisoire est retire.
    let ok = FakeBackend {
        repo: dockerfile_repo(),
        ..Default::default()
    };
    let (result, messages) = run(&spec, &ok, &dir).await;
    let images = result.unwrap().images;
    let tag = format!("ciphera/web-production:{}", spec.deployment_id);
    assert_eq!(images, [ImageRef(tag.clone())]);
    let staging = format!("ciphera-staging/{}:build", spec.deployment_id);
    assert_eq!(
        ok.loaded.lock().unwrap().as_slice(),
        std::slice::from_ref(&staging)
    );
    assert_eq!(*ok.tags.lock().unwrap(), [(staging.clone(), tag)]);
    assert_eq!(*ok.removed.lock().unwrap(), [staging]);
    assert_eq!(
        stages(&messages).last(),
        Some(&(Stage::Image, StageStatus::Succeeded))
    );
}

#[tokio::test]
async fn workdir_removed_after_failure() {
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: dockerfile_repo(),
        fail_build: true,
        ..Default::default()
    };
    let spec = spec(BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    });
    let (result, _) = run(&spec, &backend, &dir).await;
    assert!(result.is_err());
    let workdir = format!("{}/{}", dir.0, spec.deployment_id);
    // Le clone a bien eu lieu dans ce dossier, supprime ensuite.
    let clone = backend.calls.lock().unwrap()[0].clone();
    assert_eq!(clone.args.last().unwrap(), &format!("{workdir}/src"));
    assert!(!Path::new(&workdir).exists(), "{workdir} encore present");

    // Chemin de build hors du depot : echec, dossier supprime aussi.
    let escape = FakeBackend {
        repo: dockerfile_repo(),
        ..Default::default()
    };
    let spec = spec_with(BuildMethod::Dockerfile {
        path: "../../Dockerfile".into(),
    });
    let (result, _) = run(&spec, &escape, &dir).await;
    assert!(matches!(result, Err(BuildError::Invalid(_))), "{result:?}");
    assert!(!Path::new(&format!("{}/{}", dir.0, spec.deployment_id)).exists());

    // Apres un succes aussi.
    let ok = FakeBackend {
        repo: dockerfile_repo(),
        ..Default::default()
    };
    let spec = spec_with(BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    });
    run(&spec, &ok, &dir).await.0.unwrap();
    assert!(!Path::new(&format!("{}/{}", dir.0, spec.deployment_id)).exists());

    // BuildKit indisponible (agent degrade) : erreur claire, rien de clone.
    let down = FakeBackend {
        unavailable: true,
        ..Default::default()
    };
    let (result, messages) = run(&spec, &down, &dir).await;
    assert!(matches!(result, Err(BuildError::Unavailable(_))));
    assert!(down.calls.lock().unwrap().is_empty());
    assert!(matches!(
        stages(&messages).last(),
        Some((Stage::Build, StageStatus::Failed { message })) if message.contains("BuildKit indisponible")
    ));
}

fn spec_with(build: BuildMethod) -> BuildSpec {
    spec(build)
}

/// Dockerfile ou fichier Compose du depot qui est un lien vers l'exterieur :
/// refuse apres le clone (chemin canonique hors du depot).
#[cfg(unix)]
#[tokio::test]
async fn symlinked_build_paths_outside_the_repo_are_refused() {
    let dir = BuildsDir::new();
    for method in [
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
        BuildMethod::Compose {
            path: "compose.yaml".into(),
            service: "web".into(),
        },
    ] {
        let backend = FakeBackend {
            links: vec![
                ("Dockerfile", "/etc/hostname"),
                ("compose.yaml", "/etc/hostname"),
            ],
            ..Default::default()
        };
        let spec = spec(method);
        let (result, _) = run(&spec, &backend, &dir).await;
        assert!(matches!(result, Err(BuildError::Path(_))), "{result:?}");
        assert!(backend.loaded.lock().unwrap().is_empty());
        assert!(!Path::new(&format!("{}/{}", dir.0, spec.deployment_id)).exists());
    }
    // env_file du depot qui pointe hors du depot : refuse avant
    // `docker compose config` (qui le lirait).
    let backend = FakeBackend {
        repo: vec![(
            "compose.yaml",
            "services:\n  web:\n    image: nginx\n    env_file: web.env\n",
        )],
        links: vec![("web.env", "/etc/hostname")],
        ..Default::default()
    };
    let spec = spec(BuildMethod::Compose {
        path: "compose.yaml".into(),
        service: "web".into(),
    });
    let (result, _) = run(&spec, &backend, &dir).await;
    assert!(matches!(result, Err(BuildError::Path(_))), "{result:?}");
    assert!(
        backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|c| c.program != AllowedBin::DockerCompose.program())
    );
}

/// Fichier Compose du depot, a la racine `/repo`.
fn compose(text: &str) -> Result<(), compose_policy::PolicyViolation> {
    let root = Path::new("/repo");
    let file = ComposeFile::from_yaml(text, root, root).expect("YAML valide");
    compose_policy::check(&file)
}

#[test]
fn compose_policy_rejects_privileged_host_network_docker_sock_and_bind_mounts() {
    let ok = "
name: shop
x-common: &common
  restart: unless-stopped
services:
  web:
    <<: *common
    build:
      context: ./app
      dockerfile: Dockerfile
      target: runtime
    environment:
      DATABASE_URL: postgres://db/app
    env_file: [.env.web]
    depends_on: [db]
    volumes:
      - uploads:/srv/uploads
      - type: volume
        source: cache
        target: /cache
      - type: tmpfs
        target: /tmp
    security_opt: [no-new-privileges:true]
    cap_drop: [ALL]
    expose: ['8080']
    healthcheck:
      test: [CMD, wget, -q, http://localhost:8080/health]
    deploy:
      resources:
        limits: {cpus: '1', memory: 512M}
  db:
    image: postgres:17
    volumes: [db-data:/var/lib/postgresql/data]
volumes:
  uploads:
  cache: {}
  db-data:
";
    compose(ok).unwrap_or_else(|v| panic!("fichier sain refuse : {v}"));

    // Un cas par cle refusee : (fichier, emplacement attendu).
    let svc = |extra: &str| format!("services:\n  web:\n    image: nginx\n{extra}");
    let cases: Vec<(String, &str)> = vec![
        (svc("    privileged: true\n"), "services.web.privileged"),
        (svc("    network_mode: host\n"), "services.web.network_mode"),
        (svc("    pid: host\n"), "services.web.pid"),
        (svc("    ipc: host\n"), "services.web.ipc"),
        (svc("    cap_add: [SYS_ADMIN]\n"), "services.web.cap_add"),
        (svc("    devices: ['/dev/kvm:/dev/kvm']\n"), "services.web.devices"),
        (
            svc("    security_opt: ['seccomp:unconfined']\n"),
            "services.web.security_opt",
        ),
        (
            svc("    security_opt: ['apparmor:unconfined']\n"),
            "services.web.security_opt",
        ),
        (
            svc("    security_opt: ['no-new-privileges:false']\n"),
            "services.web.security_opt",
        ),
        (
            svc("    volumes: ['/var/run/docker.sock:/var/run/docker.sock']\n"),
            "services.web.volumes[0]",
        ),
        (svc("    volumes: ['/etc:/host-etc:ro']\n"), "services.web.volumes[0]"),
        (svc("    volumes: ['./data:/data']\n"), "services.web.volumes[0]"),
        (svc("    volumes: ['~/x:/data']\n"), "services.web.volumes[0]"),
        (
            svc("    volumes:\n      - type: bind\n        source: /\n        target: /host\n"),
            "services.web.volumes[0]",
        ),
        (svc("    ports: ['80:80']\n"), "services.web.ports"),
        (svc("    ports: ['127.0.0.1:5432:5432']\n"), "services.web.ports"),
        (svc("    userns_mode: host\n"), "services.web.userns_mode"),
        (svc("    volumes_from: [other]\n"), "services.web.volumes_from"),
        (
            svc("    external_links: [ciphera-postgres]\n"),
            "services.web.external_links",
        ),
        (svc("    container_name: ciphera-postgres\n"), "services.web.container_name"),
        (svc("    sysctls: {net.ipv4.ip_forward: 1}\n"), "services.web.sysctls"),
        (svc("    networks: [ciphera-internal]\n"), "services.web.networks"),
        (svc("    unknown_key: 1\n"), "services.web.unknown_key"),
        (svc("    env_file: ../../etc/shadow\n"), "services.web.env_file[0]"),
        (svc("    env_file: /etc/shadow\n"), "services.web.env_file[0]"),
        (svc("    env_file: ${SECRET_PATH}\n"), "services.web.env_file[0]"),
        (
            svc("    extends:\n      file: /etc/other.yaml\n      service: x\n"),
            "services.web.extends.file",
        ),
        (
            svc("    build:\n      context: ../..\n"),
            "services.web.build.context",
        ),
        (
            svc("    build:\n      context: /\n"),
            "services.web.build.context",
        ),
        (
            svc("    build: https://github.com/evil/repo.git\n"),
            "services.web.build.context",
        ),
        (
            svc("    build:\n      context: .\n      dockerfile: ../../../etc/Dockerfile\n"),
            "services.web.build.dockerfile",
        ),
        (
            svc("    deploy:\n      resources:\n        reservations:\n          devices: [{capabilities: [gpu]}]\n"),
            "services.web.deploy.resources.reservations.devices",
        ),
        (
            "include: [/etc/compose.yaml]\nservices: {}\n".to_string(),
            "include",
        ),
        (
            "services: {}\nnetworks:\n  host-net:\n    external: true\n    name: host\n"
                .to_string(),
            "networks",
        ),
        (
            "services: {}\nvolumes:\n  data:\n    external: true\n".to_string(),
            "volumes.data.external",
        ),
        (
            "services: {}\nvolumes:\n  data:\n    name: ciphera-postgres-data\n".to_string(),
            "volumes.data.name",
        ),
        (
            "services: {}\nvolumes:\n  data:\n    driver_opts: {type: none, o: bind, device: /etc}\n"
                .to_string(),
            "volumes.data.driver_opts",
        ),
        (
            "services: {}\nsecrets:\n  s:\n    file: /etc/shadow\n".to_string(),
            "secrets",
        ),
    ];
    for (text, at) in cases {
        match compose(&text) {
            Err(v) => assert_eq!(v.path, at, "{text}"),
            Ok(()) => panic!("accepte a tort :\n{text}"),
        }
    }

    // Forme normalisee : seul le reseau `default` pose par Compose passe.
    let root = Path::new("/repo");
    let normalized = br#"{"name":"shop","services":{"web":{"image":"nginx","networks":{"default":null}}},"networks":{"default":{"name":"shop_default","ipam":{}}},"volumes":{"data":{"name":"shop_data"}}}"#;
    let file = ComposeFile::from_normalized_json(normalized, root, root).unwrap();
    compose_policy::check(&file).unwrap();
    let hijack = br#"{"name":"shop","services":{"web":{"image":"nginx"}},"networks":{"default":{"name":"ciphera-internal","external":true}}}"#;
    let file = ComposeFile::from_normalized_json(hijack, root, root).unwrap();
    assert!(compose_policy::check(&file).is_err());
    let bind = br#"{"name":"shop","services":{"web":{"image":"nginx","volumes":[{"type":"bind","source":"/var/run/docker.sock","target":"/var/run/docker.sock"}]}}}"#;
    let file = ComposeFile::from_normalized_json(bind, root, root).unwrap();
    assert_eq!(
        compose_policy::check(&file).unwrap_err().path,
        "services.web.volumes[0]"
    );
}

#[tokio::test]
async fn buildkit_insecure_entitlement_refused() {
    // Compose ne peut demander ni entitlement ni reseau de build.
    for (extra, at) in [
        (
            "    build:\n      context: .\n      entitlements: [security.insecure]\n",
            "services.web.build.entitlements",
        ),
        (
            "    build:\n      context: .\n      entitlements: [network.host]\n",
            "services.web.build.entitlements",
        ),
        (
            "    build:\n      context: .\n      network: host\n",
            "services.web.build.network",
        ),
        (
            "    build:\n      context: .\n      privileged: true\n",
            "services.web.build.privileged",
        ),
    ] {
        let text = format!("services:\n  web:\n{extra}");
        assert_eq!(compose(&text).unwrap_err().path, at, "{text}");
    }

    // Aucune commande buildctl du pipeline ne demande d'entitlement, et
    // buildkitd n'en autorise aucun.
    for (method, repo) in [
        (BuildMethod::Railpack, vec![("package.json", "{}")]),
        (
            BuildMethod::Dockerfile {
                path: "Dockerfile".into(),
            },
            dockerfile_repo(),
        ),
    ] {
        let dir = BuildsDir::new();
        let backend = FakeBackend {
            repo,
            ..Default::default()
        };
        run(&spec(method), &backend, &dir).await.0.unwrap();
        let calls = backend.calls.lock().unwrap();
        let buildctl: Vec<_> = calls
            .iter()
            .filter(|c| c.program == AllowedBin::Buildctl.program())
            .collect();
        assert_eq!(buildctl.len(), 1);
        for c in buildctl {
            for a in &c.args {
                assert!(
                    !a.starts_with("--allow")
                        && !a.contains("security.insecure")
                        && !a.contains("network.host"),
                    "{c:?}"
                );
            }
        }
    }
    let infra = include_str!("../src/infra.rs");
    assert!(!infra.contains("\"--allow-insecure-entitlement\""));
}

#[tokio::test]
async fn build_already_in_progress_is_rejected_without_touching_its_workdir() {
    let dir = BuildsDir::new();
    let spec = spec(BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    });
    // Build en cours du meme deploiement : son dossier et un fichier.
    let workdir = format!("{}/{}", dir.0, spec.deployment_id);
    std::fs::create_dir_all(format!("{workdir}/src")).unwrap();
    std::fs::write(format!("{workdir}/src/en-cours"), "x").unwrap();
    let running = build::InFlight::claim(spec.deployment_id).expect("premier build");

    let backend = FakeBackend {
        repo: dockerfile_repo(),
        ..Default::default()
    };
    let (result, messages) = run(&spec, &backend, &dir).await;
    assert!(
        matches!(result, Err(BuildError::AlreadyRunning)),
        "{result:?}"
    );
    // Rien de lance, rien de rapporte (le build en cours rapporte le sien),
    // son dossier intact.
    assert!(backend.calls.lock().unwrap().is_empty());
    assert!(stages(&messages).is_empty(), "{messages:?}");
    assert!(Path::new(&format!("{workdir}/src/en-cours")).exists());

    // Une fois le premier termine, le deploiement peut etre reconstruit.
    drop(running);
    run(&spec, &backend, &dir).await.0.unwrap();
    assert!(!Path::new(&workdir).exists());
}

#[test]
fn compose_labels_in_the_ciphera_namespace_are_refused() {
    let service_map = "
services:
  web:
    image: nginx:1.27
    labels:
      dev.ciphera.project: other
";
    let service_list = "
services:
  web:
    image: nginx:1.27
    labels:
      - com.example.team=web
      - dev.ciphera.deployment=forged
";
    let build_labels = "
services:
  web:
    build:
      context: .
      labels:
        dev.ciphera.environment: production
";
    for text in [service_map, service_list, build_labels] {
        let err = compose(text).expect_err(text);
        assert!(err.path.contains("labels"), "{err}");
    }
    let service_list_err = compose(service_list).unwrap_err();
    assert!(
        service_list_err.reason.contains("dev.ciphera."),
        "{service_list_err}"
    );
    let service_map_err = compose(service_map).unwrap_err();
    assert!(
        service_map_err.reason.contains("dev.ciphera."),
        "{service_map_err}"
    );
    // Les autres etiquettes restent permises.
    compose(
        "
services:
  web:
    image: nginx:1.27
    labels:
      com.example.team: web
      ciphera.note: libre
",
    )
    .unwrap();
}

#[tokio::test]
async fn railpack_prepare_runs_in_the_sandbox_never_on_the_host() {
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: vec![("package.json", "{}")],
        ..Default::default()
    };
    let spec = spec(BuildMethod::Railpack);
    run(&spec, &backend, &dir).await.0.unwrap();
    // Aucun railpack lance sur l'hote.
    for call in backend.calls.lock().unwrap().iter() {
        assert!(!call.program.contains("railpack"), "{call:?}");
        assert!(!call.args.iter().any(|a| a == "prepare"), "{call:?}");
    }
    let prepared = backend.prepared.lock().unwrap();
    assert_eq!(prepared.len(), 1);
    let c = &prepared[0];
    assert_eq!(c.name, format!("ciphera-prepare-{}", spec.deployment_id));
    assert_eq!(c.image, SANDBOX_IMAGE);
    let host = c.body.host_config.as_ref().unwrap();
    assert_eq!(host.network_mode.as_deref(), Some("ciphera-prepare"));
    assert_eq!(c.body.user.as_deref(), Some("10003:10003"));
    assert_eq!(host.readonly_rootfs, Some(true));
    let binds = host.binds.as_ref().unwrap();
    let workdir = std::path::absolute(format!("{}/{}", dir.0, spec.deployment_id)).unwrap();
    let src = format!("{}:/src:ro", workdir.join("src").to_str().unwrap());
    let plan = format!("{}:/out:rw", workdir.join("plan").to_str().unwrap());
    assert!(binds.iter().any(|b| b.starts_with(&src)), "{binds:?}");
    assert!(binds.iter().any(|b| b.starts_with(&plan)), "{binds:?}");
    assert!(
        binds
            .iter()
            .any(|b| b.ends_with(&format!(":/tmp/railpack/mise/mise-{MISE_VERSION}:ro"))),
        "{binds:?}"
    );
    // Le jeton Git de l'ordre n'entre jamais dans le bac a sable.
    let debug = format!("{:?}", c.body);
    assert!(!debug.contains(TOKEN));
    assert!(!debug.contains("TOKEN"), "{debug}");
}

#[tokio::test]
async fn railpack_without_pinned_mise_or_sandbox_fails_clearly() {
    let dir = BuildsDir::new();
    std::fs::remove_file(dir.tools().join(format!("mise-{MISE_VERSION}"))).unwrap();
    let backend = FakeBackend {
        repo: vec![("package.json", "{}")],
        ..Default::default()
    };
    let (result, messages) = run(&spec(BuildMethod::Railpack), &backend, &dir).await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("epingle absent"), "{err}");
    assert!(backend.prepared.lock().unwrap().is_empty(), "aucun repli");
    assert!(backend.loaded.lock().unwrap().is_empty());
    assert!(matches!(
        stages(&messages).last(),
        Some((Stage::Build, StageStatus::Failed { message })) if message.contains("mise")
    ));

    // Sans compte ciphera-build : pas de bac a sable, build Railpack refuse.
    let dir = BuildsDir::new();
    let mut cfg = dir.config();
    cfg.prepare = None;
    let backend = FakeBackend {
        repo: vec![("package.json", "{}")],
        ..Default::default()
    };
    let spec = spec(BuildMethod::Railpack);
    let (tx, _rx) = mpsc::channel(1024);
    let sink = ProgressSink::new(spec.deployment_id, tx);
    let result = build::run(&spec, &cfg, &backend, &sink).await;
    assert!(
        matches!(result, Err(BuildError::Unavailable(_))),
        "{result:?}"
    );
    assert!(backend.prepared.lock().unwrap().is_empty());
}

// --- dossier racine (monorepo) ---------------------------------------------

fn with_root(build: BuildMethod, root: &str) -> BuildSpec {
    BuildSpec {
        root_directory: Some(root.into()),
        ..spec(build)
    }
}

/// Valeur `--opt`/`--local` d'un appel buildctl (`context=...`).
fn opt(call: &Call, key: &str) -> Option<String> {
    call.args
        .iter()
        .find_map(|a| a.strip_prefix(&format!("{key}=")).map(str::to_string))
}

/// `root_directory` est le contexte du build (Dockerfile, Railpack, Compose)
/// et le dossier ou ses fichiers sont cherches ; vide ou `.` : la racine.
#[tokio::test]
async fn root_directory_is_the_build_context() {
    let dockerfile = BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    };
    // Dockerfile : cherche dans le dossier racine, contexte = ce dossier.
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: vec![
            ("Dockerfile", "FROM scratch\n"),
            ("apps/web/Dockerfile", "FROM busybox\n"),
        ],
        ..Default::default()
    };
    let spec = with_root(dockerfile.clone(), "./apps/web/");
    run(&spec, &backend, &dir).await.0.unwrap();
    let src = format!("{}/{}/src", dir.0, spec.deployment_id);
    let calls = backend.calls.lock().unwrap().clone();
    let buildctl = calls
        .iter()
        .find(|c| c.program == AllowedBin::Buildctl.program())
        .unwrap();
    assert_eq!(
        opt(buildctl, "context"),
        Some(format!("{src}/apps/web")),
        "{buildctl:?}"
    );
    assert_eq!(opt(buildctl, "dockerfile"), Some(format!("{src}/apps/web")));

    // Railpack : `prepare` ne voit que le dossier racine, buildctl aussi.
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: vec![("apps/web/package.json", "{}"), ("secret.txt", "x")],
        ..Default::default()
    };
    let spec = with_root(BuildMethod::Railpack, "apps/web");
    run(&spec, &backend, &dir).await.0.unwrap();
    let src = format!("{}/{}/src", dir.0, spec.deployment_id);
    let abs = std::path::absolute(&src).unwrap().join("apps").join("web");
    let prepared = backend.prepared.lock().unwrap().clone();
    let binds = prepared[0]
        .body
        .host_config
        .as_ref()
        .unwrap()
        .binds
        .clone()
        .unwrap();
    let mount = format!("{}:/src:ro", abs.to_str().unwrap());
    assert!(binds.iter().any(|b| b.starts_with(&mount)), "{binds:?}");
    let calls = backend.calls.lock().unwrap().clone();
    let buildctl = calls
        .iter()
        .find(|c| c.program == AllowedBin::Buildctl.program())
        .unwrap();
    assert_eq!(opt(buildctl, "context"), Some(format!("{src}/apps/web")));

    // Compose : fichier cherche dans le dossier racine, projet Compose a
    // ce dossier.
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: vec![
            ("apps/web/compose.yaml", "services:\n  web:\n    build: .\n"),
            ("apps/web/Dockerfile", "FROM busybox\n"),
        ],
        compose_json: Some(r#"{"name":"web-production","services":{"web":{"build":{"context":".","dockerfile":"Dockerfile"},"networks":{"default":null}}},"networks":{"default":{"name":"web-production_default"}}}"#.into()),
        ..Default::default()
    };
    let spec = with_root(
        BuildMethod::Compose {
            path: "compose.yaml".into(),
            service: "web".into(),
        },
        "apps/web",
    );
    run(&spec, &backend, &dir).await.0.unwrap();
    let src = format!("{}/{}/src", dir.0, spec.deployment_id);
    let calls = backend.calls.lock().unwrap().clone();
    let config = calls
        .iter()
        .find(|c| c.args.iter().any(|a| a == "config"))
        .unwrap();
    assert!(
        config
            .args
            .windows(2)
            .any(|w| w[0] == "-f" && w[1] == format!("{src}/apps/web/compose.yaml")),
        "{config:?}"
    );

    // Vide ou `.` : racine du depot.
    for root in ["", "."] {
        let dir = BuildsDir::new();
        let backend = FakeBackend {
            repo: dockerfile_repo(),
            ..Default::default()
        };
        let spec = with_root(dockerfile.clone(), root);
        run(&spec, &backend, &dir).await.0.unwrap();
        let src = format!("{}/{}/src", dir.0, spec.deployment_id);
        let calls = backend.calls.lock().unwrap().clone();
        let buildctl = calls
            .iter()
            .find(|c| c.program == AllowedBin::Buildctl.program())
            .unwrap();
        assert_eq!(opt(buildctl, "context"), Some(src), "{root:?}");
    }
}

/// Dossier racine absolu, qui remonte, avec NUL ou caractere hors liste :
/// refuse avant tout clone. Absent du depot : echec clair du build.
#[tokio::test]
async fn root_directory_unsafe_or_missing_fails_the_build() {
    let dockerfile = BuildMethod::Dockerfile {
        path: "Dockerfile".into(),
    };
    for bad in ["/etc", "../x", "apps/../../x", "a\0b", "a b", "a//b", "-x"] {
        let dir = BuildsDir::new();
        let backend = FakeBackend {
            repo: dockerfile_repo(),
            ..Default::default()
        };
        let (result, messages) = run(&with_root(dockerfile.clone(), bad), &backend, &dir).await;
        assert!(
            matches!(result, Err(BuildError::Invalid(_))),
            "{bad:?} : {result:?}"
        );
        assert!(backend.calls.lock().unwrap().is_empty(), "{bad:?} : clone");
        assert!(
            matches!(
                stages(&messages).last(),
                Some((_, StageStatus::Failed { .. }))
            ),
            "{bad:?}"
        );
    }

    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: dockerfile_repo(),
        ..Default::default()
    };
    let (result, messages) = run(&with_root(dockerfile.clone(), "apps/web"), &backend, &dir).await;
    assert!(matches!(result, Err(BuildError::Path(_))), "{result:?}");
    assert!(
        matches!(
            stages(&messages).last(),
            Some((Stage::Build, StageStatus::Failed { message }))
                if message.contains("dossier racine") && message.contains("apps/web")
        ),
        "{messages:?}"
    );
    assert!(
        !backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.program == AllowedBin::Buildctl.program())
    );

    // Un fichier n'est pas un dossier racine.
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: dockerfile_repo(),
        ..Default::default()
    };
    let (result, _) = run(&with_root(dockerfile, "Dockerfile"), &backend, &dir).await;
    assert!(matches!(result, Err(BuildError::Path(_))), "{result:?}");
}

/// Dossier racine qui est un lien symbolique hors du clone : refuse apres
/// resolution.
#[cfg(unix)]
#[tokio::test]
async fn root_directory_symlink_outside_the_clone_is_refused() {
    let dir = BuildsDir::new();
    let backend = FakeBackend {
        repo: dockerfile_repo(),
        links: vec![("apps", "/etc")],
        ..Default::default()
    };
    let spec = with_root(
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
        "apps",
    );
    let (result, _) = run(&spec, &backend, &dir).await;
    assert!(matches!(result, Err(BuildError::Path(_))), "{result:?}");
    assert!(
        !backend
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.program == AllowedBin::Buildctl.program())
    );
}

// --- cloisonnement des caches de build par projet --------------------------

/// Appels buildctl d'un faux backend.
fn buildctl_calls(backend: &FakeBackend) -> Vec<Call> {
    backend
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.program == AllowedBin::Buildctl.program())
        .cloned()
        .collect()
}

/// Valeurs des `--opt` d'un appel buildctl.
fn opts(call: &Call) -> Vec<String> {
    call.args
        .windows(2)
        .filter(|w| w[0] == "--opt")
        .map(|w| w[1].clone())
        .collect()
}

const TWO_SERVICES_JSON: &str = r#"{"name":"web-production","services":{"web":{"build":{"context":".","dockerfile":"Dockerfile"},"networks":{"default":null}},"worker":{"build":{"context":".","dockerfile":"Dockerfile"},"networks":{"default":null}}},"networks":{"default":{"name":"web-production_default"}}}"#;
const ONE_SERVICE_JSON: &str = r#"{"name":"web-production","services":{"web":{"build":{"context":".","dockerfile":"Dockerfile"},"networks":{"default":null}}},"networks":{"default":{"name":"web-production_default"}}}"#;

/// Chaque build Dockerfile ou Compose porte l'espace de noms des caches de
/// montage, et chaque build la cle des caches Railpack : tous deux
/// `<UUID du projet>-<UUID de l'environnement>`, jamais les noms.
#[tokio::test]
async fn buildctl_namespaces_caches_with_the_project_uuids() {
    for (method, repo, compose, builds) in [
        (
            BuildMethod::Dockerfile {
                path: "Dockerfile".into(),
            },
            dockerfile_repo(),
            None,
            1,
        ),
        (
            BuildMethod::Compose {
                path: "compose.yaml".into(),
                service: "web".into(),
            },
            vec![
                (
                    "compose.yaml",
                    "services:\n  web:\n    build: .\n  worker:\n    build: .\n",
                ),
                ("Dockerfile", "FROM busybox\n"),
            ],
            Some(TWO_SERVICES_JSON.to_string()),
            2,
        ),
        (BuildMethod::Railpack, vec![("package.json", "{}")], None, 1),
    ] {
        let dir = BuildsDir::new();
        let backend = FakeBackend {
            repo,
            compose_json: compose,
            ..Default::default()
        };
        let railpack = matches!(method, BuildMethod::Railpack);
        let spec = spec(method);
        run(&spec, &backend, &dir).await.0.unwrap();
        let project = spec.project_id.unwrap().to_string();
        let env = spec.environment_id.unwrap().to_string();
        let calls = buildctl_calls(&backend);
        assert_eq!(calls.len(), builds, "{calls:?}");
        for call in &calls {
            let opts = opts(call);
            let key = format!("build-arg:cache-key={project}-{env}");
            assert_eq!(opts.iter().filter(|o| **o == key).count(), 1, "{opts:?}");
            let ns: Vec<_> = opts
                .iter()
                .filter(|o| o.starts_with("build-arg:BUILDKIT_CACHE_MOUNT_NS="))
                .collect();
            if railpack {
                assert!(ns.is_empty(), "{opts:?}");
            } else {
                assert_eq!(
                    ns,
                    [&format!(
                        "build-arg:BUILDKIT_CACHE_MOUNT_NS={project}-{env}"
                    )],
                    "{opts:?}"
                );
            }
            // Ni le nom du projet ni celui de l'environnement en cle.
            assert!(
                !opts.iter().any(|o| o.contains("web-production")),
                "{opts:?}"
            );
        }
    }
}

/// Ordre sans identifiant de projet ou d'environnement : build refuse avant
/// le clone (le plan de controle v6 les envoie toujours).
#[tokio::test]
async fn build_without_project_or_environment_uuid_is_refused() {
    for method in [
        BuildMethod::Dockerfile {
            path: "Dockerfile".into(),
        },
        BuildMethod::Railpack,
        BuildMethod::Compose {
            path: "compose.yaml".into(),
            service: "web".into(),
        },
    ] {
        for (project, env) in [
            (None, Some(EnvironmentId::new())),
            (Some(ProjectId::new()), None),
        ] {
            let dir = BuildsDir::new();
            let backend = FakeBackend {
                repo: dockerfile_repo(),
                ..Default::default()
            };
            let spec = BuildSpec {
                project_id: project,
                environment_id: env,
                ..spec(method.clone())
            };
            let (result, messages) = run(&spec, &backend, &dir).await;
            let err = result.unwrap_err();
            assert!(matches!(err, BuildError::Invalid(_)), "{err:?}");
            assert!(err.to_string().contains("identifiant"), "{err}");
            assert!(backend.calls.lock().unwrap().is_empty(), "aucun clone");
            assert!(matches!(
                stages(&messages).last(),
                Some((_, StageStatus::Failed { .. }))
            ));
        }
    }
}

/// Directive `syntax` d'un Dockerfile : seul `docker/dockerfile` (version
/// 1.4 ou plus, sans empreinte) est accepte ; tout autre frontend est
/// refuse avant BuildKit, sous toutes les formes que BuildKit lit.
#[test]
fn dockerfile_syntax_directive_allows_only_docker_dockerfile() {
    for ok in [
        "FROM busybox\n",
        "# syntax=docker/dockerfile:1\nFROM busybox\n",
        "# syntax = docker/dockerfile:1.7\nFROM busybox\n",
        "#syntax=docker.io/docker/dockerfile:1.4.3\r\nFROM busybox\n",
        "# check=error=true\n# syntax=docker/dockerfile\nFROM busybox\n",
        "# escape=`\n\nFROM busybox\n",
        "# un commentaire\nFROM busybox\n",
        "# syntax=docker/dockerfile:latest\nFROM busybox\n",
        "FROM busybox\n# syntax=someone/frontend\n",
    ] {
        dockerfile::check_syntax(ok).unwrap_or_else(|e| panic!("{ok:?} : {e}"));
    }
    for bad in [
        "# syntax=docker.io/someone/frontend\nFROM busybox\n",
        "# syntax=someone/frontend:1\nFROM busybox\n",
        "#SYNTAX=ghcr.io/someone/frontend\nFROM busybox\n",
        "\u{feff}# syntax=someone/frontend\nFROM busybox\n",
        "#!/usr/bin/env buildctl\n# syntax=someone/frontend\nFROM busybox\n",
        "// syntax=someone/frontend\nFROM busybox\n",
        "# check=skip=all\n#  syntax  =  someone/frontend  arg\nFROM busybox\n",
        "{\"syntax\": \"someone/frontend\"}",
        "# syntax=\nFROM busybox\n",
        // Frontend Railpack (autorise par buildkitd pour les builds Railpack) :
        // jamais depuis un Dockerfile.
        "# syntax=ghcr.io/railwayapp/railpack-frontend:v0.43.0\nFROM busybox\n",
        // Anciennes versions (sans espace de noms des caches), empreinte
        // (qui l'emporte sur l'etiquette), variantes.
        "# syntax=docker/dockerfile:1.0.0-experimental\nFROM busybox\n",
        "# syntax=docker/dockerfile:1.2\nFROM busybox\n",
        "# syntax=docker/dockerfile:1.7-labs\nFROM busybox\n",
        "# syntax=docker/dockerfile:1@sha256:0000000000000000000000000000000000000000000000000000000000000000\nFROM busybox\n",
        "# syntax=docker/dockerfile-upstream:master\nFROM busybox\n",
        "# syntax=docker/dockerfile:0.9\nFROM busybox\n",
        "# syntax=evil.example/docker/dockerfile:1\nFROM busybox\n",
    ] {
        let err = dockerfile::check_syntax(bad).unwrap_err();
        assert!(err.to_string().contains("frontend"), "{bad:?} : {err}");
    }
}

/// Le pipeline refuse un Dockerfile (ou celui d'un service Compose) qui
/// demande un autre frontend, avec un message clair et sans lancer buildctl.
#[tokio::test]
async fn foreign_frontend_in_a_dockerfile_fails_the_build_clearly() {
    let foreign = "# syntax=docker.io/someone/frontend\nFROM busybox\n";
    for (method, repo, compose) in [
        (
            BuildMethod::Dockerfile {
                path: "Dockerfile".into(),
            },
            vec![("Dockerfile", foreign)],
            None,
        ),
        (
            BuildMethod::Compose {
                path: "compose.yaml".into(),
                service: "web".into(),
            },
            vec![
                ("compose.yaml", "services:\n  web:\n    build: .\n"),
                ("Dockerfile", foreign),
            ],
            Some(ONE_SERVICE_JSON.to_string()),
        ),
    ] {
        let dir = BuildsDir::new();
        let backend = FakeBackend {
            repo,
            compose_json: compose,
            ..Default::default()
        };
        let (result, messages) = run(&spec(method), &backend, &dir).await;
        let err = result.unwrap_err();
        assert!(matches!(err, BuildError::Frontend(_)), "{err:?}");
        assert!(buildctl_calls(&backend).is_empty());
        assert!(matches!(
            stages(&messages).last(),
            Some((Stage::Build, StageStatus::Failed { message })) if message.contains("frontend")
        ));
    }
}
