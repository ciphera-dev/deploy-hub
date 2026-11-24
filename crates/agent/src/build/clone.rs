//! Clone Git du commit a construire, sans shell :
//!
//! `git -c protocol.file.allow=never -c core.hooksPath=/dev/null clone
//!  --depth 1 --no-recurse-submodules --branch <b> <url> <dir>`
//!
//! puis verification du commit (`rev-parse HEAD`) ; si la branche a avance,
//! le commit demande est recupere seul (`fetch --depth 1 origin <sha>`,
//! `checkout --detach <sha>`).
//!
//! Environnement : vide (voir `argv::command`) plus `GIT_TERMINAL_PROMPT=0`,
//! `GIT_ALLOW_PROTOCOL=https`, `GIT_CONFIG_NOSYSTEM=1`,
//! `GIT_CONFIG_GLOBAL=/dev/null` et `HOME` dans le dossier du build. Le jeton
//! d'installation n'est jamais dans les arguments, l'URL ou un journal :
//! `GIT_ASKPASS` designe le binaire de l'agent (mode cache `askpass_main`),
//! qui lit le jeton dans `CIPHERA_GIT_TOKEN`, variable posee pour ce seul
//! processus `git`.

use std::path::Path;
use std::process::Command;

use proto::GitToken;

use super::argv::{self, AllowedBin, InvalidArg, ValidatedArg};
use super::{BuildBackend, BuildError, ProgressSink, Workdir};

/// Variable qui lance le binaire de l'agent en mode `GIT_ASKPASS`.
pub const ASKPASS_ENV: &str = "CIPHERA_GIT_ASKPASS";
/// Variable du jeton, posee seulement pour le processus `git`.
pub const TOKEN_ENV: &str = "CIPHERA_GIT_TOKEN";
/// Utilisateur des jetons d'installation d'une GitHub App.
pub const TOKEN_USER: &str = "x-access-token";

/// Depot, branche et commit valides.
#[derive(Clone, Debug)]
pub struct Checkout {
    url: ValidatedArg,
    branch: ValidatedArg,
    commit: ValidatedArg,
}

impl Checkout {
    pub fn new(url: &str, branch: &str, commit_sha: &str) -> Result<Self, InvalidArg> {
        Ok(Self {
            url: ValidatedArg::git_url(url)?,
            branch: ValidatedArg::branch(branch)?,
            commit: ValidatedArg::commit_sha(commit_sha)?,
        })
    }

    pub fn commit(&self) -> &str {
        self.commit.as_str()
    }
}

/// `-c` communs : aucun protocole `file`, aucun hook.
fn git_options() -> Vec<ValidatedArg> {
    vec![
        ValidatedArg::lit("-c"),
        ValidatedArg::lit("protocol.file.allow=never"),
        ValidatedArg::lit("-c"),
        ValidatedArg::lit("core.hooksPath=/dev/null"),
    ]
}

fn git(args: Vec<ValidatedArg>, home: &ValidatedArg) -> Command {
    let mut all = git_options();
    all.extend(args);
    let mut cmd = argv::command(AllowedBin::Git, &all);
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "https")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("HOME", home.as_str());
    cmd
}

/// Jeton par `GIT_ASKPASS` (jamais en argument ni dans l'URL).
fn with_token(cmd: &mut Command, askpass: &Path, token: Option<&GitToken>) {
    if let Some(token) = token {
        cmd.env("GIT_ASKPASS", askpass)
            .env(ASKPASS_ENV, "1")
            .env(TOKEN_ENV, token.expose());
    }
}

/// `git clone --depth 1 --no-recurse-submodules --branch <b> <url> <dest>`
pub fn clone_command(
    c: &Checkout,
    dest: &ValidatedArg,
    home: &ValidatedArg,
    askpass: &Path,
    token: Option<&GitToken>,
) -> Command {
    let mut cmd = git(
        vec![
            ValidatedArg::lit("clone"),
            ValidatedArg::lit("--depth"),
            ValidatedArg::lit("1"),
            ValidatedArg::lit("--no-recurse-submodules"),
            ValidatedArg::lit("--branch"),
            c.branch.clone(),
            c.url.clone(),
            dest.clone(),
        ],
        home,
    );
    with_token(&mut cmd, askpass, token);
    cmd
}

/// `git -C <dir> rev-parse HEAD`
pub fn rev_parse_command(dir: &ValidatedArg, home: &ValidatedArg) -> Command {
    git(
        vec![
            ValidatedArg::lit("-C"),
            dir.clone(),
            ValidatedArg::lit("rev-parse"),
            ValidatedArg::lit("HEAD"),
        ],
        home,
    )
}

