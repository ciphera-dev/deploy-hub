//! Durcissement des unites systemd livrees dans `deploy/systemd/`.

use std::path::PathBuf;

fn unit(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../deploy/systemd")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Valeurs d'une directive (lignes `Cle=valeur`, commentaires ignores).
fn values(text: &str, key: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| v.trim().to_string())
        .collect()
}

const HARDENING: [(&str, &str); 4] = [
    ("NoNewPrivileges", "yes"),
    ("ProtectSystem", "strict"),
    ("ProtectHome", "yes"),
    ("PrivateTmp", "yes"),
];

fn assert_hardened(name: &str) -> String {
    let text = unit(name);
    for (key, want) in HARDENING {
        assert_eq!(values(&text, key), vec![want.to_string()], "{name}: {key}");
    }
    let users = values(&text, "User");
    assert_eq!(users.len(), 1, "{name}: un seul User=");
    assert_ne!(users[0], "root", "{name}: jamais root");
    text
}

#[test]
fn control_plane_is_hardened_without_capabilities() {
    let text = assert_hardened("ciphera.service");
    assert_eq!(values(&text, "User"), vec!["ciphera"]);
    assert!(values(&text, "AmbientCapabilities").is_empty());
    assert_eq!(values(&text, "CapabilityBoundingSet"), vec![""]);
    assert_eq!(values(&text, "Restart"), vec!["on-failure"]);
    // Plafonds de connexions des ecouteurs (http::listener) tenables.
    assert_eq!(values(&text, "LimitNOFILE"), vec!["65536"]);
    // Le port 80 appartient au proxy, qui redirige deja : aucune ecoute HTTP
    // publique de plus (8080) pour le plan de controle.
    assert!(
        !values(&text, "Environment")
            .iter()
            .any(|e| e.starts_with("CIPHERA_HTTP_PORT=")),
    );
}

#[test]
fn proxy_is_hardened_and_may_only_bind_low_ports() {
    let text = assert_hardened("ciphera-proxy.service");
    assert_eq!(values(&text, "User"), vec!["ciphera-proxy"]);
    assert_eq!(
        values(&text, "AmbientCapabilities"),
        vec!["CAP_NET_BIND_SERVICE"]
    );
    assert_eq!(
        values(&text, "CapabilityBoundingSet"),
        vec!["CAP_NET_BIND_SERVICE"]
    );
    // Plafond de connexions du proxy (conn_limit::MAX_CONNECTIONS) tenable.
    assert_eq!(values(&text, "LimitNOFILE"), vec!["65536"]);
    // Ressources plafonnees, ecoutes limitees a 80 et 443.
    assert_eq!(values(&text, "MemoryMax"), vec!["2G"]);
    assert_eq!(values(&text, "TasksMax"), vec!["4096"]);
    assert_eq!(values(&text, "SocketBindAllow"), vec!["tcp:80", "tcp:443"]);
    assert_eq!(values(&text, "SocketBindDeny"), vec!["any"]);
    assert_eq!(values(&text, "ProcSubset"), vec!["pid"]);
    assert_eq!(values(&text, "RemoveIPC"), vec!["yes"]);
    assert_eq!(values(&text, "PrivateIPC"), vec!["yes"]);
}

#[test]
fn proxy_control_socket_is_created_by_systemd_for_the_agent_only() {
    let socket = unit("ciphera-proxy.socket");
    assert_eq!(
        values(&socket, "ListenStream"),
        vec!["/run/ciphera/proxy.sock"]
    );
    assert_eq!(values(&socket, "SocketMode"), vec!["0660"]);
    assert_eq!(values(&socket, "SocketUser"), vec!["ciphera-proxy"]);
    assert_eq!(values(&socket, "SocketGroup"), vec!["ciphera-agent"]);
    // Jamais de TCP sur le socket de pilotage.
    assert!(values(&socket, "ListenDatagram").is_empty());
    assert!(
        values(&socket, "ListenStream")
            .iter()
            .all(|v| v.starts_with('/'))
    );
    let service = unit("ciphera-proxy.service");
    for key in ["Requires", "After"] {
        let deps = values(&service, key).join(" ");
        assert!(
            deps.split_whitespace().any(|u| u == "ciphera-proxy.socket"),
            "{key}={deps}"
        );
    }
    // Le proxy n'a pas besoin d'ecrire dans /run : systemd cree le socket.
    assert!(values(&service, "RuntimeDirectory").is_empty());
}

