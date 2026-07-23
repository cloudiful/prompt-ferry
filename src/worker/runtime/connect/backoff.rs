use rand::RngExt;
use std::time::Duration;

use super::super::RELAY_RECONNECT_DELAY_SECONDS;

const RELAY_RECONNECT_MAX_DELAY_SECONDS: u64 = 60;
const RELAY_RECONNECT_MAX_JITTER_MILLIS: u64 = 1_000;
const RELAY_RECONNECT_JITTER_DIVISOR: u128 = 4;

pub(crate) fn relay_reconnect_base_delay(consecutive_failures: u32) -> Duration {
    let exponent = consecutive_failures.saturating_sub(1).min(31);
    let multiplier = 2_u64.saturating_pow(exponent);
    let seconds = RELAY_RECONNECT_DELAY_SECONDS
        .saturating_mul(multiplier)
        .min(RELAY_RECONNECT_MAX_DELAY_SECONDS);
    Duration::from_secs(seconds)
}

pub(crate) fn relay_reconnect_delay_with_jitter(consecutive_failures: u32) -> Duration {
    let base_delay = relay_reconnect_base_delay(consecutive_failures);
    let jitter_cap_millis = relay_reconnect_jitter_cap_millis(base_delay);
    let jitter_millis = if jitter_cap_millis == 0 {
        0
    } else {
        rand::rng().random_range(0..=jitter_cap_millis)
    };
    base_delay.saturating_add(Duration::from_millis(jitter_millis))
}

fn relay_reconnect_jitter_cap_millis(base_delay: Duration) -> u64 {
    ((base_delay.as_millis() / RELAY_RECONNECT_JITTER_DIVISOR)
        .min(u128::from(RELAY_RECONNECT_MAX_JITTER_MILLIS))) as u64
}

#[cfg(test)]
mod tests {
    use super::{
        relay_reconnect_base_delay, relay_reconnect_delay_with_jitter,
        relay_reconnect_jitter_cap_millis,
    };
    use std::time::Duration;

    #[test]
    fn relay_reconnect_base_delay_grows_exponentially_and_caps() {
        assert_eq!(relay_reconnect_base_delay(1), Duration::from_secs(1));
        assert_eq!(relay_reconnect_base_delay(2), Duration::from_secs(2));
        assert_eq!(relay_reconnect_base_delay(3), Duration::from_secs(4));
        assert_eq!(relay_reconnect_base_delay(6), Duration::from_secs(32));
        assert_eq!(relay_reconnect_base_delay(7), Duration::from_secs(60));
        assert_eq!(relay_reconnect_base_delay(20), Duration::from_secs(60));
    }

    #[test]
    fn relay_reconnect_delay_caps_applied_jitter() {
        assert_eq!(
            relay_reconnect_jitter_cap_millis(Duration::from_secs(1)),
            250
        );
        assert_eq!(
            relay_reconnect_jitter_cap_millis(Duration::from_secs(4)),
            1_000
        );
        assert_eq!(
            relay_reconnect_jitter_cap_millis(Duration::from_secs(60)),
            1_000
        );
    }

    #[test]
    fn relay_reconnect_delay_with_jitter_stays_in_expected_window() {
        let delay = relay_reconnect_delay_with_jitter(4);
        assert!(delay >= Duration::from_secs(8));
        assert!(delay <= Duration::from_secs(9));
    }
}
