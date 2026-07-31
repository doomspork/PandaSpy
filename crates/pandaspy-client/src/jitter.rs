//! Reconnect jitter, behind a trait so the scheduler stays testable.
//!
//! [`Backoff`](crate::Backoff) is deliberately jitter-free: it is a pure
//! function of the attempt count, so a test can assert its exact values. But a
//! fleet of PandaSpy instances all reconnecting to the same rebooted printer in
//! lockstep is a thundering herd, so the *supervisor* spreads reconnects by
//! perturbing each delay. That perturbation needs randomness, and randomness
//! makes a scheduler non-deterministic — so it lives behind this trait:
//! production uses [`EqualJitter`], tests use [`NoJitter`] and get a scheduler
//! they can reason about exactly.

use std::time::Duration;

/// Spreads a backoff delay to avoid synchronised reconnects.
pub trait Jitter: Send + Sync + std::fmt::Debug {
    /// Perturb `base` into the actual delay to wait.
    fn jitter(&self, base: Duration) -> Duration;
}

/// No perturbation — returns the delay unchanged. For tests and anywhere a
/// deterministic schedule is wanted.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoJitter;

impl Jitter for NoJitter {
    fn jitter(&self, base: Duration) -> Duration {
        base
    }
}

/// "Equal jitter": half the delay fixed, half random, giving a value in
/// `[base/2, base]`. Keeps a floor (reconnects never bunch up at ~0) while
/// still spreading the herd — the widely-recommended middle ground between no
/// jitter and full `[0, base]` jitter.
#[derive(Debug, Clone, Copy, Default)]
pub struct EqualJitter;

impl Jitter for EqualJitter {
    fn jitter(&self, base: Duration) -> Duration {
        let half = base / 2;
        let span = half.as_nanos() as u64;
        if span == 0 {
            return base;
        }
        // Non-cryptographic use; any spread will do. getrandom never fails on
        // the platforms PandaSpy targets, but if it ever did, falling back to
        // the un-jittered delay is harmless.
        let mut bytes = [0_u8; 8];
        let extra = match getrandom::fill(&mut bytes) {
            Ok(()) => u64::from_le_bytes(bytes) % span,
            Err(_) => 0,
        };
        half + Duration::from_nanos(extra)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_jitter_is_the_identity() {
        assert_eq!(
            NoJitter.jitter(Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn equal_jitter_stays_within_the_upper_half() {
        let base = Duration::from_secs(8);
        for _ in 0..1000 {
            let delayed = EqualJitter.jitter(base);
            assert!(delayed >= base / 2, "below the floor: {delayed:?}");
            assert!(delayed <= base, "above the base: {delayed:?}");
        }
    }

    #[test]
    fn equal_jitter_actually_varies() {
        let base = Duration::from_secs(30);
        let sample: std::collections::BTreeSet<_> =
            (0..64).map(|_| EqualJitter.jitter(base)).collect();
        assert!(sample.len() > 1, "jitter produced no spread at all");
    }

    #[test]
    fn a_zero_base_does_not_divide_by_zero() {
        assert_eq!(EqualJitter.jitter(Duration::ZERO), Duration::ZERO);
    }
}