#[test]
fn only_the_proxy_gets_ambient_capabilities() {
    for name in [
        "ciphera.service",
        "ciphera-agent.service",
        "ciphera-updater.service",
        "ciphera-egress-check.service",
        "ciphera-certsign.service",
    ] {
        assert!(
            values(&unit(name), "AmbientCapabilities").is_empty(),
            "{name}"
        );
    }
}

#[test]
fn units_never_carry_secrets() {
    for name in [
        "ciphera.service",
        "ciphera-proxy.service",
        "ciphera-proxy.socket",
        "ciphera-agent.service",
        "ciphera-updater.service",
        "ciphera-updater.path",
        "ciphera-updater-recover.service",
        "ciphera-egress-check.service",
        "ciphera-egress-check.timer",
        "ciphera-certsign.service",
        "ciphera-certsign.path",
    ] {
        let text = unit(name);
        for env in values(&text, "Environment") {
            let upper = env.to_uppercase();
            assert!(
                !upper.contains("PASSWORD=") && !upper.contains("DATABASE_URL="),
                "{name}: {env}"
            );
        }
    }
}

#[test]
fn control_plane_reads_db_password_from_a_file() {
    let text = unit("ciphera.service");
    let env = values(&text, "Environment").join(" ");
    assert!(
        env.contains("CIPHERA_DB_PASSWORD_FILE=/etc/ciphera/db_password"),
        "{env}"
    );
}

#[test]
fn updater_path_triggers_the_updater_service() {
    let text = unit("ciphera-updater.path");
    assert_eq!(values(&text, "Unit"), vec!["ciphera-updater.service"]);
    let svc = unit("ciphera-updater.service");
    assert_eq!(values(&svc, "Type"), vec!["oneshot"]);
    // Meme fichier que celui ecrit par le plan de controle et lu par
    // l'updater (une seule source : shared::update).
    assert_eq!(
        values(&text, "PathChanged"),
        vec![format!(
            "{}/{}",
            shared::update::REQUEST_DIR,
            shared::update::REQUEST_FILE
        )]
    );
    assert_eq!(values(&text, "WantedBy"), vec!["multi-user.target"]);
}

/// Les unites root ne chargent aucun fichier d'environnement :
/// `/etc/ciphera/ciphera.env` est modifiable par le compte `ciphera`
/// (PATH, LD_PRELOAD, cle de test y donneraient root). L'updater lit sa
/// propre configuration root (`/etc/ciphera-updater/updater.env`).
#[test]
fn root_units_load_no_environment_file() {
    for name in [
        "ciphera-updater.service",
        "ciphera-updater-recover.service",
        "ciphera-egress-check.service",
        "ciphera-certsign.service",
    ] {
        let text = unit(name);
        assert!(values(&text, "EnvironmentFile").is_empty(), "{name}");
        assert!(values(&text, "Environment").is_empty(), "{name}");
    }
}

/// Reprise au demarrage : journal d'une mise a jour interrompue rejoue
/// avant les services de Ciphera.
#[test]
fn updater_recovery_runs_at_boot_before_the_services() {
    let svc = unit("ciphera-updater-recover.service");
    assert_eq!(values(&svc, "Type"), vec!["oneshot"]);
    assert_eq!(
        values(&svc, "ExecStart"),
        vec!["/usr/local/bin/ciphera-cli updater --recover"]
    );
    assert!(
        values(&svc, "ConditionPathExists")
            .contains(&"/var/lib/ciphera-updater/journal.json".to_string())
    );
    let before: Vec<String> = values(&svc, "Before")
        .iter()
        .flat_map(|v| v.split_whitespace().map(String::from))
        .collect();
    for s in [
        "ciphera.service",
        "ciphera-agent.service",
        "ciphera-proxy.service",
    ] {
        assert!(before.iter().any(|b| b == s), "Before= sans {s}");
    }
    assert_eq!(values(&svc, "WantedBy"), vec!["multi-user.target"]);
    // Reprise en echec : les services ne demarrent pas.
    let required: Vec<String> = values(&svc, "RequiredBy")
        .iter()
        .flat_map(|v| v.split_whitespace().map(String::from))
        .collect();
    for s in [
        "ciphera.service",
        "ciphera-agent.service",
        "ciphera-proxy.service",
    ] {
        assert!(required.iter().any(|r| r == s), "RequiredBy= sans {s}");
    }
    assert_hardened_root(&svc);
}

