//! Variables d'environnement : du projet (par environnement) et globales
//! (portee environnements x projets).
//!
//! - Un secret n'est stocke que chiffre par `SecretBox` (age, identite de
//!   l'instance) et n'est jamais relu par l'API : son clair n'existe qu'en
//!   memoire (`Zeroizing`), a l'ecriture, a l'envoi du deploiement
//!   (`resolve_env`) et pour le masquage des journaux (`VariableSecrets`).
//! - Priorite au deploiement : variable du projet, puis globale propre a
//!   l'environnement (le moins d'environnements), puis globale de tous les
//!   environnements ; a egalite, une selection de projets avant « tous les
//!   projets », puis la plus recente.
//! - Une variable est « a appliquer » si elle a change apres l'envoi du
//!   dernier deploiement reussi de l'environnement (ou s'il n'y en a pas).
//! - Plafonds verifies a chaque ecriture (`check_caps`) : environnement
//!   envoye a l'agent (`MAX_ENV_BYTES`), et secrets d'un projet, globales
//!   comprises (`MAX_ENV_BYTES` octets, `redact::MAX_FRAGMENTS` fragments
//!   masques dans les journaux).

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use api_types::variables::{GlobalVariable, ProjectVariable};
use proto::{EnvValue, EnvVar};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::domain::logs::redact::{
    self, MAX_FRAGMENTS, SecretValues, SecretsResult, SecretsUnavailable,
};
use crate::secrets::{SecretBox, SecretError, labels};

/// Taille maximale d'une valeur (octets).
pub const MAX_VALUE_BYTES: usize = 32 * 1024;
/// Variables au plus par environnement d'un projet, et globales au plus.
pub const MAX_VARIABLES: usize = 500;
/// Taille totale maximale de l'environnement envoye a l'agent (octets), et
/// des valeurs secretes d'un projet (globales comprises).
pub const MAX_ENV_BYTES: usize = 1024 * 1024;
/// Environnements au plus d'une variable globale.
pub const MAX_ENVIRONMENTS: usize = 16;

const TS: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'";

/// Cle : `^[A-Z_][A-Z0-9_]{0,127}$`.
pub fn valid_key(key: &str) -> bool {
    let b = key.as_bytes();
    (1..=128).contains(&b.len())
        && (b[0].is_ascii_uppercase() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// Nom d'environnement : `^[a-z0-9][a-z0-9-]{0,62}$`.
pub fn valid_environment(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=63).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Erreur de validation ou d'ecriture (message montre a l'utilisateur, sans
/// jamais une valeur).
#[derive(Debug)]
pub enum VarError {
    /// 422.
    Invalid(String),
    /// 404 (variable renommee, ou globale, introuvable).
    NotFound,
    Seal,
    Db(sqlx::Error),
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(m) => f.write_str(m),
            Self::NotFound => f.write_str("variable introuvable"),
            Self::Seal => f.write_str("chiffrement impossible"),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

impl From<sqlx::Error> for VarError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

fn invalid(m: impl Into<String>) -> VarError {
    VarError::Invalid(m.into())
}

pub fn invalid_key(key: &str) -> VarError {
    // Cle tronquee : jamais un texte arbitraire long dans un message.
    let shown: String = key.chars().take(40).collect();
    invalid(format!(
        "Clé invalide « {shown} » : majuscules, chiffres et _ (128 au plus), sans chiffre en tête."
    ))
}

fn check_value(value: &str) -> Result<(), VarError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(invalid("Valeur trop longue (32 Kio au plus)."));
    }
    if value.contains('\0') {
        return Err(invalid("Valeur invalide (caractère nul)."));
    }
    Ok(())
}

// --- valeur stockee -------------------------------------------------------------

/// Valeur telle qu'en base : en clair (publique) ou chiffree (secret).
#[derive(Clone, PartialEq, Eq)]
pub enum Stored {
    Plain(String),
    Sealed(Vec<u8>),
}

impl fmt::Debug for Stored {
    /// Sans la valeur.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Stored::Plain(_) => "Stored::Plain(..)",
            Stored::Sealed(_) => "Stored::Sealed(..)",
        })
    }
}

impl Stored {
    fn from_row(secret: bool, value: Option<String>, sealed: Option<Vec<u8>>) -> Option<Self> {
        if secret {
            sealed.map(Stored::Sealed)
        } else {
            value.map(Stored::Plain)
        }
    }

    pub fn is_secret(&self) -> bool {
        matches!(self, Stored::Sealed(_))
    }

    fn columns(&self) -> (bool, Option<&str>, Option<&[u8]>) {
        match self {
            Stored::Plain(v) => (false, Some(v), None),
            Stored::Sealed(b) => (true, None, Some(b)),
        }
    }
}

/// Ligne qui stocke un secret : son chiffre y est lie
/// (`SecretBox::seal_bound`). Copie dans une autre ligne (autre
/// environnement, autre cle, autre globale), il ne s'ouvre plus.
#[derive(Clone, Copy, Debug)]
pub enum Owner<'a> {
    /// Variable `key` de l'environnement `environment`.
    Project { environment: Uuid, key: &'a str },
    /// Variable globale `id` (son nom peut changer sans rechiffrement).
    Global { id: Uuid },
}

impl Owner<'_> {
    fn label(&self) -> String {
        match self {
            Owner::Project { environment, key } => format!("project_variables|{environment}|{key}"),
            Owner::Global { id } => format!("global_variables|{id}"),
        }
    }

    /// Chiffre `value` pour cette ligne.
    pub fn seal(&self, secrets: &SecretBox, value: &[u8]) -> Result<Vec<u8>, SecretError> {
        secrets.seal_bound(self.label().as_bytes(), value)
    }

    /// Dechiffre un secret de cette ligne ; refuse le chiffre d'une autre.
    pub fn open(
        &self,
        secrets: &SecretBox,
        sealed: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SecretError> {
        secrets.open_bound(self.label().as_bytes(), sealed)
    }
}

/// Valeur demandee par une ecriture (vide ou absente : « conserver »).
pub struct Wanted<'a> {
    pub value: Option<&'a str>,
    pub secret: bool,
}

