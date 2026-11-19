use crate::{
    authz::Authorizer,
    config::RelayConfig,
    ip_acl::CompiledRelayIpPolicy,
    protocol::{
        BridgeMessage, ClientRoute, McpResponseStart, RealtimeServerEventMessage, ResponseError,
        ResponseStart,
    },
    relay_tls::TlsListener,
};
use axum::serve::IncomingStream;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, atomic::AtomicUsize},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, mpsc, oneshot},
};

pub(crate) type WorkerSender = mpsc::Sender<BridgeMessage>;
pub(crate) const MAX_WORKER_CONNECTIONS: usize = 4;

/// How many edge nodes one database node serves.
pub(crate) const MAX_NODE_PEERS: usize = 256;

/// Whether the node already serves the maximum number of peers.
///
/// The cap belongs to the node, not to the relay: a database node serves a
/// bounded number of edge nodes, and the edge nodes of one database node
/// share that budget.
pub(crate) fn node_peer_cap_reached(peers: usize) -> bool {
    peers >= MAX_NODE_PEERS
}

#[derive(Clone)]
pub(crate) struct WorkerSelection {
    pub(crate) worker_id: usize,
    pub(crate) sender: WorkerSender,
}

#[derive(Debug)]
pub(crate) struct QueuedResponseChunk {
    pub(crate) data: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct QueuedRealtimeEvent {
    pub(crate) event: RealtimeServerEventMessage,
}

#[derive(Debug)]
pub(crate) enum ForwardedResponseItem {
    Chunk(QueuedResponseChunk),
    Error(ResponseError),
}

#[derive(Debug)]
pub(crate) enum ForwardedRealtimeItem {
    Event(QueuedRealtimeEvent),
    Error(ResponseError),
}

#[derive(Clone, Copy, Debug)]
pub struct RemoteAddr(pub SocketAddr);

impl axum::extract::connect_info::Connected<IncomingStream<'_, TcpListener>> for RemoteAddr {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

impl axum::extract::connect_info::Connected<IncomingStream<'_, TlsListener>> for RemoteAddr {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        Self(*stream.remote_addr())
    }
}

impl axum::extract::connect_info::Connected<SocketAddr> for RemoteAddr {
    fn connect_info(remote_addr: SocketAddr) -> Self {
        Self(remote_addr)
    }
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: RelayConfig,
    pub(crate) inner: Arc<RelayState>,
    /// The process-wide authorization engine. Production states always carry
    /// the engine built at startup; fixture-only states that never exercise
    /// managed client authentication may leave it absent, and the managed
    /// branch then fails closed.
    pub(crate) authorizer: Option<Arc<Authorizer>>,
    /// Managed client keys are the only accepted AI credential.
    ///
    /// The node runtime has no static client token: a managed `pfy_` key is
    /// the only way to authenticate, and the flag keeps the legacy static
    /// token path closed for it without changing the legacy relay.
    pub(crate) managed_only_auth: bool,
}

impl AppState {
    /// Allocate the next local worker id of this relay state.
    ///
    /// The in-memory node worker uses the same counter as the socket worker
    /// path, so the pending-request bookkeeping of both transports agrees on
    /// which worker owns a request.
    pub(crate) fn allocate_worker_id(&self) -> usize {
        self.inner
            .next_worker_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub struct RelayHandle {
    pub(crate) inner: Arc<RelayState>,
}

impl RelayHandle {
    pub async fn worker_count(&self) -> usize {
        self.inner.workers.lock().await.len()
    }

    pub async fn config_version(&self) -> Option<i64> {
        *self.inner.config_version.lock().await
    }

    /// Ready to serve when at least one worker is connected and a config
    /// snapshot has populated client routes. A connected worker with no routes
    /// (cold start before the first snapshot) stays not-ready on purpose so
    /// managed `pfy_` keys never fall through to the legacy token branch.
    pub async fn is_ready(&self) -> bool {
        self.inner.is_ready().await
    }
}

impl RelayState {
    pub(crate) async fn is_ready(&self) -> bool {
        let workers_empty = self.workers.lock().await.is_empty();
        let routes_empty = self.routes.lock().await.is_empty();
        !workers_empty && !routes_empty
    }
}

pub(crate) struct RelayState {
    pub(crate) workers: Mutex<HashMap<usize, WorkerSender>>,
    pub(crate) worker_loads: Mutex<HashMap<usize, usize>>,
    pub(crate) pending: Mutex<HashMap<String, PendingRequest>>,
    pub(crate) pending_mcp: Mutex<HashMap<String, PendingMcpRequest>>,
    pub(crate) pending_realtime_sessions: Mutex<HashMap<String, PendingRealtimeSession>>,
    pub(crate) routes: Mutex<HashMap<String, ClientRoute>>,
    pub(crate) relay_ip_policy: Mutex<CompiledRelayIpPolicy>,
    pub(crate) config_version: Mutex<Option<i64>>,
    pub(crate) next_worker_id: AtomicUsize,
}

pub(crate) struct PendingRequest {
    pub(crate) start_tx: Option<oneshot::Sender<Result<ResponseStart, ResponseError>>>,
    pub(crate) chunk_tx: mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    pub(crate) forward_tx: mpsc::UnboundedSender<ForwardedResponseItem>,
    pub(crate) worker_id: usize,
    pub(crate) worker: WorkerSender,
    pub(crate) queued_bytes: usize,
    pub(crate) response_started: bool,
}

pub(crate) struct PendingMcpRequest {
    pub(crate) start_tx: Option<oneshot::Sender<Result<McpResponseStart, ResponseError>>>,
    pub(crate) chunk_tx: mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    pub(crate) forward_tx: mpsc::UnboundedSender<ForwardedResponseItem>,
    pub(crate) worker_id: usize,
    pub(crate) worker: WorkerSender,
    pub(crate) queued_bytes: usize,
    pub(crate) response_started: bool,
}

pub(crate) struct PendingRealtimeSession {
    pub(crate) event_tx: mpsc::Sender<Result<QueuedRealtimeEvent, ResponseError>>,
    pub(crate) forward_tx: mpsc::UnboundedSender<ForwardedRealtimeItem>,
    pub(crate) worker_id: usize,
    pub(crate) worker: WorkerSender,
    pub(crate) queued_bytes: usize,
    pub(crate) response_started: bool,
}

#[cfg(test)]
pub(crate) fn test_state() -> AppState {
    AppState {
        config: RelayConfig::default(),
        inner: Arc::new(RelayState {
            workers: Mutex::new(HashMap::new()),
            worker_loads: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            pending_mcp: Mutex::new(HashMap::new()),
            pending_realtime_sessions: Mutex::new(HashMap::new()),
            routes: Mutex::new(HashMap::new()),
            relay_ip_policy: Mutex::new(CompiledRelayIpPolicy::default()),
            config_version: Mutex::new(None),
            next_worker_id: AtomicUsize::new(1),
        }),
        authorizer: None,
        managed_only_auth: false,
    }
}

#[cfg(test)]
pub(crate) async fn test_state_with_authorizer() -> AppState {
    let mut state = test_state();
    state.authorizer = Some(Arc::new(
        crate::authz::build()
            .await
            .expect("test authorization engine builds"),
    ));
    state
}