/// Un service aux dependances par defaut est `After=basic.target`, lui-meme
/// apres `sockets.target` et `paths.target` : le placer avant un socket ou un
/// path (ou ces cibles) cree un cycle que systemd casse a chaque demarrage
/// en supprimant un job (sockets.target, paths.target ou le service).
#[test]
fn default_dependency_services_are_never_ordered_before_early_boot_units() {
    for name in [
        "ciphera.service",
        "ciphera-proxy.service",
        "ciphera-agent.service",
        "ciphera-updater.service",
        "ciphera-updater-recover.service",
        "ciphera-egress-check.service",
        "ciphera-certsign.service",
    ] {
        let text = unit(name);
        if values(&text, "DefaultDependencies") == vec!["no"] {
            continue;
        }
        for before in values(&text, "Before")
            .iter()
            .flat_map(|v| v.split_whitespace().map(String::from))
        {
            assert!(
                !before.ends_with(".socket")
                    && !before.ends_with(".path")
                    && !matches!(
                        before.as_str(),
                        "sockets.target" | "paths.target" | "basic.target" | "sysinit.target"
                    ),
                "{name}: Before={before} (cycle avec basic.target)"
            );
        }
    }
}

/// L'updater tourne en root (binaires, unites, services) mais durci comme
/// les autres unites : ecritures limitees aux dossiers de Ciphera,
/// capacites limitees aux fichiers d'autres comptes, sans ambiante.
#[test]
fn updater_is_a_hardened_root_oneshot() {
    let svc = unit("ciphera-updater.service");
    assert_eq!(
        values(&svc, "ExecStart"),
        vec!["/usr/local/bin/ciphera-cli updater"]
    );
    assert_hardened_root(&svc);
}

fn assert_hardened_root(svc: &str) {
    for (key, want) in HARDENING {
        assert_eq!(values(svc, key), vec![want.to_string()], "{key}");
    }
    assert!(values(svc, "User").is_empty(), "root : aucun User=");
    for key in [
        "PrivateDevices",
        "ProtectKernelTunables",
        "ProtectKernelModules",
        "ProtectControlGroups",
        "RestrictSUIDSGID",
        "RestrictNamespaces",
        "LockPersonality",
        "MemoryDenyWriteExecute",
    ] {
        assert_eq!(values(svc, key), vec!["yes".to_string()], "{key}");
    }
    assert_eq!(
        values(svc, "CapabilityBoundingSet"),
        vec!["CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH CAP_FOWNER CAP_CHOWN CAP_FSETID"]
    );
    assert!(values(svc, "AmbientCapabilities").is_empty());
    let writable: Vec<String> = values(svc, "ReadWritePaths")
        .iter()
        .flat_map(|v| {
            v.split_whitespace()
                .map(|p| p.trim_start_matches('-').to_string())
        })
        .collect();
    for path in &writable {
        assert!(
            path.contains("ciphera")
                || [
                    "/usr/local/bin",
                    "/etc/systemd/system",
                    "/etc/tmpfiles.d",
                    // Profil AppArmor de buildkitd (remplace par renommage).
                    "/etc/apparmor.d",
                ]
                .contains(&path.as_str()),
            "{path} : ecriture hors des dossiers de Ciphera"
        );
    }
    for needed in [
        "/usr/local/bin",
        "/var/backups/ciphera",
        "/run/ciphera",
        "/var/lib/ciphera",
        "/etc/apparmor.d",
    ] {
        assert!(writable.iter().any(|p| p == needed), "{needed} absent");
    }
}

#[test]
fn control_plane_starts_after_the_agent_and_reaches_postgres_by_socket() {
    let text = unit("ciphera.service");
    for key in ["After", "Wants"] {
        let deps = values(&text, key).join(" ");
        assert!(
            deps.split_whitespace()
                .any(|u| u == "ciphera-agent.service"),
            "{key}={deps}"
        );
    }
    let env = values(&text, "Environment");
    assert!(
        env.iter()
            .any(|e| e == "CIPHERA_DB_SOCKET_DIR=/run/ciphera/postgres"),
        "{env:?}"
    );
    // Jamais de TCP vers la base.
    assert!(
        !env.iter().any(|e| e.starts_with("CIPHERA_DB_HOST=")),
        "{env:?}"
    );
}

