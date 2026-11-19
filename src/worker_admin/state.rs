use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};

use anyhow::anyhow;
use axum::{
    Json,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::session::SessionStore;
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock, mpsc};

use super::chatgpt_quota_service::ChatGptQuotaService;
use super::openai_org_usage_cache::OpenAiOrgUsageCache;
use super::quota_inference_cache::QuotaInferenceCache;
use super::token_plan_cache::TokenPlanQuotaCache;
use crate::{
    authz::{Authorizer, ControlAction, Operation, UserAction},
    db::{ConfigRepository, SharedValkeyConfig, StreamDeltaBatchingSettings, UserStore},
    endpoint_models::EndpointModelCache,
    keys::generate_client_key,
    mcp::{McpCatalogCache, McpCatalogService},
    naming::SESSION_COOKIE_NAME,
    protocol::BridgeMessage,
    redact,
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    worker_admin_types::{
        CacheAlertSettings, RequestContentLoggingResponse, SessionUser, UsageRetentionSettings,
    },
};

pub(crate) fn quota_inference_available(
    provider: crate::db::EndpointProvider,
    plan: crate::db::EndpointPlan,
    storage_available: bool,
) -> bool {
    provider == crate::db::EndpointProvider::OpenAi
        && plan == crate::db::EndpointPlan::ChatgptSubscription
        && storage_available
}

pub(crate) struct NodeSetupAuth {
    master: [u8; 32],
    pub(crate) attempts: tokio::sync::Semaphore,
}

impl NodeSetupAuth {
    pub(crate) fn new(master: [u8; 32]) -> Self {
        Self {
            master,
            attempts: tokio::sync::Semaphore::new(4),
        }
    }

    pub(crate) fn verify(&self, candidate: &str) -> bool {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        if candidate.len() != 44 {
            return false;
        }
        let Ok(bytes) = STANDARD.decode(candidate) else {
            return false;
        };
        if bytes.len() != 32 {
            return false;
        }
        self.master
            .iter()
            .zip(bytes.iter())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    }
}

