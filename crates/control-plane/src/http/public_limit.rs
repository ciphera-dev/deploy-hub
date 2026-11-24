//! Limitation de debit en memoire des routes publiques (deploy hooks,
//! webhook GitHub), appliquee AVANT toute lecture ou ecriture en base : une
//! rafale ne remplit pas `login_attempts` et ne fait pas dechiffrer le
//! secret du webhook a chaque requete.
//!
//! Fenetre fixe d'une minute par cle (`IpKey` : IPv4, ou /64 IPv6). Les
//! compteurs vivent dans le processus (remis a zero au redemarrage) ; la
//! limitation en base des echecs (`rate_limit`, 15 min) reste en place
//! derriere.
//!
//! Une table par categorie (appels de hooks par IP, declenchements par hook
//! valide, livraisons du webhook par IP), chacune bornee a `MAX_KEYS` cles.
//! Table pleine : les fenetres echues sont purgees, puis la cle la plus
//! ancienne est evincee. Jamais de compteur partage entre sources : une
//! rafale venue de nombreuses adresses (un /48 IPv6 couvre 65 536 /64) ne
//! peut ni faire refuser les livraisons signees de GitHub, ni les
//! declenchements d'un hook valide ; au pire, elle remet a zero le compteur
//! d'une autre source (echec ouvert, la verification HMAC ou du secret
//! restant derriere).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::auth::rate_limit::IpKey;

/// Fenetre de comptage.
pub const WINDOW: Duration = Duration::from_secs(60);
/// Appels de deploy hooks par IP et par fenetre (reussis ou non).
pub const HOOK_PER_IP: u32 = 30;
/// Deploiements declenches par un meme hook (secret valide) par fenetre.
pub const HOOK_PER_HOOK: u32 = 10;
/// Livraisons du webhook GitHub par IP et par fenetre.
pub const WEBHOOK_PER_IP: u32 = 120;
/// Cles suivies au plus, par categorie.
pub const MAX_KEYS: usize = 10_000;

/// Compteurs d'une categorie : cle -> (debut de la fenetre, appels).
#[derive(Default)]
struct Windows(HashMap<String, (Instant, u32)>);

impl Windows {
    /// Compte un appel de `key` ; `Err(secondes)` (valeur de `Retry-After`)
    /// au-dela de `max` dans la fenetre (l'appel refuse n'est pas compte).
    fn admit(&mut self, key: &str, max: u32, now: Instant) -> Result<(), u64> {
        let map = &mut self.0;
        // Table pleine : purge et recherche de la plus ancienne en O(n), sous
        // le verrou. Borne par `MAX_KEYS` (10 000 entrees, quelques dizaines
        // de microsecondes) et seulement pour une cle nouvelle.
        if !map.contains_key(key) && map.len() >= MAX_KEYS {
            map.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
            if map.len() >= MAX_KEYS
                && let Some(oldest) = map
                    .iter()
                    .min_by_key(|(_, (start, _))| *start)
                    .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
        let entry = map.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        if entry.1 >= max {
            let left = WINDOW.saturating_sub(now.duration_since(entry.0));
            return Err(left.as_secs().max(1));
        }
        entry.1 += 1;
        Ok(())
    }
}

#[derive(Default)]
struct Tables {
    hook_ip: Windows,
    hook: Windows,
    webhook_ip: Windows,
}

/// Compteurs par categorie, partages par l'application.
#[derive(Clone, Default)]
pub struct PublicLimits(Arc<Mutex<Tables>>);

impl std::fmt::Debug for PublicLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublicLimits").finish_non_exhaustive()
    }
}

impl PublicLimits {
    fn tables(&self) -> MutexGuard<'_, Tables> {
        // Aucune operation ne panique sous ce verrou.
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Appel d'un deploy hook depuis `ip`.
    pub fn hook_ip(&self, ip: IpAddr) -> Result<(), u64> {
        self.tables()
            .hook_ip
            .admit(IpKey::login(ip).as_str(), HOOK_PER_IP, Instant::now())
    }

    /// Deploiement demande par le hook `hook` (secret verifie).
    pub fn hook(&self, hook: uuid::Uuid) -> Result<(), u64> {
        self.tables()
            .hook
            .admit(&hook.to_string(), HOOK_PER_HOOK, Instant::now())
    }

    /// Livraison du webhook GitHub depuis `ip`.
    pub fn webhook_ip(&self, ip: IpAddr) -> Result<(), u64> {
        self.tables()
            .webhook_ip
            .admit(IpKey::login(ip).as_str(), WEBHOOK_PER_IP, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_counts_then_refuses_with_retry_after() {
        let mut w = Windows::default();
        let now = Instant::now();
        for _ in 0..3 {
            assert_eq!(w.admit("k", 3, now), Ok(()));
        }
        let retry = w.admit("k", 3, now).unwrap_err();
        assert!((1..=60).contains(&retry), "{retry}");
        assert_eq!(w.admit("autre", 3, now), Ok(()), "cles independantes");
    }

    #[test]
    fn key_table_is_bounded() {
        let mut w = Windows::default();
        let now = Instant::now();
        for i in 0..MAX_KEYS + 100 {
            let _ = w.admit(&format!("k{i}"), 1_000_000, now);
        }
        assert!(w.0.len() <= MAX_KEYS);
    }

    /// IPv6 /64 numero `i` (un /48 en contient 65 536).
    fn v6(i: usize) -> IpAddr {
        format!("2001:db8:7:{:x}::1", i).parse().unwrap()
    }

    /// Rafale de sources distinctes, table pleine, chacune au plafond : une
    /// nouvelle source, une livraison GitHub et un hook valide passent
    /// toujours (aucun compteur partage, echec ouvert a l'eviction).
    #[test]
    fn a_flood_of_sources_never_locks_out_others() {
        let limits = PublicLimits::default();
        for i in 0..MAX_KEYS + 200 {
            for _ in 0..HOOK_PER_IP {
                let _ = limits.hook_ip(v6(i));
            }
            for _ in 0..WEBHOOK_PER_IP {
                let _ = limits.webhook_ip(v6(i));
            }
            // Hooks valides (secrets fuites) declenches jusqu'au plafond.
            let hook = uuid::Uuid::from_u128(i as u128);
            for _ in 0..HOOK_PER_HOOK {
                let _ = limits.hook(hook);
            }
        }
        {
            let tables = limits.tables();
            for (name, t) in [
                ("hook_ip", &tables.hook_ip),
                ("hook", &tables.hook),
                ("webhook_ip", &tables.webhook_ip),
            ] {
                assert_eq!(t.0.len(), MAX_KEYS, "table {name} pleine");
            }
        }
        let github: IpAddr = "140.82.112.1".parse().unwrap();
        assert_eq!(limits.webhook_ip(github), Ok(()), "livraison GitHub");
        assert_eq!(limits.hook(uuid::Uuid::new_v4()), Ok(()), "hook valide");
        assert_eq!(
            limits.hook_ip(v6(MAX_KEYS + 500)),
            Ok(()),
            "nouvelle source"
        );
        // La source qui inonde reste, elle, bornee.
        let last = v6(MAX_KEYS + 199);
        assert!(limits.hook_ip(last).is_err());
        assert!(limits.tables().hook_ip.0.len() <= MAX_KEYS);
    }
}