/// Agent : compte sans capacite, appels systeme d'un service ordinaire,
/// /dev prive, familles de sockets limitees (Docker CLI et compose : Unix et
/// netlink ; lien et git : IPv4/IPv6). Prouve avec Docker reel par
/// install/tests/run-docker-host.sh (memes reglages, docker et compose).
#[test]
fn agent_is_hardened_with_syscall_device_and_socket_filters() {
    let text = assert_hardened("ciphera-agent.service");
    assert_eq!(values(&text, "User"), vec!["ciphera-agent"]);
    assert_eq!(values(&text, "CapabilityBoundingSet"), vec![""]);
    assert_eq!(values(&text, "SystemCallFilter"), vec!["@system-service"]);
    assert_eq!(values(&text, "SystemCallErrorNumber"), vec!["EPERM"]);
    assert_eq!(values(&text, "SystemCallArchitectures"), vec!["native"]);
    assert_eq!(
        values(&text, "RestrictAddressFamilies"),
        vec!["AF_UNIX AF_INET AF_INET6 AF_NETLINK"]
    );
    for key in [
        "PrivateDevices",
        "ProtectKernelTunables",
        "ProtectKernelModules",
        "ProtectKernelLogs",
        "ProtectControlGroups",
        "ProtectClock",
        "ProtectHostname",
        "RestrictSUIDSGID",
        "RestrictRealtime",
        "RestrictNamespaces",
        "LockPersonality",
    ] {
        assert_eq!(values(&text, key), vec!["yes".to_string()], "{key}");
    }
    // Filtrage de sortie charge en root avant l'agent, echec bloquant.
    assert_eq!(
        values(&text, "ExecStartPre"),
        vec!["+/usr/local/bin/ciphera-agent egress load"]
    );
}

/// Controle periodique du filtrage : root reduit a CAP_NET_ADMIN, sans
/// fichier d'environnement, seulement la ou Docker tourne ; minuterie de
/// 5 minutes activee par install.sh.
#[test]
fn egress_check_is_a_hardened_root_oneshot_every_five_minutes() {
    let svc = unit("ciphera-egress-check.service");
    for (key, want) in HARDENING {
        assert_eq!(values(&svc, key), vec![want.to_string()], "{key}");
    }
    assert!(values(&svc, "User").is_empty(), "root : aucun User=");
    assert_eq!(values(&svc, "Type"), vec!["oneshot"]);
    assert_eq!(
        values(&svc, "ExecStart"),
        vec!["/usr/local/bin/ciphera-agent egress check"]
    );
    assert_eq!(values(&svc, "CapabilityBoundingSet"), vec!["CAP_NET_ADMIN"]);
    assert!(values(&svc, "AmbientCapabilities").is_empty());
    assert!(values(&svc, "ReadWritePaths").is_empty());
    assert_eq!(
        values(&svc, "RestrictAddressFamilies"),
        vec!["AF_UNIX AF_NETLINK"]
    );
    assert_eq!(values(&svc, "SystemCallFilter"), vec!["@system-service"]);
    assert_eq!(
        values(&svc, "ConditionPathExists"),
        vec!["/var/run/docker.sock"]
    );
    let timer = unit("ciphera-egress-check.timer");
    assert_eq!(values(&timer, "OnCalendar"), vec!["*:0/5"]);
    assert_eq!(values(&timer, "Unit"), vec!["ciphera-egress-check.service"]);
    assert_eq!(values(&timer, "WantedBy"), vec!["timers.target"]);
}

/// Signataire de certificats root (agents::signer) : declenche par les
/// demandes du plan de controle (meme dossier que le code), oneshot root
/// durci sans fichier d'environnement, ecritures limitees aux demandes,
/// reponses et a la cle de la CA ; lance aussi au demarrage (passage d'une
/// installation anterieure).
#[test]
fn certsign_is_a_hardened_root_oneshot_triggered_by_requests() {
    use control_plane::agents::signer;
    let path = unit("ciphera-certsign.path");
    assert_eq!(
        values(&path, "PathExistsGlob"),
        vec![format!(
            "{}/{}/*.json",
            shared::config::Config::DEFAULT_DATA_DIR,
            signer::REQUESTS_DIR
        )]
    );
    assert_eq!(values(&path, "Unit"), vec!["ciphera-certsign.service"]);
    assert_eq!(values(&path, "WantedBy"), vec!["multi-user.target"]);

    let svc = unit("ciphera-certsign.service");
    for (key, want) in HARDENING {
        assert_eq!(values(&svc, key), vec![want.to_string()], "{key}");
    }
    assert!(values(&svc, "User").is_empty(), "root : aucun User=");
    assert_eq!(values(&svc, "Type"), vec!["oneshot"]);
    assert_eq!(
        values(&svc, "ExecStart"),
        vec!["/usr/local/bin/ciphera admin sign-requests"]
    );
    for key in [
        "PrivateDevices",
        "ProtectKernelTunables",
        "ProtectKernelModules",
        "ProtectControlGroups",
        "RestrictSUIDSGID",
        "RestrictNamespaces",
        "LockPersonality",
        "MemoryDenyWriteExecute",
    ] {
        assert_eq!(values(&svc, key), vec!["yes".to_string()], "{key}");
    }
    assert_eq!(
        values(&svc, "CapabilityBoundingSet"),
        vec!["CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH CAP_FOWNER CAP_CHOWN"]
    );
    assert_eq!(
        values(&svc, "ReadWritePaths"),
        vec!["-/var/lib/ciphera", "-/etc/ciphera/ca"]
    );
    assert_eq!(
        values(&svc, "RestrictAddressFamilies"),
        vec!["AF_UNIX AF_INET AF_INET6"]
    );
    assert_eq!(values(&svc, "SystemCallFilter"), vec!["@system-service"]);
    assert_eq!(values(&svc, "WantedBy"), vec!["multi-user.target"]);
}

