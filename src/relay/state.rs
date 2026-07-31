use crate::{
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
    pub(crate) worker_id: usize,
    pub(crate) worker: WorkerSender,
    pub(crate) queued_bytes: usize,
    pub(crate) awaiting_approval: bool,
}

pub(crate) struct PendingMcpRequest {
    pub(crate) start_tx: Option<oneshot::Sender<Result<McpResponseStart, ResponseError>>>,
    pub(crate) chunk_tx: mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    pub(crate) worker_id: usize,
    pub(crate) worker: WorkerSender,
    pub(crate) queued_bytes: usize,
}

pub(crate) struct PendingRealtimeSession {
    pub(crate) event_tx: mpsc::Sender<Result<QueuedRealtimeEvent, ResponseError>>,
    pub(crate) worker_id: usize,
    pub(crate) worker: WorkerSender,
    pub(crate) queued_bytes: usize,
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
    }
}
