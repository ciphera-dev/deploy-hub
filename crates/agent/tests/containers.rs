//! Specifications des conteneurs, verifiees sans Docker : infrastructure
//! (images epinglees, PostgreSQL sur le reseau interne, buildkitd rootless
//! sur son propre reseau, pas de TCP pour buildkitd, mot de passe jamais en
//! variable d'environnement, montages etiquetes pour SELinux) et
//! applications (reseau du projet, aucun port publie, aucun privilege ni
//! montage de l'hote, NET_RAW et MKNOD retires, nombre de processus borne).

mod common;

use std::path::PathBuf;

use bollard::models::{ContainerCreateBody, HostConfig, MountTypeEnum};
use ciphera_agent::docker::DockerError;
use ciphera_agent::docker::{self, ContainerSpec};
use ciphera_agent::infra::{self, BuildkitAccount, InfraConfig, InfraError, SubIds};
use common::FakeDocker;
use proto::ImageRef;
use shared::ids::{DeploymentId, ProjectId};

fn cfg() -> InfraConfig {
    InfraConfig {
        postgres_secret_dir: PathBuf::from("/etc/ciphera/postgres"),
        postgres_socket_dir: PathBuf::from("/run/ciphera/postgres"),
        postgres_user: (991, 991),
        buildkit_socket_dir: PathBuf::from("/run/ciphera-agent/buildkit"),
        buildkit_apparmor_profile: None,
        buildkit_account: Some(account()),
        buildkit_state_dir: PathBuf::from("/var/lib/ciphera-buildkit"),
        buildkit_identity_dir: PathBuf::from("/var/lib/ciphera-agent/buildkit-identity"),
    }
}

fn account() -> BuildkitAccount {
    BuildkitAccount {
        user: (995, 994),
        subuids: SubIds {
            start: 165536,
            count: 65536,
        },
        subgids: SubIds {
            start: 231072,
            count: 65536,
        },
    }
}

/// Dossier temporaire propre a un test (supprime a la fin).
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ciphera-agent-{tag}-{}-{}",
            std::process::id(),
            DeploymentId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn digest_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../deploy/containers")
        .join(name);
    std::fs::read_to_string(path).unwrap().trim().to_string()
}

fn host(body: &ContainerCreateBody) -> &HostConfig {
    body.host_config.as_ref().expect("host_config")
}

fn assert_pinned(image: &str) {
    let (_, digest) = image.split_once("@sha256:").expect("image sans digest");
    assert_eq!(digest.len(), 64, "{image}");
    assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()), "{image}");
}

fn assert_no_published_ports(body: &ContainerCreateBody) {
    let hc = host(body);
    assert!(
        hc.port_bindings.as_ref().is_none_or(|p| p.is_empty()),
        "{:?}",
        hc.port_bindings
    );
    assert_ne!(hc.publish_all_ports, Some(true));
    assert!(body.exposed_ports.as_ref().is_none_or(|p| p.is_empty()));
}

fn security_opt(body: &ContainerCreateBody) -> Vec<String> {
    host(body).security_opt.clone().unwrap_or_default()
}

fn assert_no_new_privileges(body: &ContainerCreateBody) {
    let opts = security_opt(body);
    assert!(
        opts.iter().any(|o| o == "no-new-privileges:true"),
        "{opts:?}"
    );
}

fn binds(body: &ContainerCreateBody) -> Vec<String> {
    host(body).binds.clone().unwrap_or_default()
}

fn spec_named(name: &str) -> ContainerSpec {
    infra::infra_specs(&cfg())
        .unwrap()
        .into_iter()
        .find(|s| s.name() == name)
        .unwrap()
}

#[tokio::test]
async fn ensure_infra_uses_pinned_digest() {
    let docker = FakeDocker::default();
    let scratch = Scratch::new("pinned");
    let mut c = cfg();
    c.buildkit_identity_dir = scratch.0.join("identity");
    infra::ensure_infra(&docker, &c).await.unwrap();

    let runs = docker.runs.lock().unwrap().clone();
    let names: Vec<&str> = runs.iter().map(|s| s.name()).collect();
    assert_eq!(names, ["ciphera-postgres", "ciphera-buildkitd"]);
    for (spec, file) in runs.iter().zip(["postgres.digest", "buildkit.digest"]) {
        assert!(spec.image().contains("@sha256:"), "{}", spec.image());
        assert_pinned(spec.image());
        assert_eq!(spec.image(), digest_file(file));
        let body = docker::create_body(spec);
        assert_eq!(body.image.as_deref(), Some(spec.image()));
        assert_no_published_ports(&body);
        assert_ne!(host(&body).privileged, Some(true), "{}", spec.name());
        assert!(host(&body).restart_policy.is_some());
    }
    assert_eq!(runs[0].network(), infra::INFRA_NETWORK);
    assert_eq!(runs[1].network(), infra::BUILD_NETWORK);
    // BuildKit rootless : image `-rootless` de la meme version.
    assert!(
        runs[1]
            .image()
            .starts_with("docker.io/moby/buildkit:v0.35.1-rootless@sha256:"),
        "{}",
        runs[1].image()
    );
    // Les reseaux existent avant le premier conteneur.
    let calls = docker.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            "network:ciphera-internal",
            "network:ciphera-build",
            "run:ciphera-postgres",
            "run:ciphera-buildkitd"
        ]
    );
}

