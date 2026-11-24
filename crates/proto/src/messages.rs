//! Messages du protocole (version `PROTO_VERSION`).
//!
//! Compatibilite : `Hello` est la premiere variante des deux enums et
//! `version` son premier champ, pour qu'un pair d'une autre version puisse
//! toujours lire le `Hello` (postcard encode la variante par son rang). Ne
//! jamais reordonner les variantes ni les champs d'une version publiee.

use std::fmt;
use std::net::SocketAddr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use shared::ids::{DeploymentId, ProjectId, ServerId, ServiceId};
use zeroize::Zeroizing;

/// Codes d'erreur des messages `Error`, repris comme codes de fermeture de
/// la connexion QUIC.
pub mod error_code {
    /// Fermeture normale.
    pub const NONE: u32 = 0;
    /// Version du protocole non prise en charge.
    pub const VERSION_MISMATCH: u32 = 1;
    /// Agent inconnu, revoque, ou identite incoherente avec son certificat.
    pub const UNAUTHORIZED: u32 = 2;
    /// Message illisible ou inattendu a ce moment de l'echange.
    pub const PROTOCOL: u32 = 3;
    /// Erreur interne du pair.
    pub const INTERNAL: u32 = 4;
}

/// Reference d'image OCI (`registre/depot:tag` ou `@sha256:...`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef(pub String);

/// Ce que l'agent doit faire tourner pour un deploiement.
///
/// L'agent demarre le nouveau conteneur sur `ciphera-p-<project_id>`,
/// verifie sa sante, bascule les routes du service puis arrete l'ancien
/// deploiement du service apres le drainage. Un rollback est un nouveau
/// deploiement dont `image` (ou `compose.bundle`) est celle d'un
/// deploiement conserve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeploySpec {
    pub deployment_id: DeploymentId,
    pub service_id: ServiceId,
    /// Image a lancer (projet Compose : image du service expose, a titre
    /// indicatif ; l'ensemble vient de `compose.bundle`).
    pub image: ImageRef,
    /// Port ecoute par le conteneur sur le reseau du projet, s'il est
    /// expose (healthcheck HTTP et upstream du proxy).
    pub internal_port: Option<u16>,
    /// Projet : reseau `ciphera-p-<project_id>`.
    pub project_id: ProjectId,
    pub healthcheck: Healthcheck,
    /// Projet Compose : service expose et ensemble d'images a lancer.
    pub compose: Option<ComposeDeploy>,
    /// Nom du service : le conteneur est joignable sous l'alias
    /// `<nom>.internal` sur le reseau du projet (valide par l'agent ; sans
    /// valeur, aucun alias). Ajoute apres la v1 : dernier champ (une trame
    /// plus ancienne, sans lui, se lit avec `None`).
    #[serde(default)]
    pub service_name: Option<String>,
    /// Variables d'environnement du conteneur (projet puis globales, deja
    /// resolues), secrets dechiffres par le plan de controle juste avant
    /// l'envoi. L'agent ne les ecrit jamais (etat local, journaux, issues).
    /// Ajoute apres `service_name` : dernier champ (une trame plus ancienne,
    /// sans lui, se lit avec un environnement vide).
    #[serde(default)]
    pub env: Vec<EnvVar>,
}

/// Une variable d'environnement d'un deploiement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    /// `^[A-Z_][A-Z0-9_]{0,127}$` (verifiee par l'agent).
    pub key: String,
    pub value: EnvValue,
}

/// Valeur d'une variable d'environnement, secrete ou non : effacee de la
/// memoire a sa liberation, masquee par `Debug`. Elle n'est serialisee que
/// dans la trame `proto` (lien QUIC + mTLS).
#[derive(Clone, PartialEq, Eq)]
pub struct EnvValue(Zeroizing<String>);

impl EnvValue {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    /// Valeur en clair : a ne passer qu'a l'environnement du conteneur.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EnvValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnvValue(<masque>)")
    }
}

impl Serialize for EnvValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for EnvValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

