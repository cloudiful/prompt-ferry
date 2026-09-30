//! Shared SQLite admin-state harness for the configuration-import tests.
//!
//! Each state owns its own `StandaloneConfigStore` so a source archive can be
//! restored into a second, empty instance. The PostgreSQL pool stays lazy, so
//! every assertion runs against the real router and `ensure_admin` chain
//! without a database server.

use super::*;

use axum::body::Body;
use axum::http::Request;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;

use crate::db::config_repository::archive::{decode_archive, encode_archive};
use crate::db::config_repository::build_config_snapshot;
use crate::db::config_repository::{ConfigBackendKind, ConfigSnapshot};
use crate::db::{self, ConfigRepository};
use crate::relay_secrets::RelaySecretManager;
use crate::replay_cache::ReplayCache;
use crate::standalone_config::StandaloneConfigStore;
use crate::worker_admin_state::AdminStateInit;
use crate::worker_admin_types::{
    RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser, UsageRetentionSettings,
};

pub(super) const PASSPHRASE: &str = "config-import-passphrase";

pub(super) fn manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager")
}

pub(super) async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("prompt-ferry-import-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    db::migrate_standalone(store.pool())
        .await
        .expect("sqlite migrations");
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

/// Read a response body as JSON.
pub(super) async fn json_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body bytes"),
    )
    .expect("json body")
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