#[test]
fn infra_network_is_internal_and_project_networks_are_not() {
    assert_eq!(infra::INFRA_NETWORK, "ciphera-internal");
    let internal = docker::network_create_request("ciphera-internal");
    assert_eq!(internal.internal, Some(true));
    // Reseau de buildkitd : sortie vers Internet (images de base, git).
    assert_eq!(infra::BUILD_NETWORK, "ciphera-build");
    let build = docker::network_create_request("ciphera-build");
    assert_eq!(build.internal, Some(false));
    for request in [&internal, &build] {
        let labels = request.labels.clone().unwrap_or_default();
        assert_eq!(
            labels.get(docker::MANAGED_LABEL).map(String::as_str),
            Some("true")
        );
    }
    let project = docker::project_network(ProjectId::new());
    assert!(project.starts_with("ciphera-p-"), "{project}");
    assert_eq!(
        docker::network_create_request(&project).internal,
        Some(false)
    );
    // Pont de buildkitd nomme comme le reseau : le filtrage de sortie
    // (ciphera-agent.service) le reconnait a ce nom d'interface.
    let bridge = |r: &bollard::models::NetworkCreateRequest| {
        r.options
            .as_ref()
            .and_then(|o| o.get(docker::BRIDGE_NAME_OPTION).cloned())
    };
    assert_eq!(bridge(&build).as_deref(), Some("ciphera-build"));
    assert_eq!(bridge(&internal), None);
    assert_eq!(bridge(&docker::network_create_request(&project)), None);
}

/// Filtrage de la sortie des builds, charge (root) avant chaque demarrage de
/// l'agent : les deux ponts de build, l'hote (chaine input) et les plages
/// privees ou de metadonnees (chaine forward) ; jamais ignore en cas
/// d'echec (pas de prefixe `-`).
#[test]
fn agent_unit_loads_the_build_egress_filter_first() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../deploy/systemd/ciphera-agent.service");
    let unit = std::fs::read_to_string(path)
        .unwrap()
        .replace("\\\r\n", "")
        .replace("\\\n", "");
    let pre: Vec<&str> = unit
        .lines()
        .filter_map(|l| l.strip_prefix("ExecStartPre="))
        .collect();
    assert_eq!(pre.len(), 1, "{pre:?}");
    let rules = pre[0]
        .strip_prefix("+/usr/sbin/nft ")
        .expect("nft en root, echec bloquant");
    for bridge in [
        infra::BUILD_NETWORK,
        ciphera_agent::build::sandbox::PREPARE_NETWORK,
    ] {
        assert!(rules.contains(&format!("\\\"{bridge}\\\"")), "{bridge}");
    }
    assert!(rules.contains("hook input"));
    assert!(rules.contains("hook forward"));
    for range in [
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "169.254.0.0/16",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "168.63.129.16",
        "fc00::/7",
        "fe80::/10",
    ] {
        assert!(rules.contains(range), "{range}");
    }
    // Exceptions : le DNS (resolveurs prives des clouds) et la decouverte
    // de voisins IPv6 ; aucune autre regle n'accepte.
    let accepts: Vec<&str> = rules
        .split(';')
        .map(str::trim)
        .filter(|r| r.ends_with(" accept") && !r.starts_with("policy"))
        .collect();
    assert_eq!(accepts.len(), 3, "{accepts:?}");
    for rule in accepts {
        assert!(
            rule.contains("meta l4proto { tcp, udp } th dport 53 accept")
                || rule.contains("icmpv6 type"),
            "{rule}"
        );
    }
    // Une seule transaction : table remplacee, jamais doublee.
    assert!(
        rules.starts_with("\"add table inet ciphera_egress; delete table inet ciphera_egress; "),
        "{rules}"
    );
}