/// Lecture de `DeploySpec`, comme `BuildSpec` : `service_name` puis `env`
/// absents (fin de la charge, trame d'un pair plus ancien) se lisent `None`
/// et vide ; toute autre erreur, et une fin de charge sur un autre champ,
/// restent des erreurs.
impl<'de> Deserialize<'de> for DeploySpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, SeqAccess, Visitor};

        const FIELDS: &[&str] = &[
            "deployment_id",
            "service_id",
            "image",
            "internal_port",
            "project_id",
            "healthcheck",
            "compose",
            "service_name",
            "env",
        ];
        const EXPECTING: &str = "un ordre de deploiement";

        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = DeploySpec;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(EXPECTING)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<DeploySpec, A::Error> {
                fn next<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(
                    seq: &mut A,
                    i: usize,
                ) -> Result<T, A::Error> {
                    seq.next_element()?
                        .ok_or_else(|| A::Error::invalid_length(i, &EXPECTING))
                }
                let deployment_id = next(&mut seq, 0)?;
                let service_id = next(&mut seq, 1)?;
                let image = next(&mut seq, 2)?;
                let internal_port = next(&mut seq, 3)?;
                let project_id = next(&mut seq, 4)?;
                let healthcheck = next(&mut seq, 5)?;
                let compose = next(&mut seq, 6)?;
                let (service_name, more) = match seq.next_element::<Option<String>>() {
                    Ok(v) => (v.flatten(), true),
                    Err(e) if is_end_of_payload(&e) => (None, false),
                    Err(e) => return Err(e),
                };
                let env = if more {
                    match seq.next_element::<Vec<EnvVar>>() {
                        Ok(v) => v.unwrap_or_default(),
                        Err(e) if is_end_of_payload(&e) => Vec::new(),
                        Err(e) => return Err(e),
                    }
                } else {
                    Vec::new()
                };
                Ok(DeploySpec {
                    deployment_id,
                    service_id,
                    image,
                    internal_port,
                    project_id,
                    healthcheck,
                    compose,
                    service_name,
                    env,
                })
            }
        }

        deserializer.deserialize_struct("DeploySpec", FIELDS, V)
    }
}

/// Healthcheck HTTP du nouveau conteneur : `GET <path>` sur son adresse du
/// reseau du projet, `tries` essais espaces de `interval_secs` secondes ;
/// reussi sur un statut 2xx ou 3xx. Sans port expose : le conteneur doit
/// etre en marche (sain pour Docker).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Healthcheck {
    /// Chemin absolu (`/` par defaut), ASCII visible sans espace.
    pub path: String,
    /// 1 a `Healthcheck::MAX_TRIES`.
    pub tries: u8,
    /// 1 a `Healthcheck::MAX_INTERVAL_SECS`.
    pub interval_secs: u16,
}

impl Healthcheck {
    pub const DEFAULT_TRIES: u8 = 5;
    pub const DEFAULT_INTERVAL_SECS: u16 = 5;
    pub const MAX_TRIES: u8 = 30;
    pub const MAX_INTERVAL_SECS: u16 = 60;
    pub const MAX_PATH_LEN: usize = 1024;
}

impl Default for Healthcheck {
    /// `/`, 5 essais toutes les 5 s.
    fn default() -> Self {
        Self {
            path: "/".to_string(),
            tries: Self::DEFAULT_TRIES,
            interval_secs: Self::DEFAULT_INTERVAL_SECS,
        }
    }
}

/// Deploiement d'un projet Compose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeDeploy {
    /// Service expose par le domaine (bascule sans interruption) ; les
    /// autres sont redemarres dans l'ordre de `depends_on`.
    pub service: String,
    /// Deploiement dont le build a produit l'ensemble Compose (le meme
    /// deploiement, ou un deploiement conserve pour un rollback).
    pub bundle: DeploymentId,
}

/// Jeton d'installation GitHub d'un build (Task 25 le cree, l'agent le passe
/// a `git` par `GIT_ASKPASS`). Efface de la memoire a sa liberation ;
/// `Debug` et `Display` le masquent. Il n'est serialise que dans la trame
/// `proto` (lien QUIC + mTLS), jamais dans un journal.
#[derive(Clone, PartialEq, Eq)]
pub struct GitToken(Zeroizing<String>);

impl GitToken {
    pub fn new(token: String) -> Self {
        Self(Zeroizing::new(token))
    }