/// Nouvelle valeur a stocker a partir de l'actuelle (`current`) ; `None` si
/// rien ne change. Secret vide : valeur actuelle conservee (chiffree au
/// besoin) ; un secret ne redevient public qu'avec une nouvelle valeur.
pub fn next_value(
    secrets: &SecretBox,
    owner: Owner<'_>,
    current: Option<&Stored>,
    wanted: Wanted<'_>,
) -> Result<Option<Stored>, VarError> {
    let given = wanted.value.filter(|v| !v.is_empty());
    if let Some(v) = given {
        check_value(v)?;
    }
    let seal = |v: &str| {
        owner
            .seal(secrets, v.as_bytes())
            .map(Stored::Sealed)
            .map_err(|_| VarError::Seal)
    };
    let next = match (wanted.secret, given, current) {
        (true, Some(v), Some(Stored::Sealed(old))) => {
            let same = owner
                .open(secrets, old)
                .map(|clear| clear.as_slice() == v.as_bytes())
                .unwrap_or(false);
            if same {
                return Ok(None);
            }
            seal(v)?
        }
        (true, Some(v), _) => seal(v)?,
        (true, None, Some(Stored::Sealed(_))) => return Ok(None),
        (true, None, Some(Stored::Plain(old))) => seal(old)?,
        (true, None, None) => return Err(invalid("Valeur requise pour un secret.")),
        (false, _, Some(Stored::Sealed(_))) if given.is_none() => {
            return Err(invalid(
                "Saisissez une nouvelle valeur pour rendre ce secret public.",
            ));
        }
        (false, Some(v), _) => Stored::Plain(v.to_string()),
        // Valeur publique vide (explicite) ou absente.
        (false, None, current) => match (wanted.value, current) {
            (None, Some(Stored::Plain(old))) => Stored::Plain(old.clone()),
            _ => Stored::Plain(String::new()),
        },
    };
    Ok((current != Some(&next)).then_some(next))
}

/// L'ecriture stocke un secret trop court pour etre masque dans les
/// journaux (accepte, avec l'avertissement `too_short_to_redact`).
pub fn short_secret(secret: bool, value: Option<&str>, current: Option<&Stored>) -> bool {
    if !secret {
        return false;
    }
    match value.filter(|v| !v.is_empty()) {
        Some(v) => redact::too_short_to_redact(v),
        // Publique rendue secrete : sa valeur actuelle.
        None => matches!(current, Some(Stored::Plain(old)) if redact::too_short_to_redact(old)),
    }
}

// --- format .env ----------------------------------------------------------------

/// Ligne `CLE=valeur` lue d'un contenu `.env`.
pub struct DotenvLine {
    /// Numero de ligne (a partir de 1).
    pub line: usize,
    pub key: String,
    pub value: Zeroizing<String>,
    /// Suivie du commentaire `# secret`.
    pub secret: bool,
}

/// Lit un contenu `.env` : lignes vides et commentaires (`#`) ignores,
/// `export ` facultatif, valeur nue (commentaire apres ` #`), entre `"…"`
/// (`\n`, `\r`, `\"`, `\\`) ou entre `'…'` (litterale). Erreur : numero de
/// ligne et raison (jamais la valeur).
pub fn parse_dotenv(text: &str) -> Result<Vec<DotenvLine>, (usize, &'static str)> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, rest)) = line.split_once('=') else {
            return Err((n, "« CLE=valeur » attendu"));
        };
        let key = key.trim();
        if !valid_key(key) {
            return Err((n, "clé invalide"));
        }
        let rest = rest.trim();
        let (value, comment) = if let Some(body) = rest.strip_prefix('"') {
            let mut value = String::new();
            let mut chars = body.char_indices();
            let mut end = None;
            while let Some((j, c)) = chars.next() {
                match c {
                    '"' => {
                        end = Some(j + 1);
                        break;
                    }
                    '\\' => match chars.next().map(|(_, c)| c) {
                        Some('n') => value.push('\n'),
                        Some('r') => value.push('\r'),
                        Some('"') => value.push('"'),
                        Some('\\') => value.push('\\'),
                        Some(other) => {
                            value.push('\\');
                            value.push(other);
                        }
                        None => return Err((n, "guillemet fermant manquant")),
                    },
                    c => value.push(c),
                }
            }
            let Some(end) = end else {
                return Err((n, "guillemet fermant manquant"));
            };
            (Zeroizing::new(value), body[end..].trim())
        } else if let Some(body) = rest.strip_prefix('\'') {
            let Some(end) = body.find('\'') else {
                return Err((n, "apostrophe fermante manquante"));
            };
            (
                Zeroizing::new(body[..end].to_string()),
                body[end + 1..].trim(),
            )
        } else if rest.starts_with('#') {
            (Zeroizing::new(String::new()), rest)
        } else {
            let cut = rest
                .char_indices()
                .find(|&(j, c)| c == '#' && rest[..j].ends_with([' ', '\t']))
                .map(|(j, _)| j);
            match cut {
                Some(j) => (Zeroizing::new(rest[..j].trim_end().to_string()), &rest[j..]),
                None => (Zeroizing::new(rest.to_string()), ""),
            }
        };
        if !comment.is_empty() && !comment.starts_with('#') {
            return Err((n, "texte inattendu après la valeur"));
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err((n, "valeur trop longue (32 Kio au plus)"));
        }
        let secret = comment
            .strip_prefix('#')
            .is_some_and(|c| c.trim().eq_ignore_ascii_case("secret"));
        out.push(DotenvLine {
            line: n,
            key: key.to_string(),
            value,
            secret,
        });
    }
    Ok(out)
}

/// Ligne `.env` d'une valeur publique (entre guillemets si besoin).
pub fn dotenv_line(key: &str, value: &str) -> String {
    let plain = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-./:@%+,=?&~*^!$[]{}()<>;|".contains(c));
    if plain {
        return format!("{key}={value}\n");
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            c => quoted.push(c),
        }
    }
    format!("{key}=\"{quoted}\"\n")
}

// --- environnements -----------------------------------------------------------

/// Environnement `name` du projet (`None` : projet ou environnement absent).
pub async fn environment(
    conn: &mut PgConnection,
    project: Uuid,
    name: &str,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1 AND name = $2")
        .bind(project)
        .bind(name)
        .fetch_optional(conn)
        .await
}

/// Verrouille l'environnement : ecritures sur ses variables en serie.
pub async fn lock_environment(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT 1 FROM environments WHERE id = $1 FOR UPDATE")
        .bind(environment)
        .execute(conn)
        .await?;
    Ok(())
}

// --- lecture (projet + globales heritees) ---------------------------------------

#[derive(sqlx::FromRow)]
struct Row {
    /// Environnement (variable du projet) ou identifiant (globale) : ligne
    /// a laquelle le secret est lie.
    owner: Uuid,
    key: String,
    secret: bool,
    value: Option<String>,
    value_sealed: Option<Vec<u8>>,
    updated_at: String,
    updated_by: Option<String>,
    pending_apply: bool,
}

