//! Issue #528 Task 2: sampled `debug!` timing for the upstream redaction
//! critical path (`redact`/`persist`/`restore`). Pure observation: sampled
//! logging only, no behavior change.

use std::sync::atomic::{AtomicU64, Ordering};

/// Log when a segment exceeds this many microseconds, or on every Nth call.
const SLOW_US: u64 = 10_000;
const SAMPLE_EVERY: u64 = 100;

pub(crate) const PATH_REDACT: &str = "redact";
pub(crate) const PATH_PERSIST: &str = "persist";
pub(crate) const PATH_RESTORE: &str = "restore";

/// Return the debug event fields shared by all path-timing logs, or `None`
/// when this call is neither slow enough nor the sampled Nth call.
pub(crate) fn timing_sample(elapsed_us: u64, counter: &AtomicU64) -> Option<(u64, bool)> {
    let call = counter.fetch_add(1, Ordering::Relaxed);
    let is_slow = elapsed_us > SLOW_US;
    let sampled = call % SAMPLE_EVERY == 0;
    (is_slow || sampled).then_some((elapsed_us, is_slow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_first_call_and_slow_calls_but_not_fast_middle_calls() {
        let counter = AtomicU64::new(0);
        assert!(timing_sample(50, &counter).is_some(), "call 0 sampled");
        assert!(timing_sample(50, &counter).is_none());
        assert!(
            timing_sample(10_001, &counter).is_some(),
            "slow always logs"
        );
        assert_eq!(
            timing_sample(50, &counter).map(|(us, _)| us),
            None,
            "call 3 unsampled"
        );
    }

    #[test]
    fn logs_every_hundredth_call() {
        let counter = AtomicU64::new(99);
        assert!(timing_sample(1, &counter).is_none(), "index 99 unsampled");
        let (elapsed_us, is_slow) = timing_sample(1, &counter).expect("index 100 sampled");
        assert_eq!(elapsed_us, 1);
        assert!(!is_slow);
    }
}
