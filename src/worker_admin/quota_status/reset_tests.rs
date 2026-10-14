use std::time::Instant;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use uuid::Uuid;

use super::*;
use crate::{
    db::EndpointRegion,
    worker_admin::quota_reset::subscription_windows_state,
    worker_admin_types::{
        SubscriptionWindowAvailability, SubscriptionWindowUsage, TokenPlanModelUsage,
        TokenPlanWindowUsage,
    },
};

fn minimax_usage(
    key_id: Uuid,
    end_at: Option<DateTime<Utc>>,
    remains_time_ms: Option<i64>,
) -> TokenPlanUsageResponse {
    let key = TokenPlanKeyUsage {
        key_id,
        key_label: "key".into(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        quota_status: None,
        model_remains: vec![TokenPlanModelUsage {
            model_name: "general".into(),
            interval: Some(TokenPlanWindowUsage {
                status: Some(200),
                remaining_percent: Some(0.0),
                total_count: None,
                usage_count: None,
                boost_permille: None,
                start_at: None,
                end_at,
                remains_time_ms,
            }),
            weekly: None,
            windows: None,
            observation: None,
        }],
        balances: None,
        five_hour: None,
        weekly: None,
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
        openrouter_balance: None,
        openrouter_spend: None,
        glm_five_hour: None,
        glm_weekly: None,
        deepseek_balance: None,
        quota_windows: None,
    };
    TokenPlanUsageResponse {
        provider: EndpointProvider::Minimax,
        provider_region: None::<EndpointRegion>,
        keys: vec![key],
        local_today_tokens: None,
    }
}

fn subscription_window(
    remaining: Option<f64>,
    reset_at: Option<DateTime<Utc>>,
    reset_after_seconds: Option<i64>,
) -> SubscriptionWindowUsage {
    SubscriptionWindowUsage {
        source_window: "primary".into(),
        window_seconds: Some(604_800),
        used_percent: remaining.map(|remaining| 100.0 - remaining),
        remaining_percent: remaining,
        reset_at,
        reset_after_seconds,
        availability: if remaining.is_some() {
            SubscriptionWindowAvailability::Known
        } else {
            SubscriptionWindowAvailability::Unknown
        },
    }
}

#[tokio::test]
async fn minimax_relative_zero_matches_routing_as_exhausted() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let usage = minimax_usage(key_id, None, Some(60_000));
    cache.store_for_test(endpoint_id, usage.clone()).await;

    assert_eq!(
        cache.key_remaining_percent_scoped_now(endpoint_id, key_id, Some("general"), true),
        Some(0.0)
    );
    assert_eq!(
        status_for_key(
            &cache,
            endpoint_id,
            EndpointProvider::Minimax,
            &usage.keys[0],
            Utc::now(),
            Instant::now(),
        )
        .state,
        TokenPlanQuotaState::Exhausted
    );
    cache.invalidate(endpoint_id).await;
    assert_eq!(
        status_for_key(
            &cache,
            endpoint_id,
            EndpointProvider::Minimax,
            &usage.keys[0],
            Utc::now(),
            Instant::now(),
        )
        .state,
        TokenPlanQuotaState::Unknown
    );
}

#[tokio::test]
async fn minimax_any_passed_reset_clock_makes_zero_unknown_in_both_paths() {
    let cases = [
        (Some(Utc::now() + ChronoDuration::hours(1)), Some(0)),
        (Some(Utc::now() - ChronoDuration::seconds(1)), Some(60_000)),
    ];
    for (end_at, remains_time_ms) in cases {
        let cache = TokenPlanQuotaCache::default();
        let endpoint_id = Uuid::new_v4();
        let key_id = Uuid::new_v4();
        let usage = minimax_usage(key_id, end_at, remains_time_ms);
        cache.store_for_test(endpoint_id, usage.clone()).await;

        assert_eq!(
            cache.key_remaining_percent_scoped_now(endpoint_id, key_id, Some("general"), true),
            None
        );
        assert_eq!(
            status_for_key(
                &cache,
                endpoint_id,
                EndpointProvider::Minimax,
                &usage.keys[0],
                Utc::now(),
                Instant::now(),
            )
            .state,
            TokenPlanQuotaState::Unknown
        );
    }
}

#[test]
fn subscription_reset_evidence_controls_effective_metadata() {
    let now = Utc::now();
    let observed_at = now - ChronoDuration::seconds(5);
    let future = subscription_window(Some(0.0), Some(now + ChronoDuration::hours(1)), None);
    let missing = subscription_window(Some(0.0), None, None);
    let past = subscription_window(Some(0.0), Some(now - ChronoDuration::seconds(1)), None);
    for window in [&future, &missing] {
        assert_eq!(
            subscription_windows_state(&[window], observed_at, now),
            TokenPlanQuotaState::Exhausted
        );
    }

    let relative = subscription_window(Some(0.0), None, Some(1));
    let relative_expired_absolute_future =
        subscription_window(Some(0.0), Some(now + ChronoDuration::hours(1)), Some(1));
    let mut absolute_expired_relative_future =
        subscription_window(Some(0.0), Some(now - ChronoDuration::seconds(1)), Some(60));
    absolute_expired_relative_future.reset_after_seconds = Some(60);
    let positive_passed =
        subscription_window(Some(50.0), Some(now - ChronoDuration::seconds(1)), None);
    for window in [
        &past,
        &relative,
        &relative_expired_absolute_future,
        &absolute_expired_relative_future,
        &positive_passed,
    ] {
        assert_eq!(
            subscription_windows_state(&[window], observed_at, now),
            TokenPlanQuotaState::Unknown
        );
    }
}
