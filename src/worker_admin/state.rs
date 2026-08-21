use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
};

use anyhow::anyhow;
use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use rmcp::transport::streamable_http_server::session::SessionStore;
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use uuid::Uuid;

use super::token_plan_cache::TokenPlanQuotaCache;
use crate::{
    db::{ConfigRepository, ManagedRelayRuntimeStatus, StreamDeltaBatchingSettings, UserStore},
    endpoint_models::EndpointModelCache,
    keys::generate_client_key,
    llm_review::{ApprovalResolution, LlmReviewSettings},
    mcp::{McpCatalogCache, McpCatalogService},
    naming::SESSION_COOKIE_NAME,
    protocol::BridgeMessage,
    raw_payload_store::RawPayloadStore,
    redact,
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    worker_admin_types::{RequestContentLoggingResponse, SessionUser, UsageRetentionSettings},
};

#[derive(Debug)]
pub enum RelaySupervisorCommand {
    Reconcile {
        response: oneshot::Sender<anyhow::Result<()>>,
    },
    Reconnect {
        relay_id: Uuid,
        response: oneshot::Sender<anyhow::Result<()>>,
    },
}

#[derive(Clone)]
pub struct ManagedRelaySupervisorHandle {
    tx: mpsc::UnboundedSender<RelaySupervisorCommand>,
}

impl ManagedRelaySupervisorHandle {
    pub fn new(tx: mpsc::UnboundedSender<RelaySupervisorCommand>) -> Self {
        Self { tx }
    }

    pub async fn reconcile(&self) -> anyhow::Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(RelaySupervisorCommand::Reconcile {
                response: response_tx,
            })
            .map_err(|_| anyhow!("relay supervisor is not available"))?;
        response_rx
            .await
            .map_err(|_| anyhow!("relay supervisor response dropped"))?
    }

    pub async fn reconnect(&self, relay_id: Uuid) -> anyhow::Result<()> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .send(RelaySupervisorCommand::Reconnect {
                relay_id,
                response: response_tx,
            })
            .map_err(|_| anyhow!("relay supervisor is not available"))?;
        response_rx
            .await
            .map_err(|_| anyhow!("relay supervisor response dropped"))?
    }
}

#[derive(Clone)]
pub struct AdminState {
    pub pool: PgPool,
    pub lease_pool: PgPool,
    pub user_store: UserStore,
    pub config_repository: ConfigRepository,
    pub replay_cache: ReplayCache,
    pub relay_senders: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<BridgeMessage>>>>,
    pub configured_relays: Arc<Vec<String>>,
    pub managed_mode: bool,
    pub relay_secret_manager: Option<RelaySecretManager>,
    pub managed_relay_statuses: Arc<RwLock<HashMap<Uuid, ManagedRelayRuntimeStatus>>>,
    pub relay_supervisor: Arc<RwLock<Option<ManagedRelaySupervisorHandle>>>,
    pub snapshot_version: Arc<AtomicI64>,
    pub redaction_enabled: Arc<AtomicBool>,
    pub model_route_whitelist_enabled: Arc<AtomicBool>,
    pub request_content_logging: Arc<RwLock<RequestContentLoggingResponse>>,
    pub usage_retention: Arc<RwLock<UsageRetentionSettings>>,
    pub raw_payload_store: Option<Arc<RawPayloadStore>>,
    pub stream_delta_batching: Arc<RwLock<StreamDeltaBatchingSettings>>,
    pub llm_review_settings: Arc<RwLock<LlmReviewSettings>>,
    pub approval_waiters: Arc<Mutex<HashMap<Uuid, oneshot::Sender<ApprovalResolution>>>>,
    pub mcp_catalog_cache: McpCatalogCache,
    pub mcp_catalog_service: McpCatalogService,
    pub mcp_session_store: Option<Arc<dyn SessionStore>>,
    pub mcp_allowed_origins: Vec<String>,
    pub mcp_quota_valkey: crate::mcp::McpQuotaValkey,
    pub endpoint_model_cache: EndpointModelCache,
    pub(crate) token_plan_quota: TokenPlanQuotaCache,
}

pub struct AdminStateInit {
    pub pool: PgPool,
    pub lease_pool: PgPool,
    pub replay_cache: ReplayCache,
    pub configured_relays: Vec<String>,
    pub managed_mode: bool,
    pub relay_secret_manager: Option<RelaySecretManager>,
    pub redaction_enabled: bool,
    pub model_route_whitelist_enabled: bool,
    pub request_content_logging: RequestContentLoggingResponse,
    pub usage_retention: UsageRetentionSettings,
    pub raw_payload_store: Option<Arc<RawPayloadStore>>,
    pub stream_delta_batching: StreamDeltaBatchingSettings,
    pub llm_review_settings: LlmReviewSettings,
    pub mcp_catalog_cache: McpCatalogCache,
    pub mcp_catalog_service: McpCatalogService,
    pub mcp_session_store: Option<Arc<dyn SessionStore>>,
    pub mcp_allowed_origins: Vec<String>,
    pub mcp_quota_valkey: crate::mcp::McpQuotaValkey,
    pub endpoint_model_cache: EndpointModelCache,
}

