//! DeepSeek quota-selection routing consumption (issue #287 P0).
//! `is_available=false` maps to weight 0 so the key is excluded; an available
//! key carries full weight. Exercises the real production quota cache.
use super::selection::materialize_route_api_key_selection_with_quota;
use crate::{
    db,
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{
        DeepSeekBalance, DeepSeekCurrencyBalance, TokenPlanKeyUsage, TokenPlanUsageResponse,
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
            balances: vec![DeepSeekCurrencyBalance {
                currency: "CNY".to_string(),
                total_balance: Some(110.0),
                granted_balance: Some(0.0),
                topped_up_balance: Some(110.0),
            }],
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
        service_tier: None,
        proxy_url: None,
        dev_system_normalize: false,
        thinking_effort_override: None,
        compact_mode: crate::db::CompactMode::Passthrough,
        thinking_downgrade_enabled: false,
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

#[tokio::test]
async fn quota_key_lb_zeroes_a_key_that_reports_unavailable_without_amounts() {
    // Issue #712 P1: a body that only carries the availability signal (no
    // parseable currency entries) must still drive routing — `is_available`
    // alone is the authoritative DeepSeek quota input, so a `false` flag
    // keeps weight 0 even when every amount is unknown.
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
    let mut unavailable_key = deepseek_key(unavailable, "unavailable", false);
    if let Some(balance) = unavailable_key.deepseek_balance.as_mut() {
        balance.balances.clear();
    }
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            usage(vec![
                unavailable_key,
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
async fn quota_key_lb_weights_ignore_parsed_balances() {
    // Issue #712 P1: multi-currency balances are display data. A key with
    // unknown (null) amounts and one with a large positive balance carry the
    // same flag-derived weight, so balance parsing cannot inflate or deflate
    // eligibility.
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let mut key = deepseek_key(key_id, "k", true);
    if let Some(balance) = key.deepseek_balance.as_mut() {
        for entry in &mut balance.balances {
            entry.total_balance = None;
            entry.granted_balance = None;
            entry.topped_up_balance = None;
        }
    }
    let cache = TokenPlanQuotaCache::default();
    cache.store_for_test(endpoint_id, usage(vec![key])).await;
    assert_eq!(
        cache.key_weight_percent_now(endpoint_id, key_id, None),
        Some(100.0)
    );
}
