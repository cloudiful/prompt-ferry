use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::*;
use crate::{
    db::{EndpointProvider, EndpointRegion},
    worker_admin::{
        quota_feedback::{QuotaFeedbackCause, QuotaFeedbackScope},
        quota_reset::subscription_windows_state,
    },
    worker_admin_types::{
        CommandCodeBalances, CommandCodeWindowUsage, DeepSeekBalance, GlmWindowUsage,
        OpenRouterBalance, OpencodeGoWindowUsage, SubscriptionQuotaObservation,
        SubscriptionWindowAvailability, SubscriptionWindowUsage, TokenPlanModelUsage,
        TokenPlanQuotaState::{
            Available, Cooling, Exhausted, RateLimited, Unknown, UpstreamUnavailable,
        },
        TokenPlanWindowUsage,
    },
};

fn key(key_id: Uuid) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: "key".into(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        quota_status: None,
        model_remains: Vec::new(),
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
    }
}

fn usage(provider: EndpointProvider, key: TokenPlanKeyUsage) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        provider,
        provider_region: None::<EndpointRegion>,
        keys: vec![key],
        local_today_tokens: None,
    }
}

fn assert_state(
    provider: EndpointProvider,
    key: &TokenPlanKeyUsage,
    now: DateTime<Utc>,
    expected: TokenPlanQuotaState,
) {
    assert_eq!(provider_state(provider, key, now), expected);
}

fn window(remaining: f64, end_at: Option<DateTime<Utc>>) -> TokenPlanWindowUsage {
    TokenPlanWindowUsage {
        status: Some(1),
        remaining_percent: Some(remaining),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at,
        remains_time_ms: None,
    }
}

fn subscription_window(
    remaining: Option<f64>,
    availability: SubscriptionWindowAvailability,
    reset_at: Option<DateTime<Utc>>,
) -> SubscriptionWindowUsage {
    SubscriptionWindowUsage {
        source_window: "primary".into(),
        window_seconds: Some(18_000),
        used_percent: remaining.map(|remaining| 100.0 - remaining),
        remaining_percent: remaining,
        reset_at,
        reset_after_seconds: None,
        availability,
    }
}

#[test]
fn default_text_zero_blocks_but_reset_passed_zero_is_unknown() {
    let now = Utc::now();
    let mut key = key(Uuid::new_v4());
    key.model_remains = vec![TokenPlanModelUsage {
        model_name: "General".into(),
        interval: Some(window(0.0, Some(now + ChronoDuration::minutes(5)))),
        weekly: Some(window(35.0, Some(now + ChronoDuration::days(3)))),
        windows: None,
        observation: None,
    }];
    assert_state(EndpointProvider::Minimax, &key, now, Exhausted);

    key.model_remains[0].interval = Some(window(0.0, None));
    assert_state(EndpointProvider::Minimax, &key, now, Exhausted);
    key.model_remains[0].interval = Some(window(0.0, Some(now - ChronoDuration::seconds(1))));
    assert_state(EndpointProvider::Minimax, &key, now, Unknown);
}

#[test]
fn supported_provider_windows_require_known_current_capacity() {
    let now = Utc::now();
    let future = Some(now + ChronoDuration::hours(1));

    let mut opencode_go = key(Uuid::new_v4());
    opencode_go.opencodego_rolling = Some(OpencodeGoWindowUsage {
        status: Some(200),
        percent: Some(100.0),
        resets_at: future,
    });
    assert_state(EndpointProvider::OpencodeGo, &opencode_go, now, Exhausted);
    let mut openrouter = key(Uuid::new_v4());
    openrouter.openrouter_balance = Some(OpenRouterBalance {
        limit: Some(10.0),
        limit_remaining: Some(0.0),
        limit_reset: None,
        is_free_tier: false,
        total_credits: None,
        total_usage: None,
    });
    assert_state(EndpointProvider::OpenRouter, &openrouter, now, Exhausted);
    let mut glm = key(Uuid::new_v4());
    glm.glm_five_hour = Some(GlmWindowUsage {
        limit: 100.0,
        current_value: 100.0,
        remaining: 0.0,
        percentage: Some(100.0),
        next_reset_at: future,
    });
    assert_eq!(provider_state(EndpointProvider::Glm, &glm, now), Exhausted);
    let mut deepseek = key(Uuid::new_v4());
    deepseek.deepseek_balance = Some(DeepSeekBalance {
        is_available: true,
        balances: Vec::new(),
    });
    assert_state(EndpointProvider::DeepSeek, &deepseek, now, Available);
    deepseek.deepseek_balance.as_mut().unwrap().is_available = false;
    assert_state(EndpointProvider::DeepSeek, &deepseek, now, Exhausted);
}

