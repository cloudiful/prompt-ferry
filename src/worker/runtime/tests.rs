use super::*;
use crate::{
    db::{self, ModelRouteCandidate, ModelRouteCandidateTarget},
    mcp::targeting::McpRequestMetadata,
    redact::{self, RedactionConfig, TEST_REDACTION_LOCK},
    worker::runtime::context::{ResponseLimits, RuntimeServices},
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
};
use anyhow::anyhow;
use base64::Engine as _;
use chrono::Utc;
use redactor::RedactionRules;

use super::connect::is_expected_relay_disconnect;
use super::routing::{
    materialize_route_api_key_selection, rendezvous_target, select_route_for_candidate,
    stable_unrecognized_session_target,
};

#[test]
fn builds_upstream_url_without_double_slash() {
    assert_eq!(
        upstream_url("https://api.example.com/", "/v1/models"),
        "https://api.example.com/v1/models"
    );
}

#[test]
fn rejects_missing_upstream_key() {
    let config = WorkerConfig {
        upstream_api_key: String::new(),
        worker_token: "token".to_string(),
        ..WorkerConfig::default()
    };
    assert!(validate_config(&config).is_err());
}

#[test]
fn rejects_upstream_base_url_with_v1_path() {
    let config = WorkerConfig {
        upstream_base_url: "https://api.example.com/v1/".to_string(),
        upstream_api_key: "key".to_string(),
        worker_token: "token".to_string(),
        ..WorkerConfig::default()
    };
    assert!(validate_config(&config).is_err());
}

#[test]
fn managed_mode_requires_relay_secret_master_key() {
    let config = WorkerConfig {
        database_url: "postgres://postgres:postgres@localhost/prompt_ferry".to_string(),
        upstream_api_key: String::new(),
        relay_urls: Vec::new(),
        worker_token: "token".to_string(),
        relay_secret_master_key: String::new(),
        ..WorkerConfig::default()
    };
    assert!(validate_config(&config).is_err());
}

#[test]
fn managed_mode_allows_zero_relays_with_master_key() {
    let config = WorkerConfig {
        database_url: "postgres://postgres:postgres@localhost/prompt_ferry".to_string(),
        upstream_api_key: String::new(),
        relay_urls: Vec::new(),
        worker_token: "token".to_string(),
        relay_secret_master_key: base64::engine::general_purpose::STANDARD.encode([5_u8; 32]),
        ..WorkerConfig::default()
    };
    assert!(validate_config(&config).is_ok());
}

#[test]
fn safe_error_redacts_and_truncates() {
    let _guard = TEST_REDACTION_LOCK.lock().expect("test lock poisoned");
    redact::apply_config(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            secret: true,
            ..RedactionRules::default()
        },
        ..Default::default()
    })
    .expect("config should apply");
    let message = safe_error(
        &anyhow!("API_TOKEN=sk_live_1234567890ABCDEFghij failed"),
        true,
        None,
    );

    assert!(!message.contains("sk_live_1234567890ABCDEFghij"));
    assert!(message.len() <= 243);
}

#[test]
fn safe_error_preserves_error_chain() {
    let err = anyhow!("connection reset by peer")
        .context("error decoding response body")
        .context("failed reading upstream response");

    assert_eq!(
        safe_error(&err, false, None),
        "failed reading upstream response: error decoding response body: connection reset by peer"
    );
}

#[test]
fn request_execution_context_preserves_usage_fields() {
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-5".to_string()),
        None,
        Some("desktop".to_string()),
        Some(7),
        uuid::Uuid::new_v4(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_seq: Some(3),
            conversation_source: "responses".to_string(),
            ..RequestPromptLog::default()
        },
    );
    let request = BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"gpt-5","input":"hello"}"#.to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(7),
        client_key_hash: None,
        request_user_agent: Some("Codex Desktop".to_string()),
        http_request_content_encoding: Some("gzip".to_string()),
        http_request_compressed: true,
        http_request_compressed_bytes: Some(32),
        http_request_decompressed_bytes: Some(48),
        http_request_compression_ratio: Some(1.5),
    };

    let log = request_ctx.ai_usage_log(&request, None);
    assert_eq!(log.request_id, request_ctx.request_id);
    assert_eq!(log.client_key_label.as_deref(), Some("desktop"));
    assert_eq!(log.model.as_deref(), Some("gpt-5"));
    assert_eq!(log.conversation_seq, Some(3));
    assert_eq!(log.http_request_content_encoding.as_deref(), Some("gzip"));
    assert_eq!(log.http_request_compressed_bytes, Some(32));
}

