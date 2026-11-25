//! Hachage des mots de passe : Argon2id, parametres par defaut de la crate
//! (19 Mio, 2 passes, 1 voie : minimum recommande par l'OWASP).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use shared::error::ApiError;
use tokio::sync::Semaphore;

/// Longueur minimale (en caracteres).
pub const MIN_LEN: usize = 12;
/// Longueur maximale (en caracteres) : borne le cout du hachage.
pub const MAX_LEN: usize = 1024;

/// Calculs Argon2 simultanes, toutes routes confondues. Chacun reserve
/// 19 Mio : sans plafond, des connexions sans compte depuis quelques IP (ou un
/// /56 IPv6) occuperaient les 512 fils bloquants de tokio, soit pres de
/// 10 Gio, et le plan de controle serait tue faute de memoire.
pub const MAX_CONCURRENT: usize = 4;
/// Attente maximale d'une place ; au-dela, 503 (service occupe).
pub const MAX_WAIT: Duration = Duration::from_secs(10);

static SLOTS: Semaphore = Semaphore::const_new(MAX_CONCURRENT);

/// Calculs Argon2 delegues depuis le demarrage (tests : verifier qu'un refus
/// n'en declenche aucun).
static OFFLOADED: AtomicU64 = AtomicU64::new(0);

/// Nombre de calculs Argon2 delegues par [`offload`] depuis le demarrage.
pub fn offloaded() -> u64 {
    OFFLOADED.load(Ordering::Relaxed)
}

/// Echec d'un calcul Argon2 delegue.
#[derive(Debug)]
pub enum OffloadError {
    /// Aucune place libre avant `MAX_WAIT`.
    Busy,
    /// Tache interrompue (panique).
    Failed(String),
}

impl fmt::Display for OffloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("calculs Argon2 en attente trop longue"),
            Self::Failed(e) => write!(f, "calcul Argon2 interrompu : {e}"),
        }
    }
}

impl OffloadError {
    /// Reponse de l'API : 503 si le serveur est occupe, 500 sinon.
    pub fn into_api(self, context: &str) -> ApiError {
        match self {
            Self::Busy => {
                tracing::warn!("{context} : calculs Argon2 satures");
                ApiError::new(
                    503,
                    "busy",
                    "Serveur occupé : réessayez dans quelques instants.",
                )
            }
            Self::Failed(e) => super::internal(context, e),
        }
    }
}

/// Verifie la politique de longueur ; renvoie le message FR en cas d'echec.
pub fn check_policy(password: &str) -> Result<(), &'static str> {
    let n = password.chars().count();
    if n < MIN_LEN {
        Err("Le mot de passe doit contenir au moins 12 caractères.")
    } else if n > MAX_LEN {
        Err("Le mot de passe ne doit pas dépasser 1024 caractères.")
    } else {
        Ok(())
    }
}

/// Hache un mot de passe (sel aleatoire) en chaine PHC `$argon2id$...`.
pub fn hash(password: &str) -> Result<String, argon2::password_hash::Error> {
    Ok(Argon2::default()
        .hash_password(password.as_bytes())?
        .to_string())
}

/// `true` si le mot de passe correspond au hachage PHC. Un hachage illisible vaut `false`.
pub fn verify(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Calcul Argon2 hors du thread asynchrone (connexion, re-authentification,
/// codes de secours, creation ou changement de mot de passe).
pub async fn offload<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, OffloadError> {
    OFFLOADED.fetch_add(1, Ordering::Relaxed);
    offload_with(&SLOTS, MAX_WAIT, work).await
}

async fn offload_with<T: Send + 'static>(
    slots: &'static Semaphore,
    max_wait: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, OffloadError> {
    let slot = tokio::time::timeout(max_wait, slots.acquire())
        .await
        .map_err(|_| OffloadError::Busy)?
        .map_err(|_| OffloadError::Busy)?;
    // La place part avec le calcul : elle reste prise jusqu'a sa fin, meme si
    // la requete est abandonnee entre-temps (le fil bloquant va au bout).
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        work()
    })
    .await
    .map_err(|e| OffloadError::Failed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jamais plus de `MAX_CONCURRENT` calculs a la fois, quel que soit le
    /// nombre de demandes simultanees.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn offload_bounds_concurrent_work() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let slots: &'static Semaphore = Box::leak(Box::new(Semaphore::new(2)));
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..12 {
            let (running, peak) = (running.clone(), peak.clone());
            tasks.push(tokio::spawn(async move {
                offload_with(slots, Duration::from_secs(30), move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(30));
                    running.fetch_sub(1, Ordering::SeqCst);
                })
                .await
            }));
        }
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    /// Aucune place libre a temps : `Busy` (503), sans lancer le calcul.
    #[tokio::test]
    async fn offload_gives_up_when_saturated() {
        let slots: &'static Semaphore = Box::leak(Box::new(Semaphore::new(1)));
        let _held = slots.acquire().await.unwrap();
        let r = offload_with(slots, Duration::from_millis(50), || {
            panic!("ne doit pas tourner")
        })
        .await;
        assert!(matches!(r, Err(OffloadError::Busy)), "{r:?}");
        assert_eq!(OffloadError::Busy.into_api("test").status, 503);
    }

    #[test]
    fn hash_is_argon2id_and_verifies() {
        let h = hash("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correct horse battery", &h));
        assert!(!verify("correct horse batterz", &h));
        assert_ne!(h, hash("correct horse battery").unwrap(), "sel aleatoire");
    }

    #[test]
    fn garbage_hash_never_verifies() {
        assert!(!verify("x", "pas un hachage"));
        assert!(!verify("x", ""));
    }

    #[test]
    fn policy_counts_characters() {
        assert!(check_policy("onzecaracte").is_err());
        assert!(check_policy(&"é".repeat(12)).is_ok());
        assert!(check_policy(&"x".repeat(1024)).is_ok());
        assert!(check_policy(&"x".repeat(1025)).is_err());
    }
}
