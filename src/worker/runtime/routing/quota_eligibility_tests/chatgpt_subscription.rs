use super::*;
use crate::{
    worker_admin::chatgpt_quota_service::ChatGptQuotaRoutingSnapshot,
    worker_admin_types::{SubscriptionWindowAvailability, SubscriptionWindowUsage},
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};

fn snapshot(
    age: ChronoDuration,
    limit_reached: Option<bool>,
    windows: Vec<SubscriptionWindowUsage>,
    stale: bool,
) -> ChatGptQuotaRoutingSnapshot {
    ChatGptQuotaRoutingSnapshot {
        observed_at: Utc::now() - age,
        limit_reached,
        windows,
        stale,
    }
}

fn window(
    remaining: Option<f64>,
    availability: SubscriptionWindowAvailability,
    reset_at: Option<DateTime<Utc>>,
) -> SubscriptionWindowUsage {
    SubscriptionWindowUsage {
        source_window: "primary".to_string(),
        window_seconds: Some(604_800),
        used_percent: remaining.map(|remaining| 100.0 - remaining),
        remaining_percent: remaining,
        reset_at,
        reset_after_seconds: None,
        availability,
    }
}

fn text_context() -> QuotaContext<'static> {
    QuotaContext {
        model: Some("gpt-arbitrary-alias"),
        path: "/v1/responses",
    }
}

#[test]
fn fresh_subscription_zero_is_endpoint_scoped_across_synthetic_and_configured_keys() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let configured_key = Uuid::new_v4();
    cache.store_chatgpt_subscription_snapshot(
        endpoint,
        Some(snapshot(
            ChronoDuration::seconds(5),
            Some(false),
            vec![window(
                Some(0.0),
                SubscriptionWindowAvailability::Known,
                Some(Utc::now() + ChronoDuration::minutes(5)),
            )],
            false,
        )),
    );

    assert_eq!(
        assess(Some(&cache), endpoint, Some(configured_key), text_context()),
        Eligibility::Exhausted
    );
    assert_eq!(
        assess(Some(&cache), endpoint, None, text_context()),
        Eligibility::Exhausted
    );
}

fn assert_not_exhausted(
    cache: &TokenPlanQuotaCache,
    endpoint: Uuid,
    key: Uuid,
    observation: ChatGptQuotaRoutingSnapshot,
    reason: &str,
) {
    cache.store_chatgpt_subscription_snapshot(endpoint, Some(observation));
    assert_ne!(
        assess(Some(cache), endpoint, Some(key), text_context()),
        Eligibility::Exhausted,
        "{reason}"
    );
}

#[test]
fn stale_failed_unknown_and_reset_passed_observations_do_not_prove_exhaustion() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let key = Uuid::new_v4();
    let context = text_context();

    for observation in [
        snapshot(ChronoDuration::seconds(1), Some(true), Vec::new(), true),
        snapshot(ChronoDuration::minutes(6), Some(true), Vec::new(), false),
        snapshot(
            ChronoDuration::seconds(1),
            Some(false),
            vec![window(
                Some(0.0),
                SubscriptionWindowAvailability::Known,
                Some(Utc::now() - ChronoDuration::seconds(1)),
            )],
            false,
        ),
        snapshot(
            ChronoDuration::seconds(1),
            Some(false),
            vec![window(
                None,
                SubscriptionWindowAvailability::Unknown,
                Some(Utc::now() + ChronoDuration::minutes(5)),
            )],
            false,
        ),
    ] {
        cache.store_chatgpt_subscription_snapshot(endpoint, Some(observation));
        assert_ne!(
            assess(Some(&cache), endpoint, Some(key), context),
            Eligibility::Exhausted
        );
    }

    assert_not_exhausted(
        &cache,
        endpoint,
        key,
        snapshot(
            ChronoDuration::seconds(1),
            Some(true),
            vec![window(
                Some(0.0),
                SubscriptionWindowAvailability::Known,
                Some(Utc::now() - ChronoDuration::seconds(1)),
            )],
            false,
        ),
        "an expired binding reset invalidates a fresh limit flag",
    );

    let mut relative = window(Some(0.0), SubscriptionWindowAvailability::Known, None);
    relative.reset_after_seconds = Some(1);
    assert_not_exhausted(
        &cache,
        endpoint,
        key,
        snapshot(
            ChronoDuration::seconds(5),
            Some(true),
            vec![relative],
            false,
        ),
        "an aged relative reset invalidates a fresh limit flag",
    );

    let mut conflicting = window(
        Some(0.0),
        SubscriptionWindowAvailability::Known,
        Some(Utc::now() + ChronoDuration::hours(1)),
    );
    conflicting.reset_after_seconds = Some(1);
    assert_not_exhausted(
        &cache,
        endpoint,
        key,
        snapshot(
            ChronoDuration::seconds(5),
            Some(true),
            vec![conflicting],
            false,
        ),
        "a passed relative deadline overrides a future absolute deadline",
    );

    let mut absolute_passed = window(
        Some(0.0),
        SubscriptionWindowAvailability::Known,
        Some(Utc::now() - ChronoDuration::seconds(1)),
    );
    absolute_passed.reset_after_seconds = Some(60);
    assert_not_exhausted(
        &cache,
        endpoint,
        key,
        snapshot(
            ChronoDuration::seconds(1),
            Some(true),
            vec![absolute_passed],
            false,
        ),
        "a passed absolute deadline overrides a future relative deadline",
    );

    cache.store_chatgpt_subscription_snapshot(
        endpoint,
        Some(snapshot(
            ChronoDuration::seconds(1),
            Some(true),
            vec![
                window(
                    Some(0.0),
                    SubscriptionWindowAvailability::Known,
                    Some(Utc::now() - ChronoDuration::seconds(1)),
                ),
                window(
                    Some(0.0),
                    SubscriptionWindowAvailability::Known,
                    Some(Utc::now() + ChronoDuration::hours(1)),
                ),
            ],
            false,
        )),
    );
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), context),
        Eligibility::Exhausted,
        "a still-active binding reset continues to block"
    );
}

#[test]
fn fresh_limit_reached_flag_blocks_and_clearing_the_bridge_restores_unknown_routing() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let key = Uuid::new_v4();
    cache.store_chatgpt_subscription_snapshot(
        endpoint,
        Some(snapshot(
            ChronoDuration::seconds(1),
            Some(true),
            Vec::new(),
            false,
        )),
    );
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text_context()),
        Eligibility::Exhausted
    );

    cache.store_chatgpt_subscription_snapshot(
        endpoint,
        Some(snapshot(
            ChronoDuration::seconds(1),
            Some(true),
            vec![window(
                Some(0.0),
                SubscriptionWindowAvailability::Known,
                None,
            )],
            false,
        )),
    );
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text_context()),
        Eligibility::Exhausted,
        "missing reset evidence does not refill a fresh zero"
    );

    cache.store_chatgpt_subscription_snapshot(endpoint, None);
    assert_eq!(
        assess(Some(&cache), endpoint, Some(key), text_context()),
        Eligibility::Eligible(1.0)
    );
}