/// Variables lues par le dernier deploiement reussi de l'environnement `$1`
/// (sous-requete) : heure de lecture, pas d'envoi (une modification faite
/// entre les deux n'a pas ete envoyee).
const LAST_APPLIED: &str = "(SELECT max(COALESCE(d.variables_resolved_at, d.deploy_sent_at)) \
     FROM deployments d WHERE d.environment_id = $1 AND d.status = 'success')";

/// Variables de l'environnement modifiees (ajout, changement ou
/// suppression) : « non appliquees » jusqu'au prochain deploiement.
pub async fn touch(conn: &mut PgConnection, environment: Uuid, now: f64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE environments SET variables_changed_at = to_timestamp($2::float8) WHERE id = $1",
    )
    .bind(environment)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Verrouille les environnements `names` des projets de `scope` (ordre des
/// identifiants) avant d'horodater une globale : un deploiement qui lit ses
/// variables (`FOR SHARE`, voir `send_deploy`) attend la fin de l'ecriture,
/// ou l'ecriture attend la fin de la lecture et porte une heure posterieure.
pub async fn lock_scope(
    conn: &mut PgConnection,
    scope: &Scope,
    names: &[String],
) -> sqlx::Result<()> {
    let projects: Option<&[Uuid]> = match scope {
        Scope::All => None,
        Scope::Projects(p) => Some(p),
    };
    sqlx::query(
        "SELECT 1 FROM environments \
         WHERE name = ANY ($1) AND ($2::uuid[] IS NULL OR project_id = ANY ($2)) \
         ORDER BY id FOR UPDATE",
    )
    .bind(names)
    .bind(projects)
    .execute(conn)
    .await?;
    Ok(())
}

/// Globale creee, modifiee ou supprimee : environnements `names` des projets
/// de `scope` (union de l'ancienne et de la nouvelle portee).
pub async fn touch_scope(
    conn: &mut PgConnection,
    scope: &Scope,
    names: &[String],
    now: f64,
) -> sqlx::Result<()> {
    let projects: Option<&[Uuid]> = match scope {
        Scope::All => None,
        Scope::Projects(p) => Some(p),
    };
    sqlx::query(
        "UPDATE environments SET variables_changed_at = to_timestamp($3::float8) \
         WHERE name = ANY ($1) AND ($2::uuid[] IS NULL OR project_id = ANY ($2))",
    )
    .bind(names)
    .bind(projects)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Une modification (suppressions comprises) attend un deploiement.
pub async fn environment_pending(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COALESCE(e.variables_changed_at > {LAST_APPLIED}, \
                e.variables_changed_at IS NOT NULL) \
         FROM environments e WHERE e.id = $1"
    )))
    .bind(environment)
    .fetch_one(conn)
    .await
}

async fn own_rows(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<Vec<Row>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT v.environment_id AS owner, v.key, v.secret, v.value, v.value_sealed, \
                to_char(v.updated_at AT TIME ZONE 'UTC', {TS}) AS updated_at, \
                u.name AS updated_by, \
                COALESCE(v.updated_at > {LAST_APPLIED}, true) AS pending_apply \
         FROM project_variables v LEFT JOIN users u ON u.id = v.updated_by \
         WHERE v.environment_id = $1 ORDER BY v.key"
    )))
    .bind(environment)
    .fetch_all(conn)
    .await
}

/// Globales qui s'appliquent a (projet, environnement), une par cle, dans
/// l'ordre de priorite.
async fn global_rows(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    environment_name: &str,
) -> sqlx::Result<Vec<Row>> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT g.id AS owner, g.key, g.secret, g.value, g.value_sealed, \
                to_char(g.updated_at AT TIME ZONE 'UTC', {TS}) AS updated_at, \
                u.name AS updated_by, \
                COALESCE(g.updated_at > {LAST_APPLIED}, true) AS pending_apply \
         FROM global_variables g LEFT JOIN users u ON u.id = g.updated_by \
         WHERE $3 = ANY (g.environments) \
           AND (g.all_projects OR EXISTS (SELECT 1 FROM global_variable_projects p \
                WHERE p.variable_id = g.id AND p.project_id = $2)) \
         ORDER BY g.key, cardinality(g.environments), g.all_projects, g.updated_at DESC, g.id"
    )))
    .bind(environment)
    .bind(project)
    .bind(environment_name)
    .fetch_all(conn)
    .await?;
    let mut seen = HashSet::new();
    Ok(rows
        .into_iter()
        .filter(|r| seen.insert(r.key.clone()))
        .collect())
}

/// Variables de l'environnement (le projet l'emporte sur les globales),
/// triees par cle, valeurs des secrets jamais lues.
pub async fn list_project(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    environment_name: &str,
) -> sqlx::Result<Vec<ProjectVariable>> {
    let own = own_rows(conn, environment).await?;
    let globals = global_rows(conn, project, environment, environment_name).await?;
    let mut out = BTreeMap::new();
    for (inherited, r) in globals
        .into_iter()
        .map(|r| (true, r))
        .chain(own.into_iter().map(|r| (false, r)))
    {
        out.insert(
            r.key.clone(),
            ProjectVariable {
                key: r.key,
                value: if r.secret { None } else { r.value },
                secret: r.secret,
                inherited,
                pending_apply: r.pending_apply,
                updated_at: r.updated_at,
                updated_by: r.updated_by,
                warnings: Vec::new(),
            },
        );
    }
    Ok(out.into_values().collect())
}

// --- ecriture (projet) ----------------------------------------------------------

/// Ligne brute : cle, secret, valeur publique, valeur scellee.
type StoredRow = (String, bool, Option<String>, Option<Vec<u8>>);
/// Ligne brute d'une globale verrouillee : [`StoredRow`] + environnements, tous projets.
type LockedRow = (
    String,
    bool,
    Option<String>,
    Option<Vec<u8>>,
    Vec<String>,
    bool,
);

/// Variables actuelles de l'environnement (ligne verrouillee par l'appelant).
pub async fn current(
    conn: &mut PgConnection,
    environment: Uuid,
) -> sqlx::Result<BTreeMap<String, Stored>> {
    let rows: Vec<StoredRow> = sqlx::query_as(
        "SELECT key, secret, value, value_sealed FROM project_variables \
         WHERE environment_id = $1",
    )
    .bind(environment)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(k, s, v, b)| Stored::from_row(s, v, b).map(|v| (k, v)))
        .collect())
}

