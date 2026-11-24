//! Reglages Git d'un projet (`git_sources`) : lecture, validation et
//! ecriture, et decision de deploiement d'un push.

use api_types::project_git::{GitSettings, UpdateGitSettings};
use shared::validate::{Invalid, branch, path};
use sqlx::PgConnection;
use uuid::Uuid;

use super::watch;

/// Projet absent, ou a source image.
#[derive(Debug)]
pub enum GitError {
    NotFound,
    NotGit,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for GitError {
    fn from(e: sqlx::Error) -> Self {
        GitError::Db(e)
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    repository: Option<String>,
    branch: Option<String>,
    root_directory: Option<String>,
    watch_paths: Option<Vec<String>>,
    auto_deploy: Option<bool>,
    skip_ci: Option<bool>,
    ignore_bots: Option<bool>,
}

/// Reglages du projet ; `lock` verrouille la ligne jusqu'a la fin de la
/// transaction.
pub async fn get(
    conn: &mut PgConnection,
    project: Uuid,
    lock: bool,
) -> Result<GitSettings, GitError> {
    let sql = format!(
        "SELECT g.repository, g.branch, g.root_directory, g.watch_paths, g.auto_deploy, \
                g.skip_ci, g.ignore_bots \
         FROM projects p LEFT JOIN git_sources g ON g.project_id = p.id WHERE p.id = $1{}",
        if lock { " FOR UPDATE OF p" } else { "" }
    );
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(project)
        .fetch_optional(conn)
        .await?;
    let row = row.ok_or(GitError::NotFound)?;
    match row {
        Row {
            repository: Some(repository),
            branch: Some(branch),
            root_directory: Some(root_directory),
            watch_paths: Some(watch_paths),
            auto_deploy: Some(auto_deploy),
            skip_ci: Some(skip_ci),
            ignore_bots: Some(ignore_bots),
        } => Ok(GitSettings {
            repository,
            branch,
            root_directory,
            watch_paths,
            auto_deploy,
            skip_ci,
            ignore_bots,
        }),
        _ => Err(GitError::NotGit),
    }
}

/// Dossier racine normalise : `""`, `.`, `./` = racine ; `./a/b/` -> `a/b`.
pub fn root_directory(value: &str) -> Result<String, Invalid> {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix("./").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(String::new());
    }
    path(trimmed).map_err(|_| {
        "Dossier racine invalide : chemin relatif au dépôt, avec lettres, chiffres, « . », « _ », « - » et « / » seulement."
    })?;
    Ok(trimmed.to_string())
}

/// Motifs valides, sans doublon, dans l'ordre donne.
pub fn watch_paths(values: Vec<String>) -> Result<Vec<String>, Invalid> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        let v = v.trim().to_string();
        watch::validate(&v)?;
        if !out.contains(&v) {
            out.push(v);
        }
    }
    if out.len() > watch::MAX_PATTERNS {
        return Err("20 chemins surveillés au plus.");
    }
    Ok(out)
}

/// Applique `input` a `current` apres validation.
pub fn apply(mut current: GitSettings, input: UpdateGitSettings) -> Result<GitSettings, Invalid> {
    if let Some(b) = input.branch {
        branch(&b)?;
        current.branch = b;
    }
    if let Some(r) = input.root_directory {
        current.root_directory = root_directory(&r)?;
    }
    if let Some(w) = input.watch_paths {
        current.watch_paths = watch_paths(w)?;
    }
    if let Some(v) = input.auto_deploy {
        current.auto_deploy = v;
    }
    if let Some(v) = input.skip_ci {
        current.skip_ci = v;
    }
    if let Some(v) = input.ignore_bots {
        current.ignore_bots = v;
    }
    Ok(current)
}

pub async fn save(conn: &mut PgConnection, project: Uuid, s: &GitSettings) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE git_sources SET branch = $2, root_directory = $3, watch_paths = $4, \
         auto_deploy = $5, skip_ci = $6, ignore_bots = $7 WHERE project_id = $1",
    )
    .bind(project)
    .bind(&s.branch)
    .bind(&s.root_directory)
    .bind(&s.watch_paths)
    .bind(s.auto_deploy)
    .bind(s.skip_ci)
    .bind(s.ignore_bots)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE projects SET updated_at = now() WHERE id = $1")
        .bind(project)
        .execute(conn)
        .await?;
    Ok(())
}