impl AdminState {
    pub fn new(init: AdminStateInit) -> Self {
        let user_store = UserStore::postgres(&init.pool);
        let config_repository = ConfigRepository::postgres(&init.pool);
        Self {
            pool: init.pool,
            lease_pool: init.lease_pool,
            user_store,
            config_repository,
            replay_cache: init.replay_cache,
            relay_senders: Arc::new(Mutex::new(HashMap::new())),
            configured_relays: Arc::new(init.configured_relays),
            managed_mode: init.managed_mode,
            relay_secret_manager: init.relay_secret_manager,
            managed_relay_statuses: Arc::new(RwLock::new(HashMap::new())),
            relay_supervisor: Arc::new(RwLock::new(None)),
            snapshot_version: Arc::new(AtomicI64::new(0)),
            redaction_enabled: Arc::new(AtomicBool::new(init.redaction_enabled)),
            model_route_whitelist_enabled: Arc::new(AtomicBool::new(
                init.model_route_whitelist_enabled,
            )),
            request_content_logging: Arc::new(RwLock::new(init.request_content_logging)),
            usage_retention: Arc::new(RwLock::new(init.usage_retention)),
            raw_payload_store: init.raw_payload_store,
            stream_delta_batching: Arc::new(RwLock::new(init.stream_delta_batching)),
            llm_review_settings: Arc::new(RwLock::new(init.llm_review_settings)),
            approval_waiters: Arc::new(Mutex::new(HashMap::new())),
            mcp_catalog_cache: init.mcp_catalog_cache,
            mcp_catalog_service: init.mcp_catalog_service,
            mcp_session_store: init.mcp_session_store,
            mcp_allowed_origins: init.mcp_allowed_origins,
            mcp_quota_valkey: init.mcp_quota_valkey,
            endpoint_model_cache: init.endpoint_model_cache,
            token_plan_quota: TokenPlanQuotaCache::default(),
        }
    }

    pub fn relay_secret_manager(&self) -> anyhow::Result<&RelaySecretManager> {
        self.relay_secret_manager
            .as_ref()
            .ok_or_else(|| anyhow!("relay secret manager is not configured"))
    }

    pub fn sqlite_capability_unavailable(&self) -> Response {
        error(
            StatusCode::NOT_IMPLEMENTED,
            "capability_unavailable",
            "this Admin API capability is not available with SQLite yet",
        )
    }

    pub fn capability_unavailable(&self, capability: crate::db::Capability) -> Response {
        error(
            StatusCode::NOT_IMPLEMENTED,
            capability.as_code(),
            capability.description(),
        )
    }

    pub fn with_user_store(mut self, user_store: UserStore) -> Self {
        self.user_store = user_store;
        self
    }

    pub fn with_config_repository(mut self, repository: ConfigRepository) -> Self {
        self.config_repository = repository;
        self
    }

    pub async fn set_relay_supervisor(&self, handle: ManagedRelaySupervisorHandle) {
        *self.relay_supervisor.write().await = Some(handle);
    }

    pub async fn reconcile_relays(&self) -> anyhow::Result<()> {
        let handle = self.relay_supervisor.read().await.clone();
        match handle {
            Some(handle) => handle.reconcile().await,
            None if self.managed_mode => Err(anyhow!("relay supervisor is not ready")),
            None => Ok(()),
        }
    }

    pub async fn reconnect_relay(&self, relay_id: Uuid) -> anyhow::Result<()> {
        let handle = self.relay_supervisor.read().await.clone();
        match handle {
            Some(handle) => handle.reconnect(relay_id).await,
            None if self.managed_mode => Err(anyhow!("relay supervisor is not ready")),
            None => Ok(()),
        }
    }

    pub async fn managed_runtime_status_or_default(
        &self,
        relay_id: Uuid,
    ) -> ManagedRelayRuntimeStatus {
        self.managed_relay_statuses
            .read()
            .await
            .get(&relay_id)
            .cloned()
            .unwrap_or_default()
    }

    pub async fn set_managed_relay_error(&self, relay_id: Uuid, message: impl Into<String>) {
        let mut statuses = self.managed_relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = false;
        status.last_error = Some(message.into());
        status.last_disconnected_at = Some(Utc::now());
    }
}

pub fn bad_request(message: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "bad_request", message)
}

pub async fn ensure_admin(
    state: &AdminState,
    headers: &HeaderMap,
) -> Result<SessionUser, Response> {
    let user = current_user(state, headers).await?;
    if user.is_admin {
        Ok(user)
    } else {
        Err(error(StatusCode::FORBIDDEN, "forbidden", "admin required"))
    }
}

pub async fn current_user(
    state: &AdminState,
    headers: &HeaderMap,
) -> Result<SessionUser, Response> {
    let Some(id) = session_id(headers) else {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "login required",
        ));
    };
    match state.replay_cache.read_session_refresh(id).await {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "login required",
        )),
        Err(err) => {
            tracing::warn!(error = %maybe_redact(state, &err.to_string()), "session backend unavailable");
            Err(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "service_unavailable",
                "session backend unavailable",
            ))
        }
    }
}

pub fn maybe_redact(state: &AdminState, text: &str) -> String {
    if state.redaction_enabled.load(Ordering::SeqCst) {
        redact::redact_text(text)
    } else {
        text.to_string()
    }
}

pub fn session_id(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")))
}

pub fn new_session_id() -> String {
    let (secret, _, _) = generate_client_key();
    secret
}

pub fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
            }
        })),
    )
        .into_response()
}

pub fn internal(state: &AdminState, err: impl std::fmt::Display) -> Response {
    let message = format!("{err:#}");
    tracing::warn!(error = %maybe_redact(state, &message), "admin api error");
    error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        "internal server error",
    )
}
