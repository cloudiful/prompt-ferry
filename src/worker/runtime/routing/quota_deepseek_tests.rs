//! DeepSeek quota-selection routing consumption (issue #287 P0).
//! `is_available=false` maps to weight 0 so the key is excluded; an available
//! key carries full weight. Exercises the real production quota cache.
use super::selection::materialize_route_api_key_selection_with_quota;
use crate::{
    db,
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{DeepSeekBalance, TokenPlanKeyUsage, TokenPlanUsageResponse},
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

fn deepseek_key(key_id: Uuid, label: &str, is_available: bool) -> TokenPlanKeyUsage {
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
        openrouter_balance: None,
        openrouter_spend: None,
        glm_five_hour: None,
        glm_weekly: None,
        deepseek_balance: Some(DeepSeekBalance {
            is_available,
            currency: "CNY".to_string(),
            total_balance: if is_available { 110.0 } else { 0.0 },
            granted_balance: 0.0,
            topped_up_balance: if is_available { 110.0 } else { 0.0 },
        }),
    }
}

fn usage(keys: Vec<TokenPlanKeyUsage>) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::DeepSeek,
        provider_region: None,
        keys,
    }
}

fn request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/chat/completions".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"deepseek-chat","messages":[{"role":"user","content":"ping"}]}"#
            .to_vec(),
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
        base_url: "https://api.deepseek.com".to_string(),
        api_key: keys.first().map(|k| k.api_key.clone()).unwrap_or_default(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: keys,
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Chat,
        upstream_model: None,
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::DeepSeek,
        service_tier: db::MinimaxServiceTier::Standard,
    }
}

#[tokio::test]
async fn quota_key_lb_skips_unavailable_deepseek_key() {
    let endpoint_id = Uuid::new_v4();
    let unavailable = Uuid::new_v4();
    let available = Uuid::new_v4();
    let route = route(
        endpoint_id,
        vec![
            endpoint_key(endpoint_id, unavailable, "unavailable", 0),
            endpoint_key(endpoint_id, available, "available", 1),
        ],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            usage(vec![
                deepseek_key(unavailable, "unavailable", false),
                deepseek_key(available, "available", true),
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
async fn quota_key_lb_still_routes_unavailable_deepseek_key_without_signal() {
    // A failed balance fetch carries no deepseek section, so there is no quota
    // signal and selection falls back to the stable candidate.
    let endpoint_id = Uuid::new_v4();
    let unknown = Uuid::new_v4();
    let route = route(
        endpoint_id,
        vec![endpoint_key(endpoint_id, unknown, "unknown", 0)],
    );
    let mut key = deepseek_key(unknown, "unknown", true);
    key.deepseek_balance = None;
    let cache = TokenPlanQuotaCache::default();
    cache.store_for_test(endpoint_id, usage(vec![key])).await;
    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );
    assert_eq!(selected.selection.key_id, Some(unknown));
    assert_eq!(selected.selection.key_label.as_deref(), Some("unknown"));
}
