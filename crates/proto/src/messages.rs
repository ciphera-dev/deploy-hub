//! Messages du protocole (version `PROTO_VERSION`).
//!
//! Compatibilite : `Hello` est la premiere variante des deux enums et
//! `version` son premier champ, pour qu'un pair d'une autre version puisse
//! toujours lire le `Hello` (postcard encode la variante par son rang). Ne
//! jamais reordonner les variantes ni les champs d'une version publiee.

use std::fmt;
use std::net::SocketAddr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use shared::ids::{DeploymentId, EnvironmentId, ProjectId, ServerId, ServiceId};
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
    /// valeur, aucun alias). Ajoute apres la v1 : champ de fin (une trame
    /// plus ancienne, sans lui, se lit avec `None`).
    #[serde(default)]
    pub service_name: Option<String>,
    /// Variables d'environnement du conteneur (projet puis globales, deja
    /// resolues), secrets dechiffres par le plan de controle juste avant
    /// l'envoi. L'agent ne les ecrit jamais (etat local, journaux, issues).
    /// Ajoute apres `service_name` : champ de fin (une trame plus ancienne,
    /// sans lui, se lit avec un environnement vide).
    #[serde(default)]
    pub env: Vec<EnvVar>,
    /// Environnement du deploiement. Ajoute apres `env` (v6) : champ de fin
    /// (une trame plus ancienne, sans lui, se lit avec `None`).
    #[serde(default)]
    pub environment_id: Option<EnvironmentId>,
    /// Utilisateur du conteneur (v6), sans valeur : celui de l'image. Ajoute
    /// apres `environment_id`.
    #[serde(default)]
    pub run_as: Option<String>,
    /// Systeme de fichiers racine en lecture seule (v6). Ajoute apres
    /// `run_as`, en dernier.
    #[serde(default)]
    pub read_only_rootfs: bool,
}

/// `DeploySpec::run_as` valide : `^[0-9]{1,10}(:[0-9]{1,10})?$` (uid, ou
/// uid:gid, numeriques). Point unique du plan de controle et de l'agent.
pub fn valid_run_as(run_as: &str) -> bool {
    let id = |s: &str| (1..=10).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    match run_as.split_once(':') {
        Some((uid, gid)) => id(uid) && id(gid),
        None => id(run_as),
    }
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
            "environment_id",
            "run_as",
            "read_only_rootfs",
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
                // Champs de fin : une fin de charge exactement avant l'un
                // d'eux arrete la lecture (les suivants sont absents aussi).
                let service_name = tail_option::<_, String>(&mut seq)?;
                let env = if service_name.is_present() {
                    tail_vec::<_, EnvVar>(&mut seq)?
                } else {
                    Tail::Absent
                };
                let environment_id = if env.is_present() {
                    tail_option::<_, EnvironmentId>(&mut seq)?
                } else {
                    Tail::Absent
                };
                let run_as = if environment_id.is_present() {
                    tail_option::<_, String>(&mut seq)?
                } else {
                    Tail::Absent
                };
                let read_only_rootfs = if run_as.is_present() {
                    tail_field::<_, bool>(&mut seq)?
                } else {
                    Tail::Absent
                };
                let service_name = service_name.into_value();
                let env = env.into_value();
                let environment_id = environment_id.into_value();
                let run_as = run_as.into_value();
                let read_only_rootfs = read_only_rootfs.into_value();
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
                    environment_id,
                    run_as,
                    read_only_rootfs,
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
/// l'image `ciphera.invalid/<projet>-<environnement>:<deployment_id>`.
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
    /// vide : racine du depot. Ajoute apres la v1 : champ de fin (une trame
    /// plus ancienne, sans lui, se lit avec `None`). Valide par l'agent.
    #[serde(default)]
    pub root_directory: Option<String>,
    /// Projet du build (v6). Ajoute apres `root_directory` : une trame plus
    /// ancienne, sans lui, se lit avec `None`.
    #[serde(default)]
    pub project_id: Option<ProjectId>,
    /// Environnement du build (v6). Ajoute apres `project_id`, en dernier.
    #[serde(default)]
    pub environment_id: Option<EnvironmentId>,
}

