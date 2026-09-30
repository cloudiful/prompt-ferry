//! Shared SQLite admin-state harness for the configuration-archive audit tests.
//!
//! A standalone `StandaloneConfigStore` backs the config repository and the
//! user store while the PostgreSQL pool stays lazy, so the export/import
//! handlers under test write their audit rows into the real standalone audit
//! table and the list endpoint reads them through the real router.

use super::*;

use axum::body::Body;
use axum::http::Request;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;

use crate::db::config_repository::archive::encode_archive;
use crate::db::config_repository::build_config_snapshot;
use crate::db::{self, ConfigRepository};
use crate::relay_secrets::RelaySecretManager;
use crate::replay_cache::ReplayCache;
use crate::standalone_config::StandaloneConfigStore;
use crate::worker_admin_state::AdminStateInit;
use crate::worker_admin_types::{
    RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser, UsageRetentionSettings,
};

pub(super) const PASSPHRASE: &str = "config-audit-passphrase";

pub(super) fn manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([11_u8; 32])).expect("manager")
}

pub(super) async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("prompt-ferry-audit-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let user_store = db::UserStore::sqlite(store.pool().clone());
    user_store
        .bootstrap_admin("admin", "admin-password")
        .await
        .expect("bootstrap admin");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")
        .expect("lazy pool");
    let state = AdminState::new(AdminStateInit {
        pool: pool.clone(),
        lease_pool: pool.clone(),
        replay_cache: ReplayCache::for_tests(),
        configured_relays: Vec::new(),
        managed_mode: false,
        relay_secret_manager: Some(manager()),
        redaction_enabled: false,
        model_route_whitelist_enabled: true,
        request_content_logging: RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        },
        usage_retention: UsageRetentionSettings::default(),
        raw_payload_store: None,
        stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
        llm_review_settings: crate::llm_review::LlmReviewSettings::default(),
        mcp_catalog_cache: crate::mcp::McpCatalogCache::new(),
        mcp_catalog_service: crate::mcp::McpCatalogService::new(
            pool.clone(),
            crate::mcp::McpCatalogCache::new(),
        ),
        mcp_session_store: None,
        mcp_allowed_origins: Vec::new(),
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
            std::time::Duration::from_secs(60),
        ),
    })
    .with_user_store(user_store)
    .with_config_repository(ConfigRepository::sqlite(store.clone(), manager()));
    (state, store, path)
}

pub(super) async fn attach_session(state: &AdminState, id: &str, is_admin: bool) {
    state
        .replay_cache
        .write_session(
            id,
            &SessionUser {
                user_id: 1,
                login_name: "admin".to_string(),
                display_name: "Admin".to_string(),
                is_admin,
            },
        )
        .await
        .expect("session");
}

pub(super) async fn close_state(store: Arc<StandaloneConfigStore>, path: std::path::PathBuf) {
    store.pool().close().await;
    let _ = std::fs::remove_file(path);
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

pub(super) fn export_request(session: Option<&str>, passphrase: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/config-export")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(session) = session {
        builder = builder.header(header::COOKIE, format!("prompt_ferry_session={session}"));
    }
    builder
        .body(Body::from(
            serde_json::json!({ "passphrase": passphrase }).to_string(),
        ))
        .expect("request")
}

pub(super) fn import_request(
    uri: &str,
    session: Option<&str>,
    passphrase: &str,
    archive: &[u8],
) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(session) = session {
        builder = builder.header(header::COOKIE, format!("prompt_ferry_session={session}"));
    }
    builder
        .body(Body::from(
            serde_json::json!({
                "passphrase": passphrase,
                "archive_base64": STANDARD.encode(archive),
            })
            .to_string(),
        ))
        .expect("request")
}

/// Seal the state's current configuration and return the archive bytes.
pub(super) async fn export_archive(state: &AdminState) -> Vec<u8> {
    let snapshot = build_config_snapshot(
        &state.config_repository,
        &state.user_store,
        state.relay_secret_manager.as_ref(),
    )
    .await
    .expect("snapshot");
    encode_archive(PASSPHRASE, snapshot.manifest.backend_kind, snapshot)
        .expect("encode archive")
        .bytes
}

pub(super) async fn json_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes"),
    )
    .expect("json body")
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