#[test]
fn mcp_usage_log_respects_request_content_logging_mode() {
    let request_ctx = RequestExecutionContext::for_mcp(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some(7),
        uuid::Uuid::new_v4(),
        RequestPromptLog::default(),
    );
    let request = BufferedMcpRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        server_name: Some("catalog".to_string()),
        method: "POST".to_string(),
        path: "/mcp".to_string(),
        headers: Vec::new(),
        body: br#"{"method":"tools/call"}"#.to_vec(),
        user_id: Some(7),
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    };
    let metadata = McpRequestMetadata {
        server_name: Some("catalog".to_string()),
        protocol_method: Some("tools/call".to_string()),
        operation_name: Some("search".to_string()),
        selected_token_slot: None,
        request_raw_json: Some(serde_json::json!({ "secret": "raw-token" })),
    };

    let off = request_ctx.mcp_usage_log(
        &request,
        &metadata,
        &RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        },
        false,
    );
    assert!(off.request_raw_json.is_none());

    let normalized_only = request_ctx.mcp_usage_log(
        &request,
        &metadata,
        &RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::NormalizedOnly,
            raw_retention_days: 3,
        },
        false,
    );
    assert!(normalized_only.request_raw_json.is_none());

    let normalized_and_raw = request_ctx.mcp_usage_log(
        &request,
        &metadata,
        &RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::NormalizedAndRaw,
            raw_retention_days: 3,
        },
        false,
    );
    assert_eq!(
        normalized_and_raw.request_raw_json,
        Some(serde_json::json!({ "secret": "raw-token" }))
    );
}

#[test]
fn classifies_tls_close_notify_eof_as_expected_disconnect() {
    let err = anyhow!("websocket read failed").context(
        "IO error: peer closed connection without sending TLS close_notify: https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof"
    );
    assert!(is_expected_relay_disconnect(&err));
}

#[test]
fn does_not_classify_protocol_decode_failure_as_expected_disconnect() {
    let err = anyhow!("failed to decode relay message");
    assert!(!is_expected_relay_disconnect(&err));
}

#[test]
fn preferred_target_is_stable_for_same_key() {
    let candidate = sample_candidate();
    let first = rendezvous_target(&candidate, Some("key-a")).unwrap();
    let second = rendezvous_target(&candidate, Some("key-a")).unwrap();
    assert_eq!(first.endpoint_id, second.endpoint_id);
}

#[test]
fn session_affinity_without_stable_identifier_falls_back_to_first_target() {
    let candidate = session_affinity_candidate();
    let request = BufferedBridgeRequest {
        client_key_hash: None,
        ..sample_request()
    };

    let selected =
        stable_unrecognized_session_target(&candidate, &request, &RequestPromptLog::default())
            .expect("preferred target");

    assert_eq!(selected.endpoint_id, candidate.targets[0].endpoint_id);
}

#[tokio::test]
async fn session_affinity_continuation_selects_preferred_target() {
    let runtime_state = WorkerRuntimeState::default();
    let out_tx = super::context::BridgeSender::test_sender();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        runtime_state.clone(),
        ResponseLimits::default(),
    );
    let mut candidate = session_affinity_candidate();
    candidate.session_affinity_lock_after_turns = 1;
    let preferred = candidate.targets[0].endpoint_id;
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_seq: Some(2),
            preferred_endpoint_id: Some(preferred),
            ..RequestPromptLog::default()
        },
    );

    let route = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("selected route");

    assert_eq!(route.route.route_id, preferred);
}

