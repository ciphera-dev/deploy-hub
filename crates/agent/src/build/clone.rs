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
use std::time::Duration;

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

/// Plafond de la taille du dossier cloné (`.git` compris).
pub const MAX_CLONE_BYTES: u64 = 2 << 30;
/// Intervalle de mesure de la taille du dossier pendant une commande `git`.
const SIZE_POLL: Duration = Duration::from_millis(250);

/// Taille cumulee des fichiers sous `dir` (liens non suivis), comptee
/// jusqu'a depasser `cap` au plus ; dossier absent ou illisible : 0.
fn dir_size(dir: &Path, cap: u64) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
                if total > cap {
                    return total;
                }
            }
        }
    }
    total
}

fn too_big(cap: u64) -> BuildError {
    let limit = if cap >= 1 << 30 {
        format!("{} Gio", cap >> 30)
    } else {
        format!("{} Mio", cap >> 20)
    };
    BuildError::Tool {
        tool: AllowedBin::Git.label(),
        message: format!("depot trop volumineux (plus de {limit}) : clone arrete"),
    }
}

/// Lance la commande `git` `cmd` ; sa progression est surveillee : si le
/// dossier `dest` depasse `cap` octets, la commande est arretee et l'etape
/// echoue. Le dossier est aussi mesure une fois la commande finie.
pub async fn exec_capped<B: BuildBackend>(
    backend: &B,
    cmd: Command,
    sink: &ProgressSink,
    dest: &Path,
    cap: u64,
) -> Result<(), BuildError> {
    let run = backend.exec(AllowedBin::Git, cmd, sink, false);
    let watch = async {
        loop {
            tokio::time::sleep(SIZE_POLL).await;
            let dir = dest.to_path_buf();
            let size = tokio::task::spawn_blocking(move || dir_size(&dir, cap))
                .await
                .unwrap_or(0);
            if size > cap {
                return;
            }
        }
    };
    tokio::select! {
        done = run => { done?; }
        () = watch => return Err(too_big(cap)),
    }
    let dir = dest.to_path_buf();
    let size = tokio::task::spawn_blocking(move || dir_size(&dir, cap))
        .await
        .unwrap_or(0);
    if size > cap {
        return Err(too_big(cap));
    }
    Ok(())
}

/// Clone `c` dans `workdir.src()` et s'assure que le commit extrait est
/// celui demande. Le dossier ne depasse jamais `MAX_CLONE_BYTES`.
pub async fn run<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    c: &Checkout,
    workdir: &Workdir,
    askpass: &Path,
    token: Option<&GitToken>,
) -> Result<(), BuildError> {
    run_capped(backend, sink, c, workdir, askpass, token, MAX_CLONE_BYTES).await
}

/// `run` avec un plafond de `cap` octets.
pub async fn run_capped<B: BuildBackend>(
    backend: &B,
    sink: &ProgressSink,
    c: &Checkout,
    workdir: &Workdir,
    askpass: &Path,
    token: Option<&GitToken>,
    cap: u64,
) -> Result<(), BuildError> {
    let src = workdir.src();
    let dest = ValidatedArg::path(&src)?;
    let home = ValidatedArg::path(&workdir.home())?;
    let dir = Path::new(&src);
    exec_capped(
        backend,
        clone_command(c, &dest, &home, askpass, token),
        sink,
        dir,
        cap,
    )
    .await?;
    let head = backend
        .exec(AllowedBin::Git, rev_parse_command(&dest, &home), sink, true)
        .await?;
    if String::from_utf8_lossy(&head).trim() == c.commit() {
        return Ok(());
    }
    // La branche a avance depuis l'ordre : le commit demande, seul.
    exec_capped(
        backend,
        fetch_command(c, &dest, &home, askpass, token),
        sink,
        dir,
        cap,
    )
    .await?;
    exec_capped(backend, checkout_command(c, &dest, &home), sink, dir, cap).await
}

/// Origine des depots clones (`ValidatedArg::git_url` : github.com seul).
const GIT_ORIGIN: &str = "https://github.com";
/// Origine de la question du mot de passe : l'utilisateur des jetons
/// d'installation sur github.com.
const TOKEN_ORIGIN: &str = "https://x-access-token@github.com";

/// URL d'une question de git de la forme `<kind> for '<url>': `, sans rien
/// d'autre ; `None` sinon.
fn prompt_url<'a>(prompt: &'a str, kind: &str) -> Option<&'a str> {
    let rest = prompt.strip_prefix(kind)?.strip_prefix(" for '")?;
    let (url, tail) = rest.split_once('\'')?;
    (tail.trim_end() == ":").then_some(url)
}

/// Reponse a une question de `git` (`GIT_ASKPASS <question>`) :
/// l'utilisateur des jetons d'installation si la question vient exactement
/// de `https://github.com`, le jeton si elle vient exactement de
/// `https://x-access-token@github.com`. Autre hote, autre utilisateur, autre
/// question, ou pas de jeton : `None` (rien n'est ecrit, git echoue sans
/// demander au terminal).
pub fn askpass_answer(prompt: &str, token: Option<&str>) -> Option<String> {
    if let Some(url) = prompt_url(prompt, "Username") {
        return (url == GIT_ORIGIN).then(|| TOKEN_USER.to_string());
    }
    if let Some(url) = prompt_url(prompt, "Password") {
        return token
            .filter(|t| url == TOKEN_ORIGIN && !t.is_empty())
            .map(str::to_string);
    }
    None
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

    /// Seules les questions de git pour `https://github.com` (et, pour le
    /// mot de passe, l'utilisateur des jetons d'installation) recoivent une
    /// reponse : un autre hote, un autre utilisateur ou un hote qui ne fait
    /// que commencer par `github.com` n'obtiennent rien.
    #[test]
    fn askpass_refuses_other_host() {
        let t = Some("ghs_x");
        for prompt in [
            "Username for 'https://example.com': ",
            "Username for 'https://github.com.example.com': ",
            "Username for 'https://github.com:8443': ",
            "Username for 'http://github.com': ",
            "Username for 'https://gist.github.com': ",
            "Username for https://github.com: ",
            "Password for 'https://x-access-token@example.com': ",
            "Password for 'https://x-access-token@github.com.example.com': ",
            "Password for 'https://other-user@github.com': ",
            "Password for 'https://github.com': ",
            "Password for 'https://x-access-token@github.com/acme': ",
            "Password for 'https://x-access-token@github.com' 'x': ",
        ] {
            assert_eq!(askpass_answer(prompt, t), None, "{prompt}");
        }
        // Les questions attendues restent servies.
        assert_eq!(
            askpass_answer("Username for 'https://github.com': ", t).as_deref(),
            Some(TOKEN_USER)
        );
        assert_eq!(
            askpass_answer("Password for 'https://x-access-token@github.com': ", t).as_deref(),
            Some("ghs_x")
        );
    }
}
