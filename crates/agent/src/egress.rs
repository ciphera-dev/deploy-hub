//! Filtrage de la sortie reseau des conteneurs (table nftables
//! `inet ciphera_egress`), charge en root par `ciphera-agent egress load`
//! (`ExecStartPre=+` de `ciphera-agent.service`) et rechargee toutes les
//! 5 minutes par `ciphera-agent egress check` (`ciphera-egress-check.timer`).
//! L'agent lui-meme reste sans capacite : il ne touche jamais au pare-feu.
//!
//! Ponts vises par leur nom d'interface :
//! - builds : `ciphera-build` (buildkitd et ses etapes `RUN`) et
//!   `ciphera-prepare` (`railpack prepare`) : ni l'hote (chaine `input`),
//!   ni les metadonnees des clouds, ni les plages privees, locales, CGNAT,
//!   de lien local, multicast, NAT64, 6to4 ou Teredo (chaine `forward`) ;
//! - applications : ponts de projet `cphp<10 hex>` (`docker::project_bridge_name`) :
//!   ni l'hote (sauf le trafic etabli et les ports 80 et 443 du proxy, pour
//!   joindre un domaine servi par ce meme noeud ; jamais sur une adresse de
//!   metadonnees), ni les metadonnees des clouds ; plages privees permises,
//!   sauf `CIPHERA_APP_EGRESS_DENY_PRIVATE=1`.
//!
//! DNS : le port 53 ne s'ouvre qu'aux resolveurs de l'hote, lus au chargement
//! dans `/etc/resolv.conf` (ou `/run/systemd/resolve/resolv.conf` quand le
//! premier ne cite que la boucle locale, comme Docker), car ceux des clouds
//! sont souvent prives ou de lien local (168.63.129.16, 169.254.169.253).
//!
//! Remplacement atomique (`add table`, `delete table`, `table { ... }` dans
//! une seule transaction `nft -f -`) : jamais doublee, jamais absente pendant
//! un rechargement. Un rejet dans cette table est definitif quel que soit le
//! pare-feu de Docker (iptables ou nftables).

use std::fmt;
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::build::sandbox::PREPARE_NETWORK;
use crate::docker::{BUILD_NETWORK, PROJECT_BRIDGE_PREFIX};

/// Table nftables du filtrage.
pub const TABLE: &str = "inet ciphera_egress";
/// nft (paquet nftables, installe par install.sh).
pub const NFT: &str = "/usr/sbin/nft";
/// Reglage root (0644, jamais modifiable par un compte de Ciphera) ; absent :
/// valeurs par defaut.
pub const CONFIG_FILE: &str = "/etc/ciphera-egress/egress.env";
/// `1` : les applications ne joignent pas non plus les plages privees
/// (`APP_PRIVATE4`, `APP_PRIVATE6`) ; `0` (defaut) : permises.
pub const DENY_PRIVATE_KEY: &str = "CIPHERA_APP_EGRESS_DENY_PRIVATE";
/// Resolveurs de l'hote.
pub const RESOLV_CONF: &str = "/etc/resolv.conf";
/// Resolveurs amont de systemd-resolved (quand `RESOLV_CONF` ne cite que
/// son relais local 127.0.0.53, Docker prend ceux-ci).
pub const SYSTEMD_RESOLV_CONF: &str = "/run/systemd/resolve/resolv.conf";
/// Resolveurs retenus au plus.
const MAX_RESOLVERS: usize = 16;