#[tokio::test]
async fn session_affinity_early_turn_uses_least_loaded_endpoint() {
    let runtime_state = WorkerRuntimeState::default();
    let out_tx = super::context::BridgeSender::test_sender();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        runtime_state.clone(),
        ResponseLimits::default(),
    );
    let candidate = session_affinity_candidate();
    let busy = candidate.targets[0].endpoint_id;
    let idle = candidate.targets[1].endpoint_id;
    let _busy_guard = runtime_state.reserve_endpoint(busy).unwrap();
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_seq: Some(5),
            preferred_endpoint_id: Some(busy),
            ..RequestPromptLog::default()
        },
    );

    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("selected route");

    assert_eq!(selected.route.route_id, idle);
    assert_eq!(
        selected.route.route_selection_reason,
        db::RouteSelectionReason::SessionLoadBalance
    );
    assert_eq!(runtime_state.endpoint_active_count(idle), 1);
    drop(selected.load_guard);
    assert_eq!(runtime_state.endpoint_active_count(idle), 0);
}

#[tokio::test]
async fn session_affinity_locks_after_configured_turn() {
    let runtime_state = WorkerRuntimeState::default();
    let out_tx = super::context::BridgeSender::test_sender();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        runtime_state.clone(),
        ResponseLimits::default(),
    );
    let candidate = session_affinity_candidate();
    let preferred = candidate.targets[0].endpoint_id;
    let _preferred_busy_guard = runtime_state.reserve_endpoint(preferred).unwrap();
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_seq: Some(6),
            preferred_endpoint_id: Some(preferred),
            ..RequestPromptLog::default()
        },
    );

    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("selected route");

    assert_eq!(selected.route.route_id, preferred);
    assert_eq!(
        selected.route.route_selection_reason,
        db::RouteSelectionReason::SessionAffinity
    );
}

#[tokio::test]
async fn force_passthrough_disables_early_session_load_balancing() {
    let runtime_state = WorkerRuntimeState::default();
    let out_tx = super::context::BridgeSender::test_sender();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        runtime_state.clone(),
        ResponseLimits::default(),
    );
    let mut candidate = session_affinity_candidate();
    candidate.targets[1].responses_continuation_policy =
        crate::db::ResponsesContinuationPolicy::ForcePassthrough;
    let preferred = candidate.targets[0].endpoint_id;
    let _preferred_busy_guard = runtime_state.reserve_endpoint(preferred).unwrap();
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_seq: Some(2),
            preferred_endpoint_id: Some(preferred),
            ..RequestPromptLog::default()
        },
    );

    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("selected route");

    assert_eq!(selected.route.route_id, preferred);
    assert_eq!(
        selected.route.route_selection_reason,
        db::RouteSelectionReason::SessionAffinity
    );
}

#[tokio::test]
async fn selected_route_carries_target_upstream_model_override() {
    let runtime_state = WorkerRuntimeState::default();
    let out_tx = super::context::BridgeSender::test_sender();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        runtime_state.clone(),
        ResponseLimits::default(),
    );
    let mut candidate = sample_candidate();
    let preferred = rendezvous_target(&candidate, Some("key-a"))
        .expect("preferred target")
        .target_id;
    let preferred_target = candidate
        .targets
        .iter_mut()
        .find(|target| target.target_id == preferred)
        .expect("preferred target exists");
    preferred_target.upstream_model = Some("gpt-4.1-mini".to_string());
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("auto".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog::default(),
    );

    let route = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("selected route");

    assert_eq!(route.route.upstream_model.as_deref(), Some("gpt-4.1-mini"));
}

