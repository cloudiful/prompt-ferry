//! Shared fixture and request builders for the endpoint create/update
//! regression tests (issue #599 R2e.2). Lives under `tests/support/` and is
//! pulled in with `#[path]` so each test binary keeps its own copy.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use prompt_ferry::{
    db::{self, ConfigRepository},
    endpoint_models::EndpointModelCache,
    llm_review::LlmReviewSettings,
    mcp::{McpCatalogCache, McpCatalogService},
    replay_cache::ReplayCache,
    standalone_config::StandaloneConfigStore,
    worker_admin_state::{AdminState, AdminStateInit},
    worker_admin_types::{
        RequestContentLoggingMode, RequestContentLoggingResponse, SessionUser,
        UsageRetentionSettings,
    },
};
use prompt_ferry_runtime_env::relay_secrets::RelaySecretManager;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

pub const SESSION_KEY: &str = "endpoint-create-regression";

fn test_manager() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([23_u8; 32])).expect("test manager")
}

pub struct Fixture {
    pub state: AdminState,
    store: Arc<StandaloneConfigStore>,
    path: PathBuf,
}

impl Fixture {
    pub async fn open() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-endpoint-regression-{}.sqlite",
            Uuid::new_v4()
        ));
        let store = Arc::new(StandaloneConfigStore::open(&path).await?);
        let user_store = db::UserStore::sqlite(store.pool().clone());
        user_store
            .bootstrap_admin("endpoint-admin", "endpoint-password")
            .await?;
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/prompt_ferry")?;
        let state = AdminState::new(AdminStateInit {
            pool: pool.clone(),
            lease_pool: pool.clone(),
            replay_cache: ReplayCache::for_tests(),
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
            mcp_catalog_cache: McpCatalogCache::new(),
            mcp_catalog_service: McpCatalogService::new(pool, McpCatalogCache::new()),
            mcp_session_store: None,
            mcp_allowed_origins: Vec::new(),
            endpoint_model_cache: EndpointModelCache::new(std::time::Duration::from_secs(60)),
        })
        .with_user_store(user_store)
        .with_config_repository(ConfigRepository::sqlite(store.clone(), test_manager()));
        state
            .replay_cache
            .write_session(
                SESSION_KEY,
                &SessionUser {
                    user_id: 1,
                    login_name: "endpoint-admin".to_string(),
                    display_name: "Endpoint Admin".to_string(),
                    is_admin: true,
                },
            )
            .await?;
        Ok(Self { state, store, path })
    }

    pub async fn cleanup(self) {
        let pool = self.store.pool().clone();
        drop(self.state);
        drop(self.store);
        pool.close().await;
        let _ = std::fs::remove_file(self.path);
    }
}

pub fn request(method: &str, uri: String, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri).header(
        header::COOKIE,
        format!("prompt_ferry_session={SESSION_KEY}"),
    );
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
        .expect("request")
}

pub async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body"),
    )
    .expect("JSON response")
}

/// OpenAI platform create/update payload. `api_key` is required by the admin
/// contract; the subscription plan is never requested here (that path needs a
/// stored OAuth token).
pub fn openai_endpoint_body(name: &str, api_key: &str) -> Value {
    json!({
        "scope": "admin",
        "owner_user_id": null,
        "name": name,
        "provider": "openai",
        "provider_region": null,
        "plan": null,
        "service_tier": "standard",
        "base_url": "https://api.openai.com/v1",
        "api_key": api_key,
        "api_keys": [],
        "key_lb_enabled": false,
        "protocol_mode": "auto",
        "native_api_override": null,
        "enabled": true,
        "mcp_enabled": false,
    })
}
