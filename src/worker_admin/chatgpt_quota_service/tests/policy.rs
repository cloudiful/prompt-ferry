use super::super::policy::{
    ACTIVE_REFRESH_AFTER, ACTIVITY_LOOKBACK, FAILURE_BACKOFF_MAX_SECONDS, IDLE_REFRESH_AFTER,
    periodic_due, retry_delay, stale,
};
use chrono::{Duration, TimeZone, Utc};

#[test]
fn active_and_idle_refresh_cadences_have_exact_boundaries() {
    let now = Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap();
    assert!(periodic_due(Some(now - ACTIVE_REFRESH_AFTER), true, now));
    assert!(!periodic_due(
        Some(now - ACTIVE_REFRESH_AFTER + Duration::seconds(1)),
        true,
        now
    ));
    assert!(periodic_due(Some(now - IDLE_REFRESH_AFTER), false, now));
    assert!(!periodic_due(
        Some(now - IDLE_REFRESH_AFTER + Duration::seconds(1)),
        false,
        now
    ));
    assert!(periodic_due(None, false, now));
    assert_eq!(ACTIVITY_LOOKBACK, Duration::minutes(30));
}

#[test]
fn request_wake_staleness_and_exponential_retry_are_bounded() {
    let now = Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap();
    assert!(!stale(now - Duration::minutes(5), now));
    assert!(stale(
        now - Duration::minutes(5) - Duration::nanoseconds(1),
        now
    ));
    assert_eq!(retry_delay(1), Duration::seconds(60));
    assert_eq!(retry_delay(2), Duration::seconds(120));
    assert_eq!(retry_delay(3), Duration::seconds(240));
    assert_eq!(
        retry_delay(20),
        Duration::seconds(FAILURE_BACKOFF_MAX_SECONDS)
    );
}