fn sample_candidate() -> ModelRouteCandidate {
    let endpoint_a = uuid::Uuid::new_v4();
    let endpoint_b = uuid::Uuid::new_v4();
    ModelRouteCandidate {
        rule_id: uuid::Uuid::new_v4(),
        scope: "admin".to_string(),
        owner_user_id: None,
        model_pattern: "gpt-*".to_string(),
        routing_strategy: crate::db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
        session_affinity_lock_after_turns: 5,
        daily_max_requests: None,
        monthly_max_requests: None,
        updated_at: Utc::now(),
        targets: vec![
            ModelRouteCandidateTarget {
                target_id: uuid::Uuid::new_v4(),
                endpoint_id: endpoint_a,
                endpoint_name: "primary".to_string(),
                base_url: "https://a.example.com".to_string(),
                api_key: "key-a".to_string(),
                api_keys: vec![db::EndpointApiKey {
                    key_id: uuid::Uuid::new_v4(),
                    endpoint_id: endpoint_a,
                    key_label: "primary".to_string(),
                    api_key: "key-a".to_string(),
                    position: 0,
                    enabled: true,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                }],
                key_lb_enabled: false,
                native_api: crate::config::NativeApi::Responses,
                position: 0,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            },
            ModelRouteCandidateTarget {
                target_id: uuid::Uuid::new_v4(),
                endpoint_id: endpoint_b,
                endpoint_name: "secondary".to_string(),
                base_url: "https://b.example.com".to_string(),
                api_key: "key-b".to_string(),
                api_keys: vec![db::EndpointApiKey {
                    key_id: uuid::Uuid::new_v4(),
                    endpoint_id: endpoint_b,
                    key_label: "secondary".to_string(),
                    api_key: "key-b".to_string(),
                    position: 0,
                    enabled: true,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                }],
                key_lb_enabled: false,
                native_api: crate::config::NativeApi::Responses,
                position: 1,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            },
        ],
    }
}

fn session_affinity_candidate() -> ModelRouteCandidate {
    ModelRouteCandidate {
        routing_strategy: crate::db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
        ..sample_candidate()
    }
}

fn sample_request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"gpt-5.4","input":"ping"}"#.to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(1),
        client_key_hash: Some("key-a".to_string()),
        request_user_agent: Some("Codex Desktop".to_string()),
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    }
}

fn sample_request_with_session_header(session_id: &str) -> BufferedBridgeRequest {
    let mut request = sample_request();
    request.headers = vec![("X-Session-Id".to_string(), session_id.to_string())];
    request
}

#[test]
fn endpoint_key_lb_uses_stable_selection_and_first_key_fallback() {
    let endpoint_id = uuid::Uuid::new_v4();
    let primary_key_id = uuid::Uuid::new_v4();
    let secondary_key_id = uuid::Uuid::new_v4();
    let route = db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.example.com".to_string(),
        api_key: "primary-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: vec![
            db::EndpointApiKey {
                key_id: primary_key_id,
                endpoint_id,
                key_label: "primary".to_string(),
                api_key: "primary-key".to_string(),
                position: 0,
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            db::EndpointApiKey {
                key_id: secondary_key_id,
                endpoint_id,
                key_label: "secondary".to_string(),
                api_key: "secondary-key".to_string(),
                position: 1,
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        ],
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Responses,
        upstream_model: None,
        responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
        route_selection_reason: db::RouteSelectionReason::Default,
    };
    let sticky_prompt_log = RequestPromptLog {
        request_conversation_key: Some("conv_123".to_string()),
        ..RequestPromptLog::default()
    };

    let first = materialize_route_api_key_selection(&route, &sample_request(), &sticky_prompt_log);
    let second = materialize_route_api_key_selection(&route, &sample_request(), &sticky_prompt_log);
    assert_eq!(first.selection.secret, second.selection.secret);
    assert_eq!(first.selection.key_id, second.selection.key_id);
    assert_eq!(first.selection.key_label, second.selection.key_label);
    assert!(!first.invalid_conversation_override);

    let request_without_identity = BufferedBridgeRequest {
        client_key_hash: None,
        ..sample_request()
    };
    let fallback = materialize_route_api_key_selection(
        &route,
        &request_without_identity,
        &RequestPromptLog::default(),
    );
    assert_eq!(fallback.selection.secret, "primary-key");
    assert_eq!(fallback.selection.key_id, Some(primary_key_id));

    let disabled = db::RouteConfig {
        key_lb_enabled: false,
        ..route
    };
    let disabled_pick =
        materialize_route_api_key_selection(&disabled, &sample_request(), &sticky_prompt_log);
    assert_eq!(disabled_pick.selection.secret, "primary-key");
    assert_eq!(disabled_pick.selection.key_id, Some(primary_key_id));
}

