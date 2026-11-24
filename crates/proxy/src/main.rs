//! `ciphera-proxy` : reverse proxy du noeud (Pingora), sous le compte
//! systeme `ciphera-proxy` avec la seule capacite `CAP_NET_BIND_SERVICE`.
//!
//! Variables d'environnement (toutes facultatives) :
//! - `CIPHERA_PROXY_HTTP` : adresses HTTP separees par des virgules
//!   (defaut `0.0.0.0:80,[::]:80`) ;
//! - `CIPHERA_PROXY_HTTPS` : adresses HTTPS (defaut `0.0.0.0:443,[::]:443`) ;
//! - `CIPHERA_PROXY_CONTROL_SOCKET` : socket de pilotage hors systemd
//!   (defaut `/run/ciphera/proxy.sock`) ;
//! - `CIPHERA_PROXY_PANEL_HOST` : nom d'hote du panneau redirige vers lui ;
//! - `CIPHERA_PANEL_PORT` : port HTTPS du panneau (defaut 8443) ;
//! - `CIPHERA_PROXY_STATE_DIR` : dossier d'etat (certificats chiffres, compte
//!   ACME, identite age `age.key` en 0600) ; defaut `STATE_DIRECTORY` de
//!   systemd, sinon `/var/lib/ciphera-proxy` ;
//! - `CIPHERA_PROXY_ACME_DIRECTORY` : annuaire ACME, `production` (defaut,
//!   Let's Encrypt), `staging` (Let's Encrypt de test) ou une URL `https://`
//!   (Pebble dans les tests) ;
//! - `CIPHERA_PROXY_ACME_CA_FILE` : racine PEM de confiance de l'annuaire
//!   (Pebble ; sans valeur, les racines du systeme) ;
//! - `CIPHERA_PROXY_ACME_EMAIL` : contact du compte ACME, facultatif.
//!
//! Journal : sans `RUST_LOG`, les echecs de poignee de main TLS que Pingora
//! journalise un par un (`pingora_core::services::listening`) sont coupes ;
//! le resolveur de certificats en publie un resume par minute.

#[cfg(unix)]
fn main() {
    unix::main();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("ciphera-proxy ne fonctionne que sous Linux.");
    std::process::exit(1);
}

#[cfg(unix)]
mod unix {
    use std::net::{Ipv6Addr, TcpListener};
    use std::path::PathBuf;
    use std::sync::Arc;

    use ciphera_proxy::acme::{self, AcmeConfig, AcmeIssuer, CertStore, Challenges};
    use ciphera_proxy::conn_limit::ConnLimits;
    use ciphera_proxy::control;
    use ciphera_proxy::forward::{ProxyConfig, TlsAssets};
    use ciphera_proxy::routes::{RouteTable, parse_host};

    const DEFAULT_HTTP: &str = "0.0.0.0:80,[::]:80";
    const DEFAULT_HTTPS: &str = "0.0.0.0:443,[::]:443";
    const DEFAULT_PANEL_PORT: u16 = 8443;
    const MAX_THREADS: usize = 4;
    const DEFAULT_STATE_DIR: &str = "/var/lib/ciphera-proxy";
    /// Filtre par defaut : une ligne par poignee de main TLS ratee est du
    /// bruit (resume par `acme::CertResolver`).
    const DEFAULT_LOG_FILTER: &str = "info,pingora_core::services::listening=off";

