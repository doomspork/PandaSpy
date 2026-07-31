use std::time::Duration;

/// Exponential backoff for reconnect attempts.
///
/// # Why there is no jitter in here
///
/// Jitter needs randomness, and randomness makes a scheduler untestable. The
/// caller applies jitter to the returned [`Duration`] using whatever source it
/// already has. This type stays a pure function of `(attempt, base, max)`,
/// which is why the tests below can assert exact values.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl Backoff {
    /// Doubling from `base`, never exceeding `max`.
    #[must_use]
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            attempt: 0,
        }
    }

    /// How long to wait before the next attempt, then advance.
    pub fn next_delay(&mut self) -> Duration {
        // `saturating_mul` and the clamp together mean a long-disconnected
        // printer settles at `max` instead of overflowing into a 200-year wait.
        let factor = 2_u32.saturating_pow(self.attempt.min(31));
        let delay = self.base.saturating_mul(factor).min(self.max);
        self.attempt = self.attempt.saturating_add(1);
        delay
    }

    /// Call after a successful connection, so the next outage starts cheap.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Consecutive failures so far.
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempt
    }
}

impl Default for Backoff {
    /// One second to thirty. Fast enough that a printer rebooting feels
    /// instant, slow enough that a printer that is off does not spin the radio.
    fn default() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn doubles_then_settles_at_the_ceiling() {
        let mut backoff = Backoff::new(secs(1), secs(8));

        assert_eq!(backoff.next_delay(), secs(1));
        assert_eq!(backoff.next_delay(), secs(2));
        assert_eq!(backoff.next_delay(), secs(4));
        assert_eq!(backoff.next_delay(), secs(8));
        assert_eq!(
            backoff.next_delay(),
            secs(8),
            "must clamp, not keep growing"
        );
    }

    #[test]
    fn a_long_outage_does_not_overflow() {
        let mut backoff = Backoff::new(secs(1), secs(30));
        for _ in 0..1_000 {
            assert!(backoff.next_delay() <= secs(30));
        }
        assert_eq!(backoff.attempts(), 1_000);
    }

    #[test]
    fn reconnecting_makes_the_next_outage_cheap_again() {
        let mut backoff = Backoff::new(secs(1), secs(30));
        let _ = backoff.next_delay();
        let _ = backoff.next_delay();

        backoff.reset();

        assert_eq!(backoff.attempts(), 0);
        assert_eq!(backoff.next_delay(), secs(1));
    }
}
