//! Agent Ciphera : execute sur son noeud les ordres du plan de controle,
//! recus par le lien QUIC + mTLS du protocole `proto`, et pilote Docker par
//! son API (bollard).

pub mod build;
pub mod buildkit;
pub mod config;
pub mod deploy;
pub mod docker;
pub mod infra;
pub mod logs;
pub mod metrics;
pub mod ops;
#[cfg(unix)]
pub mod proxy_client;
pub mod reconcile;
pub mod routes;
pub mod state;
pub mod transport;