#[test]
fn buildkitd_listens_only_on_a_unix_socket() {
    let body = docker::create_body(&spec_named("ciphera-buildkitd"));
    let args = body.cmd.clone().unwrap_or_default();
    let addr = args
        .iter()
        .position(|a| a == "--addr")
        .map(|i| args[i + 1].clone())
        .expect("--addr");
    assert_eq!(addr, "unix:///run/buildkit/buildkitd.sock");
    assert_eq!(args.iter().filter(|a| *a == "--addr").count(), 1);
    assert!(!args.iter().any(|a| a.contains("tcp://")), "{args:?}");
    // Dossier monte pour l'agent, etiquete pour SELinux (`z`).
    assert!(
        binds(&body).contains(&"/run/ciphera-agent/buildkit:/run/buildkit:rw,z".to_string()),
        "{:?}",
        binds(&body)
    );
    // Groupe du socket laisse tel quel (`chown(uid, -1)`) : il herite du
    // groupe ciphera-agent du dossier setgid, sans ACL.
    assert_eq!(
        args.iter()
            .filter(|a| a.starts_with("--group"))
            .collect::<Vec<_>>(),
        ["--group=-1"]
    );
    // Aucun entitlement dangereux autorise.
    assert!(
        !args.iter().any(|a| a.contains("insecure-entitlement")),
        "{args:?}"
    );
}

#[test]
fn buildkitd_is_rootless_unprivileged_and_on_its_own_network() {
    let body = docker::create_body(&spec_named("ciphera-buildkitd"));
    let hc = host(&body);
    assert_ne!(hc.privileged, Some(true));
    assert!(hc.devices.as_ref().is_none_or(|d| d.is_empty()));
    // Configuration Docker recommandee par BuildKit (docs/rootless.md) :
    // seccomp et AppArmor non confines, `systempaths=unconfined` (cote API :
    // `MaskedPaths` et `ReadonlyPaths` vides) pour que chaque etape RUN ait
    // son propre /proc. Le bac a sable des processus reste actif : jamais
    // `--oci-worker-no-process-sandbox` (une etape pourrait tuer ou tracer
    // les processus des autres builds).
    let opts = security_opt(&body);
    assert!(opts.contains(&"seccomp=unconfined".to_string()), "{opts:?}");
    assert!(
        opts.contains(&"apparmor=unconfined".to_string()),
        "{opts:?}"
    );
    assert_eq!(hc.masked_paths.as_deref(), Some(&[][..]));
    assert_eq!(hc.readonly_paths.as_deref(), Some(&[][..]));
    let args = body.cmd.clone().unwrap_or_default();
    assert!(
        !args.iter().any(|a| a.contains("no-process-sandbox")),
        "{args:?}"
    );
    // Exception documentee : newuidmap/newgidmap (setuid) exigent que
    // no-new-privileges soit absent pour ce seul conteneur.
    assert!(
        !opts.iter().any(|o| o.starts_with("no-new-privileges")),
        "{opts:?}"
    );
    // Son reseau, jamais celui de PostgreSQL ni d'un projet.
    assert_eq!(hc.network_mode.as_deref(), Some(infra::BUILD_NETWORK));
    assert!(body.networking_config.is_none());
    // Profil AppArmor installe par install.sh (Ubuntu >= 23.10) : utilise.
    let mut with_profile = cfg();
    with_profile.buildkit_apparmor_profile = Some("ciphera-buildkitd".into());
    let specs = infra::infra_specs(&with_profile).unwrap();
    let opts = security_opt(&docker::create_body(&specs[1]));
    assert!(
        opts.contains(&"apparmor=ciphera-buildkitd".to_string()),
        "{opts:?}"
    );
    assert!(
        !opts.contains(&"apparmor=unconfined".to_string()),
        "{opts:?}"
    );
}