    /// Valeur du jeton : a ne passer qu'a l'environnement du processus
    /// `git` (jamais en argument, dans une URL ou un journal).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GitToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GitToken(<masque>)")
    }
}

impl fmt::Display for GitToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<masque>")
    }
}

impl Serialize for GitToken {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for GitToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::new)
    }
}

/// Provenance du code a construire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildSource {
    /// Depot Git (`https://github.com/<proprietaire>/<nom>` en v0.1), branche
    /// et commit exact a construire.
    Git {
        url: String,
        branch: String,
        commit_sha: String,
    },
    /// Image existante : telechargee puis etiquetee comme un build.
    Image { reference: ImageRef },
}

/// Methode de build d'une source Git (chemins relatifs au depot).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildMethod {
    Railpack,
    Dockerfile {
        path: String,
    },
    /// Fichier Compose et service expose par le domaine.
    Compose {
        path: String,
        service: String,
    },
}

/// Ordre de build : l'agent clone, construit avec BuildKit et etiquette
/// l'image `ciphera/<projet>-<environnement>:<deployment_id>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BuildSpec {
    pub deployment_id: DeploymentId,
    /// Nom du projet (`^[a-z0-9][a-z0-9-]{0,62}$`).
    pub project: String,
    /// Nom de l'environnement (meme regle).
    pub environment: String,
    pub source: BuildSource,
    pub build: BuildMethod,
    /// Jeton d'acces au depot (depot prive), s'il y en a un.
    pub git_token: Option<GitToken>,
    /// Dossier du depot ou se fait le build (monorepo), relatif ; `None` ou
    /// vide : racine du depot. Ajoute apres la v1 : dernier champ (une trame
    /// plus ancienne, sans lui, se lit avec `None`). Valide par l'agent.
    #[serde(default)]
    pub root_directory: Option<String>,
}

/// Lecture de `BuildSpec` : postcard ignore `#[serde(default)]` (champs lus
/// dans l'ordre, sans noms), d'ou ce visiteur, qui lit `root_directory`
/// absent (fin de la charge, trame d'un pair plus ancien) comme `None`.
/// Toute autre erreur, et une fin de charge sur un autre champ, restent
/// des erreurs.
impl<'de> Deserialize<'de> for BuildSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, SeqAccess, Visitor};

        const FIELDS: &[&str] = &[
            "deployment_id",
            "project",
            "environment",
            "source",
            "build",
            "git_token",
            "root_directory",
        ];
        const EXPECTING: &str = "un ordre de build";

        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = BuildSpec;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(EXPECTING)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<BuildSpec, A::Error> {
                fn next<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(
                    seq: &mut A,
                    i: usize,
                ) -> Result<T, A::Error> {
                    seq.next_element()?
                        .ok_or_else(|| A::Error::invalid_length(i, &EXPECTING))
                }
                let deployment_id = next(&mut seq, 0)?;
                let project = next(&mut seq, 1)?;
                let environment = next(&mut seq, 2)?;
                let source = next(&mut seq, 3)?;
                let build = next(&mut seq, 4)?;
                let git_token = next(&mut seq, 5)?;
                let root_directory = match seq.next_element::<Option<String>>() {
                    Ok(v) => v.flatten(),
                    Err(e) if is_end_of_payload(&e) => None,
                    Err(e) => return Err(e),
                };
                Ok(BuildSpec {
                    deployment_id,
                    project,
                    environment,
                    source,
                    build,
                    git_token,
                    root_directory,
                })
            }
        }

        deserializer.deserialize_struct("BuildSpec", FIELDS, V)
    }
}

/// Erreur postcard « fin de la charge » (et elle seule).
fn is_end_of_payload<E: fmt::Display>(e: &E) -> bool {
    e.to_string() == postcard::Error::DeserializeUnexpectedEnd.to_string()
}

/// Source des journaux demandes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LogTarget {
    Deployment(DeploymentId),
    Service(ServiceId),
}

/// Certificat attendu pour une route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TlsMode {
    /// Certificat obtenu par ACME (Task 30) ; en attendant, la route est
    /// servie sur le port 80 seulement.
    Acme,
    /// HTTP seulement.
    None,
}