pub async fn upsert(
    conn: &mut PgConnection,
    environment: Uuid,
    key: &str,
    value: &Stored,
    actor: Uuid,
    now: f64,
) -> sqlx::Result<()> {
    let (secret, plain, sealed) = value.columns();
    sqlx::query(
        "INSERT INTO project_variables \
           (id, environment_id, key, secret, value, value_sealed, updated_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::float8)) \
         ON CONFLICT (environment_id, key) DO UPDATE SET \
           secret = EXCLUDED.secret, value = EXCLUDED.value, \
           value_sealed = EXCLUDED.value_sealed, updated_by = EXCLUDED.updated_by, \
           updated_at = EXCLUDED.updated_at",
    )
    .bind(Uuid::new_v4())
    .bind(environment)
    .bind(key)
    .bind(secret)
    .bind(plain)
    .bind(sealed)
    .bind(actor)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// `true` si la variable existait.
pub async fn delete(conn: &mut PgConnection, environment: Uuid, key: &str) -> sqlx::Result<bool> {
    let n = sqlx::query("DELETE FROM project_variables WHERE environment_id = $1 AND key = $2")
        .bind(environment)
        .bind(key)
        .execute(conn)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// Issue d'une ecriture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Written {
    /// Quelque chose a change.
    pub changed: bool,
    /// Secret trop court pour etre masque (`short_secret`).
    pub short_secret: bool,
}

/// Ecriture d'une variable (avec renommage depuis `previous_key`).
/// Environnement verrouille par l'appelant.
#[allow(clippy::too_many_arguments)]
pub async fn put(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    environment: Uuid,
    key: &str,
    previous_key: Option<&str>,
    wanted: Wanted<'_>,
    actor: Uuid,
    now: f64,
) -> Result<Written, VarError> {
    if !valid_key(key) {
        return Err(invalid_key(key));
    }
    let existing = current(conn, environment).await?;
    let renamed = previous_key.filter(|p| *p != key);
    let source = match renamed {
        Some(p) => Some(existing.get(p).ok_or(VarError::NotFound)?),
        None => existing.get(key),
    };
    if source.is_none() && !existing.contains_key(key) && existing.len() >= MAX_VARIABLES {
        return Err(invalid("500 variables au plus par environnement."));
    }
    let owner = Owner::Project { environment, key };
    // Renommage d'un secret : rechiffre pour sa nouvelle cle (illisible :
    // garde tel quel, une nouvelle valeur le remplace).
    let resealed;
    let source = match (renamed, source) {
        (Some(previous), Some(Stored::Sealed(old))) => {
            let previous = Owner::Project {
                environment,
                key: previous,
            };
            match previous.open(secrets, old) {
                Ok(clear) => {
                    resealed =
                        Stored::Sealed(owner.seal(secrets, &clear).map_err(|_| VarError::Seal)?);
                    Some(&resealed)
                }
                Err(_) => source,
            }
        }
        _ => source,
    };
    let short = short_secret(wanted.secret, wanted.value, source);
    // Renommage : la valeur source est comparee a la cible, ecrite meme si
    // identique (nouvelle cle).
    let next = match next_value(secrets, owner, source, wanted)? {
        Some(next) => Some(next),
        None if renamed.is_some() => source.cloned(),
        None => None,
    };
    let Some(next) = next else {
        return Ok(Written::default());
    };
    if let Some(p) = renamed {
        delete(conn, environment, p).await?;
    }
    upsert(conn, environment, key, &next, actor, now).await?;
    Ok(Written {
        changed: true,
        short_secret: short,
    })
}

/// Issue de `replace_all`.
#[derive(Debug, Default)]
pub struct Replaced {
    /// Cles creees ou modifiees.
    pub changed: Vec<String>,
    /// Cles supprimees.
    pub removed: Vec<String>,
    /// Cles ecrites comme secrets trop courts pour etre masques.
    pub short_secrets: Vec<String>,
}

/// Remplace les variables de l'environnement par `lines`. Une cle deja
/// secrete le reste ; `# secret` rend secrete une cle nouvelle ou publique.
/// Environnement verrouille par l'appelant.
pub async fn replace_all(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    environment: Uuid,
    lines: &[DotenvLine],
    actor: Uuid,
    now: f64,
) -> Result<Replaced, VarError> {
    let existing = current(conn, environment).await?;
    let mut wanted: BTreeMap<&str, &DotenvLine> = BTreeMap::new();
    for l in lines {
        // La derniere occurrence d'une cle l'emporte.
        wanted.insert(&l.key, l);
    }
    if wanted.len() > MAX_VARIABLES {
        return Err(invalid("500 variables au plus par environnement."));
    }
    let mut writes = Vec::new();
    let mut short_secrets = Vec::new();
    for (key, l) in &wanted {
        let current = existing.get(*key);
        let secret = current.is_some_and(Stored::is_secret) || l.secret;
        let next = next_value(
            secrets,
            Owner::Project { environment, key },
            current,
            Wanted {
                value: Some(l.value.as_str()),
                secret,
            },
        )
        .map_err(|e| match e {
            VarError::Invalid(m) => invalid(format!("Ligne {} : {m}", l.line)),
            other => other,
        })?;
        if let Some(next) = next {
            if short_secret(secret, Some(l.value.as_str()), current) {
                short_secrets.push(key.to_string());
            }
            writes.push((key.to_string(), next));
        }
    }
    let removed: Vec<String> = existing
        .keys()
        .filter(|k| !wanted.contains_key(k.as_str()))
        .cloned()
        .collect();
    for key in &removed {
        delete(conn, environment, key).await?;
    }
    let mut changed = Vec::new();
    for (key, next) in writes {
        upsert(conn, environment, &key, &next, actor, now).await?;
        changed.push(key);
    }
    Ok(Replaced {
        changed,
        removed,
        short_secrets,
    })
}

// --- globales -------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct GlobalRow {
    id: Uuid,
    key: String,
    secret: bool,
    value: Option<String>,
    environments: Vec<String>,
    all_projects: bool,
    projects: Vec<Uuid>,
    updated_at: String,
    updated_by: Option<String>,
}

impl From<GlobalRow> for GlobalVariable {
    fn from(r: GlobalRow) -> Self {
        GlobalVariable {
            id: r.id.to_string(),
            key: r.key,
            value: if r.secret { None } else { r.value },
            secret: r.secret,
            environments: r.environments,
            all_projects: r.all_projects,
            projects: r.projects.iter().map(Uuid::to_string).collect(),
            updated_at: r.updated_at,
            updated_by: r.updated_by,
            warnings: Vec::new(),
        }
    }
}

