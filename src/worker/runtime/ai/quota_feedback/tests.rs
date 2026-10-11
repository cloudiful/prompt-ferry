use super::retry_after::parse;
use crate::worker_admin::quota_feedback::{
    QuotaFeedbackEligibility, QuotaFeedbackScope, QuotaFeedbackStore,
};
use http::{HeaderMap, HeaderValue, header::RETRY_AFTER};
use std::time::{Duration, Instant};
use uuid::Uuid;

fn headers(value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        RETRY_AFTER,
        HeaderValue::from_str(value).expect("header value"),
    );
    headers
}

#[test]
fn retry_after_accepts_delta_and_safely_caps_large_values() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(parse(&headers("17"), now), Some(Duration::from_secs(17)));
    assert_eq!(parse(&headers("0"), now), Some(Duration::ZERO));
    assert_eq!(
        parse(&headers("999999999999999999999999999999999999999999"), now),
        Some(Duration::from_secs(3600))
    );
}

#[test]
fn retry_after_accepts_http_gmt_dates_and_caps_or_expires_them() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-11T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(
        parse(&headers("Sun, 11 Oct 2026 12:00:30 GMT"), now),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        parse(&headers("Sun, 11 Oct 2026 11:59:59 GMT"), now),
        Some(Duration::ZERO)
    );
    assert_eq!(
        parse(&headers("Sun, 11 Oct 2026 14:00:00 GMT"), now),
        Some(Duration::from_secs(3600))
    );
}

#[test]
fn malformed_negative_and_multiple_retry_after_values_use_default_policy() {
    let now = chrono::Utc::now();
    for value in ["", "-1", "later", "1.5", "Sun, 11 Oct 2026 12:00:00 PST"] {
        assert_eq!(parse(&headers(value), now), None, "value={value:?}");
    }
    let mut multiple = HeaderMap::new();
    multiple.append(RETRY_AFTER, HeaderValue::from_static("3"));
    multiple.append(RETRY_AFTER, HeaderValue::from_static("4"));
    assert_eq!(parse(&multiple, now), None);
}

#[test]
fn late_success_cannot_clear_a_newer_failure_but_new_success_can() {
    let store = QuotaFeedbackStore::default();
    let scope = QuotaFeedbackScope::new(Uuid::new_v4(), Some(Uuid::new_v4()));
    let first_attempt = Instant::now();
    store.record_429(scope, None, first_attempt, first_attempt);
    let newer_failure = first_attempt + Duration::from_millis(2);
    store.record_429(scope, None, newer_failure, newer_failure);
    store.record_success(
        scope,
        first_attempt,
        newer_failure + Duration::from_millis(1),
    );
    assert_eq!(
        store.eligibility(scope, newer_failure + Duration::from_millis(1)),
        QuotaFeedbackEligibility::Cooling
    );
    let later_success = newer_failure + Duration::from_millis(2);
    store.record_success(scope, later_success, later_success);
    assert_eq!(
        store.eligibility(scope, later_success),
        QuotaFeedbackEligibility::Eligible
    );
}