#[test]
fn buildkitd_runs_under_the_dedicated_build_account() {
    let body = docker::create_body(&spec_named("ciphera-buildkitd"));
    let hc = host(&body);
    // uid:gid du compte systeme ciphera-build (jamais l'uid 1000 de l'image).
    assert_eq!(body.user.as_deref(), Some("995:994"));
    // rootlesskit et newuidmap resolvent le nom et la plage de sous-uids dans
    // des fichiers minimaux generes pour ce compte, montes en lecture seule.
    let id = PathBuf::from("/var/lib/ciphera-agent/buildkit-identity");
    let binds = binds(&body);
    for file in ["passwd", "group", "subuid", "subgid"] {
        // (separateur de chemin de la plateforme : `/` sous Linux)
        let bind = format!("{}:/etc/{file}:ro,z", id.join(file).to_string_lossy());
        assert!(binds.contains(&bind), "{bind} absent de {binds:?}");
    }
    // Etat de BuildKit : dossier de l'hote propre au compte, HOME du compte.
    assert!(
        binds.contains(&"/var/lib/ciphera-buildkit:/home/ciphera-build:rw,z".to_string()),
        "{binds:?}"
    );
    assert_eq!(binds.len(), 6, "{binds:?}");
    // Plus de volume nomme (il appartiendrait a l'uid 1000 de l'image).
    assert!(
        hc.mounts.as_ref().is_none_or(|m| m.is_empty()),
        "{:?}",
        hc.mounts
    );
    let env = body.env.clone().unwrap_or_default();
    for e in [
        "HOME=/home/ciphera-build",
        "USER=ciphera-build",
        "TMPDIR=/tmp",
        "XDG_RUNTIME_DIR=/tmp",
    ] {
        assert!(env.contains(&e.to_string()), "{e} absent de {env:?}");
    }
    // Seules capacites gardees : celles de newuidmap/newgidmap.
    assert_eq!(hc.cap_drop.clone().unwrap_or_default(), ["ALL"]);
    assert_eq!(hc.cap_add.clone().unwrap_or_default(), ["SETUID", "SETGID"]);
    // Plage de sous-uids visible (et comprise dans l'empreinte).
    let labels = body.labels.clone().unwrap_or_default();
    assert_eq!(
        labels.get(infra::SUBIDS_LABEL).map(String::as_str),
        Some("subuid=165536:65536 subgid=231072:65536")
    );
    // Ni privilege, ni bac a sable des processus desactive.
    assert_ne!(hc.privileged, Some(true));
    let args = body.cmd.clone().unwrap_or_default();
    assert!(
        !args.iter().any(|a| a.contains("no-process-sandbox")),
        "{args:?}"
    );
}

#[test]
fn buildkit_identity_files_describe_only_the_build_account() {
    let files = infra::buildkit_identity(&account());
    let get = |name: &str| {
        files
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| c.as_str())
            .unwrap_or_else(|| panic!("{name} absent"))
    };
    // root : uid 0 dans l'espace de noms de rootlesskit (git, ssh le
    // resolvent) ; ciphera-build : compte de l'hote qui lance rootlesskit.
    assert_eq!(
        get("passwd"),
        "root:x:0:0:root:/root:/sbin/nologin\n\
         ciphera-build:x:995:994::/home/ciphera-build:/sbin/nologin\n"
    );
    assert_eq!(get("group"), "root:x:0:\nciphera-build:x:994:\n");
    assert_eq!(get("subuid"), "ciphera-build:165536:65536\n");
    assert_eq!(get("subgid"), "ciphera-build:231072:65536\n");
    assert_eq!(files.len(), 4);
}

#[test]
fn subid_lookup_finds_the_range_by_name_only() {
    let text = "# commentaire\n\
                runner:100000:65536\n\
                ciphera-build:165536:65536\n\
                ciphera-build:300000:10\n";
    assert_eq!(
        infra::lookup_subids(text, "ciphera-build"),
        Some(SubIds {
            start: 165536,
            count: 65536
        })
    );
    // Comme install.sh (recherche par nom) : une ligne a l'uid en chiffres
    // ne compte pas.
    assert_eq!(
        infra::lookup_subids("995:231072:65536\n", "ciphera-build"),
        None
    );
    for bad in [
        "",
        "ciphera-build:x:65536\n",
        "ciphera-build:1:0\n",
        "ciphera-build:4294967295:2\n",
        "ciphera-buildx:165536:65536\n",
    ] {
        assert_eq!(infra::lookup_subids(bad, "ciphera-build"), None, "{bad:?}");
    }
}

fn write_etc(dir: &std::path::Path, passwd: &str, subuid: &str, subgid: &str) {
    std::fs::write(dir.join("passwd"), passwd).unwrap();
    std::fs::write(dir.join("subuid"), subuid).unwrap();
    std::fs::write(dir.join("subgid"), subgid).unwrap();
}

