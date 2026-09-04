use super::*;
use crate::{
    db::{self, ModelRouteCandidate, ModelRouteCandidateTarget},
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::targeting::McpRequestMetadata,
    mcp::{McpCatalogCache, McpCatalogService},
    redact_test_support::secret_redaction,
    replay_cache::ReplayCache,
    worker::runtime::context::{ResponseLimits, RuntimeServices},
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, UsageRetentionSettings,
    },
};
use anyhow::anyhow;
use base64::Engine as _;
use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

use super::connect::is_expected_relay_disconnect;
use super::routing::{
    RouteAffinityError, materialize_route_api_key_selection, rendezvous_target,
    select_route_for_candidate,
};

#[test]
fn builds_upstream_url_without_double_slash() {
    assert_eq!(
        upstream_url("https://api.example.com/", "/v1/models"),
        "https://api.example.com/v1/models"
    );
}

#[test]
fn first_startup_validation_allows_missing_upstream_key() {
    let config = WorkerConfig {
        upstream_api_key: String::new(),
        worker_token: String::new(),
        ..WorkerConfig::default()
    };
    // Fresh startup may have no upstream API key, no worker token, and no
    // manually configured encryption key; the Admin setup flow completes
    // configuration later.
    assert!(validate_config(&config).is_ok());
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
fn managed_mode_rejects_invalid_encryption_key() {
    let config = WorkerConfig {
        database_url: "postgres://postgres:postgres@localhost/prompt_ferry".to_string(),
        upstream_api_key: String::new(),
        relay_urls: Vec::new(),
        worker_token: "token".to_string(),
        relay_secret_master_key: "definitely-not-base64!!".to_string(),
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
    let _guard = secret_redaction();
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
fn rendezvous_target_matches_full_order_for_empty_single_and_multiple_targets() {
    let empty = ModelRouteCandidate {
        targets: Vec::new(),
        ..sample_candidate()
    };
    assert!(rendezvous_target(&empty, Some("key-a")).is_none());
    assert!(crate::routing::choose_preferred_target(&empty, Some("key-a")).is_none());

    let mut single = sample_candidate();
    single.targets.truncate(1);
    let single_expected = crate::routing::ordered_route_targets(&single, Some("key-a"))
        .first()
        .map(|target| target.endpoint_id);
    assert_eq!(
        rendezvous_target(&single, Some("key-a")).map(|target| target.endpoint_id),
        single_expected
    );

    let multiple = sample_candidate();
    let multiple_expected = crate::routing::ordered_route_targets(&multiple, Some("key-a"))
        .first()
        .map(|target| target.endpoint_id);
    assert_eq!(
        rendezvous_target(&multiple, Some("key-a")).map(|target| target.endpoint_id),
        multiple_expected
    );
    assert_eq!(
        crate::routing::choose_preferred_target(&multiple, Some("key-a"))
            .map(|target| target.endpoint_id),
        multiple_expected
    );
}

#[test]
fn rendezvous_target_preserves_position_and_input_order_ties() {
    let mut by_position = sample_candidate();
    by_position.targets[1].endpoint_id = by_position.targets[0].endpoint_id;
    by_position.targets[1].position = by_position.targets[0].position + 1;
    assert_eq!(
        rendezvous_target(&by_position, Some("key-a"))
            .unwrap()
            .position,
        by_position.targets[0].position
    );

    let mut by_input_order = by_position.clone();
    by_input_order.targets[1].position = by_input_order.targets[0].position;
    assert_eq!(
        rendezvous_target(&by_input_order, Some("key-a"))
            .unwrap()
            .endpoint_id,
        by_input_order.targets[0].endpoint_id
    );
}

#[tokio::test]
async fn session_affinity_uses_preferred_endpoint() {
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), ReplayCache::for_tests());
    let candidate = session_affinity_candidate();
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
            conversation_seq: Some(1),
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
async fn session_affinity_uses_rendezvous_for_new_identity() {
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), ReplayCache::for_tests());
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
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

    let expected = rendezvous_target(&candidate, Some(&format!("conversation:{conversation_id}")))
        .unwrap()
        .endpoint_id;
    assert_eq!(selected.route.route_id, expected);
    assert_eq!(
        selected.route.route_selection_reason,
        db::RouteSelectionReason::SessionAffinity
    );
}

#[test]
fn session_affinity_rendezvous_distributes_independent_identities() {
    let candidate = session_affinity_candidate();
    let endpoints = (1..=64)
        .map(|value| {
            let conversation_id = uuid::Uuid::from_u128(value);
            rendezvous_target(&candidate, Some(&format!("conversation:{conversation_id}")))
                .expect("candidate should have a target")
                .endpoint_id
        })
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(endpoints.len(), candidate.targets.len());
}