/// Route du proxy du noeud : `host` + prefixe de chemin -> conteneurs sains
/// du service (adresses IP sur le reseau du projet), ou redirection 308.
/// La table complete est envoyee a chaque fois (`ApplyRoutes`) et remplace
/// la precedente d'un bloc.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Nom d'hote en minuscules, sans port.
    pub host: String,
    /// Prefixe de chemin (`/` pour tout l'hote).
    pub path_prefix: String,
    /// Plan de controle -> agent : ignore. L'agent les remplit lui-meme
    /// (adresse du conteneur servant `service` sur le reseau de son projet)
    /// avant d'envoyer la table au proxy.
    pub upstreams: Vec<SocketAddr>,
    /// Origine `https://hote[:port]` (ou `http://`) vers laquelle rediriger
    /// (308), chemin et requete conserves ; ni chemin, ni requete, ni
    /// identifiants. Prioritaire sur `upstreams`.
    pub redirect_to: Option<String>,
    pub tls: TlsMode,
    /// Service servi par la route. Sans valeur : redirection seulement, ou,
    /// sans `redirect_to` non plus, route du domaine du panneau (amont :
    /// l'ecoute locale du panneau une fois son certificat actif, sinon
    /// aucun ; jamais servie en HTTP clair, et cible du repli des hotes IP
    /// du proxy des qu'elle a un amont).
    pub service: Option<ServiceId>,
}

/// Reponse du proxy du noeud a `ControlToAgent::ApplyRoutes`,
/// `IssueCertificate`, `CheckCertificate` ou `RemoveCertificate`, recue sur
/// son socket de pilotage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyReply {
    /// Table appliquee (nombre de routes).
    Applied { routes: u32 },
    /// Table refusee : la precedente reste en place.
    Rejected { reason: String },
    /// Issue de `IssueCertificate`. Ajoute apres `Rejected`, en dernier.
    Certificate(CertificateOutcome),
}

/// Issue d'une demande de certificat ACME (HTTP-01) au proxy du noeud.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CertificateOutcome {
    /// Certificat emis, installe et stocke chiffre par le proxy (la cle
    /// privee ne quitte jamais le proxy). `not_after` : fin de validite
    /// (secondes Unix) ; `issuer` : nom commun de l'emetteur.
    Issued { not_after: i64, issuer: String },
    /// Echec (raison sans secret).
    Failed { reason: String },
    /// Reponse a `CheckCertificate` : le proxy n'a aucun certificat pour
    /// l'hote (autre serveur, fichier perdu). Ajoute apres `Failed`.
    Missing,
    /// Reponse a `RemoveCertificate` : certificat et cle supprimes (ou
    /// deja absents). Ajoute apres `Missing`, en dernier.
    Removed,
}

/// Etapes d'un deploiement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Clone,
    Build,
    Image,
    Start,
    Healthcheck,
    Switch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StageStatus {
    Started,
    Succeeded,
    Failed {
        message: String,
    },
    /// Annule par `Stop` : build et nouveau conteneur nettoyes, routes
    /// inchangees.
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogStream {
    Stdout,
    Stderr,
}

/// Une ligne de journal d'un conteneur ou d'un build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub target: LogTarget,
    pub stream: LogStream,
    /// Millisecondes depuis l'epoque Unix.
    pub timestamp_ms: i64,
    pub line: String,
}

/// Metriques de base du noeud.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NodeMetrics {
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    pub mem_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    /// Conteneur servi de chaque service de l'agent. Ajoute apres la v1 :
    /// dernier champ (une trame plus ancienne, sans lui, se lit avec une
    /// liste vide).
    #[serde(default)]
    pub containers: Vec<ContainerMetrics>,
}

/// Mesure du conteneur servi d'un service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainerMetrics {
    pub deployment_id: DeploymentId,
    pub service_id: ServiceId,
    /// CPU en pourcentage d'un coeur (100 = un coeur plein).
    pub cpu_percent: f32,
    pub mem_used_bytes: u64,
    /// En marche et sain pour Docker (faux : arrete, en echec ou disparu).
    pub running: bool,
}

