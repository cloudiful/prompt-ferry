use super::{
    quota_eligibility::{Eligibility, QuotaContext, assess},
    unified_pool_fixtures::*,
};
use crate::worker_admin::{
    quota_feedback::QuotaFeedbackScope, token_plan_cache::TokenPlanQuotaCache,
};
use std::time::{Duration, Instant};
use uuid::Uuid;

mod chatgpt_subscription;
mod model_scope;
mod overrides;
mod providers;

fn text() -> QuotaContext<'static> {
    QuotaContext {
        model: Some("arbitrary-alias"),
        path: "/v1/responses",
    }
}

#[tokio::test]
async fn unknown_failure_reset_and_feedback_are_distinct() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let key = Uuid::new_v4();
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text()),
        Eligibility::Eligible(1.0)
    );
    cache
        .store_for_test(endpoint, opencode_go_usage(&[(key, "k", 0.0)]))
        .await;
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text()),
        Eligibility::Exhausted
    );
    cache.invalidate(endpoint).await;
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text()),
        Eligibility::Eligible(1.0)
    );
    let mut usage = opencode_go_usage(&[(key, "k", 0.0)]);
    usage.keys[0].opencodego_weekly.as_mut().unwrap().resets_at =
        Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    cache.store_for_test(endpoint, usage).await;
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text()),
        Eligibility::Eligible(1.0)
    );
    let scope = QuotaFeedbackScope::new(endpoint, Some(key));
    let now = Instant::now();
    cache.feedback.record_429(scope, None, now, now);
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text()),
        Eligibility::Cooling
    );
    let past = now - Duration::from_secs(31);
    let other = Uuid::new_v4();
    let probe_scope = QuotaFeedbackScope::new(endpoint, Some(other));
    cache.feedback.record_429(probe_scope, None, past, past);
    assert!(
        assess(Some(&cache), endpoint, Some(other), text())
            .weight()
            .is_some()
    );
    assert!(cache.feedback.reserve_probe(probe_scope, now));
    assert_eq!(
        assess(Some(&cache), endpoint, Some(other), text()),
        Eligibility::ProbeBusy
    );
}

#[test]
fn unavailable_is_typed_and_missing_configuration_is_not_quota() {
    let mut pool = super::quota_eligibility::QuotaPool::<()>::default();
    assert!(pool.checked().is_ok());
    pool.consider(Eligibility::Exhausted, |_| ());
    let error = anyhow::Error::new(pool.checked().unwrap_err());
    assert!(
        error
            .downcast_ref::<super::QuotaUnavailableError>()
            .is_some()
    );
    assert_eq!(super::QuotaUnavailableError::CODE, "quota_unavailable");
    pool.consider(Eligibility::Eligible(1.0), |_| ());
    assert!(pool.checked().is_ok());
}