#[test]
fn resolve_reads_the_build_account_and_its_subid_ranges() {
    let etc = Scratch::new("etc");
    let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                  ciphera-postgres:x:991:990::/nonexistent:/usr/sbin/nologin\n\
                  ciphera-build:x:995:994::/nonexistent:/usr/sbin/nologin\n";
    write_etc(
        &etc.0,
        passwd,
        "runner:100000:65536\nciphera-build:165536:65536\n",
        "runner:100000:65536\nciphera-build:231072:65536\n",
    );
    let c = InfraConfig::resolve(&etc.0).unwrap();
    assert_eq!(c.postgres_user, (991, 990));
    assert!(c.builds_available());
    let a = c.buildkit_account.unwrap();
    assert_eq!(a.user, (995, 994));
    assert_eq!(
        a.subuids,
        SubIds {
            start: 165536,
            count: 65536
        }
    );
    assert_eq!(a.subgids.start, 231072);
    assert_eq!(
        c.buildkit_state_dir,
        PathBuf::from(infra::DEFAULT_BUILDKIT_STATE_DIR)
    );
    assert_eq!(
        c.buildkit_identity_dir,
        PathBuf::from(infra::DEFAULT_BUILDKIT_IDENTITY_DIR)
    );

    // Compte absent, plage absente, compte root : refus explicite pour
    // buildkitd ; PostgreSQL (dont le plan de controle a besoin) reste
    // configure et seuls les builds sont indisponibles.
    write_etc(&etc.0, "ciphera-postgres:x:991:990::/:/x\n", "", "");
    assert!(matches!(
        infra::resolve_buildkit_account(&etc.0),
        Err(InfraError::MissingAccount("ciphera-build"))
    ));
    let c = InfraConfig::resolve(&etc.0).unwrap();
    assert_eq!(c.postgres_user, (991, 990));
    assert!(c.buildkit_account.is_none());
    assert!(!c.builds_available());
    write_etc(
        &etc.0,
        passwd,
        "ciphera-build:165536:65536\n",
        "runner:1:2\n",
    );
    assert!(matches!(
        infra::resolve_buildkit_account(&etc.0),
        Err(InfraError::MissingSubIds("subgid"))
    ));
    assert!(!InfraConfig::resolve(&etc.0).unwrap().builds_available());
    write_etc(
        &etc.0,
        "ciphera-postgres:x:991:990::/:/x\nciphera-build:x:0:0::/:/x\n",
        "ciphera-build:165536:65536\n",
        "ciphera-build:165536:65536\n",
    );
    assert!(matches!(
        infra::resolve_buildkit_account(&etc.0),
        Err(InfraError::RootAccount("ciphera-build"))
    ));
    assert!(!InfraConfig::resolve(&etc.0).unwrap().builds_available());
    // Sans le compte de PostgreSQL, l'agent ne peut rien faire : erreur.
    write_etc(&etc.0, &passwd.replace("ciphera-postgres", "x"), "", "");
    assert!(matches!(
        InfraConfig::resolve(&etc.0),
        Err(InfraError::MissingAccount("ciphera-postgres"))
    ));
}

#[tokio::test]
async fn without_build_account_only_postgres_runs_and_buildkitd_is_stopped() {
    let docker = FakeDocker::default();
    let mut c = cfg();
    c.buildkit_account = None;
    infra::ensure_infra(&docker, &c).await.unwrap();
    let calls = docker.calls.lock().unwrap().clone();
    // Un buildkitd d'une configuration precedente (uid 1000) est arrete.
    assert_eq!(
        calls,
        [
            "network:ciphera-internal",
            "run:ciphera-postgres",
            "stop:ciphera-buildkitd"
        ]
    );
    let specs = infra::infra_specs(&c).unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name(), "ciphera-postgres");
}

#[tokio::test]
async fn ensure_infra_writes_the_identity_files_readable_by_the_container() {
    let scratch = Scratch::new("identity");
    let mut c = cfg();
    c.buildkit_identity_dir = scratch.0.join("identity");
    let docker = FakeDocker::default();
    infra::ensure_infra(&docker, &c).await.unwrap();
    for (name, content) in infra::buildkit_identity(&account()) {
        let path = c.buildkit_identity_dir.join(name);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o644, "{name}");
        }
    }
    // Idempotent ; une plage modifiee reecrit le fichier.
    infra::ensure_infra(&docker, &c).await.unwrap();
    c.buildkit_account.as_mut().unwrap().subuids.start = 300000;
    infra::ensure_infra(&docker, &c).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(c.buildkit_identity_dir.join("subuid")).unwrap(),
        "ciphera-build:300000:65536\n"
    );
    // Aucun fichier temporaire laisse.
    let names: Vec<String> = std::fs::read_dir(&c.buildkit_identity_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 4, "{names:?}");
}