#[cfg(test)]
mod node_setup_tests {
    use super::NodeSetupAuth;
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    #[test]
    fn peer_generation_cleanup_preserves_replacement() {
        use super::NodeStatusState;
        use crate::worker_admin_types::{NodePeerSummary, NodeStatusResponse};
        let id = uuid::Uuid::from_u128(1);
        let mut state = NodeStatusState {
            own: NodeStatusResponse {
                node_id: id,
                group_id: id,
                has_database: true,
                runtime_version: "1".into(),
                protocol_version: "1".into(),
                peers: vec![],
            },
            peers: Default::default(),
        };
        let peer = NodePeerSummary {
            node_id: uuid::Uuid::from_u128(2),
            has_database: false,
            runtime_version: "2".into(),
            protocol_version: "1".into(),
            connected_at: None,
        };
        state.admit(1, peer.clone());
        let serialized = serde_json::to_string(&state.own).unwrap();
        assert!(!serialized.contains("master"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("password"));
        state.admit(2, peer.clone());
        state.release(peer.node_id, 1);
        assert_eq!(state.peers.get(&peer.node_id).unwrap().0, 2);
        state.release(peer.node_id, 2);
        assert!(state.peers.is_empty());
    }

    #[test]
    fn proof_is_fixed_length_base64_and_matches_master() {
        let auth = NodeSetupAuth::new([7; 32]);
        assert!(auth.verify(&STANDARD.encode([7; 32])));
        assert!(!auth.verify(&STANDARD.encode([8; 32])));
        assert!(!auth.verify("not base64"));
        assert!(!auth.verify(&"A".repeat(8192)));
        assert!(!auth.verify(&STANDARD.encode([7; 31])));
    }
}

pub(crate) struct NodeStatusState {
    pub(crate) own: crate::worker_admin_types::NodeStatusResponse,
    pub(crate) peers: HashMap<uuid::Uuid, (u64, crate::worker_admin_types::NodePeerSummary)>,
}

impl NodeStatusState {
    pub(crate) fn admit(
        &mut self,
        generation: u64,
        peer: crate::worker_admin_types::NodePeerSummary,
    ) {
        self.peers.insert(peer.node_id, (generation, peer));
    }
    pub(crate) fn release(&mut self, id: uuid::Uuid, generation: u64) {
        if self
            .peers
            .get(&id)
            .is_some_and(|(current, _)| *current == generation)
        {
            self.peers.remove(&id);
        }
    }
}

#[derive(Clone)]
pub struct AdminState {
    pub(crate) node_setup: Option<Arc<NodeSetupAuth>>,
    pub(crate) node_status: Option<Arc<StdMutex<NodeStatusState>>>,
    /// The process-wide authorization engine shared with the relay state.
    pub authorizer: Arc<Authorizer>,
    pub pool: PgPool,
    pub lease_pool: PgPool,
    pub user_store: UserStore,
    pub config_repository: ConfigRepository,
    pub replay_cache: ReplayCache,
    /// The shared Valkey revision this process resolved at startup. A newer
    /// database revision means the process is stale until it restarts.
    pub shared_valkey: SharedValkeyConfig,
    pub relay_senders: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<BridgeMessage>>>>,
    pub configured_relays: Arc<Vec<String>>,
    pub managed_mode: bool,
    pub relay_secret_manager: Option<RelaySecretManager>,
    pub snapshot_version: Arc<AtomicI64>,
    /// The fingerprint of the snapshot content that was published last.
    ///
    /// It is shared by the administrative publications and the periodic
    /// refresh, and it is only recorded after a successful publication.
    pub published_snapshot_fingerprint: Arc<Mutex<Option<u64>>>,
    /// Serializes the snapshot publications of this process.
    ///
    /// The publication boundary itself is a database lock, so two workers
    /// publish one after the other. Inside one worker the mutex keeps the
    /// local callers from queueing up on the database boundary and holding
    /// pool connections while they wait for it.
    pub snapshot_publication_lock: Arc<Mutex<()>>,
    /// Owns every in-progress admin API handler so shutdown can cancel and
    /// join the real request futures instead of relying on the Axum serve
    /// future to stop its separately spawned connection tasks.
    pub admin_executions: AdminExecutionTracker,
    pub redaction_enabled: Arc<AtomicBool>,
    pub model_route_whitelist_enabled: Arc<AtomicBool>,
    pub request_content_logging: Arc<RwLock<RequestContentLoggingResponse>>,
    pub usage_retention: Arc<RwLock<UsageRetentionSettings>>,
    pub cache_alert: Arc<RwLock<CacheAlertSettings>>,
    pub stream_delta_batching: Arc<RwLock<StreamDeltaBatchingSettings>>,
    pub mcp_catalog_cache: McpCatalogCache,
    pub mcp_catalog_service: McpCatalogService,
    pub mcp_session_store: Option<Arc<dyn SessionStore>>,
    pub mcp_allowed_origins: Vec<String>,
    pub endpoint_model_cache: EndpointModelCache,
    pub(crate) token_plan_quota: TokenPlanQuotaCache,
    /// Issue #589 P2b: display-only OpenAI organization usage cache.
    pub(crate) openai_org_usage: OpenAiOrgUsageCache,
    pub(crate) quota_inference: QuotaInferenceCache,
    pub(crate) chatgpt_quota_service: Option<Arc<ChatGptQuotaService>>,
    /// Personal MCP OAuth backend. `None` disables personal OAuth only; no
    /// other auth mode or MCP behavior depends on it.
    pub(crate) mcp_oauth: Option<Arc<crate::mcp::oauth::McpOAuthService>>,
}

/// Tracks the admin API handler futures that are executing right now.
///
/// Axum's connection tasks are spawned separately from the serve future, so
/// dropping the serve future is not proof that a handler stopped. Admission
/// and closure share one mutex: a handler is either counted before admission
/// closes or refused, never admitted after the closed state was observed, and
/// the idle wait can therefore trust a zero count.
#[derive(Clone, Default)]
pub struct AdminExecutionTracker {
    inner: Arc<AdminExecutionTrackerInner>,
}

#[derive(Default)]
struct AdminExecutionTrackerInner {
    state: StdMutex<AdminExecutionState>,
    idle: tokio::sync::Notify,
}

#[derive(Default)]
struct AdminExecutionState {
    active: usize,
    closed: bool,
}

impl AdminExecutionTracker {
    fn lock_state(&self) -> std::sync::MutexGuard<'_, AdminExecutionState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Register one executing handler, but only while admission is open.
    ///
    /// The closed check and the count increment are one critical section, so
    /// `cancel` either observes this guard or refuses a later admission.
    pub fn try_track(&self) -> Option<AdminExecutionGuard> {
        let mut state = self.lock_state();
        if state.closed {
            return None;
        }
        state.active += 1;
        Some(AdminExecutionGuard {
            tracker: self.clone(),
        })
    }

    /// Close admission and stop every current and future handler at its next
    /// await point. Idempotent.
    pub fn cancel(&self) {
        {
            let mut state = self.lock_state();
            if state.closed {
                return;
            }
            state.closed = true;
        }
        self.inner.idle.notify_waiters();
    }

    pub fn is_closed(&self) -> bool {
        self.lock_state().closed
    }

