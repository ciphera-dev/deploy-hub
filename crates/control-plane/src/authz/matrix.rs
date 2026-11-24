//! Matrice par defaut des roles, recopiee de `prototypes/dashboard/Team.jsx`
//! (`ROLES[].perms` ; 0 = Aucun, 1 = Lecture, 2 = Ecriture, 3 = Admin).
//! Fixe en v0.1 : son edition arrive avec l'ecran Equipe (v0.2).
//!
//! Colonnes de la maquette non reprises : `db` (bases de donnees) et
//! `billing` (facturation) ne sont pas des ressources de la v0.1.
//! `settings` (Parametres) n'est pas une colonne de la maquette : Admin pour
//! le proprietaire (`full(3)`) et l'administrateur (`full(3)` hors
//! facturation), Aucun pour les autres roles.

use super::{Level, Resource, Role};

/// Niveaux dans l'ordre de `COLUMNS`.
type Row = [u8; 9];

/// Ordre des colonnes de chaque ligne.
const COLUMNS: [Resource; 9] = [
    Resource::Projects,    // projects
    Resource::Deployments, // deploy
    Resource::Servers,     // servers
    Resource::Domains,     // domains
    Resource::Variables,   // vars
    Resource::Security,    // security
    Resource::Logs,        // logs
    Resource::Team,        // team
    Resource::Settings,    // (hors maquette)
];

//                            proj dep srv dom var sec log team set
const OWNER: Row = /*      */ [3, 3, 3, 3, 3, 3, 3, 3, 3];
const ADMIN: Row = /*      */ [3, 3, 3, 3, 3, 3, 3, 3, 3];
const DEVELOPER: Row = /*  */ [2, 2, 1, 2, 2, 0, 1, 0, 0];
const OPS: Row = /*        */ [1, 2, 3, 2, 1, 3, 1, 0, 0];
const VIEWER: Row = /*     */ [1, 1, 1, 1, 0, 1, 1, 0, 0];

fn row(role: Role) -> &'static Row {
    match role {
        Role::Owner => &OWNER,
        Role::Admin => &ADMIN,
        Role::Developer => &DEVELOPER,
        Role::Ops => &OPS,
        Role::Viewer => &VIEWER,
    }
}

/// Niveau accorde a `role` sur `resource`.
pub fn level(role: Role, resource: Resource) -> Level {
    let column = COLUMNS
        .iter()
        .position(|r| *r == resource)
        .expect("chaque ressource a sa colonne");
    match row(role)[column] {
        0 => Level::None,
        1 => Level::Read,
        2 => Level::Write,
        _ => Level::Admin,
    }
}

/// `true` si `role` a au moins `level` sur `resource`.
pub fn allowed(role: Role, resource: Resource, level: Level) -> bool {
    self::level(role, resource) >= level
}
