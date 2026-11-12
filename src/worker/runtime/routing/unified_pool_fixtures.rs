//! Shared fixtures for the unified key-pool routing tests: endpoint/key
//! builders and OpencodeGo quota snapshots used by both the selection and the
//! failover suites.

use chrono::Utc;

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
        local_today_tokens: None,
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
                quota_status: None,
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
                deepseek_balance: None,
                quota_windows: None,
            })
            .collect(),
    }
}

/// One OpencodeGo key whose windows carry explicit reset deltas, so the
/// urgency factor has real `resets_in_seconds` data to work with.

pub(super) fn target(
    endpoint_id: uuid::Uuid,
    position: i32,
    key_lb_enabled: bool,
    keys: Vec<db::EndpointApiKey>,
) -> db::ModelRouteCandidateTarget {
    db::ModelRouteCandidateTarget {
        embedding_dimensions_override: None,
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
        target_native_api: crate::config::NativeApi::Responses,
        position,
        enabled: true,
        upstream_model: None,
        provider: db::EndpointProvider::OpencodeGo,
        service_tier: None,
        proxy_url: None,

        proxy_url_override: None,
        active_windows: None,
        endpoint_active_windows: None,
        dev_system_normalize: false,
        thinking_downgrade_enabled: false,
        thinking_effort_override: None,
        compact_mode: crate::db::CompactMode::Passthrough,
        has_oauth_token: false,
    }
}

pub(super) fn candidate(targets: Vec<db::ModelRouteCandidateTarget>) -> db::ModelRouteCandidate {
    db::ModelRouteCandidate {
        rule_id: uuid::Uuid::new_v4(),
        scope: "admin".to_string(),
        owner_user_id: None,
        model_pattern: "opencode-go".to_string(),
        routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
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