fn select_globals() -> String {
    format!(
        "SELECT g.id, g.key, g.secret, g.value, g.environments, g.all_projects, \
                COALESCE(ARRAY(SELECT p.project_id FROM global_variable_projects p \
                    WHERE p.variable_id = g.id ORDER BY p.project_id), '{{}}') AS projects, \
                to_char(g.updated_at AT TIME ZONE 'UTC', {TS}) AS updated_at, \
                u.name AS updated_by \
         FROM global_variables g LEFT JOIN users u ON u.id = g.updated_by"
    )
}

/// Variables globales, la plus recente d'abord.
pub async fn list_globals(conn: &mut PgConnection) -> sqlx::Result<Vec<GlobalVariable>> {
    let rows: Vec<GlobalRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} ORDER BY g.updated_at DESC, g.key, g.id",
        select_globals()
    )))
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(GlobalVariable::from).collect())
}

pub async fn get_global(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<GlobalVariable>> {
    let row: Option<GlobalRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE g.id = $1",
        select_globals()
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(GlobalVariable::from))
}

/// Portee d'une globale : projets concernes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    All,
    Projects(Vec<Uuid>),
}

impl Scope {
    /// Union de deux portees.
    pub fn union(self, other: Scope) -> Scope {
        match (self, other) {
            (Scope::Projects(mut a), Scope::Projects(b)) => {
                a.extend(b);
                a.sort();
                a.dedup();
                Scope::Projects(a)
            }
            _ => Scope::All,
        }
    }

    /// Projets concernes (tous les projets pour `All`).
    pub async fn projects(self, conn: &mut PgConnection) -> sqlx::Result<Vec<Uuid>> {
        match self {
            Scope::Projects(p) => Ok(p),
            Scope::All => {
                sqlx::query_scalar("SELECT id FROM projects")
                    .fetch_all(conn)
                    .await
            }
        }
    }
}

/// Globale verrouillee : valeur, portee.
pub struct LockedGlobal {
    pub key: String,
    pub value: Stored,
    pub environments: Vec<String>,
    pub scope: Scope,
}

