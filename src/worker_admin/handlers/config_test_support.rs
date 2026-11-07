//! Shared test-only request and state helpers for the administrator
//! configuration export, import, and audit harnesses.

use super::*;

use axum::body::Body;
use axum::http::Request;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;

use crate::db::ConfigRepository;
use crate::db::config_repository::archive::encode_archive;
use crate::db::config_repository::build_config_snapshot;
use crate::relay_secrets::RelaySecretManager;
use crate::replay_cache::ReplayCache;
use crate::standalone_config::StandaloneConfigStore;
use crate::worker_admin_state::AdminStateInit;

#[allow(dead_code)]
mod test_db_url {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_db_url.rs"
    ));
}

pub(super) fn manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([11_u8; 32])).expect("manager")
}

pub(super) async fn test_state(
    prefix: &str,
) -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    let path =
        std::env::temp_dir().join(format!("prompt-ferry-{prefix}-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
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

pub(super) async fn export_archive(state: &AdminState, passphrase: &str) -> Vec<u8> {
    let snapshot = build_config_snapshot(
        &state.config_repository,
        &state.user_store,
        state.relay_secret_manager.as_ref(),
    )
    .await
    .expect("snapshot");
    encode_archive(passphrase, snapshot.manifest.backend_kind, snapshot)
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