#[test]
fn subscription_observation_freshness_is_required_for_available_state() {
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();
    let cache = TokenPlanQuotaCache::default();
    let observed_at = now - ChronoDuration::seconds(10);
    let quota_window = subscription_window(
        Some(50.0),
        SubscriptionWindowAvailability::Known,
        Some(now + ChronoDuration::minutes(1)),
    );
    let mut key = key(Uuid::new_v4());
    key.model_remains = vec![TokenPlanModelUsage {
        model_name: "plus".into(),
        interval: None,
        weekly: None,
        windows: Some(vec![quota_window]),
        observation: None,
    }];
    let mut observation = SubscriptionQuotaObservation {
        observed_at: Some(observed_at),
        source: Some("manual".into()),
        stale: false,
        last_error_code: None,
        next_retry_at: None,
        refreshing: false,
    };
    let state = |observation: &SubscriptionQuotaObservation| {
        subscription_state(&cache, endpoint_id, &key, observation, now)
    };
    assert_eq!(state(&observation), Available);
    observation.stale = true;
    assert_eq!(state(&observation), Unknown);
    observation.stale = false;
    observation.last_error_code = Some("refresh_failed".into());
    assert_eq!(state(&observation), Unknown);
    observation.last_error_code = None;
    observation.observed_at = Some(now - ChronoDuration::minutes(6));
    assert_eq!(state(&observation), Unknown);
}

#[test]
fn command_code_positive_native_balance_is_available() {
    let now = Utc::now();
    let mut command_code = key(Uuid::new_v4());
    command_code.balances = Some(CommandCodeBalances {
        monthly_credits: 10.0,
        purchased_credits: 0.0,
        free_credits: 0.0,
        remaining_credits: 2.5,
    });
    assert_eq!(
        provider_state(EndpointProvider::CommandCode, &command_code, now),
        Available
    );
}

#[test]
fn subscription_state_requires_fresh_known_observation_and_future_reset() {
    let now = Utc::now();
    let observed_at = now - ChronoDuration::seconds(10);
    let window = subscription_window(
        Some(0.0),
        SubscriptionWindowAvailability::Known,
        Some(now + ChronoDuration::minutes(1)),
    );
    assert_eq!(
        subscription_windows_state(&[&window], observed_at, now),
        Exhausted
    );
    let passed = subscription_window(
        Some(0.0),
        SubscriptionWindowAvailability::Known,
        Some(now - ChronoDuration::seconds(1)),
    );
    assert_eq!(
        subscription_windows_state(&[&passed], observed_at, now),
        Unknown
    );

    let missing = subscription_window(None, SubscriptionWindowAvailability::Unknown, None);
    assert_eq!(
        subscription_windows_state(&[&missing], observed_at, now),
        Unknown
    );
}

#[tokio::test]
async fn failed_refresh_is_unknown_and_retains_the_success_timestamp() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let mut observed = key(key_id);
    observed.five_hour = Some(CommandCodeWindowUsage {
        used: 0.0,
        cap: 100.0,
        used_percent: Some(0.0),
        remaining_percent: Some(100.0),
        reset_at: Some(Utc::now() + ChronoDuration::hours(5)),
    });
    let observed_usage = usage(EndpointProvider::CommandCode, observed.clone());
    cache.store_for_test(endpoint_id, observed_usage).await;
    let before = cache
        .quota_observation_now(endpoint_id, key_id)
        .and_then(|observation| observation.observed_at)
        .expect("fresh observation timestamp");

    cache.invalidate(endpoint_id).await;
    let decorated = decorate_usage(
        &cache,
        endpoint_id,
        usage(EndpointProvider::CommandCode, observed),
    );
    let status = decorated.keys[0].quota_status.as_ref().unwrap();
    assert_eq!(status.state, Unknown);
    assert!(status.refresh_failed);
    assert_eq!(status.observed_at, Some(before));
}

