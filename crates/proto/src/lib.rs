//! Protocole entre le plan de controle et les agents, transporte par QUIC +
//! mTLS. L'agent local passe aussi par ce protocole, comme s'il etait distant.

pub mod codec;
pub mod messages;

pub use messages::*;

/// Version du protocole, echangee dans les `Hello`. 2 : deploiements
/// (`DeploySpec` etendu, `Route::service`, `StageStatus::Cancelled`,
/// `AgentState`). 3 : ajouts des taches 26 a 31 (`StopLogs`,
/// `root_directory`, `StopContainers`, `service_name`,
/// `RemoveCertificate`/`CheckCertificate`, `env`, metriques). 4 :
/// `RequestState` (resynchronisation apres des messages perdus). 5 :
/// `RenewedCertificate` (renouvellement du certificat de l'agent). 6 :
/// `RemoveProject`, `DeploymentRemoved`, `ProjectRemoved`, identifiants de
/// projet et d'environnement dans `BuildSpec` et `DeploySpec`, `run_as` et
/// `read_only_rootfs` dans `DeploySpec`, `runs_as_root` dans
/// `RunningDeployment`, `panel_confirmed` dans `Route`.
///
/// Regle : tout changement de `messages` (meme un ajout) incremente cette
/// version. Plan de controle, agent et proxy sont livres et mis a jour
/// ensemble (`ciphera-cli updater`) ; un `Hello` d'une autre version est
/// refuse avec `error_code::VERSION_MISMATCH`.
pub const PROTO_VERSION: u16 = 6;

/// Identifiant ALPN du lien plan de controle <-> agent.
pub const ALPN: &[u8] = b"ciphera/1";
