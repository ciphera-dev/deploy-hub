//! Debit des lignes de journal d'un agent : seau a jetons applique a la
//! reception (avant toute diffusion), lignes en trop ignorees et comptees.

use std::time::Instant;

use proto::LogTarget;

/// Lignes acceptees par seconde et par agent (rafale d'une seconde).
pub const AGENT_LINES_PER_SEC: u32 = 2000;

/// Seau a jetons des lignes d'un agent.
pub struct LineBucket {
    tokens: f64,
    at: Instant,
    /// Lignes ignorees depuis le dernier releve, et leur derniere cible.
    ignored: u64,
    ignored_target: Option<LogTarget>,
}

impl LineBucket {
    pub fn new(now: Instant) -> Self {
        Self {
            tokens: f64::from(AGENT_LINES_PER_SEC),
            at: now,
            ignored: 0,
            ignored_target: None,
        }
    }

    /// `false` : la ligne est ignoree (et comptee).
    pub fn admit(&mut self, target: LogTarget, now: Instant) -> bool {
        let rate = f64::from(AGENT_LINES_PER_SEC);
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(rate);
        self.at = now;
        if self.tokens < 1.0 {
            self.ignored += 1;
            self.ignored_target = Some(target);
            return false;
        }
        self.tokens -= 1.0;
        true
    }

    /// Lignes ignorees depuis le dernier releve (remises a zero) et la
    /// derniere cible touchee.
    pub fn take_ignored(&mut self) -> Option<(u64, LogTarget)> {
        let target = self.ignored_target.take()?;
        Some((std::mem::take(&mut self.ignored), target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_limits_counts_and_refills() {
        let target = LogTarget::Deployment(shared::ids::DeploymentId::new());
        let t0 = Instant::now();
        let mut bucket = LineBucket::new(t0);
        for _ in 0..AGENT_LINES_PER_SEC {
            assert!(bucket.admit(target, t0));
        }
        assert!(bucket.take_ignored().is_none());
        for _ in 0..7 {
            assert!(!bucket.admit(target, t0));
        }
        assert_eq!(bucket.take_ignored(), Some((7, target)));
        assert!(bucket.take_ignored().is_none());
        assert!(bucket.admit(target, t0 + std::time::Duration::from_secs(1)));
    }
}
