//! Shared fixtures for the unified key-pool routing tests: endpoint/key
//! builders and OpencodeGo quota snapshots used by both the selection and the
//! failover suites.

use chrono::Utc;

use super::super::prompt_log::RequestPromptLog;
use super::{select_route_for_candidate, session_affinity_tests::request_context};
use crate::{
    db,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage, TokenPlanUsageResponse},
};

pub(super) fn endpoint_key(
    endpoint_id: uuid::Uuid,
    key_id: uuid::Uuid,
    label: &str,
    position: i32,
) -> db::EndpointApiKey {
    db::EndpointApiKey {
        key_id,
        endpoint_id,
        key_label: label.to_string(),
        api_key: format!("{label}-key"),
        position,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

pub(super) fn opencode_go_window(percent: f64) -> OpencodeGoWindowUsage {
    OpencodeGoWindowUsage {
        status: None,
        percent: Some(percent),
        resets_at: None,
    }
}

/// `keys` pairs each key with its remaining percent; the wire window stores
/// the used percent, so it is inverted here.
pub(super) fn opencode_go_usage(keys: &[(uuid::Uuid, &str, f64)]) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        provider: db::EndpointProvider::OpencodeGo,
        provider_region: None,
        keys: keys
            .iter()
            .map(|(key_id, key_label, remaining)| TokenPlanKeyUsage {
                key_id: *key_id,
                key_label: (*key_label).to_string(),
                ok: true,
                status: Some(200),
                error_code: None,
                error_message: None,
                model_remains: Vec::new(),
                balances: None,
                five_hour: None,
                weekly: None,
                opencodego_rolling: None,
                opencodego_weekly: Some(opencode_go_window(100.0 - *remaining)),
                opencodego_monthly: None,
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
            })
            .collect(),
    }
}

/// One OpencodeGo key whose windows carry explicit reset deltas, so the
/// urgency factor has real `resets_in_seconds` data to work with.
pub(super) fn opencode_go_urgent_key(
    key_id: uuid::Uuid,
    key_label: &str,
    rolling: Option<(f64, chrono::Duration)>,
    weekly: Option<(f64, chrono::Duration)>,
    monthly: Option<(f64, chrono::Duration)>,
) -> TokenPlanKeyUsage {
    let window = |value: Option<(f64, chrono::Duration)>| {
        value.map(|(used, resets_in)| OpencodeGoWindowUsage {
            status: None,
            percent: Some(used),
            resets_at: Some(Utc::now() + resets_in),
        })
    };
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
        opencodego_rolling: window(rolling),
        opencodego_weekly: window(weekly),
        opencodego_monthly: window(monthly),
        openrouter_balance: None,
        openrouter_spend: None,
        glm_five_hour: None,
        glm_weekly: None,
    }
}

pub(super) fn opencode_go_urgent_usage(keys: Vec<TokenPlanKeyUsage>) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        provider: db::EndpointProvider::OpencodeGo,
        provider_region: None,
        keys,
    }
}

pub(super) fn target(
    endpoint_id: uuid::Uuid,
    position: i32,
    key_lb_enabled: bool,
    keys: Vec<db::EndpointApiKey>,
) -> db::ModelRouteCandidateTarget {
    db::ModelRouteCandidateTarget {
        target_id: uuid::Uuid::new_v4(),
        endpoint_id,
        endpoint_name: format!("endpoint-{position}"),
        base_url: format!("https://endpoint-{position}.example.com"),
        api_key: keys
            .first()
            .map(|key| key.api_key.clone())
            .unwrap_or_default(),
        api_keys: keys,
        key_lb_enabled,
        native_api: crate::config::NativeApi::Responses,
        position,
        enabled: true,
        upstream_model: None,
        provider: db::EndpointProvider::OpencodeGo,
        service_tier: db::MinimaxServiceTier::Standard,
    }
}

pub(super) fn candidate(targets: Vec<db::ModelRouteCandidateTarget>) -> db::ModelRouteCandidate {
    db::ModelRouteCandidate {
        rule_id: uuid::Uuid::new_v4(),
        scope: "admin".to_string(),
        owner_user_id: None,
        model_pattern: "opencode-go".to_string(),
        routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
        daily_max_requests: None,
        monthly_max_requests: None,
        updated_at: Utc::now(),
        targets,
    }
}

pub(super) fn request(client_key: &str) -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"opencode-go","input":"ping"}"#.to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(1),
        client_key_hash: Some(client_key.to_string()),
        request_user_agent: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    }
}

pub(super) async fn route_selected_endpoint(
    services: &crate::worker::runtime::context::RuntimeServices,
    candidate: &db::ModelRouteCandidate,
    client_key: &str,
) -> (uuid::Uuid, Option<uuid::Uuid>, String) {
    let request_ctx = request_context(uuid::Uuid::new_v4(), RequestPromptLog::default());
    let selected = select_route_for_candidate(
        services,
        &request_ctx,
        candidate,
        &request(client_key),
        1,
        Some(client_key),
    )
    .await
    .expect("route selection")
    .expect("route must be selected");
    (
        selected.route.route_id,
        selected.route.endpoint_key_id,
        selected.route.api_key,
    )
}
