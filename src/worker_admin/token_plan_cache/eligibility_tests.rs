use super::*;
use crate::worker_admin::quota_feedback::{QuotaFeedbackCause, QuotaFeedbackScope};
use crate::worker_admin_types::{TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse};
use std::time::Duration;

fn window(remaining_percent: f64) -> crate::worker_admin_types::TokenPlanWindowUsage {
    crate::worker_admin_types::TokenPlanWindowUsage {
        status: Some(1),
        remaining_percent: Some(remaining_percent),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at: None,
        remains_time_ms: None,
    }
}

fn minimax_usage(key_id: Uuid, interval: f64, weekly: f64) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: crate::db::EndpointProvider::Minimax,
        provider_region: None,
        keys: vec![TokenPlanKeyUsage {
            key_id,
            key_label: "k".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            quota_status: None,
            model_remains: vec![TokenPlanModelUsage {
                model_name: "general".to_string(),
                interval: Some(window(interval)),
                weekly: Some(window(weekly)),
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
        }],
    }
}

fn general_scope(endpoint_id: Uuid, key_id: Uuid) -> QuotaScope<'static> {
    QuotaScope::new(endpoint_id, Some(key_id), Some("general"), true)
}

#[tokio::test]
async fn a_zero_weekly_budget_is_exhausted_even_with_a_full_interval() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, minimax_usage(key_id, 100.0, 0.0))
        .await;

    assert_eq!(
        cache.assess_scope(general_scope(endpoint_id, key_id)),
        QuotaAssessment::Exhausted
    );
}

#[tokio::test]
async fn a_healthy_budget_stays_eligible_with_its_weight() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, minimax_usage(key_id, 79.0, 61.0))
        .await;

    let assessment = cache.assess_scope(general_scope(endpoint_id, key_id));
    let QuotaAssessment::Eligible(weight) = assessment else {
        panic!("a funded key must stay eligible, got {assessment:?}");
    };
    assert!(weight > 0.0 && weight <= 100.0, "weight={weight}");
}

#[tokio::test]
async fn an_unknown_budget_is_neutral_and_not_exhausted() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();

    assert_eq!(
        cache.assess_scope(general_scope(endpoint_id, Uuid::new_v4())),
        QuotaAssessment::Eligible(1.0)
    );
}

#[tokio::test]
async fn a_scope_without_a_snapshot_ignores_the_cached_budget() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, minimax_usage(key_id, 100.0, 0.0))
        .await;

    let scope = QuotaScope::without_snapshot(endpoint_id, Some(key_id), Some("general"), true);
    assert_eq!(cache.assess_scope(scope), QuotaAssessment::Eligible(1.0));
}

#[tokio::test]
async fn a_scope_without_a_snapshot_still_honours_the_feedback_cooldown() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let scope = QuotaScope::without_snapshot(endpoint_id, Some(key_id), Some("general"), true);
    cache.feedback.record_failure(
        QuotaFeedbackScope::new(endpoint_id, Some(key_id)),
        QuotaFeedbackCause::RateLimited,
        None,
        Instant::now(),
        Instant::now(),
    );

    assert_eq!(cache.assess_scope(scope), QuotaAssessment::Cooling);
}

#[tokio::test]
async fn a_probe_scope_admits_one_holder_at_a_time() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let scope = general_scope(endpoint_id, key_id);
    let feedback_scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    cache.feedback.record_429(
        feedback_scope,
        Some(Duration::ZERO),
        Instant::now(),
        Instant::now(),
    );

    let claim = cache.claim_scope_probe(scope);
    let QuotaProbeClaim::Reserved(mut guard) = claim else {
        panic!("the first claim must own the probe slot");
    };
    assert!(matches!(
        cache.claim_scope_probe(scope),
        QuotaProbeClaim::Busy
    ));

    guard.release();
    assert!(matches!(
        cache.claim_scope_probe(scope),
        QuotaProbeClaim::Reserved(_)
    ));
}