/// Lecture de `BuildSpec` : postcard ignore `#[serde(default)]` (champs lus
/// dans l'ordre, sans noms), d'ou ce visiteur, qui lit `root_directory`,
/// `project_id` et `environment_id` absents (fin de la charge exactement a
/// la frontiere du champ, trame d'un pair plus ancien) comme `None`. Une
/// charge coupee au milieu d'un champ, ou toute autre erreur, reste une
/// erreur.
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
            "project_id",
            "environment_id",
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
                let root_directory = tail_option::<_, String>(&mut seq)?;
                let project_id = if root_directory.is_present() {
                    tail_option::<_, ProjectId>(&mut seq)?
                } else {
                    Tail::Absent
                };
                let environment_id = if project_id.is_present() {
                    tail_option::<_, EnvironmentId>(&mut seq)?
                } else {
                    Tail::Absent
                };
                let root_directory = root_directory.into_value();
                let project_id = project_id.into_value();
                let environment_id = environment_id.into_value();
                Ok(BuildSpec {
                    deployment_id,
                    project,
                    environment,
                    source,
                    build,
                    git_token,
                    root_directory,
                    project_id,
                    environment_id,
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

/// Champ de fin d'une structure : absent (la charge s'arrete exactement a
/// sa frontiere) ou lu.
enum Tail<T> {
    Absent,
    Present(T),
}

impl<T: Default> Tail<T> {
    fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }

    /// Valeur lue, ou valeur par defaut (`None`, liste vide) si absent.
    fn into_value(self) -> T {
        match self {
            Self::Absent => T::default(),
            Self::Present(v) => v,
        }
    }
}

/// Lit un champ de fin `Option<T>`. Seule la fin de charge sur l'octet de
/// tete (`None`/`Some`) vaut « champ absent » : une charge coupee dans la
/// valeur d'un `Some` est une erreur.
fn tail_option<'de, A, T>(seq: &mut A) -> Result<Tail<Option<T>>, A::Error>
where
    A: serde::de::SeqAccess<'de>,
    T: Deserialize<'de>,
{
    tail_field::<A, TailOption<T>>(seq).map(|t| match t {
        Tail::Absent => Tail::Absent,
        Tail::Present(TailOption(v)) => Tail::Present(v),
    })
}

/// Comme `tail_option`, pour un champ de fin `Vec<T>` : seule la fin de
/// charge sur la longueur vaut « champ absent » ; une charge coupee dans un
/// element est une erreur.
fn tail_vec<'de, A, T>(seq: &mut A) -> Result<Tail<Vec<T>>, A::Error>
where
    A: serde::de::SeqAccess<'de>,
    T: Deserialize<'de>,
{
    tail_field::<A, TailVec<T>>(seq).map(|t| match t {
        Tail::Absent => Tail::Absent,
        Tail::Present(TailVec(v)) => Tail::Present(v),
    })
}

fn tail_field<'de, A, T>(seq: &mut A) -> Result<Tail<T>, A::Error>
where
    A: serde::de::SeqAccess<'de>,
    T: Deserialize<'de>,
{
    match seq.next_element::<T>() {
        Ok(Some(v)) => Ok(Tail::Present(v)),
        Ok(None) => Ok(Tail::Absent),
        Err(e) if is_end_of_payload(&e) => Ok(Tail::Absent),
        Err(e) => Err(e),
    }
}

/// Erreur d'une charge coupee dans un champ de fin (distincte de la fin de
/// charge a la frontiere, que `is_end_of_payload` reconnait).
const MID_FIELD: &str = "charge tronquee au milieu d'un champ";

struct TailOption<T>(Option<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for TailOption<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Visitor};

        struct V<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = TailOption<T>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("un champ optionnel")
            }

            fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
                Ok(TailOption(None))
            }

            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                T::deserialize(d).map(|v| TailOption(Some(v))).map_err(|e| {
                    if is_end_of_payload(&e) {
                        D::Error::custom(MID_FIELD)
                    } else {
                        e
                    }
                })
            }
        }
        deserializer.deserialize_option(V(std::marker::PhantomData))
    }
}

