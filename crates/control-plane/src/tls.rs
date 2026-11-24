//! Certificat du panneau : `<config_dir>/tls/panel.{crt,key}` (ecrits par
//! `ciphera init`). TLS 1.2 minimum (rustls n'implemente rien en dessous).

use std::net::IpAddr;
use std::path::PathBuf;

use axum_server::tls_rustls::RustlsConfig;
use shared::config::Config;

pub fn panel_paths(cfg: &Config) -> (PathBuf, PathBuf) {
    let dir = cfg.config_dir.join("tls");
    (dir.join("panel.crt"), dir.join("panel.key"))
}

pub async fn load(cfg: &Config) -> std::io::Result<RustlsConfig> {
    let (crt, key) = panel_paths(cfg);
    RustlsConfig::from_pem_file(crt, key).await
}

/// Origine de l'IP du noeud (`Config::public_ip`), journalisee au demarrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpSource {
    /// `CIPHERA_PUBLIC_IP`.
    Env,
    /// Hote IP de `CIPHERA_PUBLIC_URL`.
    Url,
    /// SAN IP unique du certificat du panneau.
    PanelCert,
    /// Aucune : `ignored` liste les IP d'un certificat qui en porte
    /// plusieurs (aucune n'est choisie au hasard).
    Unknown { ignored: Vec<IpAddr> },
}

/// Sans `CIPHERA_PUBLIC_IP` (ni IP dans `CIPHERA_PUBLIC_URL`), l'IP du noeud
/// est celle du certificat du panneau (`ciphera init --public-ip`) :
/// verification DNS des domaines et domaine genere fonctionnent apres une
/// installation standard. Certificat absent ou sans IP unique : inchangee.
/// `env_set` : `CIPHERA_PUBLIC_IP` est definie.
pub fn with_panel_ip(mut cfg: Config, env_set: bool) -> (Config, IpSource) {
    if cfg.public_ip.is_some() {
        let source = if env_set {
            IpSource::Env
        } else {
            IpSource::Url
        };
        return (cfg, source);
    }
    let (crt, _) = panel_paths(&cfg);
    let ips: Vec<IpAddr> = std::fs::read_to_string(crt)
        .map(|pem| pki::ips_from_cert_pem(&pem))
        .unwrap_or_default()
        .into_iter()
        .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
        .collect();
    match ips.as_slice() {
        [ip] => {
            cfg.public_ip = Some(*ip);
            (cfg, IpSource::PanelCert)
        }
        [] => (cfg, IpSource::Unknown { ignored: vec![] }),
        _ => (cfg, IpSource::Unknown { ignored: ips }),
    }
}