/// `git -C <dir> fetch --depth 1 --no-recurse-submodules origin <sha>`
pub fn fetch_command(
    c: &Checkout,
    dir: &ValidatedArg,
    home: &ValidatedArg,
    askpass: &Path,
    token: Option<&GitToken>,
) -> Command {
    let mut cmd = git(
        vec![
            ValidatedArg::lit("-C"),
            dir.clone(),
            ValidatedArg::lit("fetch"),
            ValidatedArg::lit("--depth"),
            ValidatedArg::lit("1"),
            ValidatedArg::lit("--no-recurse-submodules"),
            ValidatedArg::lit("origin"),
            c.commit.clone(),
        ],
        home,
    );
    with_token(&mut cmd, askpass, token);
    cmd
}

/// `git -C <dir> -c advice.detachedHead=false checkout --detach <sha>`
pub fn checkout_command(c: &Checkout, dir: &ValidatedArg, home: &ValidatedArg) -> Command {
    git(
        vec![
            ValidatedArg::lit("-C"),
            dir.clone(),
            ValidatedArg::lit("-c"),
            ValidatedArg::lit("advice.detachedHead=false"),
            ValidatedArg::lit("checkout"),
            ValidatedArg::lit("--detach"),
            c.commit.clone(),
        ],
        home,
    )
}

/// Clone `c` dans `workdir.src()` et s'assure que le commit extrait est
/// celui demande.
pub async fn run<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    c: &Checkout,
    workdir: &Workdir,
    askpass: &Path,
    token: Option<&GitToken>,
) -> Result<(), BuildError> {
    let dest = ValidatedArg::path(&workdir.src())?;
    let home = ValidatedArg::path(&workdir.home())?;
    backend
        .exec(
            AllowedBin::Git,
            clone_command(c, &dest, &home, askpass, token),
            sink,
            false,
        )
        .await?;
    let head = backend
        .exec(AllowedBin::Git, rev_parse_command(&dest, &home), sink, true)
        .await?;
    if String::from_utf8_lossy(&head).trim() == c.commit() {
        return Ok(());
    }
    // La branche a avance depuis l'ordre : le commit demande, seul.
    backend
        .exec(
            AllowedBin::Git,
            fetch_command(c, &dest, &home, askpass, token),
            sink,
            false,
        )
        .await?;
    backend
        .exec(
            AllowedBin::Git,
            checkout_command(c, &dest, &home),
            sink,
            false,
        )
        .await?;
    Ok(())
}

/// Reponse a une question de `git` (`GIT_ASKPASS <question>`) : l'utilisateur
/// des jetons d'installation, ou le jeton. Autre question, ou pas de jeton :
/// `None` (git echoue sans demander au terminal).
pub fn askpass_answer(prompt: &str, token: Option<&str>) -> Option<String> {
    if prompt.starts_with("Username") {
        Some(TOKEN_USER.to_string())
    } else if prompt.starts_with("Password") {
        token.filter(|t| !t.is_empty()).map(str::to_string)
    } else {
        None
    }
}

/// Mode `GIT_ASKPASS` du binaire de l'agent : actif seulement si
/// `CIPHERA_GIT_ASKPASS=1`. Ecrit la reponse sur la sortie standard et
/// renvoie le code de sortie ; `None` hors de ce mode.
pub fn askpass_main() -> Option<i32> {
    if std::env::var_os(ASKPASS_ENV).is_none_or(|v| v != "1") {
        return None;
    }
    let prompt = std::env::args().nth(1).unwrap_or_default();
    let token = std::env::var(TOKEN_ENV).ok().map(GitToken::new);
    match askpass_answer(&prompt, token.as_ref().map(GitToken::expose)) {
        Some(answer) => {
            println!("{answer}");
            Some(0)
        }
        None => Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn askpass_answers_user_then_token_only() {
        let t = Some("ghs_x");
        assert_eq!(
            askpass_answer("Username for 'https://github.com': ", t).as_deref(),
            Some(TOKEN_USER)
        );
        assert_eq!(
            askpass_answer("Password for 'https://x-access-token@github.com': ", t).as_deref(),
            Some("ghs_x")
        );
        assert_eq!(askpass_answer("Password for 'x': ", None), None);
        assert_eq!(askpass_answer("Are you sure?", t), None);
    }
}
