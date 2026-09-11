//! OpenRouter quota-selection routing consumption (issue #203 P4).
//! Balance-weighted arms exercising the real production quota cache.
use super::selection::materialize_route_api_key_selection_with_quota;
use crate::{
    db,
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{
        OpenRouterBalance, OpenRouterSpend, TokenPlanKeyUsage, TokenPlanUsageResponse,
    },
};
use uuid::Uuid;
fn endpoint_key(endpoint_id: Uuid, key_id: Uuid, label: &str, pos: i32) -> db::EndpointApiKey {
    db::EndpointApiKey {
        key_id,
        endpoint_id,
        key_label: label.to_string(),
        api_key: format!("{label}-key"),
        position: pos,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

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

fn usage(keys: Vec<TokenPlanKeyUsage>) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::OpenRouter,
        provider_region: None,
        keys,
    }
}

fn request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"openrouter","input":"ping"}"#.to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(1),
        client_key_hash: Some("client-key".to_string()),
        request_user_agent: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    }
}

fn route(endpoint_id: Uuid, keys: Vec<db::EndpointApiKey>) -> db::RouteConfig {
    db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://openrouter.ai/api".to_string(),
        api_key: keys.first().map(|k| k.api_key.clone()).unwrap_or_default(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: keys,
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::OpenRouter,
        service_tier: db::MinimaxServiceTier::Standard,
    }
}

#[tokio::test]
async fn quota_key_lb_skips_openrouter_key_with_no_remaining() {
    let endpoint_id = Uuid::new_v4();
    let exhausted = Uuid::new_v4();
    let available = Uuid::new_v4();
    let route = route(
        endpoint_id,
        vec![
            endpoint_key(endpoint_id, exhausted, "exhausted", 0),
            endpoint_key(endpoint_id, available, "available", 1),
        ],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            usage(vec![
                openrouter_key(exhausted, "exhausted", Some(100.0), Some(0.0), true),
                openrouter_key(available, "available", Some(100.0), Some(80.0), true),
            ]),
        )
        .await;
    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );
    assert_eq!(selected.selection.key_id, Some(available));
    assert_eq!(selected.selection.key_label.as_deref(), Some("available"));
}

#[tokio::test]
async fn quota_key_lb_unlimited_openrouter_key_carries_full_weight() {
    let endpoint_id = Uuid::new_v4();
    let exhausted = Uuid::new_v4();
    let unlimited = Uuid::new_v4();
    let route = route(
        endpoint_id,
        vec![
            endpoint_key(endpoint_id, exhausted, "exhausted", 0),
            endpoint_key(endpoint_id, unlimited, "unlimited", 1),
        ],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            usage(vec![
                openrouter_key(exhausted, "exhausted", Some(100.0), Some(0.0), true),
                openrouter_key(unlimited, "unlimited", None, None, true),
            ]),
        )
        .await;
    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );
    assert_eq!(selected.selection.key_id, Some(unlimited));
    assert_eq!(selected.selection.key_label.as_deref(), Some("unlimited"));
}

#[tokio::test]
async fn quota_key_lb_still_routes_payg_openrouter_key_without_signal() {
    // No spend snapshot means no quota signal; falls back to stable candidate.
    let endpoint_id = Uuid::new_v4();
    let payg = Uuid::new_v4();
    let route = route(
        endpoint_id,
        vec![endpoint_key(endpoint_id, payg, "payg", 0)],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            usage(vec![openrouter_key(payg, "payg", None, None, false)]),
        )
        .await;
    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );
    assert_eq!(selected.selection.key_id, Some(payg));
    assert_eq!(selected.selection.key_label.as_deref(), Some("payg"));
}