#[test]
fn endpoint_key_override_wins_and_invalid_override_falls_back() {
    let endpoint_id = uuid::Uuid::new_v4();
    let fixed_key_id = uuid::Uuid::new_v4();
    let route = db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.example.com".to_string(),
        api_key: "primary-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: vec![
            db::EndpointApiKey {
                key_id: uuid::Uuid::new_v4(),
                endpoint_id,
                key_label: "primary".to_string(),
                api_key: "primary-key".to_string(),
                position: 0,
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            db::EndpointApiKey {
                key_id: fixed_key_id,
                endpoint_id,
                key_label: "fixed".to_string(),
                api_key: "fixed-key".to_string(),
                position: 1,
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        ],
        key_lb_enabled: true,
        native_api: crate::config::NativeApi::Responses,
        upstream_model: None,
        responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
        route_selection_reason: db::RouteSelectionReason::Default,
    };
    let fixed_prompt_log = RequestPromptLog {
        conversation_override_endpoint_key_id: Some(fixed_key_id),
        ..RequestPromptLog::default()
    };
    let fixed = materialize_route_api_key_selection(&route, &sample_request(), &fixed_prompt_log);
    assert_eq!(fixed.selection.secret, "fixed-key");
    assert_eq!(fixed.selection.key_id, Some(fixed_key_id));
    assert!(!fixed.invalid_conversation_override);

    let disabled_prompt_log = RequestPromptLog {
        conversation_override_endpoint_key_id: Some(fixed_key_id),
        ..RequestPromptLog::default()
    };
    let disabled_route = db::RouteConfig {
        api_keys: route
            .api_keys
            .iter()
            .map(|key| db::EndpointApiKey {
                enabled: key.key_id != fixed_key_id,
                ..key.clone()
            })
            .collect(),
        ..route.clone()
    };
    let disabled = materialize_route_api_key_selection(
        &disabled_route,
        &sample_request(),
        &disabled_prompt_log,
    );
    assert_eq!(disabled.selection.secret, "primary-key");
    assert!(disabled.invalid_conversation_override);

    let deleted = RequestPromptLog {
        conversation_override_endpoint_key_id: Some(uuid::Uuid::new_v4()),
        ..RequestPromptLog::default()
    };
    let deleted_route = db::RouteConfig {
        key_lb_enabled: false,
        ..route.clone()
    };
    let deleted = materialize_route_api_key_selection(&deleted_route, &sample_request(), &deleted);
    assert_eq!(deleted.selection.secret, "primary-key");
    assert!(deleted.invalid_conversation_override);

    let cross_endpoint = db::RouteConfig {
        api_keys: vec![db::EndpointApiKey {
            key_id: fixed_key_id,
            endpoint_id: uuid::Uuid::new_v4(),
            key_label: "other".to_string(),
            api_key: "other-key".to_string(),
            position: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }],
        ..route
    };
    let cross_endpoint =
        materialize_route_api_key_selection(&cross_endpoint, &sample_request(), &fixed_prompt_log);
    assert_eq!(cross_endpoint.selection.secret, "primary-key");
    assert!(cross_endpoint.invalid_conversation_override);
}

#[test]
fn session_affinity_unrecognized_header_is_stable() {
    let candidate = session_affinity_candidate();
    let request = sample_request_with_session_header("sess-123");

    let first =
        stable_unrecognized_session_target(&candidate, &request, &RequestPromptLog::default())
            .expect("preferred target")
            .endpoint_id;
    let second =
        stable_unrecognized_session_target(&candidate, &request, &RequestPromptLog::default())
            .expect("preferred target")
            .endpoint_id;

    assert_eq!(first, second);
}
