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
/// Regle : la version change avec chaque version publiee de Ciphera dont les
/// messages different. Avant v0.1.0, la v6 est la version de travail : elle
/// a ete posee une seule fois (correctifs de l'audit, R1) et les ajouts
/// suivants (`panel_confirmed`) y entrent sans la changer. Ce qui est
/// garanti : plan de controle, agent et proxy sont toujours livres et mis a
/// jour ensemble (`ciphera-cli updater`), et un `Hello` d'une autre version
/// est refuse avec `error_code::VERSION_MISMATCH`. Deux binaires de la meme
/// version de travail mais de constructions differentes ne sont pas pris en
/// charge ; l'encodage postcard d'une `Route` est fige par les tests du codec
/// (`route_wire_encoding_is_pinned`).
pub const PROTO_VERSION: u16 = 6;

/// Identifiant ALPN du lien plan de controle <-> agent.
pub const ALPN: &[u8] = b"ciphera/1";