    pub fn active(&self) -> usize {
        self.lock_state().active
    }

    /// Resolve once admission is closed.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.inner.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_closed() {
                return;
            }
            notified.await;
        }
    }

    /// Wait up to `budget` for every tracked handler to stop.
    ///
    /// A zero result is permanent: admission is closed, so no later handler
    /// can raise the count again. Callers must close admission first; the
    /// non-zero result must keep them from claiming that admin mutations
    /// stopped.
    pub async fn wait_for_idle(&self, budget: Duration) -> usize {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let notified = self.inner.idle.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let active = self.active();
            if active == 0 {
                return 0;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return active;
            }
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep(remaining) => return self.active(),
            }
        }
    }
}

/// One tracked admin handler execution. Dropping it releases the count.
pub struct AdminExecutionGuard {
    tracker: AdminExecutionTracker,
}

impl Drop for AdminExecutionGuard {
    fn drop(&mut self) {
        let notify = {
            let mut state = self.tracker.lock_state();
            state.active = state.active.saturating_sub(1);
            state.active == 0
        };
        if notify {
            self.tracker.inner.idle.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AdminExecutionTracker;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    #[tokio::test]
    async fn cancelling_the_tracker_is_observed_by_the_guard_and_idle_wait() {
        let tracker = AdminExecutionTracker::default();
        let guard = tracker.try_track().expect("admission is open");
        assert_eq!(tracker.active(), 1);

        tracker.cancel();
        assert!(tracker.is_closed());

        let wait_tracker = tracker.clone();
        let waiting = tokio::spawn(async move {
            wait_tracker.cancelled().await;
            wait_tracker.wait_for_idle(Duration::from_secs(1)).await
        });
        drop(guard);

        assert_eq!(
            waiting.await.expect("cancelled wait task ran"),
            0,
            "the released guard leaves no executing admin handler"
        );
        assert!(
            tracker.try_track().is_none(),
            "a closed tracker refuses every later handler"
        );
    }

    #[tokio::test]
    async fn wait_for_idle_returns_the_live_count_at_the_deadline() {
        let tracker = AdminExecutionTracker::default();
        let _guard = tracker.try_track().expect("admission is open");
        tracker.cancel();

        assert_eq!(
            tracker.wait_for_idle(Duration::from_millis(10)).await,
            1,
            "a live tracked handler must never be reported as stopped"
        );
    }

    #[tokio::test]
    async fn closed_admission_stays_closed_while_other_tasks_are_tracking() {
        let tracker = AdminExecutionTracker::default();
        let admitted = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..16 {
            let tracker = tracker.clone();
            let admitted = admitted.clone();
            tasks.push(tokio::spawn(async move {
                for _ in 0..256 {
                    if let Some(guard) = tracker.try_track() {
                        admitted.fetch_add(1, Ordering::SeqCst);
                        tokio::task::yield_now().await;
                        drop(guard);
                    }
                }
            }));
        }

        tracker.cancel();
        assert_eq!(
            tracker.wait_for_idle(Duration::from_secs(1)).await,
            0,
            "the closed tracker drains the handlers admitted before the close"
        );
        let admitted_at_close = admitted.load(Ordering::SeqCst);
        for _ in 0..256 {
            assert!(
                tracker.try_track().is_none(),
                "no handler may be admitted after the close was observed"
            );
        }
        for task in tasks {
            task.await.expect("tracking task ran");
        }
        assert_eq!(
            admitted.load(Ordering::SeqCst),
            admitted_at_close,
            "the zero idle count is permanent once admission closed"
        );
        assert_eq!(tracker.active(), 0);
    }
}

pub struct AdminStateInit {
    pub authorizer: Arc<Authorizer>,
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
    pub stream_delta_batching: StreamDeltaBatchingSettings,
    pub mcp_catalog_cache: McpCatalogCache,
    pub mcp_catalog_service: McpCatalogService,
    pub mcp_session_store: Option<Arc<dyn SessionStore>>,
    pub mcp_allowed_origins: Vec<String>,
    pub endpoint_model_cache: EndpointModelCache,
    pub shared_valkey: SharedValkeyConfig,
}

impl AdminState {
    pub fn new(init: AdminStateInit) -> Self {
        let user_store = UserStore::postgres(&init.pool);
        let config_repository = ConfigRepository::postgres(&init.pool);
        Self {
            node_setup: None,
            node_status: None,
            authorizer: init.authorizer,
            pool: init.pool,
            lease_pool: init.lease_pool,
            user_store,
            config_repository,
            replay_cache: init.replay_cache,
            shared_valkey: init.shared_valkey,
            relay_senders: Arc::new(Mutex::new(HashMap::new())),
            configured_relays: Arc::new(init.configured_relays),
            managed_mode: init.managed_mode,
            relay_secret_manager: init.relay_secret_manager,
            snapshot_version: Arc::new(AtomicI64::new(0)),
            published_snapshot_fingerprint: Arc::new(Mutex::new(None)),
            snapshot_publication_lock: Arc::new(Mutex::new(())),
            admin_executions: AdminExecutionTracker::default(),
            redaction_enabled: Arc::new(AtomicBool::new(init.redaction_enabled)),
            model_route_whitelist_enabled: Arc::new(AtomicBool::new(
                init.model_route_whitelist_enabled,
            )),
            request_content_logging: Arc::new(RwLock::new(init.request_content_logging)),
            usage_retention: Arc::new(RwLock::new(init.usage_retention)),
            cache_alert: Arc::new(RwLock::new(CacheAlertSettings::default())),
            stream_delta_batching: Arc::new(RwLock::new(init.stream_delta_batching)),
            mcp_catalog_cache: init.mcp_catalog_cache,
            mcp_catalog_service: init.mcp_catalog_service,
            mcp_session_store: init.mcp_session_store,
            mcp_allowed_origins: init.mcp_allowed_origins,
            endpoint_model_cache: init.endpoint_model_cache,
            token_plan_quota: TokenPlanQuotaCache::default(),
            openai_org_usage: OpenAiOrgUsageCache::default(),
            quota_inference: QuotaInferenceCache::default(),
            chatgpt_quota_service: None,
            mcp_oauth: None,
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

    /// Replace the shared catalog service (used to attach the personal MCP
    /// OAuth backend to owner-triggered refreshes).
    pub(crate) fn with_mcp_catalog_service(
        mut self,
        service: crate::mcp::McpCatalogService,
    ) -> Self {
        self.mcp_catalog_service = service;
        self
    }

    /// Attach the personal MCP OAuth backend. The builder keeps
    /// `AdminStateInit` free of mandatory OAuth fields.
    pub(crate) fn with_mcp_oauth_service(
        mut self,
        service: Arc<crate::mcp::oauth::McpOAuthService>,
    ) -> Self {
        self.mcp_oauth = Some(service);
        self
    }

    pub(crate) fn with_chatgpt_quota_service(mut self, service: Arc<ChatGptQuotaService>) -> Self {
        self.chatgpt_quota_service = Some(service);
        self
    }

    /// Attach the persisted continuous-session cache alert policy loaded from
    /// `worker_settings`; without it the monitor starts from the defaults.
    pub fn with_cache_alert(mut self, settings: CacheAlertSettings) -> Self {
        self.cache_alert = Arc::new(RwLock::new(settings));
        self
    }
}

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn internal(state: &AdminState, err: impl std::fmt::Display) -> Self {
        let message = format!("{err:#}");
        tracing::warn!(error = %maybe_redact(state, &message), "admin api error");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "internal server error",
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<ApiError> for Response {
    fn from(err: ApiError) -> Response {
        error(err.status, &err.code, &err.message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        Response::from(self)
    }
}

pub fn bad_request(message: &str) -> Response {
    error(StatusCode::BAD_REQUEST, "bad_request", message)
}

pub async fn ensure_admin(
    state: &AdminState,
    headers: &HeaderMap,
    action: ControlAction,
) -> Result<SessionUser, ApiError> {
    let user = current_user(state, headers).await?;
    if crate::authz::check(
        &state.authorizer,
        user.user_id,
        user.is_admin,
        Operation::Control(action),
    )
    .is_allowed()
    {
        Ok(user)
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "admin required",
        ))
    }
}

/// Admit one authenticated user business action.
///
/// The existing SQL owner filters stay authoritative for row visibility; this
/// decision only rejects a session the live user role does not model.
pub async fn ensure_user(
    state: &AdminState,
    headers: &HeaderMap,
    action: UserAction,
) -> Result<SessionUser, ApiError> {
    let user = current_user(state, headers).await?;
    if crate::authz::check(
        &state.authorizer,
        user.user_id,
        user.is_admin,
        Operation::User(action),
    )
    .is_allowed()
    {
        Ok(user)
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "forbidden",
        ))
    }
}

pub async fn current_user(
    state: &AdminState,
    headers: &HeaderMap,
) -> Result<SessionUser, ApiError> {
    let Some(id) = session_id(headers) else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "login required",
        ));
    };
    match state.replay_cache.read_session_refresh(id).await {
        Ok(Some(user)) => Ok(user),
        Ok(None) => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "login required",
        )),
        Err(err) => {
            tracing::warn!(error = %maybe_redact(state, &err.to_string()), "session backend unavailable");
            Err(ApiError::new(
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