/// Journalise l'IP du noeud et son origine ; avertit quand un certificat a
/// plusieurs IP a ete ignore ou qu'aucune IP n'est connue.
pub fn log_ip_source(cfg: &Config, source: &IpSource) {
    let ip = cfg.public_ip.map(|ip| ip.to_string()).unwrap_or_default();
    match source {
        IpSource::Env => tracing::info!(%ip, "IP du noeud : CIPHERA_PUBLIC_IP"),
        IpSource::Url => tracing::info!(%ip, "IP du noeud : hote de CIPHERA_PUBLIC_URL"),
        IpSource::PanelCert => tracing::info!(%ip, "IP du noeud : certificat du panneau"),
        IpSource::Unknown { ignored } if !ignored.is_empty() => {
            let ignored = ignored
                .iter()
                .map(IpAddr::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                %ignored,
                "IP du noeud inconnue : le certificat du panneau porte plusieurs IP, \
                 aucune n'est choisie (definir CIPHERA_PUBLIC_IP)"
            );
        }
        IpSource::Unknown { .. } => tracing::warn!(
            "IP du noeud inconnue (ni CIPHERA_PUBLIC_IP, ni IP dans CIPHERA_PUBLIC_URL \
             ou le certificat du panneau) : verification DNS et domaine genere indisponibles"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &std::path::Path, public_ip: Option<&str>) -> Config {
        let dir = dir.to_string_lossy().into_owned();
        Config::from_lookup(|name| match name {
            "CIPHERA_CONFIG_DIR" => Some(dir.clone()),
            "CIPHERA_DATABASE_URL" => Some("postgres://u@localhost/db".into()),
            "CIPHERA_PUBLIC_IP" => public_ip.map(str::to_string),
            _ => None,
        })
        .unwrap()
    }

    fn panel_dir(sans: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ciphera-tls-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("tls")).unwrap();
        let ca = pki::Ca::generate("test").unwrap();
        let sans: Vec<String> = sans.iter().map(|s| s.to_string()).collect();
        let bundle = ca.issue_server_cert(&sans).unwrap();
        std::fs::write(dir.join("tls/panel.crt"), bundle.cert_pem).unwrap();
        dir
    }

    #[test]
    fn public_ip_falls_back_to_panel_certificate() {
        let dir = panel_dir(&["127.0.0.1", "localhost"]);
        let (cfg, source) = with_panel_ip(config(&dir, None), false);
        assert_eq!(cfg.node_ips().0, Some("127.0.0.1".parse().unwrap()));
        assert_eq!(source, IpSource::PanelCert);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn explicit_public_ip_wins_over_certificate() {
        let dir = panel_dir(&["198.51.100.4", "localhost"]);
        let (cfg, source) = with_panel_ip(config(&dir, Some("203.0.113.7")), true);
        assert_eq!(cfg.public_ip, Some("203.0.113.7".parse().unwrap()));
        assert_eq!(source, IpSource::Env);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_certificate_leaves_public_ip_unknown() {
        let dir = std::env::temp_dir().join(format!("ciphera-tls-{}", uuid::Uuid::new_v4()));
        let (cfg, source) = with_panel_ip(config(&dir, None), false);
        assert_eq!(cfg.public_ip, None);
        assert_eq!(source, IpSource::Unknown { ignored: vec![] });
    }

    /// Certificat a deux IP : aucune n'est choisie, et la raison est dite.
    #[test]
    fn certificate_with_two_ips_is_ignored_with_a_reason() {
        let dir = panel_dir(&["198.51.100.4", "203.0.113.9", "localhost"]);
        let (cfg, source) = with_panel_ip(config(&dir, None), false);
        assert_eq!(cfg.public_ip, None);
        assert_eq!(
            source,
            IpSource::Unknown {
                ignored: vec![
                    "198.51.100.4".parse().unwrap(),
                    "203.0.113.9".parse().unwrap()
                ]
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Certificat a une seule IPv6 : IP du noeud en IPv6 (AAAA).
    #[test]
    fn ipv6_certificate_gives_an_ipv6_node_address() {
        let dir = panel_dir(&["2001:db8::7", "localhost"]);
        let (cfg, source) = with_panel_ip(config(&dir, None), false);
        assert_eq!(source, IpSource::PanelCert);
        assert_eq!(cfg.node_ips(), (None, Some("2001:db8::7".parse().unwrap())));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// IP prise dans `CIPHERA_PUBLIC_URL` : source annoncee comme telle.
    #[test]
    fn ip_from_public_url_is_reported_as_such() {
        let dir = panel_dir(&["198.51.100.4", "localhost"]);
        let d = dir.to_string_lossy().into_owned();
        let cfg = Config::from_lookup(|name| match name {
            "CIPHERA_CONFIG_DIR" => Some(d.clone()),
            "CIPHERA_DATABASE_URL" => Some("postgres://u@localhost/db".into()),
            "CIPHERA_PUBLIC_URL" => Some("https://203.0.113.5:8443".into()),
            _ => None,
        })
        .unwrap();
        let (cfg, source) = with_panel_ip(cfg, false);
        assert_eq!(cfg.public_ip, Some("203.0.113.5".parse().unwrap()));
        assert_eq!(source, IpSource::Url);
        let _ = std::fs::remove_dir_all(dir);
    }
}
