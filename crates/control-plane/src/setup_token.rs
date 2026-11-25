//! Jeton de l'assistant d'installation (`<config_dir>/setup_token`).
//!
//! Ecrit par `ciphera init`, puis par `ciphera admin setup-token` (qui en
//! regenere un tant qu'aucun compte n'existe). Il expire 24 heures apres son
//! emission : la date est ecrite dans le fichier (`expires_at=<secondes
//! Unix>`, deuxieme ligne). Un fichier sans cette ligne (ecrit par une
//! version precedente) expire 24 heures apres sa derniere modification.
//!
//! L'adresse imprimee porte le jeton dans le fragment (`/setup#token=…`) : le
//! navigateur ne l'envoie jamais au serveur ni dans un en-tete `Referer`.

use std::fs;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::TryRng;
use rand::rngs::SysRng;
use zeroize::Zeroizing;

/// Nom du fichier, relatif au dossier de configuration.
pub const FILE: &str = "setup_token";
/// Duree de validite d'un jeton.
pub const LIFETIME: Duration = Duration::from_secs(24 * 3600);
const EXPIRES_PREFIX: &str = "expires_at=";

/// Contenu lu du fichier.
pub struct Stored {
    pub token: Zeroizing<String>,
    /// Fin de validite (secondes Unix), si le fichier la porte.
    pub expires_at: Option<u64>,
}

/// Secondes Unix courantes.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Nouveau jeton : 32 octets aleatoires, base64 URL sans remplissage.
pub fn generate() -> io::Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    SysRng
        .try_fill_bytes(bytes.as_mut())
        .map_err(|_| io::Error::other("generateur aleatoire du systeme indisponible"))?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(&bytes[..])))
}

/// Contenu du fichier pour `token`, valable jusqu'a `expires_at`.
pub fn render(token: &str, expires_at: u64) -> Zeroizing<String> {
    Zeroizing::new(format!("{token}\n{EXPIRES_PREFIX}{expires_at}\n"))
}

/// Lit le contenu du fichier. `None` : aucun jeton. Une ligne d'expiration
/// illisible vaut une expiration passee (refus).
pub fn parse(text: &str) -> Option<Stored> {
    let mut lines = text.lines().map(str::trim);
    let token = lines.next().filter(|t| !t.is_empty())?;
    let expires_at = lines
        .find_map(|l| l.strip_prefix(EXPIRES_PREFIX))
        .map(|v| v.parse::<u64>().unwrap_or(0));
    Some(Stored {
        token: Zeroizing::new(token.to_string()),
        expires_at,
    })
}

/// Fin de validite effective : celle du fichier, sinon sa derniere
/// modification plus 24 heures.
pub fn effective_expiry(stored: &Stored, modified: Option<SystemTime>) -> u64 {
    stored.expires_at.unwrap_or_else(|| {
        modified
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs().saturating_add(LIFETIME.as_secs()))
            .unwrap_or(0)
    })
}

/// Adresse de l'assistant : le jeton est dans le fragment.
pub fn url(addr: SocketAddr, token: &str) -> String {
    format!("https://{addr}/setup#token={token}")
}

/// Remplace le jeton de `config_dir` par un nouveau, valable 24 heures, et
/// le renvoie. Le fichier est cree a neuf (0600, jamais un lien suivi), donne
/// a `owner` (uid, gid) si fourni, puis mis en place par renommage : le
/// precedent jeton cesse aussitot d'etre valable.
pub fn replace(
    config_dir: &Path,
    owner: Option<(u32, u32)>,
    now: u64,
) -> io::Result<Zeroizing<String>> {
    let token = generate()?;
    let contents = render(&token, now.saturating_add(LIFETIME.as_secs()));
    let suffix = generate()?;
    let tmp = config_dir.join(format!(".{FILE}.tmp-{}", &suffix[..12]));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let written = (|| {
        let mut file = options.open(&tmp)?;
        #[cfg(unix)]
        if let Some((uid, gid)) = owner {
            std::os::unix::fs::fchown(&file, Some(uid), Some(gid))?;
        }
        #[cfg(not(unix))]
        let _ = owner;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, config_dir.join(FILE))
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_expiry() {
        let text = render("abc", 1_700_000_000);
        let stored = parse(&text).unwrap();
        assert_eq!(stored.token.as_str(), "abc");
        assert_eq!(stored.expires_at, Some(1_700_000_000));
        assert_eq!(effective_expiry(&stored, None), 1_700_000_000);
    }

    #[test]
    fn legacy_file_expires_a_day_after_its_modification() {
        let stored = parse("abc\n").unwrap();
        assert_eq!(stored.expires_at, None);
        let modified = UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(effective_expiry(&stored, Some(modified)), 1_000 + 86_400);
        assert_eq!(effective_expiry(&stored, None), 0);
    }

    #[test]
    fn unreadable_expiry_is_already_past() {
        let stored = parse("abc\nexpires_at=demain\n").unwrap();
        assert_eq!(stored.expires_at, Some(0));
        assert!(parse("\n").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn url_carries_the_token_in_the_fragment() {
        let addr: SocketAddr = "[2001:db8::1]:8443".parse().unwrap();
        assert_eq!(
            url(addr, "t0k"),
            "https://[2001:db8::1]:8443/setup#token=t0k"
        );
    }
}