#[test]
fn postgres_reads_its_password_from_a_file_and_listens_on_a_socket_only() {
    let specs = infra::infra_specs(&cfg()).unwrap();
    let pg = specs
        .iter()
        .find(|s| s.name() == "ciphera-postgres")
        .unwrap();
    let body = docker::create_body(pg);
    let env = body.env.clone().unwrap_or_default();
    assert!(
        env.contains(&"POSTGRES_PASSWORD_FILE=/run/secrets/ciphera/db_password".to_string()),
        "{env:?}"
    );
    assert!(
        !env.iter().any(|e| e.starts_with("POSTGRES_PASSWORD=")),
        "{env:?}"
    );
    // Authentification par mot de passe aussi sur le socket local.
    assert!(
        env.iter()
            .any(|e| e.starts_with("POSTGRES_INITDB_ARGS=")
                && e.contains("--auth-local=scram-sha-256")),
        "{env:?}"
    );
    let hc = host(&body);
    // Montages de l'hote : secret en lecture seule, tous etiquetes pour
    // SELinux (`z`) afin que le conteneur les lise aussi sous Fedora.
    let binds = binds(&body);
    assert!(
        binds.contains(&"/etc/ciphera/postgres:/run/secrets/ciphera:ro,z".to_string()),
        "{binds:?}"
    );
    assert!(
        binds.contains(&"/run/ciphera/postgres:/var/run/postgresql:rw,z".to_string()),
        "{binds:?}"
    );
    assert_eq!(binds.len(), 2, "{binds:?}");
    let mounts = hc.mounts.clone().unwrap_or_default();
    assert!(mounts.iter().all(|m| m.typ == Some(MountTypeEnum::VOLUME)));
    assert!(mounts.iter().any(|m| {
        m.typ == Some(MountTypeEnum::VOLUME) && m.source.as_deref() == Some("ciphera-postgres-data")
    }));
    // Compte non root dedie, aucune capacite.
    assert_eq!(body.user.as_deref(), Some("991:991"));
    assert_eq!(hc.cap_drop.clone().unwrap_or_default(), ["ALL"]);
    assert_no_new_privileges(&body);
    assert!(hc.cap_add.as_ref().is_none_or(|c| c.is_empty()));
    assert_ne!(hc.privileged, Some(true));
    // Pas d'ecoute TCP.
    let cmd = body.cmd.clone().unwrap_or_default();
    assert_eq!(cmd, ["postgres", "-c", "listen_addresses="]);
}

fn app_spec(project: ProjectId) -> ContainerSpec {
    ContainerSpec::app(
        DeploymentId::new(),
        project,
        &ImageRef("ghcr.io/acme/web:1".into()),
        vec![("PORT".into(), "3000".into())],
    )
}

#[test]
fn app_containers_on_project_network_without_published_ports() {
    let project = ProjectId::new();
    let spec = app_spec(project);
    assert_eq!(spec.network(), format!("ciphera-p-{project}"));
    let body = docker::create_body(&spec);
    let hc = host(&body);
    assert_eq!(
        hc.network_mode.as_deref(),
        Some(format!("ciphera-p-{project}").as_str())
    );
    assert_ne!(hc.network_mode.as_deref(), Some(infra::INFRA_NETWORK));
    assert!(
        body.networking_config
            .as_ref()
            .and_then(|n| n.endpoints_config.as_ref())
            .is_none_or(|e| !e.contains_key(infra::INFRA_NETWORK))
    );
    assert_no_published_ports(&body);
    assert_eq!(body.env.clone().unwrap_or_default(), ["PORT=3000"]);
}

#[test]
fn app_container_spec_has_no_new_privileges_and_no_host_mounts() {
    let body = docker::create_body(&app_spec(ProjectId::new()));
    assert_no_new_privileges(&body);
    let hc = host(&body);
    assert!(
        hc.mounts.as_ref().is_none_or(|m| m.is_empty()),
        "{:?}",
        hc.mounts
    );
    assert!(hc.binds.as_ref().is_none_or(|b| b.is_empty()));
    assert!(hc.volumes_from.as_ref().is_none_or(|v| v.is_empty()));
    assert!(body.volumes.as_ref().is_none_or(|v| v.is_empty()));
    assert_ne!(hc.privileged, Some(true));
    assert!(hc.cap_add.as_ref().is_none_or(|c| c.is_empty()));
    // Capacites par defaut de Docker, moins les sockets bruts et mknod.
    let dropped = hc.cap_drop.clone().unwrap_or_default();
    assert!(dropped.contains(&"NET_RAW".to_string()), "{dropped:?}");
    assert!(dropped.contains(&"MKNOD".to_string()), "{dropped:?}");
    // Nombre de processus borne (bombe fork).
    assert!(hc.pids_limit.is_some_and(|n| n > 0), "{:?}", hc.pids_limit);
    assert!(hc.devices.as_ref().is_none_or(|d| d.is_empty()));
    for mode in [&hc.pid_mode, &hc.ipc_mode, &hc.userns_mode, &hc.uts_mode] {
        assert!(mode.as_deref().is_none_or(|m| m != "host"), "{mode:?}");
    }
    assert_ne!(hc.network_mode.as_deref(), Some("host"));
    assert!(body.user.is_none(), "utilisateur de l'image");
}

