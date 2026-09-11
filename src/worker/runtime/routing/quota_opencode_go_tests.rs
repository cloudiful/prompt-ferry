//! OpencodeGo quota-selection routing consumption (issue #193 P4).
//!
//! Keys carry no `model_remains`; the quota cache weights them by the
//! tightest (lowest) remaining of the rolling/weekly/monthly percent windows.
//! These exercise the real `materialize_route_api_key_selection_with_quota`
//! against the production `TokenPlanQuotaCache`, mirroring how the
//! CommandCode arms are laid out in `quota_selection_tests.rs`.

use super::selection::materialize_route_api_key_selection_with_quota;
use crate::{
    db,
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage, TokenPlanUsageResponse},
};
use uuid::Uuid;

fn endpoint_key(
    endpoint_id: Uuid,
    key_id: Uuid,
    key_label: &str,
    position: i32,
) -> db::EndpointApiKey {
    db::EndpointApiKey {
        key_id,
        endpoint_id,
        key_label: key_label.to_string(),
        api_key: format!("{key_label}-key"),
        position,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn opencode_go_window(percent: f64) -> OpencodeGoWindowUsage {
    OpencodeGoWindowUsage {
        status: None,
        percent: Some(percent),
        resets_at: None,
    }
}

fn opencode_go_key_usage(
    key_id: Uuid,
    key_label: &str,
    rolling: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: key_label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        model_remains: Vec::new(),
        balances: None,
        five_hour: None,
        weekly: None,
        opencodego_rolling: rolling.map(opencode_go_window),
        opencodego_weekly: weekly.map(opencode_go_window),
        opencodego_monthly: monthly.map(opencode_go_window),
        openrouter_balance: None,
        openrouter_spend: None,
        glm_five_hour: None,
        glm_weekly: None,
        deepseek_balance: None,
    }
}

fn opencode_go_usage_response(keys: Vec<TokenPlanKeyUsage>) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::OpencodeGo,
        provider_region: None,
        keys,
    }
}

fn opencode_go_request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"opencode-go","input":"ping"}"#.to_vec(),
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

fn opencode_go_route(endpoint_id: Uuid, keys: Vec<db::EndpointApiKey>) -> db::RouteConfig {
    db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://opencode.ai/zen/go/v1".to_string(),
        api_key: keys
            .first()
            .map(|key| key.api_key.clone())
            .unwrap_or_default(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: keys,
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::OpencodeGo,
        service_tier: db::MinimaxServiceTier::Standard,
    }
}

#[tokio::test]
async fn quota_key_lb_uses_tighter_opencode_go_window_and_single_arm() {
    let endpoint_id = Uuid::new_v4();
    let tight_key_id = Uuid::new_v4();
    let single_arm_key_id = Uuid::new_v4();
    let route = opencode_go_route(
        endpoint_id,
        vec![
            endpoint_key(endpoint_id, tight_key_id, "exhausted", 0),
            endpoint_key(endpoint_id, single_arm_key_id, "available", 1),
        ],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            opencode_go_usage_response(vec![
                // Tightest remaining is 0% (rolling at 100% used): skipped
                // even though weekly/monthly are far from exhausted.
                opencode_go_key_usage(
                    tight_key_id,
                    "exhausted",
                    Some(100.0),
                    Some(25.0),
                    Some(10.0),
                ),
                // Single weekly arm still weights the key (20% remaining).
                opencode_go_key_usage(single_arm_key_id, "available", None, Some(80.0), None),
            ]),
        )
        .await;

    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &opencode_go_request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(single_arm_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("available"));
}

#[tokio::test]
async fn quota_key_lb_skips_opencode_go_key_with_no_remaining_window() {
    let endpoint_id = Uuid::new_v4();
    let exhausted_key_id = Uuid::new_v4();
    let available_key_id = Uuid::new_v4();
    let route = opencode_go_route(
        endpoint_id,
        vec![
            endpoint_key(endpoint_id, exhausted_key_id, "exhausted", 0),
            endpoint_key(endpoint_id, available_key_id, "available", 1),
        ],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            opencode_go_usage_response(vec![
                // All three windows at 100% used => 0% remaining.
                opencode_go_key_usage(
                    exhausted_key_id,
                    "exhausted",
                    Some(100.0),
                    Some(100.0),
                    Some(100.0),
                ),
                opencode_go_key_usage(available_key_id, "available", Some(50.0), None, None),
            ]),
        )
        .await;

    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &opencode_go_request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(available_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("available"));
}

#[tokio::test]
async fn quota_key_lb_still_routes_payg_opencode_go_key_without_windows() {
    // PAYG keys degrade to None windows (no quota signal); routing falls
    // back to the stable candidate instead of dropping the key.
    let endpoint_id = Uuid::new_v4();
    let payg_key_id = Uuid::new_v4();
    let route = opencode_go_route(
        endpoint_id,
        vec![endpoint_key(endpoint_id, payg_key_id, "payg", 0)],
    );
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            opencode_go_usage_response(vec![opencode_go_key_usage(
                payg_key_id,
                "payg",
                None,
                None,
                None,
            )]),
        )
        .await;

    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &opencode_go_request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(payg_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("payg"));
}
