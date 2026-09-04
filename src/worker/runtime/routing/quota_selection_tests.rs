use super::selection::materialize_route_api_key_selection_with_quota;
use crate::{
    db,
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{
        TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse, TokenPlanWindowUsage,
    },
};
use chrono::Utc;

#[tokio::test]
async fn quota_key_lb_skips_a_key_with_no_remaining_window() {
    let endpoint_id = uuid::Uuid::new_v4();
    let exhausted_key_id = uuid::Uuid::new_v4();
    let available_key_id = uuid::Uuid::new_v4();
    let route = db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.minimaxi.com".to_string(),
        api_key: "exhausted-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: vec![
            endpoint_key(endpoint_id, exhausted_key_id, "exhausted", 0),
            endpoint_key(endpoint_id, available_key_id, "available", 1),
        ],
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Responses,
        upstream_model: None,
        responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::Minimax,
        service_tier: db::MinimaxServiceTier::Standard,
    };
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            TokenPlanUsageResponse {
                provider: db::EndpointProvider::Minimax,
                provider_region: db::EndpointRegion::Cn,
                keys: vec![
                    token_plan_key_usage(exhausted_key_id, "exhausted", 0.0),
                    token_plan_key_usage(available_key_id, "available", 100.0),
                ],
            },
        )
        .await;

    let request = BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"MiniMax-M3","input":"ping"}"#.to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(1),
        client_key_hash: Some("client-key".to_string()),
        request_user_agent: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    };
    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &request,
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(available_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("available"));
}

fn endpoint_key(
    endpoint_id: uuid::Uuid,
    key_id: uuid::Uuid,
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
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn token_plan_key_usage(
    key_id: uuid::Uuid,
    key_label: &str,
    remaining_percent: f64,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: key_label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        model_remains: vec![TokenPlanModelUsage {
            model_name: "general".to_string(),
            interval: Some(window(remaining_percent)),
            weekly: Some(window(remaining_percent)),
        }],
    }
}

fn window(remaining_percent: f64) -> TokenPlanWindowUsage {
    TokenPlanWindowUsage {
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
