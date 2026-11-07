//! Shared SQLite admin-state harness for the configuration-archive audit tests.
//!
//! A standalone `StandaloneConfigStore` backs the config repository and the
//! user store while the PostgreSQL pool stays lazy, so the export/import
//! handlers under test write their audit rows into the real standalone audit
//! table and the list endpoint reads them through the real router.

use super::*;

use axum::body::Body;
use axum::http::Request;
use std::sync::Arc;

use crate::db;
use crate::standalone_config::StandaloneConfigStore;

use crate::worker_admin::handlers::config_test_support as shared;
pub(super) use crate::worker_admin::handlers::config_test_support::{
    attach_session, close_state, export_archive, export_request, import_request, json_body,
};

pub(super) const PASSPHRASE: &str = "config-audit-passphrase";

pub(super) async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    shared::test_state("audit").await
}

fn session_request(uri: &str, session: Option<&str>) -> axum::http::request::Builder {
    let mut builder = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(session) = session {
        builder = builder.header(header::COOKIE, format!("prompt_ferry_session={session}"));
    }
    builder
}

pub(super) fn audit_request(session: Option<&str>, query: &str) -> Request<Body> {
    session_request(&format!("/api/v1/admin/config-audit{query}"), session)
        .body(Body::empty())
        .expect("request")
}

/// The audit page as read straight from the standalone table.
pub(super) async fn audit_page(
    state: &AdminState,
) -> crate::db::config_repository::ConfigAuditPage {
    state
        .config_repository
        .list_config_audit(0, 100)
        .await
        .expect("audit page")
}

/// Seed one endpoint with an API key, a client key, a setting, and an MCP
/// server carrying bearer tokens, so an audited archive has real domains and
/// real secret values to leak if the trail were careless.
pub(super) async fn seed_configuration(state: &AdminState) {
    let endpoint_id = Uuid::new_v4();
    state
        .config_repository
        .create_endpoint(
            endpoint_id,
            db::EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: "audited upstream".to_string(),
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
                proxy_url: None,
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
                name: "audited-mcp".to_string(),
                aggregate_naming_mode: "passthrough_preferred".to_string(),
                transport: "http".to_string(),
                provider_kind: Some(db::MCP_PROVIDER_CONTEXT7.to_string()),
                url: Some("https://mcp.example/mcp".to_string()),
                command: None,
                args: serde_json::json!([]),
                env_json: serde_json::json!({}),
                bearer_tokens_json: serde_json::json!([
                    { "token": "bearer-one", "enabled": true }
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
}
