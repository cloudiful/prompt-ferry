//! Shared SQLite admin-state harness for the configuration-export tests.
//!
//! A standalone `StandaloneConfigStore` backs the config repository and the
//! user store while the PostgreSQL pool stays lazy, so the export exercises the
//! real router, `ensure_admin`, and the SQLite snapshot reader without a
//! database server.

use super::*;

use axum::body::Body;
use axum::http::Request;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;

use crate::{
    db::{self, ConfigRepository},
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    standalone_config::StandaloneConfigStore,
    worker_admin_state::AdminStateInit,
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};

pub(super) const PASSPHRASE: &str = "config-export-passphrase";

#[allow(dead_code)]
mod test_db_url {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_db_url.rs"
    ));
}

pub(super) fn manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([9_u8; 32])).expect("manager")
}

pub(super) async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("prompt-ferry-export-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    db::migrate_standalone(store.pool())
        .await
        .expect("sqlite migrations");
    let user_store = db::UserStore::sqlite(store.pool().clone());
    user_store
        .bootstrap_admin("admin", "admin-password")
        .await
        .expect("bootstrap admin");
    let pool = test_db_url::lazy_test_pool();
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

/// Release the temporary SQLite store so the file can be removed.
pub(super) async fn close_state(store: Arc<StandaloneConfigStore>, path: std::path::PathBuf) {
    store.pool().close().await;
    let _ = std::fs::remove_file(path);
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

pub(super) fn metadata_request(session: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/v1/admin/config-export/metadata")
        .header(header::COOKIE, format!("prompt_ferry_session={session}"))
        .body(Body::empty())
        .expect("request")
}

/// Seed one endpoint (with its API key) plus a client key and a setting, so the
/// archive has restorable configuration to carry.
pub(super) async fn seed_configuration(state: &AdminState) -> Uuid {
    let endpoint_id = Uuid::new_v4();
    state
        .config_repository
        .create_endpoint(
            endpoint_id,
            db::EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: "exported upstream".to_string(),
                provider: db::EndpointProvider::Generic,
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
        .create_client_key(1, Some("codex"), true)
        .await
        .expect("create client key");
    state
        .config_repository
        .set_json_setting("redaction_config", &serde_json::json!({ "enabled": true }))
        .await
        .expect("set setting");
    endpoint_id
}
