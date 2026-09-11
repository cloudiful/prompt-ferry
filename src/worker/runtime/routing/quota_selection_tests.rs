use super::select_route_for_candidate;
use super::selection::materialize_route_api_key_selection_with_quota;
use super::session_affinity_tests::request_context;
use crate::{
    db,
    replay_cache::ReplayCache,
    worker::runtime::prompt_log::RequestPromptLog,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker::runtime::tests::{session_affinity_candidate, session_affinity_services},
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{
        CommandCodeBalances, CommandCodeWindowUsage, TokenPlanKeyUsage, TokenPlanModelUsage,
        TokenPlanUsageResponse, TokenPlanWindowUsage,
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
                provider_region: Some(db::EndpointRegion::Cn),
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

// CommandCode routing consumption (issue #184 P5): keys carry no
// `model_remains`; the quota cache weights them by the tighter of the
// 5-hour/weekly USD windows instead.
fn command_code_window(remaining_percent: f64) -> CommandCodeWindowUsage {
    CommandCodeWindowUsage {
        used: 10.0 - remaining_percent / 10.0,
        cap: 10.0,
        used_percent: Some(100.0 - remaining_percent),
        remaining_percent: Some(remaining_percent),
        reset_at: None,
    }
}

fn command_code_key_usage(
    key_id: uuid::Uuid,
    key_label: &str,
    five_hour: Option<f64>,
    weekly: Option<f64>,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: key_label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        model_remains: Vec::new(),
        balances: Some(CommandCodeBalances {
            monthly_credits: 80.0,
            purchased_credits: 0.0,
            free_credits: 0.0,
            remaining_credits: 80.0,
        }),
        five_hour: five_hour.map(command_code_window),
        weekly: weekly.map(command_code_window),
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
        openrouter_balance: None,
        openrouter_spend: None,
        glm_five_hour: None,
        glm_weekly: None,
    }
}

fn command_code_route(
    endpoint_id: uuid::Uuid,
    exhausted_key_id: uuid::Uuid,
    available_key_id: uuid::Uuid,
) -> db::RouteConfig {
    db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.commandcode.ai".to_string(),
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
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::CommandCode,
        service_tier: db::MinimaxServiceTier::Standard,
    }
}

fn command_code_request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"command-code","input":"ping"}"#.to_vec(),
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

#[tokio::test]
async fn quota_key_lb_skips_command_code_key_with_no_remaining_window() {
    let endpoint_id = uuid::Uuid::new_v4();
    let exhausted_key_id = uuid::Uuid::new_v4();
    let available_key_id = uuid::Uuid::new_v4();
    let route = command_code_route(endpoint_id, exhausted_key_id, available_key_id);
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            TokenPlanUsageResponse {
                provider: db::EndpointProvider::CommandCode,
                provider_region: None,
                keys: vec![
                    command_code_key_usage(exhausted_key_id, "exhausted", Some(0.0), Some(0.0)),
                    command_code_key_usage(available_key_id, "available", Some(100.0), Some(100.0)),
                ],
            },
        )
        .await;

    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &command_code_request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(available_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("available"));
}

#[tokio::test]
async fn quota_key_lb_uses_tighter_command_code_window_and_single_arm() {
    let endpoint_id = uuid::Uuid::new_v4();
    let tight_key_id = uuid::Uuid::new_v4();
    let single_arm_key_id = uuid::Uuid::new_v4();
    let route = command_code_route(endpoint_id, tight_key_id, single_arm_key_id);
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            TokenPlanUsageResponse {
                provider: db::EndpointProvider::CommandCode,
                provider_region: None,
                keys: vec![
                    // Tighter arm is 0%: skipped even though weekly is 100%.
                    command_code_key_usage(tight_key_id, "exhausted", Some(0.0), Some(100.0)),
                    // Single weekly arm still weights the key.
                    command_code_key_usage(single_arm_key_id, "available", None, Some(60.0)),
                ],
            },
        )
        .await;

    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &command_code_request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(single_arm_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("available"));
}

#[tokio::test]
async fn quota_key_lb_still_routes_payg_command_code_key_without_windows() {
    // PAYG keys degrade to None windows (no quota signal); routing falls
    // back to the stable candidate instead of dropping the key.
    let endpoint_id = uuid::Uuid::new_v4();
    let payg_key_id = uuid::Uuid::new_v4();
    let route = db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.commandcode.ai".to_string(),
        api_key: "payg-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: vec![endpoint_key(endpoint_id, payg_key_id, "payg", 0)],
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: db::RouteSelectionReason::Default,
        provider: db::EndpointProvider::CommandCode,
        service_tier: db::MinimaxServiceTier::Standard,
    };
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            endpoint_id,
            TokenPlanUsageResponse {
                provider: db::EndpointProvider::CommandCode,
                provider_region: None,
                keys: vec![command_code_key_usage(payg_key_id, "payg", None, None)],
            },
        )
        .await;

    let selected = materialize_route_api_key_selection_with_quota(
        &route,
        &command_code_request(),
        &RequestPromptLog::default(),
        Some(&cache),
    );

    assert_eq!(selected.selection.key_id, Some(payg_key_id));
    assert_eq!(selected.selection.key_label.as_deref(), Some("payg"));
}

#[tokio::test]
async fn unified_pool_skips_a_target_with_no_remaining_quota() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
    let mut candidate = session_affinity_candidate();
    candidate.routing_strategy = db::ModelRouteRoutingStrategy::ClientKeyRendezvous;
    let exhausted_endpoint = candidate.targets[0].endpoint_id;
    let exhausted_key_id = candidate.targets[0].api_keys[0].key_id;
    services
        .admin_state()
        .expect("admin state")
        .token_plan_quota
        .store_for_test(
            exhausted_endpoint,
            TokenPlanUsageResponse {
                provider: db::EndpointProvider::Minimax,
                provider_region: Some(db::EndpointRegion::Cn),
                keys: vec![token_plan_key_usage(exhausted_key_id, "primary", 0.0)],
            },
        )
        .await;

    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog::default(),
    );
    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &command_code_request(),
        1,
        Some("client-key"),
    )
    .await
    .expect("route selection")
    .expect("route must be selected");
    assert_ne!(
        selected.route.route_id, exhausted_endpoint,
        "a pool unit with no remaining quota must be skipped",
    );
    assert_eq!(
        selected.route.route_selection_reason,
        db::RouteSelectionReason::Default
    );
}
