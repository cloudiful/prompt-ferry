//! OpenRouter session-affinity routing consumption (issue #203 P4).
//! Bound-key gating via the real quota cache: exhausted keys migrate.

use super::select_route_for_candidate;
use super::session_affinity_quota_tests::{bind_key, request};
use super::session_affinity_tests::request_context;
use crate::{
    db,
    replay_cache::ReplayCache,
    worker::runtime::prompt_log::RequestPromptLog,
    worker_admin_types::{
        OpenRouterBalance, OpenRouterSpend, TokenPlanKeyUsage, TokenPlanUsageResponse,
    },
};
use uuid::Uuid;

fn openrouter_key(
    key_id: Uuid,
    label: &str,
    limit: Option<f64>,
    remaining: Option<f64>,
    spend: bool,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        model_remains: Vec::new(),
        balances: None,
        five_hour: None,
        weekly: None,
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
        openrouter_balance: Some(OpenRouterBalance {
            limit,
            limit_remaining: remaining,
            limit_reset: None,
            is_free_tier: false,
            total_credits: None,
            total_usage: None,
        }),
        openrouter_spend: spend.then_some(OpenRouterSpend {
            usage: 1.0,
            daily: 0.5,
            weekly: 0.75,
            monthly: 1.0,
        }),
        glm_five_hour: None,
        glm_weekly: None,
        deepseek_balance: None,
    }
}

fn openrouter_usage(keys: Vec<TokenPlanKeyUsage>) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::OpenRouter,
        provider_region: None,
        keys,
    }
}

#[tokio::test]
async fn exhausted_openrouter_bound_key_migrates_to_alternate_key() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services =
        super::super::tests::session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = super::super::tests::session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;
    let alternate_key_id = Uuid::new_v4();
    target.api_keys.push(db::EndpointApiKey {
        key_id: alternate_key_id,
        endpoint_id,
        key_label: "alternate".to_string(),
        api_key: "alternate-key".to_string(),
        position: 1,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    let conversation_id = Uuid::new_v4();
    bind_key(
        &replay_cache,
        candidate.rule_id,
        conversation_id,
        endpoint_id,
        bound_key_id,
        "key-a",
    )
    .await;
    services
        .admin_state()
        .expect("admin state")
        .token_plan_quota
        .store_for_test(
            endpoint_id,
            openrouter_usage(vec![
                openrouter_key(bound_key_id, "primary", Some(100.0), Some(0.0), true),
                openrouter_key(alternate_key_id, "alternate", Some(100.0), Some(80.0), true),
            ]),
        )
        .await;
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            preferred_endpoint_id: Some(endpoint_id),
            ..RequestPromptLog::default()
        },
    );
    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &request(),
        1,
        Some("key-a"),
    )
    .await
    .expect("exhausted openrouter bound key must migrate within the candidate")
    .expect("migration must select a route");
    assert_ne!(
        selected.route.endpoint_key_id,
        Some(bound_key_id),
        "the exhausted openrouter bound unit must be removed"
    );
    assert_ne!(selected.route.api_key, "key-a");
    assert_eq!(
        selected.route.route_selection_reason,
        db::RouteSelectionReason::QuotaFailover
    );
}

#[tokio::test]
async fn available_openrouter_bound_key_is_still_honored() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services =
        super::super::tests::session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = super::super::tests::session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;
    let conversation_id = Uuid::new_v4();
    bind_key(
        &replay_cache,
        candidate.rule_id,
        conversation_id,
        endpoint_id,
        bound_key_id,
        "key-a",
    )
    .await;
    services
        .admin_state()
        .expect("admin state")
        .token_plan_quota
        .store_for_test(
            endpoint_id,
            openrouter_usage(vec![openrouter_key(
                bound_key_id,
                "primary",
                Some(100.0),
                Some(60.0),
                true,
            )]),
        )
        .await;
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            preferred_endpoint_id: Some(endpoint_id),
            ..RequestPromptLog::default()
        },
    );
    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &request(),
        1,
        Some("key-a"),
    )
    .await
    .expect("available bound key must still route")
    .expect("available bound key must select a route");
    assert_eq!(selected.route.endpoint_key_id, Some(bound_key_id));
}