/// Destinations interdites aux builds (IPv4).
pub const BUILD_BLOCKED4: [&str; 11] = [
    "0.0.0.0/8",
    "10.0.0.0/8",
    "100.64.0.0/10",
    "127.0.0.0/8",
    "168.63.129.16",
    "169.254.0.0/16",
    "172.16.0.0/12",
    "192.0.0.0/24",
    "192.168.0.0/16",
    "198.18.0.0/15",
    "224.0.0.0/3",
];
/// Destinations interdites aux builds (IPv6) : dont NAT64 (`64:ff9b::/96`,
/// `64:ff9b:1::/48`), 6to4 (`2002::/16`) et Teredo (`2001::/32`), qui
/// meneraient a une adresse IPv4 interdite.
pub const BUILD_BLOCKED6: [&str; 9] = [
    "::/127",
    "::ffff:0:0/96",
    "64:ff9b::/96",
    "64:ff9b:1::/48",
    "2001::/32",
    "2002::/16",
    "fc00::/7",
    "fe80::/10",
    "ff00::/8",
];
/// Metadonnees des clouds interdites aux applications (IPv4) : lien local
/// entier (169.254.169.254 et voisines), Alibaba, Azure (WireServer).
pub const APP_METADATA4: [&str; 3] = ["100.100.100.200", "168.63.129.16", "169.254.0.0/16"];
/// Metadonnees des clouds interdites aux applications (IPv6, AWS).
pub const APP_METADATA6: [&str; 1] = ["fd00:ec2::254"];
/// Plages privees, interdites aux applications avec `DENY_PRIVATE_KEY=1`.
pub const APP_PRIVATE4: [&str; 4] = [
    "10.0.0.0/8",
    "100.64.0.0/10",
    "172.16.0.0/12",
    "192.168.0.0/16",
];
/// Plages privees IPv6 (ULA), interdites avec `DENY_PRIVATE_KEY=1`.
pub const APP_PRIVATE6: [&str; 1] = ["fc00::/7"];
/// Ports du proxy du noeud joignables sur l'hote depuis une application.
pub const APP_HOST_PORTS: [u16; 2] = [80, 443];

/// Reglages du filtrage, lus au chargement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EgressConfig {
    /// Applications sans acces aux plages privees.
    pub deny_private: bool,
    /// Resolveurs DNS de l'hote (port 53 seulement).
    pub resolvers: Vec<IpAddr>,
}

#[derive(Debug)]
pub enum EgressError {
    /// Reglage invalide (`CONFIG_FILE`) : rien n'est charge.
    Config(String),
    /// nft absent, en echec ou table refusee.
    Nft(String),
}

impl fmt::Display for EgressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(e) => write!(f, "{CONFIG_FILE} : {e}"),
            Self::Nft(e) => write!(f, "nft : {e}"),
        }
    }
}

impl std::error::Error for EgressError {}

/// Valeur de `DENY_PRIVATE_KEY` dans le texte de `CONFIG_FILE` (lignes
/// `CLE=valeur`, commentaires `#` et lignes vides ignores, autres cles
/// ignorees) : `0` ou absente => `false`, `1` => `true`, autre => erreur.
pub fn parse_config(text: &str) -> Result<bool, EgressError> {
    let mut deny = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(EgressError::Config(format!("ligne invalide : {line:?}")));
        };
        if key.trim() != DENY_PRIVATE_KEY {
            continue;
        }
        deny = match value.trim() {
            "0" | "" => false,
            "1" => true,
            v => {
                return Err(EgressError::Config(format!(
                    "{DENY_PRIVATE_KEY}={v:?} : 0 ou 1 attendu"
                )));
            }
        };
    }
    Ok(deny)
}

/// Lit `path` ; absent => reglage par defaut. Fichier d'un autre
/// proprietaire que root, ou modifiable par le groupe ou les autres :
/// refuse (il decide du filtrage applique en root).
pub fn read_config(path: &Path) -> Result<bool, EgressError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(EgressError::Config(e.to_string())),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path).map_err(|e| EgressError::Config(e.to_string()))?;
        if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
            return Err(EgressError::Config(
                "doit appartenir a root et n'etre modifiable que par lui".into(),
            ));
        }
    }
    parse_config(&text)
}

/// Resolveurs `nameserver` d'un `resolv.conf`, sans la boucle locale
/// (injoignable depuis un pont), sans zone (`fe80::1%eth0`), sans doublon.
pub fn nameservers(text: &str) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for line in text.lines() {
        let mut f = line.split_whitespace();
        if f.next() != Some("nameserver") {
            continue;
        }
        let Some(Ok(ip)) = f.next().map(str::parse::<IpAddr>) else {
            continue;
        };
        if !ip.is_loopback() && !ip.is_unspecified() && !out.contains(&ip) {
            out.push(ip);
        }
    }
    out.truncate(MAX_RESOLVERS);
    out
}

/// Resolveurs retenus : ceux de `resolv_conf`, ou ceux de `systemd` quand
/// le premier n'en cite aucun hors de la boucle locale (relais 127.0.0.53).
pub fn resolvers(resolv_conf: Option<&str>, systemd: Option<&str>) -> Vec<IpAddr> {
    let primary = resolv_conf.map(nameservers).unwrap_or_default();
    if !primary.is_empty() {
        return primary;
    }
    systemd.map(nameservers).unwrap_or_default()
}