pub async fn lock_global(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<LockedGlobal>> {
    let row: Option<LockedRow> = sqlx::query_as(
        "SELECT key, secret, value, value_sealed, environments, all_projects \
             FROM global_variables WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((key, secret, value, sealed, environments, all)) = row else {
        return Ok(None);
    };
    let scope = if all {
        Scope::All
    } else {
        Scope::Projects(
            sqlx::query_scalar(
                "SELECT project_id FROM global_variable_projects WHERE variable_id = $1 \
                 ORDER BY project_id",
            )
            .bind(id)
            .fetch_all(&mut *conn)
            .await?,
        )
    };
    let Some(value) = Stored::from_row(secret, value, sealed) else {
        return Ok(None);
    };
    Ok(Some(LockedGlobal {
        key,
        value,
        environments,
        scope,
    }))
}

/// Environnements valides et dedoublonnes (ordre garde).
pub fn clean_environments(envs: &[String]) -> Result<Vec<String>, VarError> {
    let mut out: Vec<String> = Vec::new();
    for e in envs {
        if !valid_environment(e) {
            return Err(invalid("Nom d'environnement invalide."));
        }
        if !out.contains(e) {
            out.push(e.clone());
        }
    }
    if out.is_empty() || out.len() > MAX_ENVIRONMENTS {
        return Err(invalid("Choisissez de 1 à 16 environnements."));
    }
    Ok(out)
}

/// Projets d'une selection : identifiants valides et existants.
pub async fn clean_scope(
    conn: &mut PgConnection,
    all: bool,
    projects: &[String],
) -> Result<Scope, VarError> {
    if all {
        return Ok(Scope::All);
    }
    let mut ids = Vec::new();
    for p in projects {
        let id = Uuid::parse_str(p).map_err(|_| invalid("Projet inconnu."))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err(invalid("Choisissez au moins un projet."));
    }
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE id = ANY ($1)")
        .bind(&ids)
        .fetch_one(conn)
        .await?;
    if known != ids.len() as i64 {
        return Err(invalid("Projet inconnu."));
    }
    ids.sort();
    Ok(Scope::Projects(ids))
}

/// Ecrit une globale (creation si `id` est nouveau) et sa portee.
#[allow(clippy::too_many_arguments)]
pub async fn save_global(
    conn: &mut PgConnection,
    id: Uuid,
    key: &str,
    value: &Stored,
    environments: &[String],
    scope: &Scope,
    actor: Uuid,
    now: f64,
) -> sqlx::Result<()> {
    let (secret, plain, sealed) = value.columns();
    sqlx::query(
        "INSERT INTO global_variables \
           (id, key, secret, value, value_sealed, environments, all_projects, updated_by, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, to_timestamp($9::float8)) \
         ON CONFLICT (id) DO UPDATE SET key = EXCLUDED.key, secret = EXCLUDED.secret, \
           value = EXCLUDED.value, value_sealed = EXCLUDED.value_sealed, \
           environments = EXCLUDED.environments, all_projects = EXCLUDED.all_projects, \
           updated_by = EXCLUDED.updated_by, updated_at = EXCLUDED.updated_at",
    )
    .bind(id)
    .bind(key)
    .bind(secret)
    .bind(plain)
    .bind(sealed)
    .bind(environments)
    .bind(*scope == Scope::All)
    .bind(actor)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM global_variable_projects WHERE variable_id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if let Scope::Projects(projects) = scope {
        sqlx::query(
            "INSERT INTO global_variable_projects (variable_id, project_id) \
             SELECT $1, unnest($2::uuid[])",
        )
        .bind(id)
        .bind(projects)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

pub async fn count_globals(conn: &mut PgConnection) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM global_variables")
        .fetch_one(conn)
        .await
}

/// `true` si la globale existait.
pub async fn delete_global(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<bool> {
    let n = sqlx::query("DELETE FROM global_variables WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// Lignes `.env` des globales publiques (de `environment` s'il est donne) ;
/// les secrets n'y figurent pas, cles comprises.
pub async fn export_globals(
    conn: &mut PgConnection,
    environment: Option<&str>,
) -> sqlx::Result<String> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM global_variables \
         WHERE NOT secret AND ($1::text IS NULL OR $1 = ANY (environments)) \
         ORDER BY key, updated_at DESC, id",
    )
    .bind(environment)
    .fetch_all(conn)
    .await?;
    Ok(rows.iter().map(|(k, v)| dotenv_line(k, v)).collect())
}

// --- deploiement et masquage ----------------------------------------------------

#[derive(Debug)]
pub enum ResolveError {
    Db(sqlx::Error),
    /// Secret illisible (cle de l'instance changee ?) : sa cle.
    Unreadable(String),
    TooLarge,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(e) => write!(f, "{e}"),
            Self::Unreadable(key) => {
                write!(f, "Secret {key} illisible : enregistrez-le de nouveau.")
            }
            Self::TooLarge => {
                f.write_str("Variables d'environnement trop volumineuses (1 Mio au plus).")
            }
        }
    }
}

/// Variables effectives de l'environnement (le projet l'emporte sur les
/// globales), par cle : (globale, ligne).
async fn effective_rows(
    conn: &mut PgConnection,
    project: Uuid,
    environment: Uuid,
    environment_name: &str,
) -> sqlx::Result<BTreeMap<String, (bool, Row)>> {
    let own = own_rows(conn, environment).await?;
    let globals = global_rows(conn, project, environment, environment_name).await?;
    let mut rows = BTreeMap::new();
    for (global, r) in globals
        .into_iter()
        .map(|r| (true, r))
        .chain(own.into_iter().map(|r| (false, r)))
    {
        rows.insert(r.key.clone(), (global, r));
    }
    Ok(rows)
}

/// Taille (octets) d'une variable dans l'environnement envoye a l'agent.
fn env_entry_bytes(key: &str, value: &str) -> usize {
    key.len() + value.len() + 2
}

/// Environnement d'un deploiement (projet puis globales), secrets
/// dechiffres : a appeler juste avant l'envoi a l'agent.
pub async fn resolve_env(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    project: Uuid,
    environment: Uuid,
    environment_name: &str,
) -> Result<Vec<EnvVar>, ResolveError> {
    let rows = effective_rows(conn, project, environment, environment_name)
        .await
        .map_err(ResolveError::Db)?;
    let mut env = Vec::with_capacity(rows.len());
    let mut total = 0usize;
    for (key, (global, r)) in rows {
        let owner = if global {
            Owner::Global { id: r.owner }
        } else {
            Owner::Project {
                environment: r.owner,
                key: &key,
            }
        };
        let value = match (r.secret, r.value, r.value_sealed) {
            (true, _, Some(sealed)) => {
                let clear = owner
                    .open(secrets, &sealed)
                    .map_err(|_| ResolveError::Unreadable(key.clone()))?;
                let text = std::str::from_utf8(&clear)
                    .map_err(|_| ResolveError::Unreadable(key.clone()))?;
                EnvValue::new(text.to_string())
            }
            (false, Some(v), _) => EnvValue::new(v),
            _ => continue,
        };
        total += env_entry_bytes(&key, value.expose());
        if total > MAX_ENV_BYTES {
            return Err(ResolveError::TooLarge);
        }
        env.push(EnvVar { key, value });
    }
    Ok(env)
}

/// Taille de l'environnement envoye a l'agent (comme `resolve_env`) ; un
/// secret illisible, qui ne pourra pas etre envoye, ne compte pas.
async fn env_bytes(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    project: Uuid,
    environment: Uuid,
    environment_name: &str,
) -> sqlx::Result<usize> {
    let rows = effective_rows(conn, project, environment, environment_name).await?;
    let mut total = 0usize;
    for (key, (global, r)) in rows {
        let owner = if global {
            Owner::Global { id: r.owner }
        } else {
            Owner::Project {
                environment: r.owner,
                key: &key,
            }
        };
        let len = match (r.secret, r.value, r.value_sealed) {
            (true, _, Some(sealed)) => match owner.open(secrets, &sealed) {
                Ok(clear) => clear.len(),
                Err(_) => continue,
            },
            (false, Some(v), _) => v.len(),
            _ => continue,
        };
        total += key.len() + len + 2;
    }
    Ok(total)
}

/// Plafonds a verifier apres une ecriture, dans sa transaction (422 sinon) :
/// chaque environnement de `environments` (identifiant, nom) reste envoyable
/// (`MAX_ENV_BYTES`), et les secrets du projet, globales comprises, restent
/// masquables (`MAX_ENV_BYTES` octets, `MAX_FRAGMENTS` fragments).
///
/// Verrou consultatif du projet (jusqu'a la fin de la transaction) avant
/// les lectures : deux ecritures du meme projet (environnements differents,
/// ou variable du projet et globale) se verifient l'une apres l'autre, la
/// seconde voyant la premiere validee. Plusieurs projets : dans l'ordre de
/// leurs identifiants (`check_scope_caps`).
pub async fn check_caps(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    project: Uuid,
    environments: &[(Uuid, String)],
) -> Result<(), VarError> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-var-caps:' || $1::text, 0))",
    )
    .bind(project)
    .execute(&mut *conn)
    .await?;
    for (environment, name) in environments {
        if env_bytes(conn, secrets, project, *environment, name).await? > MAX_ENV_BYTES {
            return Err(invalid(
                "Variables d'environnement trop volumineuses (1 Mio au plus, variables globales comprises).",
            ));
        }
    }
    let values = project_secret_values(conn, secrets, project).await?;
    if values.iter().map(|v| v.len()).sum::<usize>() > MAX_ENV_BYTES {
        return Err(invalid(
            "Secrets du projet trop volumineux (1 Mio au plus, tous environnements et variables globales comprises).",
        ));
    }
    if redact::fragments(&values).len() > MAX_FRAGMENTS {
        return Err(invalid(format!(
            "Trop de valeurs secrètes à masquer dans les journaux : {MAX_FRAGMENTS} au plus par projet, \
             variables globales comprises (chaque ligne d'un secret de plusieurs lignes compte)."
        )));
    }
    Ok(())
}

/// `check_caps` pour chaque projet de `scope`, sur ses environnements
/// nommes dans `environments` (ecriture d'une globale).
pub async fn check_scope_caps(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    scope: Scope,
    environments: &[String],
) -> Result<(), VarError> {
    let mut projects = scope.projects(&mut *conn).await?;
    // Verrous des projets toujours pris dans le meme ordre.
    projects.sort();
    for project in projects {
        let envs: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, name FROM environments WHERE project_id = $1 AND name = ANY ($2)",
        )
        .bind(project)
        .bind(environments)
        .fetch_all(&mut *conn)
        .await?;
        check_caps(conn, secrets, project, &envs).await?;
    }
    Ok(())
}

