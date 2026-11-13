use uuid::Uuid;

use super::*;
use crate::config::{BridgeEncryptionMode, NativeApi, NativeApiSource, TlsMode};

fn sample_config() -> StandaloneConfig {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    StandaloneConfig {
        relays: vec![ManagedRelayConfig {
            relay_id: Uuid::new_v4(),
            name: "local relay".to_string(),
            relay_url: "ws://127.0.0.1:8788/ws/worker".to_string(),
            enabled: true,
            tls_mode: TlsMode::Off,
            bridge_encryption_mode: BridgeEncryptionMode::Required,
            relay_ca_pem: Some("CA PEM".to_string()),
            client_cert_pem: Some("CERT PEM".to_string()),
            client_key_pem: Some("CLIENT KEY".to_string()),
            bridge_encryption_key: Some("BRIDGE KEY".to_string()),
        }],
        endpoints: vec![ProviderEndpointConfig {
            endpoint_id,
            name: "upstream".to_string(),
            provider: EndpointProvider::Generic,
            provider_region: Some(EndpointRegion::Global),
            service_tier: None,
            base_url: "https://api.example.test".to_string(),
            native_api: NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            key_lb_enabled: true,
            enabled: true,
            mcp_enabled: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            api_key: "endpoint-secret".to_string(),
            api_keys: vec![EndpointApiKeyConfig {
                key_id,
                endpoint_id,
                key_label: "primary".to_string(),
                api_key: "endpoint-key-secret".to_string(),
                position: 0,
                enabled: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            proxy_url: None,
            active_windows: None,
        }],
        routes: vec![ModelRouteConfig {
            rule_id: Uuid::new_v4(),
            scope: RouteScope::Admin,
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: RoutingStrategy::ResponsesSessionAffinity,
            enabled: true,
            targets: vec![ModelRouteTargetConfig {
                embedding_dimensions_override: None,
                target_id: Uuid::new_v4(),
                endpoint_id,
                position: 0,
                enabled: true,
                upstream_model: Some("provider-model".to_string()),
                native_api: crate::config::NativeApi::Auto,
                proxy_url_override: None,
                active_windows: None,
                dev_system_normalize: false,
                thinking_effort_override: None,
                compact_mode: "passthrough".to_string(),
                thinking_downgrade_enabled: false,
                service_tier: None,
            }],
        }],
        client_keys: vec![ClientKeyConfig {
            key_id: Uuid::new_v4(),
            user_id: 7,
            key_prefix: "pfy_test".to_string(),
            label: "test client".to_string(),
            secret: "client-secret".to_string(),
            enabled: true,
        }],
        settings: vec![SettingConfig {
            key: "redaction_config".to_string(),
            version: 1,
            value: serde_json::json!({"enabled": true}),
        }],
    }
}

#[test]
fn endpoint_oauth_token_debug_redacts_both_secrets() {
    // Issue #599 R2a: refresh tokens must never reach logs.
    let record = super::models::EndpointOAuthTokenConfig {
        endpoint_id: Uuid::new_v4(),
        access_token: "access-secret-value".to_string(),
        refresh_token: "refresh-secret-value".to_string(),
        expires_at: None,
    };
    let rendered = format!("{record:?}");
    assert!(
        !rendered.contains("access-secret-value"),
        "access token must not leak into Debug: {rendered}"
    );
    assert!(
        !rendered.contains("refresh-secret-value"),
        "refresh token must not leak into Debug: {rendered}"
    );
}

#[test]
fn public_debug_output_redacts_all_standalone_secrets() {
    let config = sample_config();
    let output = format!("{config:?}");
    for secret in [
        "CA PEM",
        "CERT PEM",
        "CLIENT KEY",
        "BRIDGE KEY",
        "endpoint-secret",
        "endpoint-key-secret",
        "client-secret",
    ] {
        assert!(!output.contains(secret), "debug output leaked {secret:?}");
    }
    assert!(output.contains("local relay"));
    assert!(output.contains("[REDACTED;"));

    let seed = BootstrapSeed {
        relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
        tls_mode: TlsMode::Off,
        relay_ca_pem: Some("seed CA".to_string()),
        client_cert_pem: Some("seed cert".to_string()),
        client_key_pem: Some("seed key".to_string()),
        bridge_encryption_mode: BridgeEncryptionMode::Required,
        bridge_encryption_key: Some("seed bridge".to_string()),
        upstream_base_url: "https://api.example.test".to_string(),
        upstream_api_key: "seed endpoint secret".to_string(),
        upstream_native_api: NativeApi::Responses,
    };
    let seed_output = format!("{seed:?}");
    for secret in [
        "seed CA",
        "seed cert",
        "seed key",
        "seed bridge",
        "seed endpoint secret",
    ] {
        assert!(
            !seed_output.contains(secret),
            "debug output leaked {secret:?}"
        );
    }
}

// ---- Phase 1A standalone usage ledger tests ----

fn sample_usage_record(
    request_id: Uuid,
    path: &str,
    model: Option<&str>,
) -> StandaloneUsageSummaryRecord {
    StandaloneUsageSummaryRecord {
        request_id,
        event_kind: "request".to_string(),
        category: "ai".to_string(),
        state: "completed".to_string(),
        path: path.to_string(),
        recorded_at: chrono::Utc::now(),
        status: Some(200),
        ok: Some(true),
        duration_ms: Some(123),
        ttft_ms: Some(45),
        model: model.map(str::to_string),
        requested_model: model.map(str::to_string),
        upstream_model: model.map(str::to_string),
        endpoint_id: Some(Uuid::new_v4()),
        endpoint_key_id: Some(Uuid::new_v4()),
        model_route_rule_id: Some(Uuid::new_v4()),
        mcp_server_id: None,
        input_tokens: Some(11),
        output_tokens: Some(22),
        total_tokens: Some(33),
        cached_tokens: Some(0),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        error_code: None,
        failure_family: None,
        redaction_applied: true,
        redaction_findings_count: 1,
        redaction_replacements_count: 1,
        redaction_types: vec!["email".to_string()],
        redaction_fields: vec!["messages.email".to_string()],
        route_selection_reason: "default".to_string(),
        user_id: None,
        client_key_id: None,
        client_key_label: None,
        request_user_agent: None,
        endpoint_key_label: None,
        mcp_server_name: None,
        mcp_protocol_method: None,
        mcp_operation_name: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
        conversation_source: "none".to_string(),
        client_installation_id: None,
        session_header_id: None,
        session_parent_id: None,
        provider_response_id: None,
        provider_conversation_key: None,
        request_storage_mode: "full".to_string(),
        error_message: None,
        request_has_previous_response_id: false,
        request_previous_response_id: None,
        request_previous_response_parent_found: None,
        request_conversation_key: None,
        request_conversation_parent_found: None,
        upstream_redaction_enabled: false,
        response_capture_truncated: false,
    }
}

// ---- Phase 1C-a standalone usage metadata tests ----

fn ai_metadata_record(request_id: Uuid) -> StandaloneUsageSummaryRecord {
    let mut record = sample_usage_record(request_id, "/v1/responses", Some("gpt-5"));
    record.user_id = Some(42);
    record.client_key_id = Some(7);
    record.client_key_label = Some("test client".to_string());
    record.request_user_agent = Some("prompt-ferry-cli/0.4".to_string());
    record.endpoint_key_label = Some("primary".to_string());
    record.http_request_content_encoding = Some("gzip".to_string());
    record.http_request_compressed = true;
    record.http_request_compressed_bytes = Some(2048);
    record.http_request_decompressed_bytes = Some(8192);
    record.http_request_compression_ratio = Some(0.25);
    record.conversation_source = "responses".to_string();
    record.client_installation_id = Some("install-abc".to_string());
    record.session_header_id = Some("ses_child".to_string());
    record.session_parent_id = Some("ses_parent".to_string());
    record.provider_response_id = Some("resp_001".to_string());
    record.provider_conversation_key = Some("conv-001".to_string());
    record.request_storage_mode = "summary".to_string();
    record.error_message = Some("upstream returned HTTP 502".to_string());
    record.request_has_previous_response_id = true;
    record.request_previous_response_id = Some("resp_000".to_string());
    record.request_previous_response_parent_found = Some(true);
    record.request_conversation_key = Some("conv-001".to_string());
    record.request_conversation_parent_found = Some(false);
    record.upstream_redaction_enabled = true;
    record.response_capture_truncated = true;
    record
}

#[tokio::test]
async fn metadata_record_debug_output_omits_raw_body_and_secret_field_names() {
    let record = ai_metadata_record(Uuid::new_v4());
    let debug = format!("{record:?}");
    for forbidden in [
        "request_body",
        "response_body",
        "raw_body",
        "session_secret",
        "request_raw_json",
        "response_raw_body",
        "upstream_redacted_request_json",
    ] {
        assert!(
            !debug.contains(forbidden),
            "metadata record debug output must not contain {forbidden:?}; got: {debug}"
        );
    }
    // The Phase 1C-a metadata fields must be visible in the debug output so
    // operators can inspect them.
    for visible in [
        "user_id",
        "client_key_id",
        "client_key_label",
        "request_user_agent",
        "endpoint_key_label",
        "mcp_server_name",
        "mcp_protocol_method",
        "mcp_operation_name",
        "http_request_content_encoding",
        "http_request_compressed",
        "http_request_compressed_bytes",
        "http_request_decompressed_bytes",
        "http_request_compression_ratio",
        "conversation_source",
        "client_installation_id",
        "provider_response_id",
        "provider_conversation_key",
        "request_storage_mode",
        "error_message",
        "request_has_previous_response_id",
        "request_previous_response_id",
        "request_conversation_key",
        "upstream_redaction_enabled",
        "response_capture_truncated",
    ] {
        assert!(
            debug.contains(visible),
            "metadata record debug output must contain {visible:?}"
        );
    }
}

// ---- Phase 1C-b standalone replay snapshot tests ----

#[test]
fn setting_model_projection_preserves_the_full_i64_version_range() {
    let version = i64::from(i32::MAX) + 12_345;
    let model = crate::db::entities::sqlite::standalone_settings::Model {
        setting_key: "large-version".to_string(),
        value_version: version,
        value_json: "{\"large\":true}".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
    };
    let setting = super::rows::setting_from_model(&model).expect("project the setting row");
    assert_eq!(setting.version, version);
    assert_eq!(setting.value, serde_json::json!({"large": true}));
}

#[test]
fn client_key_model_projection_rejects_out_of_range_key_version() {
    let model = crate::db::entities::sqlite::standalone_client_keys::Model {
        key_id: Uuid::new_v4().to_string(),
        user_id: 1,
        key_prefix: "pfy_probe".to_string(),
        label: "probe".to_string(),
        enabled: 1,
        secret_ciphertext: vec![1, 2, 3],
        secret_nonce: vec![0; 12],
        secret_key_version: i32::from(i16::MAX) + 1,
        created_at: "2026-01-01 00:00:00".to_string(),
        updated_at: "2026-01-01 00:00:00".to_string(),
    };
    let error = super::rows::client_key_from_model(&model)
        .expect_err("an out-of-range key version must fail closed");
    assert!(error.to_string().contains("key_version"));
}