    pub fn main() {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into()),
            )
            .init();
        if let Err(e) = run() {
            tracing::error!("arret : {e}");
            std::process::exit(1);
        }
    }

    fn env(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    /// Adresses de la liste, sans les adresses IPv6 si l'IPv6 est coupe sur
    /// la machine (sinon l'ecoute echouerait au demarrage).
    fn addresses(name: &str, default: &str) -> Vec<String> {
        let ipv6 = TcpListener::bind((Ipv6Addr::UNSPECIFIED, 0)).is_ok();
        env(name)
            .unwrap_or_else(|| default.to_string())
            .split(',')
            .map(str::trim)
            .filter(|a| !a.is_empty() && (ipv6 || !a.starts_with('[')))
            .map(str::to_string)
            .collect()
    }

    fn config() -> Result<ProxyConfig, String> {
        let panel_host = match env("CIPHERA_PROXY_PANEL_HOST") {
            None => None,
            Some(h) => {
                let parsed = parse_host(h.as_bytes())
                    .ok()
                    .filter(|p| p.port.is_none())
                    .ok_or("CIPHERA_PROXY_PANEL_HOST invalide")?;
                Some(parsed.name)
            }
        };
        let panel_port = match env("CIPHERA_PANEL_PORT") {
            None => DEFAULT_PANEL_PORT,
            Some(p) => p
                .parse::<u16>()
                .ok()
                .filter(|p| *p != 0)
                .ok_or("CIPHERA_PANEL_PORT invalide")?,
        };
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(MAX_THREADS);
        Ok(ProxyConfig {
            http: addresses("CIPHERA_PROXY_HTTP", DEFAULT_HTTP),
            https: addresses("CIPHERA_PROXY_HTTPS", DEFAULT_HTTPS),
            panel_host,
            panel_port,
            threads,
            limits: ConnLimits::default(),
        })
    }

    fn acme_config() -> Result<AcmeConfig, String> {
        let contact = match env("CIPHERA_PROXY_ACME_EMAIL") {
            None => None,
            Some(e)
                if e.len() <= 254
                    && e.split_once('@')
                        .is_some_and(|(u, d)| !u.is_empty() && d.contains('.'))
                    && !e.contains(|c: char| c.is_whitespace() || c == ',') =>
            {
                Some(format!("mailto:{e}"))
            }
            Some(_) => return Err("CIPHERA_PROXY_ACME_EMAIL invalide".into()),
        };
        Ok(AcmeConfig {
            directory: acme::directory_url(env("CIPHERA_PROXY_ACME_DIRECTORY").as_deref())?,
            ca_file: env("CIPHERA_PROXY_ACME_CA_FILE").map(PathBuf::from),
            contact,
        })
    }

    fn state_dir() -> PathBuf {
        env("CIPHERA_PROXY_STATE_DIR")
            // systemd (StateDirectory=) : premier chemin de la liste.
            .or_else(|| {
                env("STATE_DIRECTORY").and_then(|v| v.split(':').next().map(str::to_string))
            })
            .map_or_else(|| PathBuf::from(DEFAULT_STATE_DIR), PathBuf::from)
    }

    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let cfg = config()?;
        let acme_cfg = acme_config()?;
        let state_dir = state_dir();
        let sealer = Arc::new(acme::load_or_create_key(&state_dir)?);
        let certs = Arc::new(
            CertStore::open(&state_dir, sealer.clone())
                .map_err(|e| format!("{} : {e}", state_dir.display()))?,
        );
        let tls = TlsAssets {
            certs: certs.clone(),
            challenges: Arc::new(Challenges::new()),
        };
        tracing::info!(annuaire = %acme_cfg.directory, "ACME (HTTP-01)");
        let issuer = Arc::new(AcmeIssuer::new(
            acme_cfg,
            state_dir,
            sealer,
            certs,
            tls.challenges.clone(),
        ));
        // Socket cree par systemd (ciphera-proxy.socket) ; sinon par le proxy.
        let listener = match control::from_systemd()? {
            Some(l) => l,
            None => {
                let path = env("CIPHERA_PROXY_CONTROL_SOCKET")
                    .map_or_else(|| PathBuf::from(control::DEFAULT_SOCKET), PathBuf::from);
                control::bind(&path).map_err(|e| format!("{} : {e}", path.display()))?
            }
        };
        tracing::info!(
            "ecoute HTTP {:?}, HTTPS {:?}, panneau sur le port {}",
            cfg.http,
            cfg.https,
            cfg.panel_port
        );
        let table = Arc::new(RouteTable::new());
        ciphera_proxy::build_server(&cfg, table, &tls, Some(listener), Some(issuer))?.run_forever()
    }
}
