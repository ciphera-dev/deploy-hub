//! Sous-processus du build sans shell (CWE-78) : un binaire fixe de la liste
//! `AllowedBin`, appele par son chemin absolu, avec un tableau d'arguments
//! dont chaque element est un `ValidatedArg`.
//!
//! Un `ValidatedArg` ne se construit que par un constructeur qui valide en
//! liste blanche (`path`, `name`, `git_url`, `branch`, `commit_sha`) ou, dans
//! ce crate, depuis une constante du code (`lit`) ou une concatenation
//! d'arguments deja valides (`concat`). Aucun constructeur depuis `String`.
//! Le programme n'est jamais `sh`, `bash` ni un autre interpreteur.

use std::borrow::Cow;
use std::fmt;
use std::process::{Command, Stdio};

/// Dossier des outils de build installes par `install.sh` (railpack et
/// buildctl, versions et SHA-256 epingles), hors de `/usr/local/bin` pour ne
/// pas remplacer un outil de l'administrateur.
pub const TOOLS_DIR: &str = "/usr/local/lib/ciphera/bin";
/// `PATH` des sous-processus : dossiers systeme et outils de Ciphera
/// seulement. Ce n'est pas une frontiere de securite : un outil peut lancer
/// ce qu'il veut par chemin absolu.
pub const SAFE_PATH: &str = "/usr/local/lib/ciphera/bin:/usr/local/bin:/usr/bin:/bin";
/// Longueur maximale d'un argument (un chemin du dossier de build).
pub const MAX_ARG: usize = 4096;

/// Binaires que l'agent peut lancer, et rien d'autre (railpack ne tourne
/// que dans son bac a sable, voir `sandbox`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllowedBin {
    Git,
    /// `docker compose` : greffon officiel, appele par `docker`.
    DockerCompose,
    /// Client officiel de BuildKit, vers le socket de `ciphera-buildkitd`.
    Buildctl,
}

impl AllowedBin {
    pub const ALL: [Self; 3] = [Self::Git, Self::DockerCompose, Self::Buildctl];

    /// Chemin absolu du programme (jamais resolu par `PATH`).
    pub fn program(self) -> &'static str {
        match self {
            Self::Git => "/usr/bin/git",
            Self::DockerCompose => "/usr/bin/docker",
            Self::Buildctl => "/usr/local/lib/ciphera/bin/buildctl",
        }
    }

    /// Nom court, pour les messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::Git => "git",
            Self::DockerCompose => "docker compose",
            Self::Buildctl => "buildctl",
        }
    }
}

/// Argument refuse. Le message ne reprend jamais la valeur (elle peut
/// contenir des identifiants).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidArg {
    pub kind: &'static str,
    pub reason: &'static str,
}

impl fmt::Display for InvalidArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} refuse : {}", self.kind, self.reason)
    }
}

impl std::error::Error for InvalidArg {}

