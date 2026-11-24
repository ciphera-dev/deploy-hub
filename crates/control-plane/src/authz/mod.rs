//! Autorisation : roles fixes, matrice par defaut et extracteur `Require`.
//!
//! Une route reservee a un niveau d'acces prend `Require<R, L>` en argument
//! (ex. `Require<res::Projects, lvl::Write>`) : session complete exigee
//! (`CurrentUser`), puis 403 `forbidden` si le role du compte n'atteint pas
//! ce niveau. Un role inconnu en base n'accorde rien.

pub mod matrix;

use std::marker::PhantomData;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use shared::error::ApiError;

pub use matrix::{allowed, level};

use crate::auth::CurrentUser;
use crate::state::AppState;

/// Roles fixes : `Owner > Admin > Ops | Developer > Viewer` (Ops et
/// Developer ne sont pas comparables).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Owner,
    Admin,
    Ops,
    Developer,
    Viewer,
}

impl Role {
    /// Valeur de la colonne `users.role`.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Ops => "ops",
            Role::Developer => "developer",
            Role::Viewer => "viewer",
        }
    }

    pub fn parse(value: &str) -> Option<Role> {
        Some(match value {
            "owner" => Role::Owner,
            "admin" => Role::Admin,
            "ops" => Role::Ops,
            "developer" => Role::Developer,
            "viewer" => Role::Viewer,
            _ => return None,
        })
    }

    /// `true` si `other` n'est pas au-dessus de `self` dans l'ordre partiel :
    /// on n'attribue jamais un role superieur au sien (ni, pour Ops et
    /// Developer, le role voisin).
    pub fn can_grant(self, other: Role) -> bool {
        match self {
            Role::Owner => true,
            Role::Admin => other != Role::Owner,
            Role::Ops | Role::Developer => other == self || other == Role::Viewer,
            Role::Viewer => other == Role::Viewer,
        }
    }

    /// `true` si `self` peut modifier ou retirer un membre de role `target` :
    /// le proprietaire gere tout autre membre (proprietaires compris), l'admin
    /// seulement ops, developer et viewer, les autres roles personne. Ne
    /// couvre pas « jamais soi-meme » ni « jamais le dernier proprietaire »,
    /// que l'appelant verifie.
    pub fn can_manage(self, target: Role) -> bool {
        match self {
            Role::Owner => true,
            Role::Admin => matches!(target, Role::Ops | Role::Developer | Role::Viewer),
            Role::Ops | Role::Developer | Role::Viewer => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Role::{self, *};

    const ALL: [Role; 5] = [Owner, Admin, Ops, Developer, Viewer];

    #[test]
    fn owner_manages_every_role() {
        assert!(ALL.iter().all(|t| Owner.can_manage(*t)));
    }

    #[test]
    fn admin_manages_only_ops_developer_viewer() {
        assert!(!Admin.can_manage(Owner));
        assert!(!Admin.can_manage(Admin));
        for target in [Ops, Developer, Viewer] {
            assert!(Admin.can_manage(target), "{target:?}");
        }
    }

    #[test]
    fn other_roles_manage_nobody() {
        for actor in [Ops, Developer, Viewer] {
            assert!(ALL.iter().all(|t| !actor.can_manage(*t)), "{actor:?}");
        }
    }

    #[test]
    fn can_grant_rules_are_unchanged() {
        assert!(Admin.can_grant(Admin) && !Admin.can_grant(Owner));
        assert!(Ops.can_grant(Viewer) && !Ops.can_grant(Developer));
    }
}

/// Ressources de la matrice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Resource {
    Projects,
    Deployments,
    Servers,
    Domains,
    Variables,
    Security,
    Logs,
    Team,
    Settings,
}

/// Niveaux d'acces, ordonnes : `None < Read < Write < Admin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    None,
    Read,
    Write,
    Admin,
}

/// Ressource portee par un type marqueur (parametre de `Require`).
pub trait ResourceMarker {
    const RESOURCE: Resource;
}

/// Niveau porte par un type marqueur (parametre de `Require`).
pub trait LevelMarker {
    const LEVEL: Level;
}

macro_rules! markers {
    ($trait:ident, $const:ident, $ty:ident: $($name:ident),+) => {
        $(
            #[derive(Debug)]
            pub enum $name {}
            impl super::$trait for $name {
                const $const: super::$ty = super::$ty::$name;
            }
        )+
    };
}

/// Ressources, en types (`Require<res::Projects, _>`).
pub mod res {
    markers!(ResourceMarker, RESOURCE, Resource: Projects, Deployments, Servers, Domains, Variables, Security, Logs, Team, Settings);
}

/// Niveaux, en types (`Require<_, lvl::Write>`).
pub mod lvl {
    markers!(LevelMarker, LEVEL, Level: Read, Write, Admin);
}

/// Compte connecte dont le role atteint `L` sur `R`.
#[derive(Debug)]
pub struct Require<R, L> {
    pub user: CurrentUser,
    pub role: Role,
    _marker: PhantomData<fn() -> (R, L)>,
}

pub fn forbidden() -> ApiError {
    ApiError::new(403, "forbidden", "Accès refusé.")
}

impl<R: ResourceMarker, L: LevelMarker> FromRequestParts<AppState> for Require<R, L> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let user = CurrentUser::from_request_parts(parts, state).await?;
        let role = Role::parse(&user.role)
            .filter(|role| allowed(*role, R::RESOURCE, L::LEVEL))
            .ok_or_else(forbidden)?;
        Ok(Require {
            user,
            role,
            _marker: PhantomData,
        })
    }
}
