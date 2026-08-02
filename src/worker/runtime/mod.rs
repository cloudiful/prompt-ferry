mod admin_proxy;
mod ai;
mod bootstrap;
mod bridge;
mod budget;
mod connect;
mod context;
mod error_handling;
#[cfg(test)]
mod error_handling_tests;
mod inbound;
mod json_walker;
mod lifecycle;
mod mcp_support;
mod prompt_log;
mod raw_maintenance;
mod request_assembly;
mod routing;

use crate::{config::WorkerConfig, worker_admin::AdminState};
use reqwest::Client;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

use self::bootstrap::{build_admin_state, validate_config};
use self::budget::check_named_request_budget;
use self::context::RequestExecutionContext;
use self::error_handling::{
    extract_mcp_error, format_mcp_response_body, redaction_enabled, safe_error,
};
use self::inbound::handle_relay_bridge_message;
use self::lifecycle::{
    ActiveRequestGuard, RequestLeaseGuard, RuntimeControl, abort_stale_requests_once,
    spawn_stale_request_reconciler,
};
use self::mcp_support::record_mcp_request_event;
use self::prompt_log::{
    RequestPromptLog, prepare_request_prompt_log, resolve_mcp_conversation_log,
};
use self::request_assembly::{
    BufferedBridgeRequest, BufferedMcpRequest, PendingIncomingRequest, RequestCancellation,
    RequestTransferStats, collect_request_chunks, forward_request_chunk,
    send_worker_shutdown_mcp_response, send_worker_shutdown_response,
};
use self::routing::{
    discover_dynamic_model_route, materialize_route_api_key_selection, select_route_for_candidate,
    upstream_url,
};

const RELAY_RECONNECT_DELAY_SECONDS: u64 = 1;
const MCP_ERROR_BODY_CAPTURE_BYTES: usize = 64 * 1024;
const MCP_RESPONSE_BODY_CAPTURE_BYTES: usize = 64 * 1024;
const REQUEST_RECORD_LEASE_SECONDS: i64 = 90;
const REQUEST_RECORD_HEARTBEAT_SECONDS: i64 = 30;
const STALE_REQUEST_SWEEP_SECONDS: i64 = 30;
const SHUTDOWN_DRAIN_TIMEOUT_SECONDS: u64 = 20;
const REQUEST_STREAM_BUFFER: usize = 16;
const REALTIME_INBOUND_BUFFER: usize = 128;
const ERROR_BODY_SAMPLE_BYTES: usize = 32 * 1024;

#[derive(Clone)]
pub(super) struct WorkerRuntimeState {
    pending_requests: Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    pending_mcp_requests: Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    request_cancellations: Arc<Mutex<HashMap<String, RequestCancellation>>>,
    mcp_request_cancellations: Arc<Mutex<HashMap<String, RequestCancellation>>>,
    pending_realtime_sessions:
        Arc<Mutex<HashMap<String, tokio::sync::mpsc::Sender<RealtimeInboundMessage>>>>,
    control: RuntimeControl,
}

#[derive(Debug, Clone)]
pub(super) enum RealtimeInboundMessage {
    Event(String),
    Close {
        code: Option<u16>,
        reason: Option<String>,
    },
}

impl Default for WorkerRuntimeState {
    fn default() -> Self {
        Self {
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            pending_mcp_requests: Arc::new(Mutex::new(HashMap::new())),
            request_cancellations: Arc::new(Mutex::new(HashMap::new())),
            mcp_request_cancellations: Arc::new(Mutex::new(HashMap::new())),
            pending_realtime_sessions: Arc::new(Mutex::new(HashMap::new())),
            control: RuntimeControl::new(),
        }
    }
}

impl WorkerRuntimeState {
    fn worker_instance_id(&self) -> uuid::Uuid {
        self.control.worker_instance_id()
    }

    fn try_track_request(&self) -> Option<ActiveRequestGuard> {
        self.control.try_track_request()
    }

    fn spawn_request_lease_guard(
        &self,
        admin_state: Option<&AdminState>,
        request_id: uuid::Uuid,
    ) -> Option<RequestLeaseGuard> {
        RequestLeaseGuard::spawn(admin_state, request_id, self.control.clone())
    }

    fn is_shutting_down(&self) -> bool {
        self.control.is_shutting_down()
    }

    fn begin_shutdown(&self) {
        self.control.begin_shutdown();
    }

    async fn wait_for_shutdown(&self) {
        self.control.wait_for_shutdown().await;
    }

    async fn wait_for_drain(&self, timeout: Duration) {
        self.control.wait_for_drain(timeout).await;
    }
}

pub async fn run(config: WorkerConfig) -> anyhow::Result<()> {
    connect::run_embedded(config).await
}

pub async fn run_embedded(config: WorkerConfig) -> anyhow::Result<()> {
    connect::run_embedded(config).await
}

pub async fn connect_for_test(config: WorkerConfig, client: Client) -> anyhow::Result<()> {
    connect::connect_for_test(config, client).await
}

pub async fn connect_for_test_with_admin(
    config: WorkerConfig,
    client: Client,
) -> anyhow::Result<()> {
    connect::connect_for_test_with_admin(config, client).await
}

fn elapsed_ms(started: Instant) -> i64 {
    started.elapsed().as_millis().try_into().unwrap_or(i64::MAX)
}

#[cfg(test)]
pub(super) mod tests;
