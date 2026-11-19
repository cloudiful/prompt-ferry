mod admin_proxy;
mod ai;
mod background_job;
mod bootstrap;
mod bridge;
#[cfg(test)]
mod bridge_tests;
mod cache_alert_monitor;
mod chatgpt_quota_collector;
pub(super) mod compaction;
mod config_refresh;
mod connect;
mod context;
mod error_handling;
#[cfg(test)]
mod error_handling_tests;
mod inbound;
pub(crate) mod json_walker;
mod lifecycle;
mod lifecycle_standalone;
mod mcp_catalog_refresh;
mod mcp_support;
mod node;
mod prompt_log;
mod quota_observation_collector;
mod raw_maintenance;
mod registry;
mod request_assembly;
mod routing;
mod standalone;

pub(crate) use self::bridge::{BridgeData, BridgeSender};
pub(crate) use self::connect::ExecutorPump;
/// The node runtime: one gated database node that serves one listener.
pub use self::node::{NodeDbRuntime, run_node, serve_node};

/// Issue #548: the cache alert check is public so integration tests can drive
/// one alerting pass without standing up the scheduler.
pub use self::cache_alert_monitor::{CacheAlertDependencies, run_cache_alert_check};

use crate::worker_admin::AdminState;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::Mutex;

use self::context::RequestExecutionContext;
use self::error_handling::{
    extract_mcp_error, format_mcp_response_body, redaction_enabled, safe_error,
};
use self::inbound::handle_relay_bridge_message;
use self::lifecycle::{
    ActiveRequestGuard, RequestLeaseGuard, RuntimeControl, abort_stale_requests_once,
    spawn_stale_request_reconciler,
};
use self::lifecycle_standalone::{StandaloneLeaseInputs, spawn_standalone_request_lease_guard};
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
    discover_dynamic_model_route, materialize_route_api_key_selection_with_quota,
    select_route_for_candidate,
};
use self::standalone::StandaloneRuntimeState;

const MCP_ERROR_BODY_CAPTURE_BYTES: usize = 64 * 1024;
const MCP_RESPONSE_BODY_CAPTURE_BYTES: usize = 64 * 1024;
const REQUEST_RECORD_LEASE_SECONDS: i64 = 90;
const REQUEST_RECORD_HEARTBEAT_SECONDS: i64 = 30;
const STALE_REQUEST_SWEEP_SECONDS: i64 = 30;
const REQUEST_STREAM_BUFFER: usize = 16;
const REALTIME_INBOUND_BUFFER: usize = 128;
const ERROR_BODY_SAMPLE_BYTES: usize = 32 * 1024;

/// Shared shutdown broadcast surfaced to long-running child tasks (admin
/// HTTP server, raw maintenance scheduler, ...). The runtime triggers
/// it when the node runtime receives the
/// OS-level signal arrives; every subscriber observes the transition
/// and runs its own bounded graceful path.
#[derive(Clone)]
pub struct WorkerShutdown {
    tx: tokio::sync::watch::Sender<bool>,
}

impl Default for WorkerShutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkerShutdown {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        Self { tx }
    }

    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<bool> {
        self.tx.subscribe()
    }

    pub fn trigger(&self) {
        // The value is replaced instead of sent: a trigger must not be lost
        // when no service subscribed to the signal yet.
        self.tx.send_replace(true);
    }

    pub fn is_triggered(&self) -> bool {
        *self.tx.borrow()
    }
}

#[derive(Clone)]
pub(crate) struct WorkerRuntimeState {
    pending_requests: Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    pending_mcp_requests: Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    request_cancellations: Arc<Mutex<HashMap<String, RequestCancellation>>>,
    mcp_request_cancellations: Arc<Mutex<HashMap<String, RequestCancellation>>>,
    pending_realtime_sessions:
        Arc<Mutex<HashMap<String, tokio::sync::mpsc::Sender<RealtimeInboundMessage>>>>,
    /// Every accepted request task is owned here until it finishes, so a
    /// fail-closed or timed-out drain can cancel and join the actual task
    /// instead of waiting on a guard that may outlive it.
    accepted_tasks: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
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
            accepted_tasks: Arc::new(Mutex::new(Vec::new())),
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

    /// Spawn an accepted request under the runtime's ownership.
    ///
    /// The handle is retained until the task finishes, so the shutdown path
    /// can abort and join the real task. Completed handles are dropped while
    /// the registry is pruned, which never detaches a task that still runs.
    async fn spawn_accepted<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(task);
        let mut tasks = self.accepted_tasks.lock().await;
        tasks.retain(|task| !task.is_finished());
        tasks.push(handle);
    }

    /// Take the owned accepted request tasks so the shutdown path can join or
    /// abort them.
    async fn take_accepted_tasks(&self) -> Vec<tokio::task::JoinHandle<()>> {
        std::mem::take(&mut *self.accepted_tasks.lock().await)
    }

    fn spawn_request_lease_guard(
        &self,
        admin_state: Option<&AdminState>,
        standalone_state: Option<&StandaloneRuntimeState>,
        request_id: uuid::Uuid,
    ) -> Option<RequestLeaseGuard> {
        if let Some(state) = standalone_state {
            let inputs = StandaloneLeaseInputs::from_standalone_state(
                state,
                self.control.worker_instance_id(),
                REQUEST_RECORD_LEASE_SECONDS,
                REQUEST_RECORD_HEARTBEAT_SECONDS,
            );
            if let Some(guard) =
                spawn_standalone_request_lease_guard(Some(inputs), self.control.clone(), request_id)
            {
                return Some(RequestLeaseGuard::standalone(guard));
            }
        }
        RequestLeaseGuard::spawn(admin_state, request_id, self.control.clone())
    }

    async fn request_cancellation(&self, request_id: &str) -> Option<RequestCancellation> {
        self.request_cancellations
            .lock()
            .await
            .get(request_id)
            .cloned()
    }

    fn begin_shutdown(&self) {
        self.control.begin_shutdown();
    }
}

// P4 (issue #230): the model-route test probe and the Realtime
// WebSocket join are single-sourced on the same provider-aware URL
// composition helper as the runtime HTTP path. The helper is re-
// exported here so the admin layer can reach it without exposing the
// private `ai::upstream` module.
pub use ai::upstream::upstream_url_for_route_parts;

fn elapsed_ms(started: Instant) -> i64 {
    started.elapsed().as_millis().try_into().unwrap_or(i64::MAX)
}

#[cfg(test)]
pub(super) mod tests;
