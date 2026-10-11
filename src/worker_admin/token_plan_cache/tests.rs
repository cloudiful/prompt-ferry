use super::*;
use crate::{
    db,
    worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage, TokenPlanUsageResponse},
};

#[test]
fn local_estimator_is_conservative_for_small_payloads() {
    assert_eq!(estimate_input_tokens(br#"{"model":"MiniMax-M3"}"#), 6);
    assert!(estimate_input_tokens("中文请求".as_bytes()) >= 1);
}

pub(super) fn opencode_go_usage(key_id: Uuid, remaining: f64) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::OpencodeGo,
        provider_region: None,
        keys: vec![TokenPlanKeyUsage {
            key_id,
            key_label: "k".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            quota_status: None,
            model_remains: Vec::new(),
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: Some(OpencodeGoWindowUsage {
                status: None,
                percent: Some(100.0 - remaining),
                resets_at: None,
            }),
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
        }],
    }
}

#[tokio::test]
async fn reservations_damp_percent_windows_but_never_starve_them() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let other_key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
        .await;
    assert_eq!(
        cache.key_weight_percent_now(endpoint_id, key_id, None),
        Some(80.0)
    );
    for _ in 0..5 {
        cache.reserve_estimated_tokens(endpoint_id, key_id, 1_000);
    }
    let damped = cache
        .key_weight_percent_now(endpoint_id, key_id, None)
        .expect("weighted key");
    assert!(damped < 80.0, "reservations must damp the weight: {damped}");
    assert!(damped > 0.0);
    for _ in 0..1_000_000 {
        cache.reserve_estimated_tokens(endpoint_id, key_id, 1);
    }
    let floor = cache
        .key_weight_percent_now(endpoint_id, key_id, None)
        .expect("weighted key");
    assert!(floor.is_finite() && floor > 0.0, "floor={floor}");
    assert_eq!(
        cache.key_weight_percent_now(endpoint_id, other_key_id, None),
        None
    );
}

#[tokio::test]
async fn raw_remaining_ignores_urgency_and_reservations() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 30.0))
        .await;
    for _ in 0..50 {
        cache.reserve_estimated_tokens(endpoint_id, key_id, 10_000);
    }
    assert_eq!(
        cache.key_remaining_percent_now(endpoint_id, key_id, None),
        Some(30.0),
        "exhaustion checks must keep the raw window percent"
    );
}

#[tokio::test]
async fn invalidate_drops_freshness_so_next_refresh_refetches() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
        .await;
    assert_eq!(
        cache
            .fresh_snapshot(endpoint_id)
            .await
            .expect("stored snapshot is fresh")
            .keys[0]
            .key_id,
        key_id
    );
    cache.invalidate(endpoint_id).await;
    assert!(cache.fresh_snapshot(endpoint_id).await.is_none());
    assert!(
        cache
            .key_weight_percent_now(endpoint_id, key_id, None)
            .is_none()
    );
    assert!(cache.raw_snapshot(endpoint_id).await.is_some());
}

#[tokio::test]
async fn fresh_snapshot_serves_the_stored_payload_for_its_own_endpoint_only() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint_id = Uuid::new_v4();
    let other_endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    cache
        .store_for_test(endpoint_id, opencode_go_usage(key_id, 80.0))
        .await;
    let served = cache
        .fresh_snapshot(endpoint_id)
        .await
        .expect("fresh entry");
    assert_eq!(served.keys.len(), 1);
    assert_eq!(served.keys[0].key_id, key_id);
    assert_eq!(
        served.keys[0]
            .opencodego_rolling
            .as_ref()
            .and_then(|window| window.percent),
        Some(20.0)
    );
    assert!(cache.fresh_snapshot(other_endpoint_id).await.is_none());
}
