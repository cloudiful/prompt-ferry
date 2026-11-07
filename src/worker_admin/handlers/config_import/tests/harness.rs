//! Shared SQLite admin-state harness for the configuration-import tests.
//!
//! Each state owns its own `StandaloneConfigStore` so a source archive can be
//! restored into a second, empty instance. The PostgreSQL pool stays lazy, so
//! every assertion runs against the real router and `ensure_admin` chain
//! without a database server.

use super::*;

use std::sync::Arc;

use crate::db;
use crate::db::config_repository::archive::{decode_archive, encode_archive};
use crate::db::config_repository::build_config_snapshot;
use crate::db::config_repository::{ConfigBackendKind, ConfigSnapshot};
use crate::standalone_config::StandaloneConfigStore;

use crate::worker_admin::handlers::config_test_support as shared;
pub(super) use crate::worker_admin::handlers::config_test_support::{
    attach_session, close_state, export_archive, import_request, json_body,
};

pub(super) const PASSPHRASE: &str = "config-import-passphrase";

pub(super) async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    shared::test_state("import").await
}

/// Current payload fingerprint of a state, used to prove preview wrote nothing.
pub(super) async fn fingerprint(state: &AdminState) -> String {
    build_config_snapshot(
        &state.config_repository,
        &state.user_store,
        state.relay_secret_manager.as_ref(),
    )
    .await
    .expect("snapshot")
    .manifest
    .payload_fingerprint
}

/// Full snapshot of a state's current configuration.
pub(super) async fn snapshot(state: &AdminState) -> ConfigSnapshot {
    build_config_snapshot(
        &state.config_repository,
        &state.user_store,
        state.relay_secret_manager.as_ref(),
    )
    .await
    .expect("snapshot")
}

/// Decode an archive produced by [`export_archive`].
pub(super) fn decode(archive: &[u8]) -> ConfigSnapshot {
    decode_archive(PASSPHRASE, archive).expect("decoded archive")
}

/// Re-seal a decoded snapshot into archive bytes for the given backend.
pub(super) fn seal(snapshot: ConfigSnapshot, backend: ConfigBackendKind) -> Vec<u8> {
    encode_archive(PASSPHRASE, backend, snapshot)
        .expect("encoded archive")
        .bytes
}

/// Seed an endpoint (with API key, proxy, OAuth, admin key), a client key, a
/// setting, and an MCP server with bearer tokens.
pub(super) async fn seed_configuration(state: &AdminState) -> Uuid {
    let endpoint_id = Uuid::new_v4();
    state
        .config_repository
        .create_endpoint(
            endpoint_id,
            db::EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: "imported upstream".to_string(),
                provider: db::EndpointProvider::OpenAi,
                provider_region: None,
                service_tier: Default::default(),
                base_url: "https://upstream.example".to_string(),
                native_api: crate::config::NativeApi::Chat,
                native_api_source: crate::config::NativeApiSource::Manual,
                api_key: "endpoint-secret".to_string(),
                api_keys: vec![db::EndpointApiKeyCreate {
                    key_label: "primary".to_string(),
                    api_key: "endpoint-secret".to_string(),
                    position: 0,
                    enabled: true,
                    key_id: None,
                }],
                key_lb_enabled: false,
                enabled: true,
                proxy_url: Some("http://proxy.example:8080".to_string()),
                active_windows: None,
            },
            false,
        )
        .await
        .expect("create endpoint");
    state
        .config_repository
        .set_endpoint_admin_api_key(endpoint_id, Some("admin-key"))
        .await
        .expect("set admin api key");
    state
        .config_repository
        .set_endpoint_oauth_token(
            endpoint_id,
            Some(db::EndpointOAuthTokenSet {
                access_token: "oauth-access".to_string(),
                refresh_token: "oauth-refresh".to_string(),
                expires_at: None,
            }),
        )
        .await
        .expect("set oauth token");
    state
        .config_repository
        .create_client_key(1, Some("codex"), true)
        .await
        .expect("create client key");
    state
        .config_repository
        .set_json_setting("redaction_config", &serde_json::json!({ "enabled": true }))
        .await
        .expect("set setting");
    state
        .config_repository
        .create_mcp_server(
            Uuid::new_v4(),
            db::McpServerInput {
                scope: "admin".to_string(),
                owner_user_id: None,
                source_endpoint_id: None,
                name: "imported-mcp".to_string(),
                aggregate_naming_mode: "passthrough_preferred".to_string(),
                transport: "http".to_string(),
                provider_kind: Some(db::MCP_PROVIDER_CONTEXT7.to_string()),
                url: Some("https://mcp.example/mcp".to_string()),
                command: None,
                args: serde_json::json!([]),
                env_json: serde_json::json!({}),
                bearer_tokens_json: serde_json::json!([
                    { "token": "bearer-one", "enabled": true },
                    { "token": "bearer-two", "enabled": false }
                ]),
                http_headers_json: serde_json::json!({}),
                auth_mode: db::MCP_AUTH_MODE_BEARER.to_string(),
                basic_username: None,
                basic_password: None,
                proxy_url: None,
                tool_filter_mode: "blacklist".to_string(),
                allowed_tools: serde_json::json!([]),
                disabled_tools: serde_json::json!([]),
                disabled_resources: serde_json::json!([]),
                enabled: true,
                timeout_ms: 30_000,
                lifecycle_policy: "auto".to_string(),
                lifecycle_manual_protocol_version: None,
            },
        )
        .await
        .expect("create mcp server");
    endpoint_id
}
