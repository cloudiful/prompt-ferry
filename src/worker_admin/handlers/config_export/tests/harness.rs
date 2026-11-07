//! Shared SQLite admin-state harness for the configuration-export tests.
//!
//! A standalone `StandaloneConfigStore` backs the config repository and the
//! user store while the PostgreSQL pool stays lazy, so the export exercises the
//! real router, `ensure_admin`, and the SQLite snapshot reader without a
//! database server.

use super::*;

use axum::body::Body;
use axum::http::Request;
use std::sync::Arc;

use crate::db;
use crate::standalone_config::StandaloneConfigStore;

use crate::worker_admin::handlers::config_test_support as shared;
pub(super) use crate::worker_admin::handlers::config_test_support::{
    attach_session, close_state, export_request, manager,
};

pub(super) const PASSPHRASE: &str = "config-export-passphrase";

pub(super) async fn test_state() -> (AdminState, Arc<StandaloneConfigStore>, std::path::PathBuf) {
    shared::test_state("export").await
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