/// Lecture de `NodeMetrics`, comme `BuildSpec` : `containers` absent (fin
/// de la charge, trame d'un agent plus ancien) se lit comme une liste vide ;
/// toute autre erreur, et une fin de charge sur un autre champ, restent des
/// erreurs.
impl<'de> Deserialize<'de> for NodeMetrics {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, SeqAccess, Visitor};

        const FIELDS: &[&str] = &[
            "cpu_percent",
            "mem_used_bytes",
            "mem_total_bytes",
            "disk_used_bytes",
            "disk_total_bytes",
            "net_rx_bytes",
            "net_tx_bytes",
            "containers",
        ];
        const EXPECTING: &str = "des metriques du noeud";

        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = NodeMetrics;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(EXPECTING)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<NodeMetrics, A::Error> {
                fn next<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(
                    seq: &mut A,
                    i: usize,
                ) -> Result<T, A::Error> {
                    seq.next_element()?
                        .ok_or_else(|| A::Error::invalid_length(i, &EXPECTING))
                }
                let cpu_percent = next(&mut seq, 0)?;
                let mem_used_bytes = next(&mut seq, 1)?;
                let mem_total_bytes = next(&mut seq, 2)?;
                let disk_used_bytes = next(&mut seq, 3)?;
                let disk_total_bytes = next(&mut seq, 4)?;
                let net_rx_bytes = next(&mut seq, 5)?;
                let net_tx_bytes = next(&mut seq, 6)?;
                let containers = match seq.next_element::<Vec<ContainerMetrics>>() {
                    Ok(v) => v.unwrap_or_default(),
                    Err(e) if is_end_of_payload(&e) => Vec::new(),
                    Err(e) => return Err(e),
                };
                Ok(NodeMetrics {
                    cpu_percent,
                    mem_used_bytes,
                    mem_total_bytes,
                    disk_used_bytes,
                    disk_total_bytes,
                    net_rx_bytes,
                    net_tx_bytes,
                    containers,
                })
            }
        }

        deserializer.deserialize_struct("NodeMetrics", FIELDS, V)
    }
}

/// Deploiement en cours d'execution sur l'agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningDeployment {
    pub deployment_id: DeploymentId,
    pub service_id: ServiceId,
    pub image: ImageRef,
    pub healthy: bool,
    /// Deploiement courant de son service (routes basculees sur lui).
    pub serving: bool,
}

/// Issue definitive d'un build ou d'un deploiement, gardee par l'agent :
/// `Failed` ou `Cancelled` (toute etape), `Succeeded` sur `Image` (build
/// conclu) ou `Switch` (deploiement servi). Un ordre rejoue pour ce
/// deploiement renvoie cette issue sans rien refaire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentOutcome {
    pub deployment_id: DeploymentId,
    pub stage: Stage,
    pub status: StageStatus,
}

/// Etat complet de l'agent, envoye a chaque connexion (instantane,
/// idempotent) : conteneurs de deploiement, operations en cours et issues
/// recentes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentState {
    pub deployments: Vec<RunningDeployment>,
    /// Builds et deploiements en cours sur l'agent (ni termines, ni
    /// annules).
    pub in_progress: Vec<DeploymentId>,
    /// Issues definitives recentes (`MAX_OUTCOMES` au plus, 30 jours), la
    /// plus recente d'abord.
    pub outcomes: Vec<DeploymentOutcome>,
}

impl AgentState {
    /// Issues gardees au plus par l'agent.
    pub const MAX_OUTCOMES: usize = 500;
    /// Duree de garde d'une issue (secondes).
    pub const OUTCOME_RETENTION_SECS: u64 = 30 * 24 * 3600;
    /// Longueur maximale (octets) d'un message d'echec garde.
    pub const MAX_OUTCOME_MESSAGE: usize = 512;
}

impl StageStatus {
    /// Etat definitif d'une operation (`Failed`, `Cancelled`, ou
    /// `Succeeded` sur la derniere etape d'un build ou d'un deploiement).
    pub fn is_terminal(&self, stage: Stage) -> bool {
        match self {
            Self::Failed { .. } | Self::Cancelled => true,
            Self::Succeeded => matches!(stage, Stage::Image | Stage::Switch),
            Self::Started => false,
        }
    }
}