#[test]
fn passwd_lookup_finds_system_accounts() {
    let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                  ciphera-postgres:x:991:990::/nonexistent:/usr/sbin/nologin\n\
                  broken:x:abc:1::/:/bin/false\n";
    assert_eq!(
        infra::lookup_account(passwd, "ciphera-postgres"),
        Some((991, 990))
    );
    assert_eq!(infra::lookup_account(passwd, "ciphera"), None);
    assert_eq!(infra::lookup_account(passwd, "broken"), None);
}

#[test]
fn names_that_could_escape_the_api_path_are_refused() {
    for bad in ["", "../containers", "a/b", "a?b", "a b", "-x", ".x"] {
        assert!(!docker::valid_name(bad), "{bad:?}");
    }
    for good in ["ciphera-postgres", "ciphera-p-0b6c", "a1_b.c"] {
        assert!(docker::valid_name(good), "{good:?}");
    }
}

#[test]
fn image_references_that_could_escape_the_api_path_are_refused() {
    for bad in [
        "",
        "-x",
        "/x",
        "ghcr.io/acme/../../containers/x",
        "x?all=1",
        "x#y",
        "x y",
        "x%2F",
        "x\\y",
    ] {
        assert!(!docker::valid_image(bad), "{bad:?}");
    }
    for good in [
        "ghcr.io/acme/web:1",
        "docker.io/library/postgres:17@sha256:b60e90042aa6b8dbecd58e224b3e36114dbf8ad754465264c2476ad58a86e45c",
        "registry.local:5000/team/app_x.y-z:v1.2",
    ] {
        assert!(docker::valid_image(good), "{good:?}");
    }
}