#[tokio::test]
async fn session_affinity_requires_stable_identity() {
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
    let request = BufferedBridgeRequest {
        client_key_hash: None,
        ..sample_request()
    };
    let request_ctx = RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            ..RequestPromptLog::default()
        },
    );

    let result = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &request,
        1,
        Some("key-a"),
    )
    .await;
    let selected = match result {
        Ok(_) => panic!("missing session identity should be rejected"),
        Err(error) => error,
    };
    assert_eq!(
        selected
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_identity_required")
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

pub(super) fn session_affinity_services(
    runtime_state: WorkerRuntimeState,
    replay_cache: ReplayCache,
) -> RuntimeServices {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")
        .expect("lazy pool");
    let catalog_cache = McpCatalogCache::new();
    let admin_state = AdminState::new(AdminStateInit {
        pool: pool.clone(),
        lease_pool: pool.clone(),
        replay_cache,
        configured_relays: Vec::new(),
        managed_mode: false,
        relay_secret_manager: None,
        redaction_enabled: false,
        model_route_whitelist_enabled: true,
        request_content_logging: RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        },
        usage_retention: UsageRetentionSettings::default(),
        raw_payload_store: None,
        stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
        llm_review_settings: LlmReviewSettings::default(),
        mcp_catalog_cache: catalog_cache.clone(),
        mcp_catalog_service: McpCatalogService::new(pool.clone(), catalog_cache),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        mcp_quota_valkey: crate::mcp::McpQuotaValkey::new(),
        endpoint_model_cache: EndpointModelCache::new(Duration::from_secs(60)),
    });
    RuntimeServices::new(
        Some(admin_state),
        super::context::BridgeSender::test_sender(),
        reqwest::Client::new(),
        runtime_state,
        ResponseLimits::default(),
    )
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

pub(super) fn session_affinity_candidate() -> ModelRouteCandidate {
    ModelRouteCandidate {
        routing_strategy: crate::db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
        ..sample_candidate()
    }
}

pub(super) fn sample_request() -> BufferedBridgeRequest {
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
fn raw_object_store_config_validates_s3_requirements() {
    use crate::raw_payload_store::{RawObjectStoreBackend, RawObjectStoreConfig};

    let mut config = RawObjectStoreConfig {
        backend: RawObjectStoreBackend::S3,
        s3_bucket: "".to_string(),
        s3_region: "auto".to_string(),
        ..RawObjectStoreConfig::default()
    };
    assert!(config.validate().is_err());

    config.s3_bucket = "my-bucket".to_string();
    config.s3_region = "".to_string();
    assert!(config.validate().is_err());

    config.s3_region = "auto".to_string();
    assert!(config.validate().is_ok());
}

#[test]
fn raw_object_store_disabled_and_local_build_correctly() {
    use crate::raw_payload_store::{RawObjectStoreBackend, RawObjectStoreConfig};

    let disabled = RawObjectStoreConfig {
        backend: RawObjectStoreBackend::Disabled,
        ..RawObjectStoreConfig::default()
    };
    let store = disabled.build_store().expect("build disabled");
    assert!(store.is_none());

    let local = RawObjectStoreConfig {
        backend: RawObjectStoreBackend::Local,
        local_dir: std::env::temp_dir()
            .join(format!("pf-test-local-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string(),
        ..RawObjectStoreConfig::default()
    };
    let store = local.build_store().expect("build local");
    assert!(store.is_some());
    if let Some(s) = store {
        // Local store should be validated successfully.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { s.validate_candidate().await.expect("local validation") });
    }
    let _ = std::fs::remove_dir_all(local.local_dir);
}

#[test]
fn raw_object_store_persisted_encrypts_and_redacts() {
    use crate::raw_payload_store::{
        RawObjectStoreBackend, RawObjectStoreConfig, RawObjectStorePersisted,
    };
    use base64::Engine as _;

    let manager = crate::relay_secrets::RelaySecretManager::from_base64(
        &base64::engine::general_purpose::STANDARD.encode([9_u8; 32]),
    )
    .expect("manager");

    let config = RawObjectStoreConfig {
        backend: RawObjectStoreBackend::S3,
        s3_endpoint: "https://s3.example.com".to_string(),
        s3_bucket: "bucket".to_string(),
        s3_region: "us-east-1".to_string(),
        s3_prefix: "pf/raw".to_string(),
        s3_allow_http: false,
        s3_path_style: true,
        s3_access_key: Some("AKIAEXAMPLE".to_string()),
        s3_secret_key: Some("secret123".to_string()),
        local_dir: "".to_string(),
    };

    let persisted = RawObjectStorePersisted::from_config(&config, &manager).expect("persist");
    assert!(persisted.has_access_key());
    assert!(persisted.has_secret_key());
    // Encrypted form should not contain plaintext.
    let json = serde_json::to_value(&persisted).expect("json");
    let json_str = json.to_string();
    assert!(!json_str.contains("AKIAEXAMPLE"));
    assert!(!json_str.contains("secret123"));

    let decrypted = persisted.into_config(&manager).expect("decrypt");
    assert_eq!(decrypted.s3_access_key.as_deref(), Some("AKIAEXAMPLE"));
    assert_eq!(decrypted.s3_secret_key.as_deref(), Some("secret123"));

    let redacted = decrypted.redacted_response();
    assert!(redacted.has_s3_access_key);
    assert!(redacted.has_s3_secret_key);
    // Response JSON must not contain secrets.
    let resp_json = serde_json::to_value(&redacted).expect("resp json");
    assert!(!resp_json.to_string().contains("AKIAEXAMPLE"));
}

#[test]
fn raw_object_store_secret_keep_clear_semantics() {
    use crate::worker_admin_types::RawObjectStoreSecretPatch;

    // Keep/clear semantics are validated via the handler helper; ensure the
    // enum serializes as expected for the OpenAPI contract.
    let keep = RawObjectStoreSecretPatch::Keep;
    let clear = RawObjectStoreSecretPatch::Clear;
    let replace = RawObjectStoreSecretPatch::Replace {
        value: "new-secret".to_string(),
    };
    assert_eq!(serde_json::to_value(&keep).unwrap()["mode"], "keep");
    assert_eq!(serde_json::to_value(&clear).unwrap()["mode"], "clear");
    assert_eq!(
        serde_json::to_value(&replace).unwrap()["value"],
        "new-secret"
    );
}

#[test]
fn raw_object_store_path_style_defaults_to_true_and_legacy_json_loads_as_path_style() {
    use crate::raw_payload_store::{
        RawObjectStoreBackend, RawObjectStoreConfig, RawObjectStorePersisted,
        RawObjectStoreSettingsResponse,
    };
    use serde_json::json;

    // Fresh defaults must be path-style to preserve RustFS compatibility.
    assert!(RawObjectStoreConfig::default().s3_path_style);
    assert!(crate::config::WorkerConfig::default().raw_object_store_path_style);

    // Legacy persisted JSON without s3_path_style (pre-migration) must still
    // deserialize as path-style.
    let legacy_config: RawObjectStoreConfig = serde_json::from_value(json!({
        "backend": "s3",
        "local_dir": "",
        "s3_endpoint": "https://s3.example.com",
        "s3_bucket": "bucket",
        "s3_region": "auto",
        "s3_prefix": "prompt-ferry/raw",
        "s3_allow_http": false,
        "s3_access_key": "AKIAEXAMPLE",
        "s3_secret_key": "secret"
    }))
    .expect("legacy config deserializes");
    assert!(legacy_config.s3_path_style);

    let legacy_persisted: RawObjectStorePersisted = serde_json::from_value(json!({
        "backend": "s3",
        "local_dir": "",
        "s3_endpoint": "https://s3.example.com",
        "s3_bucket": "bucket",
        "s3_region": "auto",
        "s3_prefix": "prompt-ferry/raw",
        "s3_allow_http": false,
        "s3_access_key": null,
        "s3_secret_key": null
    }))
    .expect("legacy persisted deserializes");
    assert!(legacy_persisted.s3_path_style);

    let legacy_response: RawObjectStoreSettingsResponse = serde_json::from_value(json!({
        "backend": "s3",
        "local_dir": "",
        "s3_endpoint": "https://s3.example.com",
        "s3_bucket": "bucket",
        "s3_region": "auto",
        "s3_prefix": "prompt-ferry/raw",
        "s3_allow_http": false,
        "has_s3_access_key": false,
        "has_s3_secret_key": false
    }))
    .expect("legacy response deserializes");
    assert!(legacy_response.s3_path_style);

    let legacy_request: crate::worker_admin_types::RawObjectStoreSettingsRequest =
        serde_json::from_value(json!({
            "backend": "s3",
            "local_dir": "",
            "s3_endpoint": "https://s3.example.com",
            "s3_bucket": "bucket",
            "s3_region": "auto",
            "s3_prefix": "prompt-ferry/raw",
            "s3_allow_http": false
        }))
        .expect("legacy request deserializes");
    assert!(legacy_request.s3_path_style);

    // WorkerConfig legacy JSON without the new field also defaults to true.
    let legacy_worker: crate::config::WorkerConfig =
        serde_json::from_value(json!({"raw_object_store_bucket": "bucket"}))
            .expect("legacy worker config deserializes");
    assert!(legacy_worker.raw_object_store_path_style);

    // Explicit false must round-trip and map through worker -> config -> persisted.
    let explicit = RawObjectStoreConfig {
        backend: RawObjectStoreBackend::S3,
        s3_bucket: "bucket".to_string(),
        s3_region: "auto".to_string(),
        s3_path_style: false,
        ..RawObjectStoreConfig::default()
    };
    let json = serde_json::to_value(&explicit).expect("serialize explicit");
    assert_eq!(json["s3_path_style"], false);
    let round_trip: RawObjectStoreConfig = serde_json::from_value(json).expect("round trip");
    assert!(!round_trip.s3_path_style);

    let worker = crate::config::WorkerConfig {
        raw_object_store_bucket: "bucket".to_string(),
        raw_object_store_path_style: false,
        ..crate::config::WorkerConfig::default()
    };
    let mapped = RawObjectStoreConfig::from_worker_config(&worker);
    assert!(!mapped.s3_path_style);
    assert!(!mapped.redacted_response().s3_path_style);
}