/// Le plan de controle ne peut jamais lire la cle de la CA.
#[test]
fn control_plane_cannot_read_the_ca_key() {
    let text = unit("ciphera.service");
    assert_eq!(
        values(&text, "InaccessiblePaths"),
        vec!["-/etc/ciphera/ca/ca.key"]
    );
}

#[test]
fn agent_runs_only_where_docker_is() {
    let text = unit("ciphera-agent.service");
    assert_eq!(
        values(&text, "ConditionPathExists"),
        vec!["/var/run/docker.sock"]
    );
    let after = values(&text, "After").join(" ");
    assert!(after.contains("docker.service"), "{after}");
}

#[test]
fn tmpfiles_create_the_socket_directories() {
    let text = unit("ciphera.tmpfiles.conf");
    let lines: Vec<Vec<&str>> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().collect())
        .collect();
    let dir = |path: &str| {
        lines
            .iter()
            .find(|f| f.len() >= 5 && f[0] == "d" && f[1] == path)
            .map(|f| (f[2], f[3], f[4]))
            .unwrap_or_else(|| panic!("{path} absent"))
    };
    // Parent sans lecture pour les autres comptes ; le plan de controle
    // (groupe ciphera) traverse vers le socket de PostgreSQL, l'agent vers le
    // socket de pilotage du proxy (0660, groupe ciphera-agent) : traversee
    // seule (x) pour les autres, chaque entree etant fermee par ses droits.
    assert_eq!(dir("/run/ciphera"), ("0751", "root", "ciphera"));
    // Sauvegardes de la base avant mise a jour : root seulement.
    assert_eq!(dir("/var/backups/ciphera"), ("0700", "root", "root"));
    assert_eq!(
        dir("/run/ciphera/postgres"),
        ("0750", "ciphera-postgres", "ciphera")
    );
    // Parent a root : systemd-tmpfiles refuse un passage d'un dossier d'un
    // compte non root vers un dossier d'un autre compte (« unsafe path
    // transition », vu en CI avec le sous-dossier buildkit).
    assert_eq!(dir("/run/ciphera-agent"), ("0750", "root", "ciphera-agent"));
    // buildkitd rootless tourne sous le compte systeme dedie ciphera-build
    // (jamais l'uid 1000 de l'image) : il cree son socket dans ce dossier ;
    // le parent ci-dessus en ferme l'acces aux autres comptes de l'hote.
    // Dossier setgid du groupe ciphera-agent : le socket (0660) cree par
    // buildkitd (lance avec `--group=-1`) herite de ce groupe. Aucune ACL
    // (heritage d'ACL par defaut non fiable depuis un espace de noms
    // utilisateur, vu en CI).
    assert_eq!(
        dir("/run/ciphera-agent/buildkit"),
        ("2770", "ciphera-build", "ciphera-agent")
    );
    // Etat de BuildKit (cache, couches) : au seul compte ciphera-build.
    assert_eq!(
        dir("/var/lib/ciphera-buildkit"),
        ("0700", "ciphera-build", "ciphera-build")
    );
    assert!(
        !lines
            .iter()
            .any(|f| f.first().is_some_and(|t| t.starts_with('a'))),
        "aucune ligne ACL attendue"
    );
}

/// Renouvellement des certificats (agents::certs) : le plan de controle
/// reecrit ses certificats du lien et du panneau, l'agent le sien ; rien
/// d'autre sous /etc n'est ouvert en ecriture.
#[test]
fn services_can_rewrite_their_own_certificates_only() {
    let cp = unit("ciphera.service");
    assert_eq!(
        values(&cp, "ReadWritePaths"),
        vec!["-/etc/ciphera/tls".to_string()]
    );
    let agent = unit("ciphera-agent.service");
    assert_eq!(
        values(&agent, "ReadWritePaths"),
        vec!["-/etc/ciphera-agent".to_string()]
    );
}
