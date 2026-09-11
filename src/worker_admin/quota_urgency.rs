//! Quota-window urgency weighting (issue #281 P1).
//!
//! A quota window that refills soon should be penalized less than one that
//! stays depleted for a long time. Each window knows how many seconds remain
//! until its reset; the elapsed share of the window's nominal period lifts
//! its remaining percent toward 100. Windows without reset information keep
//! their raw remaining, so a pool that carries no reset data degrades to the
//! plain min-bottleneck behavior.

use chrono::{DateTime, Utc};

/// Nominal period of a rolling/5-hour window.
pub(crate) const FIVE_HOUR_SECONDS: f64 = 5.0 * 3600.0;
/// Nominal period of a weekly window.
pub(crate) const WEEKLY_SECONDS: f64 = 7.0 * 24.0 * 3600.0;
/// Nominal period of a monthly window.
pub(crate) const MONTHLY_SECONDS: f64 = 30.0 * 24.0 * 3600.0;

/// Clamp a weighting percentage to the finite `0..=100` range; non-finite
/// input degrades to zero so a malformed snapshot can never produce an
/// unbounded weight.
pub(crate) fn clamp_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// Lift `remaining` toward 100 by the elapsed share of `period_seconds`.
///
/// An exhausted window (`remaining <= 0`) is never resurrected: eligibility
/// must not change just because a depleted window resets soon. Without reset
/// data (or with a bad period) the raw remaining is returned.
pub(crate) fn urgent_remaining(
    remaining: f64,
    resets_in_seconds: Option<f64>,
    period_seconds: f64,
) -> f64 {
    let remaining = clamp_percent(remaining);
    if remaining <= 0.0 {
        return 0.0;
    }
    let Some(resets_in) = resets_in_seconds else {
        return remaining;
    };
    if !(resets_in.is_finite() && period_seconds.is_finite() && period_seconds > 0.0) {
        return remaining;
    }
    let urgency = (1.0 - resets_in / period_seconds).clamp(0.0, 1.0);
    clamp_percent(remaining + (100.0 - remaining) * urgency)
}

/// Seconds from now until `reset_at`; `None` when no reset timestamp exists.
pub(crate) fn seconds_until(reset_at: Option<DateTime<Utc>>) -> Option<f64> {
    reset_at.map(|reset| (reset - Utc::now()).num_milliseconds() as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sooner_resets_are_penalized_less() {
        let soon = urgent_remaining(3.0, Some(30.0 * 60.0), FIVE_HOUR_SECONDS);
        let later = urgent_remaining(3.0, Some(4.5 * 3600.0), FIVE_HOUR_SECONDS);
        assert!(soon > later, "soon={soon}, later={later}");
        assert!(later > 3.0);
        assert!(soon <= 100.0);
    }

    #[test]
    fn missing_reset_data_falls_back_to_raw_remaining() {
        assert_eq!(urgent_remaining(42.0, None, FIVE_HOUR_SECONDS), 42.0);
        assert_eq!(
            urgent_remaining(42.0, Some(f64::NAN), FIVE_HOUR_SECONDS),
            42.0
        );
        assert_eq!(urgent_remaining(42.0, Some(5.0), 0.0), 42.0);
    }

    #[test]
    fn exhausted_windows_are_never_resurrected() {
        assert_eq!(urgent_remaining(0.0, Some(0.0), FIVE_HOUR_SECONDS), 0.0);
        assert_eq!(urgent_remaining(-4.0, Some(10.0), FIVE_HOUR_SECONDS), 0.0);
    }

    #[test]
    fn already_elapsed_resets_fill_the_window() {
        assert_eq!(
            urgent_remaining(20.0, Some(-60.0), FIVE_HOUR_SECONDS),
            100.0
        );
    }

    #[test]
    fn clamp_percent_is_finite_and_bounded() {
        assert_eq!(clamp_percent(f64::INFINITY), 0.0);
        assert_eq!(clamp_percent(f64::NAN), 0.0);
        assert_eq!(clamp_percent(-1.0), 0.0);
        assert_eq!(clamp_percent(180.0), 100.0);
    }
}