#[test]
fn fingerprint_is_a_stable_sha256_that_ignores_app_env_values() {
    let deployment = DeploymentId::new();
    let project = ProjectId::new();
    let image = ImageRef("ghcr.io/acme/web:1".into());
    let with = |name: &str, value: &str| {
        ContainerSpec::app(
            deployment,
            project,
            &image,
            vec![(name.into(), value.into())],
        )
    };
    let a = with("API_KEY", "s3cret-one");
    let fp = a.fingerprint();
    // SHA-256 en hexadecimal, identique d'un appel (et d'un binaire) a l'autre.
    assert_eq!(fp.len(), 64, "{fp}");
    assert!(fp.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    assert_eq!(fp, a.clone().fingerprint());
    // La valeur d'une variable d'application (secret en Task 29) n'entre pas
    // dans l'empreinte, visible dans `docker inspect` ; son nom, si.
    assert_eq!(fp, with("API_KEY", "s3cret-two").fingerprint());
    assert_ne!(fp, with("OTHER", "s3cret-one").fingerprint());
    // Le Debug n'affiche pas non plus les valeurs.
    assert!(!format!("{a:?}").contains("s3cret-one"), "{a:?}");
    // L'image change l'empreinte.
    let image2 = ImageRef("ghcr.io/acme/web:2".into());
    assert_ne!(
        ContainerSpec::app(deployment, project, &image2, vec![]).fingerprint(),
        ContainerSpec::app(deployment, project, &image, vec![]).fingerprint()
    );
    // Infrastructure : configuration publique, comprise dans l'empreinte.
    let mut other_cfg = cfg();
    other_cfg.postgres_user = (992, 992);
    assert_ne!(
        spec_named("ciphera-postgres").fingerprint(),
        infra::infra_specs(&other_cfg).unwrap()[0].fingerprint()
    );
    // Compte ou plage de sous-uids de buildkitd changes : conteneur recree.
    let buildkit = spec_named("ciphera-buildkitd").fingerprint();
    let mut other_cfg = cfg();
    other_cfg.buildkit_account.as_mut().unwrap().user = (996, 994);
    assert_ne!(
        buildkit,
        infra::infra_specs(&other_cfg).unwrap()[1].fingerprint()
    );
    let mut other_cfg = cfg();
    other_cfg.buildkit_account.as_mut().unwrap().subgids.start = 300000;
    assert_ne!(
        buildkit,
        infra::infra_specs(&other_cfg).unwrap()[1].fingerprint()
    );
    // Etiquettes posees a la creation.
    let labels = docker::create_body(&a).labels.unwrap_or_default();
    assert_eq!(labels.get(docker::SPEC_LABEL), Some(&fp));
    assert_eq!(
        labels.get(docker::MANAGED_LABEL).map(String::as_str),
        Some("true")
    );
}

#[test]
fn only_resources_labelled_as_managed_are_touched() {
    use std::collections::HashMap;
    let label = |k: &str, v: &str| HashMap::from([(k.to_string(), v.to_string())]);
    let managed = label(docker::MANAGED_LABEL, "true");
    assert!(docker::check_managed("ciphera-buildkitd", Some(&managed)).is_ok());
    for labels in [
        None,
        Some(HashMap::new()),
        Some(label(docker::MANAGED_LABEL, "false")),
        Some(label("com.example", "true")),
    ] {
        assert!(
            matches!(
                docker::check_managed("ciphera-buildkitd", labels.as_ref()),
                Err(DockerError::NotManaged(ref n)) if n == "ciphera-buildkitd"
            ),
            "{labels:?}"
        );
    }
}

#[test]
fn bind_paths_that_could_break_the_bind_syntax_are_refused() {
    for bad in ["", "relative/path", "/a:b", "/a,b", "/a\nb", "/a\0b"] {
        assert!(!docker::valid_bind_path(bad), "{bad:?}");
    }
    for good in [
        "/etc/ciphera/postgres",
        "/run/ciphera-agent/buildkit",
        "/tmp/x_y.z-1",
    ] {
        assert!(docker::valid_bind_path(good), "{good:?}");
    }
}

#[test]
fn app_alias_is_service_internal_on_the_project_network_only_for_a_dns_safe_name() {
    let project = ProjectId::new();
    let (deployment, image) = (
        DeploymentId::new(),
        ImageRef("ciphera/web-production:1".into()),
    );
    let app = |alias: Option<&str>| {
        let spec = ContainerSpec::app(deployment, project, &image, Vec::new());
        alias.map_or(spec.clone(), |a| spec.with_service_alias(a))
    };
    let aliases = |spec: &ContainerSpec| {
        docker::create_body(spec)
            .networking_config
            .and_then(|n| n.endpoints_config)
            .map(|e| {
                assert_eq!(e.len(), 1, "un seul reseau");
                e[&docker::project_network(project)].aliases.clone()
            })
    };
    assert_eq!(
        aliases(&app(Some("web"))),
        Some(Some(vec!["web.internal".to_string()]))
    );
    assert_eq!(
        aliases(&app(Some("billing-api"))),
        Some(Some(vec!["billing-api.internal".to_string()]))
    );
    // Sans nom, ou nom hors DNS : aucun alias (jamais de nom invente).
    assert_eq!(aliases(&app(None)), None);
    for bad in [
        "",
        "Web",
        "web_1",
        "-web",
        "web-",
        "a.b",
        "web internal",
        &"a".repeat(64),
    ] {
        assert_eq!(aliases(&app(Some(bad))), None, "{bad:?}");
    }
    // L'alias entre dans l'empreinte de la spec.
    assert_ne!(app(Some("web")).fingerprint(), app(None).fingerprint());
}

/// Journaux des conteneurs bornes (B-I2) : sans rotation, une application
/// bavarde remplit le disque de Docker, qui porte aussi PostgreSQL.
#[test]
fn every_container_gets_rotated_logs() {
    let mut bodies = vec![docker::create_body(&app_spec(ProjectId::new()))];
    for name in ["ciphera-postgres", "ciphera-buildkitd"] {
        bodies.push(docker::create_body(&spec_named(name)));
    }
    for body in bodies {
        let log = host(&body).log_config.clone().expect("log_config");
        assert_eq!(log.typ.as_deref(), Some(docker::LOG_DRIVER));
        let opts = log.config.unwrap_or_default();
        assert_eq!(
            opts.get("max-size").map(String::as_str),
            Some(docker::LOG_MAX_SIZE)
        );
        assert_eq!(
            opts.get("max-file").map(String::as_str),
            Some(docker::LOG_MAX_FILE)
        );
    }
}