/// Argument d'un sous-processus, valide en liste blanche.
#[derive(Clone, PartialEq, Eq)]
pub struct ValidatedArg(Cow<'static, str>);

impl fmt::Debug for ValidatedArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

fn invalid(kind: &'static str, reason: &'static str) -> InvalidArg {
    InvalidArg { kind, reason }
}

impl ValidatedArg {
    /// Chemin absolu (dossier de build de l'agent) ou relatif (au depot) :
    /// `[A-Za-z0-9._/-]` seulement, ni `-` en tete, ni `//`, ni composant
    /// `..`. Aucun metacaractere de shell, espace ou retour a la ligne.
    pub fn path(value: &str) -> Result<Self, InvalidArg> {
        const KIND: &str = "chemin";
        if value.is_empty() || value.len() > MAX_ARG {
            return Err(invalid(KIND, "longueur hors limites"));
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
        {
            return Err(invalid(KIND, "caractere interdit"));
        }
        if value.starts_with('-') {
            return Err(invalid(KIND, "commence par un tiret"));
        }
        if value.contains("//") || value.split('/').any(|c| c == "..") {
            return Err(invalid(KIND, "remontee ou separateur double"));
        }
        Ok(Self(Cow::Owned(value.to_string())))
    }

    /// Nom de projet, d'environnement ou d'identifiant :
    /// `^[a-z0-9][a-z0-9-]{0,62}$` (regle de `shared::validate::name`).
    pub fn name(value: &str) -> Result<Self, InvalidArg> {
        shared::validate::name(value).map_err(|_| invalid("nom", "hors de [a-z0-9-]"))?;
        Ok(Self(Cow::Owned(value.to_string())))
    }

    /// URL de depot : `https://github.com/<proprietaire>/<nom>[.git]` en
    /// v0.1 (ni `ext::`, ni `file://`, ni `ssh://`, ni `-` en tete).
    pub fn git_url(value: &str) -> Result<Self, InvalidArg> {
        shared::validate::github_url(value)
            .map_err(|_| invalid("URL de depot", "https://github.com/<proprietaire>/<nom>"))?;
        Ok(Self(Cow::Owned(value.to_string())))
    }

    /// Branche Git (regle de `shared::validate::branch`).
    pub fn branch(value: &str) -> Result<Self, InvalidArg> {
        shared::validate::branch(value).map_err(|_| invalid("branche", "nom de branche"))?;
        Ok(Self(Cow::Owned(value.to_string())))
    }

    /// Empreinte de commit : 40 ou 64 caracteres hexadecimaux minuscules.
    pub fn commit_sha(value: &str) -> Result<Self, InvalidArg> {
        shared::validate::commit_sha(value).map_err(|_| invalid("commit", "empreinte"))?;
        Ok(Self(Cow::Owned(value.to_string())))
    }

    /// Nom de service Compose (regle de `shared::validate::compose_service`).
    pub fn compose_service(value: &str) -> Result<Self, InvalidArg> {
        shared::validate::compose_service(value)
            .map_err(|_| invalid("service Compose", "nom de service"))?;
        Ok(Self(Cow::Owned(value.to_string())))
    }

    /// Constante du code (option, sous-commande, reference epinglee).
    pub(crate) const fn lit(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    /// Concatenation d'arguments valides (`context=` + chemin, par exemple).
    pub(crate) fn concat(parts: &[&ValidatedArg]) -> Self {
        Self(Cow::Owned(parts.iter().map(|p| p.as_str()).collect()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Commande prete a lancer : programme fixe de `bin`, arguments `args`,
/// environnement vide sauf `PATH` (`SAFE_PATH`) et `LC_ALL=C`, entree
/// standard fermee. L'appelant ajoute les variables propres a l'etape.
pub fn command(bin: AllowedBin, args: &[ValidatedArg]) -> Command {
    let mut cmd = Command::new(bin.program());
    if bin == AllowedBin::DockerCompose {
        cmd.arg("compose");
    }
    cmd.args(args.iter().map(ValidatedArg::as_str));
    cmd.env_clear();
    cmd.env("PATH", SAFE_PATH);
    cmd.env("LC_ALL", "C");
    cmd.stdin(Stdio::null());
    cmd
}

/// Description d'une commande pour un journal : programme et arguments,
/// jamais l'environnement (le jeton Git y passe). `Debug` de `Command`
/// affiche l'environnement : ne jamais le journaliser.
pub fn describe(cmd: &Command) -> String {
    let mut out = cmd.get_program().to_string_lossy().into_owned();
    for a in cmd.get_args() {
        out.push(' ');
        out.push_str(&a.to_string_lossy());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_joins_validated_parts() {
        let p = ValidatedArg::path("/var/lib/x").unwrap();
        let arg = ValidatedArg::concat(&[&ValidatedArg::lit("context="), &p]);
        assert_eq!(arg.as_str(), "context=/var/lib/x");
    }

    #[test]
    fn command_sets_a_minimal_environment() {
        let cmd = command(AllowedBin::DockerCompose, &[ValidatedArg::lit("version")]);
        assert_eq!(cmd.get_program(), "/usr/bin/docker");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, ["compose", "version"]);
        let envs: Vec<_> = cmd.get_envs().map(|(k, _)| k.to_owned()).collect();
        assert_eq!(envs.len(), 2, "{envs:?}");
        assert_eq!(describe(&cmd), "/usr/bin/docker compose version");
    }
}
