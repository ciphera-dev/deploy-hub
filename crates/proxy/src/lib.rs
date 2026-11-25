//! Proxy du noeud Ciphera : seul service a ecouter sur 80 et 443. Sa table de
//! routes est poussee par l'agent local sur un socket Unix.
//!
//! Pingora et les sockets Unix n'existent que sous Unix : sous Windows, seuls
//! la table de routes et le masquage des journaux sont compiles (et testes).

pub mod access_log;
pub mod acme;
pub mod routes;

#[cfg(unix)]
pub mod conn_limit;
#[cfg(unix)]
pub mod control;
#[cfg(unix)]
pub mod forward;

#[cfg(unix)]
pub use server::build_server;

#[cfg(unix)]
mod server {
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;

    use pingora::server::Server;
    use pingora::server::configuration::ServerConf;
    use pingora::services::background::background_service;

    use crate::acme::AcmeIssuer;
    use crate::control::{ControlAccess, ControlService};
    use crate::forward::{ProxyConfig, TlsAssets, proxy_services};
    use crate::routes::RouteTable;

    /// Delai laisse aux requetes en cours a l'arret (SIGTERM).
    const GRACEFUL_SHUTDOWN_SECS: u64 = 5;

    /// Serveur Pingora : service HTTP(S) (certificats et defis de `tls`)
    /// et, s'il est fourni, socket de pilotage (emissions par `issuer`).
    /// Rien n'ecoute avant `Server::run`.
    pub fn build_server(
        cfg: &ProxyConfig,
        table: Arc<RouteTable>,
        tls: &TlsAssets,
        control: Option<UnixListener>,
        issuer: Option<Arc<AcmeIssuer>>,
        access: ControlAccess,
    ) -> Result<Server, String> {
        let mut conf = ServerConf {
            threads: cfg.threads.max(1),
            grace_period_seconds: Some(0),
            graceful_shutdown_timeout_seconds: Some(GRACEFUL_SHUTDOWN_SECS),
            ..ServerConf::default()
        };
        // Aucune mise a niveau a chaud : ni fichier de PID ni socket de
        // transfert dans /tmp.
        conf.daemon = false;
        let mut server = Server::new_with_opt_and_conf(None, conf);
        for service in proxy_services(&server.configuration, cfg, table.clone(), tls)? {
            server.add_service(service);
        }
        if let Some(listener) = control {
            server.add_service(background_service(
                "pilotage",
                ControlService::new(listener, table, issuer, access),
            ));
        }
        Ok(server)
    }
}