/// Valeurs secretes d'un projet (tous environnements, globales comprises),
/// en clair. Un secret indechiffrable (cle d'instance perdue, chiffre d'une
/// autre ligne) n'a jamais pu etre injecte : il est ignore.
async fn project_secret_values(
    conn: &mut PgConnection,
    secrets: &SecretBox,
    project: Uuid,
) -> sqlx::Result<Vec<Zeroizing<String>>> {
    let sealed: Vec<(bool, Uuid, String, Vec<u8>)> = sqlx::query_as(
        "SELECT false, v.environment_id, v.key, v.value_sealed FROM project_variables v \
         JOIN environments e ON e.id = v.environment_id \
         WHERE e.project_id = $1 AND v.secret \
         UNION ALL \
         SELECT true, g.id, g.key, g.value_sealed FROM global_variables g \
         WHERE g.secret AND (g.all_projects OR EXISTS (SELECT 1 \
             FROM global_variable_projects p WHERE p.variable_id = g.id AND p.project_id = $1))",
    )
    .bind(project)
    .fetch_all(conn)
    .await?;
    Ok(sealed
        .iter()
        .map(|(global, owner, key, b)| {
            let owner = if *global {
                Owner::Global { id: *owner }
            } else {
                Owner::Project {
                    environment: *owner,
                    key,
                }
            };
            owner.open(secrets, b)
        })
        .filter_map(|opened| match opened {
            Ok(clear) => std::str::from_utf8(&clear)
                .ok()
                .map(|s| Zeroizing::new(s.to_string())),
            Err(_) => {
                tracing::warn!(%project, "secret illisible ignore pour le masquage");
                None
            }
        })
        .collect())
}

/// Portees de `rebind_all`, une par famille de secrets (marqueurs de la table
/// `secret_rebinds`). A retirer en v0.2 avec `rebind_all`.
const REBIND_SCOPES: [&str; 4] = ["variables", "smtp", "github", "totp"];

/// Rechiffre, liees a leur ligne, les valeurs secretes chiffrees avant la
/// liaison (age seul) : variables ([`Owner`]), mot de passe SMTP, GitHub App
/// et graines TOTP. Une fois par instance et par portee : au demarrage, apres
/// les migrations et avant de servir, dans une seule transaction. Une portee
/// faite (table `secret_rebinds`) n'est plus jamais rechiffree : un chiffre
/// non lie introduit ensuite reste illisible. Une valeur indechiffrable, ou
/// dont le clair est lui-meme un chiffre lie (copie d'une autre ligne), est
/// laissee telle quelle (illisible). Renvoie le nombre de valeurs rechiffrees.
pub async fn rebind_all(db: &PgPool, secrets: &SecretBox) -> sqlx::Result<u64> {
    let mut tx = db.begin().await?;
    sqlx::query("LOCK TABLE secret_rebinds IN EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    let mut rebound = 0;
    for scope in REBIND_SCOPES {
        let done: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM secret_rebinds WHERE scope = $1)")
                .bind(scope)
                .fetch_one(&mut *tx)
                .await?;
        if done {
            continue;
        }
        rebound += match scope {
            "variables" => rebind_variables(&mut tx, secrets).await?,
            "smtp" => rebind_smtp(&mut tx, secrets).await?,
            "github" => rebind_github(&mut tx, secrets).await?,
            _ => rebind_totp(&mut tx, secrets).await?,
        };
        sqlx::query("INSERT INTO secret_rebinds (scope, done_at) VALUES ($1, now())")
            .bind(scope)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    if rebound > 0 {
        tracing::info!("{rebound} secrets rechiffres lies a leur ligne");
    }
    Ok(rebound)
}

/// Chiffre lie a `label` pour un ancien chiffre `sealed`, ou `None` s'il est
/// deja lie a `label`, indechiffrable ou lie a une autre ligne.
fn rebound_value(
    secrets: &SecretBox,
    what: &str,
    label: &[u8],
    sealed: &[u8],
) -> sqlx::Result<Option<Vec<u8>>> {
    if secrets.open_bound(label, sealed).is_ok() {
        return Ok(None);
    }
    let Ok(clear) = secrets.open_unbound_for_rebind(sealed) else {
        tracing::warn!(
            what,
            "secret illisible ou deja lie a une autre ligne : laisse tel quel"
        );
        return Ok(None);
    };
    secrets
        .seal_bound(label, &clear)
        .map(Some)
        .map_err(|e| sqlx::Error::Encode(Box::new(e)))
}

async fn rebind_variables(tx: &mut PgConnection, secrets: &SecretBox) -> sqlx::Result<u64> {
    let own: Vec<(Uuid, Uuid, String, Vec<u8>)> = sqlx::query_as(
        "SELECT id, environment_id, key, value_sealed FROM project_variables \
         WHERE secret FOR UPDATE",
    )
    .fetch_all(&mut *tx)
    .await?;
    let globals: Vec<(Uuid, Vec<u8>)> =
        sqlx::query_as("SELECT id, value_sealed FROM global_variables WHERE secret FOR UPDATE")
            .fetch_all(&mut *tx)
            .await?;
    let rows = own
        .iter()
        .map(|(id, environment, key, b)| {
            let owner = Owner::Project {
                environment: *environment,
                key,
            };
            ("project_variables", *id, owner, b)
        })
        .chain(
            globals
                .iter()
                .map(|(id, b)| ("global_variables", *id, Owner::Global { id: *id }, b)),
        );
    let mut rebound = 0;
    for (table, id, owner, sealed) in rows {
        let Some(resealed) = rebound_value(secrets, table, owner.label().as_bytes(), sealed)?
        else {
            continue;
        };
        // Nom de table fixe (deux valeurs ci-dessus), aucune donnee.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET value_sealed = $2 WHERE id = $1"
        )))
        .bind(id)
        .bind(resealed)
        .execute(&mut *tx)
        .await?;
        rebound += 1;
    }
    Ok(rebound)
}

async fn rebind_smtp(tx: &mut PgConnection, secrets: &SecretBox) -> sqlx::Result<u64> {
    let sealed: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT password_sealed FROM smtp_settings WHERE password_sealed IS NOT NULL FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(sealed) = sealed else { return Ok(0) };
    let Some(resealed) = rebound_value(secrets, "smtp_settings", labels::SMTP_PASSWORD, &sealed)?
    else {
        return Ok(0);
    };
    sqlx::query("UPDATE smtp_settings SET password_sealed = $1")
        .bind(resealed)
        .execute(&mut *tx)
        .await?;
    Ok(1)
}