#[test]
fn active_feedback_is_reported_as_cooling_with_a_bounded_deadline() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let now = Instant::now();
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    cache.feedback.record_failure(
        scope,
        QuotaFeedbackCause::Exhausted,
        Some(Duration::from_secs(45)),
        now,
        now,
    );

    let decorated = decorate_usage(
        &cache,
        endpoint_id,
        usage(EndpointProvider::CommandCode, key(key_id)),
    );
    let status = decorated.keys[0].quota_status.as_ref().unwrap();
    assert_eq!(status.state, Cooling);
    assert!(status.cooldown_until.is_some());
}

#[test]
fn transient_feedback_cause_projects_its_own_state_with_the_same_deadline() {
    // The default 429 caller is throttling, not exhaustion: it must not be
    // reported as a spent quota.
    let default_rate_limit = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let now = Instant::now();
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    default_rate_limit
        .feedback
        .record_429(scope, Some(Duration::from_secs(45)), now, now);

    let status = decorate_usage(
        &default_rate_limit,
        endpoint_id,
        usage(EndpointProvider::CommandCode, key(key_id)),
    )
    .keys[0]
        .quota_status
        .clone()
        .unwrap();
    assert_eq!(status.state, RateLimited);
    assert!(status.cooldown_until.is_some());

    let unavailable = TokenPlanQuotaCache::default();
    unavailable.feedback.record_failure(
        scope,
        QuotaFeedbackCause::UpstreamUnavailable,
        Some(Duration::from_secs(45)),
        now,
        now,
    );
    let status = decorate_usage(
        &unavailable,
        endpoint_id,
        usage(EndpointProvider::CommandCode, key(key_id)),
    )
    .keys[0]
        .quota_status
        .clone()
        .unwrap();
    assert_eq!(status.state, UpstreamUnavailable);
    assert!(status.cooldown_until.is_some());
}

#[test]
fn an_expired_feedback_deadline_stops_blocking_the_key() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let now = Instant::now();
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    cache.feedback.record_failure(
        scope,
        QuotaFeedbackCause::Exhausted,
        Some(Duration::from_secs(30)),
        now,
        now,
    );

    let (_, remaining) = cache
        .feedback
        .active_cooldown(scope, now)
        .expect("active block");
    assert!(remaining <= Duration::from_secs(30));
    // Past the deadline the projection reads no active block, so the provider
    // signal is used again instead of a fabricated healthy or spent state.
    assert!(
        cache
            .feedback
            .active_cooldown(scope, now + Duration::from_secs(31))
            .is_none()
    );
}

#[test]
fn missing_fresh_quota_signal_never_claims_available() {
    let now = Utc::now();
    assert_eq!(
        provider_state(EndpointProvider::OpenRouter, &key(Uuid::new_v4()), now),
        Unknown
    );
}

#[test]
fn glm_mixed_weekly_exhausted_with_fresh_five_hour_is_exhausted() {
    // A fully consumed weekly window dominates a fresh 5h window while its
    // deadline has not passed: the key is exhausted, not available.
    let now = Utc::now();
    let mut glm = key(Uuid::new_v4());
    glm.glm_five_hour = Some(GlmWindowUsage {
        limit: 100.0,
        current_value: 0.0,
        remaining: 100.0,
        percentage: Some(0.0),
        next_reset_at: Some(now + ChronoDuration::hours(5)),
    });
    glm.glm_weekly = Some(GlmWindowUsage {
        limit: 100.0,
        current_value: 100.0,
        remaining: 0.0,
        percentage: Some(100.0),
        next_reset_at: Some(now + ChronoDuration::days(6)),
    });
    assert_state(EndpointProvider::Glm, &glm, now, Exhausted);
}