/// Plan de controle -> agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlToAgent {
    Hello {
        version: u16,
    },
    Deploy(DeploySpec),
    Stop {
        deployment_id: DeploymentId,
    },
    /// Envoie les `tail` dernieres lignes de la cible puis, avec `follow`,
    /// la suit jusqu'a `StopLogs` ou l'arret du conteneur. L'agent garde un
    /// seul suivi par cible : une nouvelle demande sur une cible deja suivie
    /// est ignoree.
    Logs {
        target: LogTarget,
        tail: u32,
        follow: bool,
    },
    Pull {
        image: ImageRef,
    },
    ApplyRoutes(Vec<Route>),
    RequestMetrics,
    RemoveDeployment {
        deployment_id: DeploymentId,
    },
    /// Ajoute apres la v1 initiale : en dernier (rang postcard stable).
    Build(BuildSpec),
    /// Arrete le suivi de `target` (dernier lecteur parti). Sans suivi en
    /// cours : ignore. Ajoute apres `Build`, en dernier.
    StopLogs {
        target: LogTarget,
    },
    /// Redemarre les conteneurs du deploiement servi `deployment_id` (et les
    /// autres services de son ensemble Compose), sans rebuild. Deploiement
    /// non servi : ignore. Ajoute apres `StopLogs`, en dernier.
    Restart {
        deployment_id: DeploymentId,
    },
    /// Arrete les conteneurs du deploiement servi `deployment_id` (et les
    /// autres services de son ensemble Compose) sans le retirer : il reste
    /// le deploiement servi, `Restart` le relance et un nouveau deploiement
    /// le remplace. Deploiement non servi, ou operation en cours : ignore.
    /// Ajoute apres `Restart`, en dernier.
    StopContainers {
        deployment_id: DeploymentId,
    },
    /// Demande au proxy du noeud un certificat ACME (HTTP-01) pour `host`
    /// (route `TlsMode::Acme` deja poussee). L'issue revient par
    /// `AgentToControl::Certificate`. Ajoute apres `StopContainers`, en
    /// dernier.
    IssueCertificate {
        host: String,
    },
    /// Demande au proxy du noeud s'il detient un certificat pour `host`,
    /// sans rien emettre : `Issued` (fin de validite et emetteur du
    /// certificat detenu) ou `Missing`, par `AgentToControl::Certificate`.
    /// Ajoute apres `IssueCertificate`.
    CheckCertificate {
        host: String,
    },
    /// Supprime le certificat et la cle de `host` du proxy du noeud (domaine
    /// supprime) ; reponse `Removed` par `AgentToControl::Certificate`.
    /// Ajoute apres `CheckCertificate`, en dernier.
    RemoveCertificate {
        host: String,
    },
    /// Redemande l'etat complet de l'agent (`AgentToControl::State`, comme a
    /// la connexion) : le plan de controle a perdu des messages et se
    /// resynchronise. Ajoute apres `RemoveCertificate`, en dernier.
    RequestState,
    /// Certificat client renouvele de l'agent (PEM), emis par la CA de
    /// l'instance pour la meme cle et le meme identifiant, aux deux tiers de
    /// la validite du certificat presente. L'agent le verifie, le garde
    /// (remplacement atomique de `agent.crt`) puis se reconnecte avec lui ;
    /// l'ancien reste accepte tant que le nouveau n'a pas servi. Ajoute
    /// apres `RequestState`, en dernier.
    RenewedCertificate {
        cert_pem: String,
    },
}

/// Agent -> plan de controle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AgentToControl {
    Hello {
        version: u16,
        agent_id: ServerId,
    },
    DeployProgress {
        deployment_id: DeploymentId,
        stage: Stage,
        status: StageStatus,
    },
    LogLine(LogLine),
    Metrics(NodeMetrics),
    State(AgentState),
    Error {
        code: u32,
        message: String,
    },
    /// Issue de `ControlToAgent::IssueCertificate`, `CheckCertificate` ou
    /// `RemoveCertificate`. Ajoute apres `Error`, en dernier.
    Certificate {
        host: String,
        outcome: CertificateOutcome,
    },
}