async fn rebind_github(tx: &mut PgConnection, secrets: &SecretBox) -> sqlx::Result<u64> {
    let row: Option<(Vec<u8>, Vec<u8>)> = sqlx::query_as(
        "SELECT private_key_sealed, webhook_secret_sealed FROM github_app FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some((key, hook)) = row else { return Ok(0) };
    let key = rebound_value(
        secrets,
        "github_app.private_key",
        labels::GITHUB_PRIVATE_KEY,
        &key,
    )?;
    let hook = rebound_value(
        secrets,
        "github_app.webhook_secret",
        labels::GITHUB_WEBHOOK_SECRET,
        &hook,
    )?;
    let rebound = u64::from(key.is_some()) + u64::from(hook.is_some());
    if let Some(key) = key {
        sqlx::query("UPDATE github_app SET private_key_sealed = $1")
            .bind(key)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(hook) = hook {
        sqlx::query("UPDATE github_app SET webhook_secret_sealed = $1")
            .bind(hook)
            .execute(&mut *tx)
            .await?;
    }
    Ok(rebound)
}

async fn rebind_totp(tx: &mut PgConnection, secrets: &SecretBox) -> sqlx::Result<u64> {
    let rows: Vec<(Uuid, Vec<u8>)> =
        sqlx::query_as("SELECT user_id, secret_sealed FROM user_totp FOR UPDATE")
            .fetch_all(&mut *tx)
            .await?;
    let mut rebound = 0;
    for (user_id, sealed) in rows {
        let Some(resealed) =
            rebound_value(secrets, "user_totp", &labels::totp_seed(user_id), &sealed)?
        else {
            continue;
        };
        sqlx::query("UPDATE user_totp SET secret_sealed = $2 WHERE user_id = $1")
            .bind(user_id)
            .bind(resealed)
            .execute(&mut *tx)
            .await?;
        rebound += 1;
    }
    Ok(rebound)
}
/// Valeurs secretes d'un projet (tous environnements, globales comprises),
/// pour le masquage des journaux.
pub struct VariableSecrets {
    db: PgPool,
    secrets: Arc<SecretBox>,
}

impl VariableSecrets {
    pub fn new(db: PgPool, secrets: Arc<SecretBox>) -> Self {
        Self { db, secrets }
    }

    async fn load(&self, project: Uuid) -> SecretsResult {
        let values = match self.db.acquire().await {
            Ok(mut conn) => project_secret_values(&mut conn, &self.secrets, project).await,
            Err(e) => Err(e),
        };
        values.map_err(|e| {
            // Echec ferme : les lignes du projet sont retenues.
            tracing::error!(%project, error = %e, "secrets du projet illisibles : lignes retenues");
            SecretsUnavailable
        })
    }
}

impl SecretValues for VariableSecrets {
    fn values(&self, project: Uuid) -> Pin<Box<dyn Future<Output = SecretsResult> + Send + '_>> {
        Box::pin(self.load(project))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        for ok in [
            "A",
            "_",
            "DATABASE_URL",
            "A1",
            &format!("_{}", "A".repeat(127)),
        ] {
            assert!(valid_key(ok), "{ok}");
        }
        for bad in [
            "",
            "a",
            "1A",
            "A-B",
            "A B",
            "É",
            &format!("A{}", "B".repeat(128)),
        ] {
            assert!(!valid_key(bad), "{bad}");
        }
    }

    #[test]
    fn dotenv_parsing() {
        let text = "# commentaire\n\nexport A=1\nB = deux mots  # note\nC=\"x \\\"y\\\"\\nz\" # secret\nD='lit # eral'\nE=  # secret\nF=a#b\n";
        let lines = parse_dotenv(text).unwrap();
        let got: Vec<(usize, &str, &str, bool)> = lines
            .iter()
            .map(|l| (l.line, l.key.as_str(), l.value.as_str(), l.secret))
            .collect();
        assert_eq!(
            got,
            vec![
                (3, "A", "1", false),
                (4, "B", "deux mots", false),
                (5, "C", "x \"y\"\nz", true),
                (6, "D", "lit # eral", false),
                (7, "E", "", true),
                (8, "F", "a#b", false),
            ]
        );
        assert_eq!(parse_dotenv("A=1\nbad=2").err(), Some((2, "clé invalide")));
        assert_eq!(parse_dotenv("NOEQUAL").err().unwrap().0, 1);
        assert_eq!(parse_dotenv("A=\"open").err().unwrap().0, 1);
        assert_eq!(parse_dotenv("A=\"x\" y").err().unwrap().0, 1);
    }

    #[test]
    fn dotenv_round_trip() {
        for value in [
            "",
            "plain",
            "a b",
            "x\"y\\z\nw",
            "https://k@o1.io/7?a=b",
            "# not comment",
        ] {
            let line = dotenv_line("K", value);
            let parsed = parse_dotenv(&line).unwrap();
            assert_eq!(parsed[0].value.as_str(), value, "{line}");
            assert!(!parsed[0].secret);
        }
    }

    #[test]
    fn next_value_rules() {
        let sb = SecretBox::from_identity(age::x25519::Identity::generate());
        let o = Owner::Project {
            environment: Uuid::nil(),
            key: "KEY",
        };
        let sealed = Stored::Sealed(o.seal(&sb, b"old-secret").unwrap());
        let plain = Stored::Plain("old".into());
        let w = |value: Option<&'static str>, secret| Wanted { value, secret };
        // Secret vide : conserve.
        assert!(
            next_value(&sb, o, Some(&sealed), w(Some(""), true))
                .unwrap()
                .is_none()
        );
        assert!(
            next_value(&sb, o, Some(&sealed), w(None, true))
                .unwrap()
                .is_none()
        );
        // Meme valeur : rien ne change.
        assert!(
            next_value(&sb, o, Some(&sealed), w(Some("old-secret"), true))
                .unwrap()
                .is_none()
        );
        // Public -> secret sans valeur : l'ancienne valeur est chiffree.
        let Some(Stored::Sealed(b)) = next_value(&sb, o, Some(&plain), w(None, true)).unwrap()
        else {
            panic!("chiffre attendu");
        };
        assert_eq!(o.open(&sb, &b).unwrap().as_slice(), b"old");
        // Secret -> public sans valeur : refuse.
        assert!(matches!(
            next_value(&sb, o, Some(&sealed), w(Some(""), false)),
            Err(VarError::Invalid(_))
        ));
        // Nouveau secret sans valeur : refuse.
        assert!(next_value(&sb, o, None, w(None, true)).is_err());
        // Public : absente garde, vide vide.
        assert!(
            next_value(&sb, o, Some(&plain), w(None, false))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            next_value(&sb, o, Some(&plain), w(Some(""), false)).unwrap(),
            Some(Stored::Plain(String::new()))
        );
        assert!(
            next_value(
                &sb,
                o,
                None,
                w(Some(&*"x".repeat(MAX_VALUE_BYTES + 1).leak()), false)
            )
            .is_err()
        );
    }
}