/// Champ de fin `Vec<T>`. La longueur (varint postcard) est lue octet par
/// octet : une fin de charge sur le premier octet est la frontiere du champ
/// (absent), sur un octet suivant ou dans un element c'est une charge coupee
/// (erreur). Format sur le fil inchange.
struct TailVec<T>(Vec<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for TailVec<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, SeqAccess, Visitor};

        struct V<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = TailVec<T>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("une liste")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                // Premier octet : son erreur de fin de charge est renvoyee
                // telle quelle (`tail_field` la lit « absent »).
                let mut b = seq
                    .next_element::<u8>()?
                    .ok_or_else(|| A::Error::custom(MID_FIELD))?;
                let mut len = u64::from(b & 0x7F);
                let mut shift = 7u32;
                while b & 0x80 != 0 {
                    b = match seq.next_element::<u8>() {
                        Ok(Some(b)) => b,
                        Ok(None) => return Err(A::Error::custom(MID_FIELD)),
                        Err(e) if is_end_of_payload(&e) => {
                            return Err(A::Error::custom(MID_FIELD));
                        }
                        Err(e) => return Err(e),
                    };
                    // 10 octets au plus, le dernier ne portant qu'un bit.
                    if shift >= 64 || (shift == 63 && b > 1) {
                        return Err(A::Error::custom("longueur invalide"));
                    }
                    len |= u64::from(b & 0x7F) << shift;
                    shift += 7;
                }
                let len =
                    usize::try_from(len).map_err(|_| A::Error::custom("longueur invalide"))?;
                // Reservation bornee : la longueur annoncee n'est pas fiable.
                let mut out = Vec::with_capacity(len.min(1024));
                for _ in 0..len {
                    match seq.next_element::<T>() {
                        Ok(Some(v)) => out.push(v),
                        Ok(None) => return Err(A::Error::custom(MID_FIELD)),
                        Err(e) if is_end_of_payload(&e) => {
                            return Err(A::Error::custom(MID_FIELD));
                        }
                        Err(e) => return Err(e),
                    }
                }
                Ok(TailVec(out))
            }
        }
        deserializer.deserialize_tuple(usize::MAX, V(std::marker::PhantomData))
    }
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunningDeployment {
    pub deployment_id: DeploymentId,
    pub service_id: ServiceId,
    pub image: ImageRef,
    pub healthy: bool,
    /// Deploiement courant de son service (routes basculees sur lui).
    pub serving: bool,
    /// Le conteneur tourne en root (v6). Ajoute apres `serving`, en dernier
    /// (une trame plus ancienne, sans lui, se lit avec `false`).
    #[serde(default)]
    pub runs_as_root: bool,
}

/// Lecture de `RunningDeployment`, comme `BuildSpec` : `runs_as_root` absent
/// (fin de la charge exactement a sa frontiere) se lit `false`. Valable
/// seulement si l'element termine la charge : un ancien element au milieu
/// d'une liste ne se relit pas (le `Hello` refuse toute autre version).
impl<'de> Deserialize<'de> for RunningDeployment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{Error, SeqAccess, Visitor};

        const FIELDS: &[&str] = &[
            "deployment_id",
            "service_id",
            "image",
            "healthy",
            "serving",
            "runs_as_root",
        ];
        const EXPECTING: &str = "un deploiement en cours";

        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = RunningDeployment;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str(EXPECTING)
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<RunningDeployment, A::Error> {
                fn next<'de, A: SeqAccess<'de>, T: Deserialize<'de>>(
                    seq: &mut A,
                    i: usize,
                ) -> Result<T, A::Error> {
                    seq.next_element()?
                        .ok_or_else(|| A::Error::invalid_length(i, &EXPECTING))
                }
                Ok(RunningDeployment {
                    deployment_id: next(&mut seq, 0)?,
                    service_id: next(&mut seq, 1)?,
                    image: next(&mut seq, 2)?,
                    healthy: next(&mut seq, 3)?,
                    serving: next(&mut seq, 4)?,
                    runs_as_root: tail_field::<_, bool>(&mut seq)?.into_value(),
                })
            }
        }

        deserializer.deserialize_struct("RunningDeployment", FIELDS, V)
    }
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
    /// Retire le projet de l'agent : conteneurs, reseau `ciphera-p-<id>` et,
    /// avec `purge_volumes`, ses volumes ; reponse `AgentToControl::
    /// ProjectRemoved`. Ajoute apres `RenewedCertificate`, en dernier (v6).
    RemoveProject {
        project_id: ProjectId,
        purge_volumes: bool,
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
    /// Accuse de `ControlToAgent::RemoveDeployment` : conteneurs et image du
    /// deploiement supprimes (ou deja absents). Ajoute apres `Certificate`
    /// (v6).
    DeploymentRemoved {
        deployment_id: DeploymentId,
    },
    /// Accuse de `ControlToAgent::RemoveProject`. Ajoute apres
    /// `DeploymentRemoved`, en dernier (v6).
    ProjectRemoved {
        project_id: ProjectId,
    },
}