fn set(name: &str, kind: &str, elements: &[String], interval: bool) -> String {
    let flags = if interval { " flags interval;" } else { "" };
    if elements.is_empty() {
        format!("\tset {name} {{ type {kind};{flags} }}\n")
    } else {
        format!(
            "\tset {name} {{ type {kind};{flags} elements = {{ {} }}; }}\n",
            elements.join(", ")
        )
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Script `nft -f -` : remplace la table en une transaction.
pub fn ruleset(cfg: &EgressConfig) -> String {
    let r4: Vec<String> = cfg
        .resolvers
        .iter()
        .filter(|ip| ip.is_ipv4())
        .map(IpAddr::to_string)
        .collect();
    let r6: Vec<String> = cfg
        .resolvers
        .iter()
        .filter(|ip| ip.is_ipv6())
        .map(IpAddr::to_string)
        .collect();
    let builds = format!("{{ \"{BUILD_NETWORK}\", \"{PREPARE_NETWORK}\" }}");
    let apps = format!("\"{PROJECT_BRIDGE_PREFIX}*\"");
    let ports = APP_HOST_PORTS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let reject = "reject with icmpx admin-prohibited";
    let dns = "meta l4proto { tcp, udp } th dport 53 accept";
    let nd = "icmpv6 type { nd-neighbor-solicit, nd-neighbor-advert, nd-router-solicit } accept";
    let mut s = String::new();
    s.push_str(&format!(
        "add table {TABLE}\ndelete table {TABLE}\ntable {TABLE} {{\n"
    ));
    s.push_str(&set(
        "build_blocked4",
        "ipv4_addr",
        &strings(&BUILD_BLOCKED4),
        true,
    ));
    s.push_str(&set(
        "build_blocked6",
        "ipv6_addr",
        &strings(&BUILD_BLOCKED6),
        true,
    ));
    s.push_str(&set(
        "app_metadata4",
        "ipv4_addr",
        &strings(&APP_METADATA4),
        true,
    ));
    s.push_str(&set(
        "app_metadata6",
        "ipv6_addr",
        &strings(&APP_METADATA6),
        true,
    ));
    s.push_str(&set(
        "app_private4",
        "ipv4_addr",
        &strings(&APP_PRIVATE4),
        true,
    ));
    s.push_str(&set(
        "app_private6",
        "ipv6_addr",
        &strings(&APP_PRIVATE6),
        true,
    ));
    s.push_str(&set("resolvers4", "ipv4_addr", &r4, false));
    s.push_str(&set("resolvers6", "ipv6_addr", &r6, false));
    s.push_str(&format!(
        "\tchain input {{\n\t\ttype filter hook input priority -10; policy accept;\n\
         \t\tiifname {builds} jump build_input\n\
         \t\tiifname {apps} jump app_input\n\t}}\n"
    ));
    s.push_str(&format!(
        "\tchain forward {{\n\t\ttype filter hook forward priority -10; policy accept;\n\
         \t\tiifname {builds} jump build_forward\n\
         \t\tiifname {apps} jump app_forward\n\t}}\n"
    ));
    // Builds : vers l'hote, seuls le DNS des resolveurs et la decouverte de
    // voisins IPv6 (passerelle du pont).
    s.push_str(&format!(
        "\tchain build_input {{\n\
         \t\tip daddr @resolvers4 {dns}\n\
         \t\tip6 daddr @resolvers6 {dns}\n\
         \t\t{nd}\n\
         \t\t{reject}\n\t}}\n"
    ));
    s.push_str(&format!(
        "\tchain build_forward {{\n\
         \t\tip daddr @resolvers4 {dns}\n\
         \t\tip6 daddr @resolvers6 {dns}\n\
         \t\tip daddr @build_blocked4 {reject}\n\
         \t\tip6 daddr @build_blocked6 {reject}\n\t}}\n"
    ));
    // Applications : reponses aux connexions ouvertes par l'hote (proxy vers
    // l'amont), decouverte de voisins, DNS, proxy du noeud ; rien d'autre, et
    // jamais une adresse de metadonnees que l'hote porterait lui-meme.
    s.push_str(&format!(
        "\tchain app_input {{\n\
         \t\tct state established,related accept\n\
         \t\t{nd}\n\
         \t\tip daddr @resolvers4 {dns}\n\
         \t\tip6 daddr @resolvers6 {dns}\n\
         \t\tip daddr @app_metadata4 {reject}\n\
         \t\tip6 daddr @app_metadata6 {reject}\n\
         \t\ttcp dport {{ {ports} }} accept\n\
         \t\t{reject}\n\t}}\n"
    ));
    // Vers un autre pont de projet : laisse a l'isolation des reseaux de
    // Docker (le meme projet communique, deux projets jamais).
    s.push_str(&format!(
        "\tchain app_forward {{\n\
         \t\tct state established,related accept\n\
         \t\toifname {apps} accept\n\
         \t\tip daddr @resolvers4 {dns}\n\
         \t\tip6 daddr @resolvers6 {dns}\n\
         \t\tip daddr @app_metadata4 {reject}\n\
         \t\tip6 daddr @app_metadata6 {reject}\n"
    ));
    if cfg.deny_private {
        s.push_str(&format!(
            "\t\tip daddr @app_private4 {reject}\n\
             \t\tip6 daddr @app_private6 {reject}\n"
        ));
    }
    s.push_str("\t}\n}\n");
    s
}

/// Reglages de l'hote : `CONFIG_FILE` et resolveurs.
pub fn host_config() -> Result<EgressConfig, EgressError> {
    let deny_private = read_config(Path::new(CONFIG_FILE))?;
    let read = |p: &str| std::fs::read_to_string(p).ok();
    Ok(EgressConfig {
        deny_private,
        resolvers: resolvers(
            read(RESOLV_CONF).as_deref(),
            read(SYSTEMD_RESOLV_CONF).as_deref(),
        ),
    })
}

/// Charge `ruleset(cfg)` par `nft -f -`.
pub fn load(cfg: &EgressConfig) -> Result<(), EgressError> {
    let mut child = Command::new(NFT)
        .args(["-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| EgressError::Nft(format!("{NFT} : {e}")))?;
    let script = ruleset(cfg);
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| EgressError::Nft(e.to_string()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| EgressError::Nft(e.to_string()))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(EgressError::Nft(format!(
            "table refusee ({}) : {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// La table est-elle chargee ?
pub fn present() -> bool {
    let (family, name) = TABLE.split_once(' ').unwrap_or(("inet", TABLE));
    Command::new(NFT)
        .args(["list", "table", family, name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Mode `ciphera-agent egress <load|check>` (root, avant toute autre
/// initialisation) : `Some(code de sortie)`, ou `None` hors de ce mode.
/// `check` signale une table disparue puis recharge dans tous les cas
/// (remplacement atomique : resolveurs et reglage a jour).
pub fn cli_main(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("egress") {
        return None;
    }
    let check = match args.get(2).map(String::as_str) {
        Some("load") if args.len() == 3 => false,
        Some("check") if args.len() == 3 => true,
        _ => {
            eprintln!("usage : ciphera-agent egress <load|check>");
            return Some(2);
        }
    };
    let missing = check && !present();
    let result = host_config().and_then(|cfg| load(&cfg).map(|()| cfg));
    Some(match result {
        Ok(cfg) => {
            if missing {
                eprintln!("ciphera-agent egress : table {TABLE} absente, retablie");
            } else if !check {
                eprintln!(
                    "ciphera-agent egress : table {TABLE} chargee ({} resolveur(s), plages privees {} aux applications)",
                    cfg.resolvers.len(),
                    if cfg.deny_private {
                        "interdites"
                    } else {
                        "permises"
                    }
                );
            }
            0
        }
        Err(e) => {
            eprintln!("ciphera-agent egress : {e}");
            1
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(deny_private: bool, resolvers: &[&str]) -> EgressConfig {
        EgressConfig {
            deny_private,
            resolvers: resolvers.iter().map(|r| r.parse().unwrap()).collect(),
        }
    }

    /// Regles (lignes) d'une chaine du script.
    fn chain<'a>(script: &'a str, name: &str) -> Vec<&'a str> {
        let start = format!("\tchain {name} {{");
        let body = &script[script.find(&start).unwrap_or_else(|| panic!("{name}"))..];
        body.lines()
            .skip(1)
            .take_while(|l| l.trim() != "}")
            .map(str::trim)
            .collect()
    }

    #[test]
    fn table_is_replaced_in_one_transaction() {
        let s = ruleset(&cfg(false, &[]));
        assert!(
            s.starts_with(
                "add table inet ciphera_egress\ndelete table inet ciphera_egress\ntable inet ciphera_egress {\n"
            ),
            "{s}"
        );
        assert_eq!(s.matches("table inet ciphera_egress").count(), 3);
        // Accolades equilibrees.
        assert_eq!(s.matches('{').count(), s.matches('}').count(), "{s}");
    }

    #[test]
    fn hooks_dispatch_builds_and_project_bridges() {
        let s = ruleset(&cfg(false, &[]));
        for hook in ["input", "forward"] {
            let rules = chain(&s, hook);
            assert_eq!(
                rules[0],
                format!("type filter hook {hook} priority -10; policy accept;")
            );
            assert_eq!(
                rules[1],
                format!("iifname {{ \"ciphera-build\", \"ciphera-prepare\" }} jump build_{hook}")
            );
            assert_eq!(rules[2], format!("iifname \"cphp*\" jump app_{hook}"));
            assert_eq!(rules.len(), 3);
        }
    }

    #[test]
    fn builds_reach_neither_the_host_nor_private_nor_tunnelled_ranges() {
        let s = ruleset(&cfg(false, &["10.0.0.2"]));
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
            // D-BUILD-2 : NAT64, Teredo, 6to4.
            "64:ff9b::/96",
            "2001::/32",
            "2002::/16",
        ] {
            assert!(s.contains(range), "{range}");
        }
        let input = chain(&s, "build_input");
        assert_eq!(input.last(), Some(&"reject with icmpx admin-prohibited"));
        let forward = chain(&s, "build_forward");
        assert!(forward.contains(&"ip daddr @build_blocked4 reject with icmpx admin-prohibited"));
        assert!(forward.contains(&"ip6 daddr @build_blocked6 reject with icmpx admin-prohibited"));
    }

    /// D-BUILD-2 : le port 53 ne s'ouvre qu'aux resolveurs de l'hote, jamais
    /// a toute adresse ; aucune autre exception que la decouverte de voisins.
    #[test]
    fn dns_opens_only_to_the_host_resolvers() {
        let s = ruleset(&cfg(false, &["168.63.129.16", "10.0.0.2", "fd00::53"]));
        assert!(
            s.contains(
                "set resolvers4 { type ipv4_addr; elements = { 168.63.129.16, 10.0.0.2 }; }"
            ),
            "{s}"
        );
        assert!(
            s.contains("set resolvers6 { type ipv6_addr; elements = { fd00::53 }; }"),
            "{s}"
        );
        for name in ["build_input", "build_forward", "app_input", "app_forward"] {
            for rule in chain(&s, name) {
                if rule.contains("dport 53") {
                    assert!(
                        rule.starts_with("ip daddr @resolvers4 ")
                            || rule.starts_with("ip6 daddr @resolvers6 "),
                        "{name}: {rule}"
                    );
                }
            }
        }
        let accepts: Vec<&str> = ["build_input", "build_forward"]
            .iter()
            .flat_map(|c| chain(&s, c))
            .filter(|r| r.ends_with(" accept"))
            .collect();
        assert_eq!(accepts.len(), 5, "{accepts:?}");
        for rule in accepts {
            assert!(
                rule.contains("th dport 53 accept") || rule.starts_with("icmpv6 type"),
                "{rule}"
            );
        }
        // Sans resolveur : ensembles vides (syntaxe sans `elements`).
        let empty = ruleset(&cfg(false, &[]));
        assert!(
            empty.contains("set resolvers4 { type ipv4_addr; }"),
            "{empty}"
        );
        assert!(
            empty.contains("set resolvers6 { type ipv6_addr; }"),
            "{empty}"
        );
    }

    #[test]
    fn apps_reach_neither_the_host_nor_cloud_metadata() {
        let s = ruleset(&cfg(false, &[]));
        let input = chain(&s, "app_input");
        assert_eq!(input[0], "ct state established,related accept");
        assert_eq!(input.last(), Some(&"reject with icmpx admin-prohibited"));
        // Seules exceptions vers l'hote : decouverte de voisins, DNS et ports
        // du proxy du noeud, jamais sur une adresse de metadonnees (portee par
        // l'hote lui-meme).
        let open: Vec<&str> = input[1..input.len() - 1].to_vec();
        assert_eq!(
            open,
            [
                "icmpv6 type { nd-neighbor-solicit, nd-neighbor-advert, nd-router-solicit } accept",
                "ip daddr @resolvers4 meta l4proto { tcp, udp } th dport 53 accept",
                "ip6 daddr @resolvers6 meta l4proto { tcp, udp } th dport 53 accept",
                "ip daddr @app_metadata4 reject with icmpx admin-prohibited",
                "ip6 daddr @app_metadata6 reject with icmpx admin-prohibited",
                "tcp dport { 80, 443 } accept",
            ]
        );
        assert!(s.contains("elements = { 100.100.100.200, 168.63.129.16, 169.254.0.0/16 }"));
        assert!(s.contains("elements = { fd00:ec2::254 }"));
        let forward = chain(&s, "app_forward");
        assert!(forward.contains(&"ip daddr @app_metadata4 reject with icmpx admin-prohibited"));
        assert!(forward.contains(&"ip6 daddr @app_metadata6 reject with icmpx admin-prohibited"));
        // Plages privees permises par defaut.
        assert!(!forward.iter().any(|r| r.contains("@app_private")));
    }

    #[test]
    fn deny_private_also_rejects_private_ranges_for_apps() {
        let s = ruleset(&cfg(true, &["10.0.0.2"]));
        let forward = chain(&s, "app_forward");
        let n = forward.len();
        assert_eq!(
            &forward[n - 2..],
            [
                "ip daddr @app_private4 reject with icmpx admin-prohibited",
                "ip6 daddr @app_private6 reject with icmpx admin-prohibited",
            ]
        );
        // Le resolveur prive reste joignable sur le port 53 : accepte avant.
        let dns = forward
            .iter()
            .position(|r| r.starts_with("ip daddr @resolvers4"))
            .unwrap();
        assert!(dns < n - 2);
        for range in ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "fc00::/7"] {
            assert!(s.contains(range), "{range}");
        }
    }

    #[test]
    fn config_accepts_only_zero_or_one() {
        assert!(!parse_config("").unwrap());
        assert!(!parse_config("# commentaire\n\nCIPHERA_APP_EGRESS_DENY_PRIVATE=0\n").unwrap());
        assert!(parse_config("CIPHERA_APP_EGRESS_DENY_PRIVATE=1\n").unwrap());
        assert!(parse_config("  CIPHERA_APP_EGRESS_DENY_PRIVATE = 1  \n").unwrap());
        // Autre cle : ignoree ; derniere valeur retenue.
        assert!(
            parse_config(
                "AUTRE=x\nCIPHERA_APP_EGRESS_DENY_PRIVATE=0\nCIPHERA_APP_EGRESS_DENY_PRIVATE=1\n"
            )
            .unwrap()
        );
        for bad in [
            "CIPHERA_APP_EGRESS_DENY_PRIVATE=yes\n",
            "CIPHERA_APP_EGRESS_DENY_PRIVATE=\"1\"\n",
            "CIPHERA_APP_EGRESS_DENY_PRIVATE=2\n",
            "pas une affectation\n",
        ] {
            assert!(parse_config(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn missing_config_file_means_defaults() {
        let path = std::env::temp_dir().join(format!(
            "ciphera-egress-absent-{}",
            shared::ids::DeploymentId::new()
        ));
        assert!(!read_config(&path).unwrap());
    }

    #[test]
    fn resolvers_skip_loopback_and_fall_back_to_systemd_resolved() {
        let stub = "# systemd-resolved\nnameserver 127.0.0.53\noptions edns0 trust-ad\nsearch x\n";
        let upstream = "nameserver 168.63.129.16\nnameserver fe80::1%eth0\nnameserver 2001:db8::53\nnameserver 168.63.129.16\n";
        assert_eq!(
            resolvers(Some(stub), Some(upstream)),
            [
                "168.63.129.16".parse::<IpAddr>().unwrap(),
                "2001:db8::53".parse().unwrap()
            ]
        );
        // Un resolveur hors de la boucle locale : systemd-resolved ignore.
        assert_eq!(
            resolvers(
                Some("nameserver 10.0.0.2\nnameserver ::1\n"),
                Some(upstream)
            ),
            ["10.0.0.2".parse::<IpAddr>().unwrap()]
        );
        assert!(resolvers(None, None).is_empty());
        assert!(resolvers(Some(stub), None).is_empty());
        assert!(nameservers("nameserver\nnameserver x\n#nameserver 1.1.1.1\n").is_empty());
    }

    #[test]
    fn cli_only_handles_the_egress_mode() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli_main(&args(&["ciphera-agent"])), None);
        assert_eq!(cli_main(&args(&["ciphera-agent", "autre"])), None);
        assert_eq!(cli_main(&args(&["ciphera-agent", "egress"])), Some(2));
        assert_eq!(
            cli_main(&args(&["ciphera-agent", "egress", "load", "x"])),
            Some(2)
        );
    }
}