/// Ce qu'un push dit de lui-meme, pour decider s'il deploie.
pub struct PushFacts<'a> {
    /// Message du commit de tete.
    pub head_message: Option<&'a str>,
    /// Compte qui a pousse (`sender.login`).
    pub sender: Option<&'a str>,
    /// Fichiers ajoutes, modifies ou supprimes par les commits du push ;
    /// `None` quand la liste n'est pas connue (aucun commit decrit, ou liste
    /// tronquee par GitHub) : le push deploie alors, faute de pouvoir dire
    /// qu'il ne touche aucun chemin surveille.
    pub files: Option<Vec<&'a str>>,
}

/// Raison pour laquelle un push ne deploie pas (`None` : il deploie).
pub fn skip_reason(settings: &PushSettings, push: &PushFacts<'_>) -> Option<&'static str> {
    if !settings.auto_deploy {
        return Some("auto_deploy");
    }
    if settings.skip_ci
        && push.head_message.is_some_and(|m| {
            let m = m.to_ascii_lowercase();
            m.contains("[skip ci]") || m.contains("[ciphera skip]")
        })
    {
        return Some("skip_ci");
    }
    if settings.ignore_bots && push.sender.is_some_and(|s| s.ends_with("[bot]")) {
        return Some("bot");
    }
    if let Some(files) = &push.files
        && !watch::any_match(&settings.watch_paths, files.iter().copied())
    {
        return Some("watch_paths");
    }
    None
}

/// Reglages lus par le webhook pour chaque projet du depot.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct PushSettings {
    pub auto_deploy: bool,
    pub skip_ci: bool,
    pub ignore_bots: bool,
    pub watch_paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> PushSettings {
        PushSettings {
            auto_deploy: true,
            skip_ci: true,
            ignore_bots: true,
            watch_paths: vec!["apps/web/**".into()],
        }
    }

    fn push<'a>(message: &'a str, sender: &'a str, files: Option<Vec<&'a str>>) -> PushFacts<'a> {
        PushFacts {
            head_message: Some(message),
            sender: Some(sender),
            files,
        }
    }

    #[test]
    fn reasons_in_order() {
        let s = settings();
        assert_eq!(
            skip_reason(&s, &push("feat", "a", Some(vec!["apps/web/x"]))),
            None
        );
        assert_eq!(
            skip_reason(&s, &push("feat", "a", Some(vec!["docs/x"]))),
            Some("watch_paths")
        );
        assert_eq!(
            skip_reason(&s, &push("x [Skip CI]", "a", Some(vec!["apps/web/x"]))),
            Some("skip_ci")
        );
        assert_eq!(
            skip_reason(&s, &push("x [ciphera skip]", "a", None)),
            Some("skip_ci")
        );
        assert_eq!(
            skip_reason(&s, &push("feat", "renovate[bot]", None)),
            Some("bot")
        );
        let off = PushSettings {
            auto_deploy: false,
            ..settings()
        };
        assert_eq!(
            skip_reason(&off, &push("feat", "a", None)),
            Some("auto_deploy")
        );
    }

    #[test]
    fn unknown_file_list_deploys() {
        assert_eq!(skip_reason(&settings(), &push("feat", "a", None)), None);
    }

    #[test]
    fn root_directory_is_normalised() {
        assert_eq!(root_directory("./").unwrap(), "");
        assert_eq!(root_directory("").unwrap(), "");
        assert_eq!(root_directory(".").unwrap(), "");
        assert_eq!(root_directory("./apps/web/").unwrap(), "apps/web");
        assert!(root_directory("../x").is_err());
        assert!(root_directory("/etc").is_err());
        assert!(root_directory("a b").is_err());
    }

    #[test]
    fn watch_paths_are_deduplicated_and_capped() {
        assert_eq!(
            watch_paths(vec!["a/**".into(), " a/** ".into(), "b".into()]).unwrap(),
            vec!["a/**".to_string(), "b".to_string()]
        );
        let many: Vec<String> = (0..21).map(|i| format!("p{i}")).collect();
        assert!(watch_paths(many).is_err());
    }
}
