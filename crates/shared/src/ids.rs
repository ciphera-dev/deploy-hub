//! Identifiants typés (newtypes sur `Uuid`) : impossible de confondre un
//! `ProjectId` et un `ServiceId`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "sqlx", derive(sqlx::Type))]
        #[cfg_attr(feature = "sqlx", sqlx(transparent))]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Nouvel identifiant aleatoire (UUID v4).
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }
    };
}

id_type!(
    /// Utilisateur du panneau.
    UserId
);
id_type!(
    /// Projet.
    ProjectId
);
id_type!(
    /// Environnement d'un projet.
    EnvironmentId
);
id_type!(
    /// Service deploye.
    ServiceId
);
id_type!(
    /// Deploiement d'un service.
    DeploymentId
);
id_type!(
    /// Domaine rattache a un service.
    DomainId
);
id_type!(
    /// Serveur gere par Ciphera.
    ServerId
);
id_type!(
    /// Tache de la file PostgreSQL.
    JobId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_plain_uuid_string() {
        let uuid = Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
        let id = ProjectId(uuid);
        assert_eq!(
            serde_json::to_string(&id).unwrap(),
            "\"67e55044-10b1-426f-9247-bb680e5fe0c8\""
        );
        let back: ProjectId =
            serde_json::from_str("\"67e55044-10b1-426f-9247-bb680e5fe0c8\"").unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn display_and_from_str_round_trip() {
        let id = ServerId::new();
        assert_eq!(id.to_string().parse::<ServerId>().unwrap(), id);
        assert!("pas-un-uuid".parse::<ServerId>().is_err());
    }

    #[test]
    fn new_ids_are_distinct() {
        assert_ne!(UserId::new(), UserId::new());
    }
}
