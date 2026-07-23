use crate::{
    auth::{check_bearer, error_response},
    bridge_wire, ip_acl,
    protocol::{ApprovalPending, BridgeMessage, ConfigSnapshot, ResponseError},
};

mod connection;

use super::{
    response_forward::{
        fail_all_pending, handle_mcp_response_chunk, handle_mcp_response_end,
        handle_mcp_response_error, handle_mcp_response_start, handle_realtime_server_event,
        handle_realtime_session_close, handle_realtime_session_error, handle_response_chunk,
        handle_response_end, handle_response_error, handle_response_start,
    },
    state::{AppState, WorkerSender},
};
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use std::{collections::HashMap, sync::atomic::Ordering, time::Duration};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

const BRIDGE_SEND_BUFFER: usize = 64;

pub(super) fn worker_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(worker_healthz))
        .route("/ws/worker", get(worker_ws))
        .with_state(state)
}

async fn worker_healthz() -> &'static str {
    "ok"
}

async fn worker_ws(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(response) = check_bearer(&headers, &state.config.worker_token) {
        return response;
    }
    if !state.inner.workers.lock().await.is_empty() {
        warn!("worker connection rejected: worker already connected");
        return error_response(
            StatusCode::CONFLICT,
            "worker_already_connected",
            "a worker is already connected",
        );
    }

    ws.max_message_size(bridge_wire::BRIDGE_WS_MAX_MESSAGE_BYTES)
        .max_frame_size(bridge_wire::BRIDGE_WS_MAX_FRAME_BYTES)
        .on_upgrade(move |socket| handle_worker_socket(state, socket))
}

async fn handle_worker_socket(state: AppState, socket: WebSocket) {
    let worker_id = state.inner.next_worker_id.fetch_add(1, Ordering::Relaxed);
    let heartbeat_timeout = Duration::from_secs(state.config.worker_heartbeat_timeout_seconds);
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (mut write_cipher, mut read_cipher) =
        match connection::perform_worker_handshake(&state, worker_id, &mut ws_tx, &mut ws_rx).await
        {
            Some(ciphers) => ciphers,
            None => return,
        };
    let (tx, mut rx) = mpsc::channel::<BridgeMessage>(BRIDGE_SEND_BUFFER);

    if !register_worker(&state, worker_id, tx).await {
        let _ = ws_tx.send(Message::Close(None)).await;
        return;
    }
    info!(worker_id, "worker connected");

    let write_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            let payload = if let Some(cipher) = write_cipher.as_mut() {
                cipher.encrypt_message(&message)
            } else {
                bridge_wire::encode_message(&message)
            };
            match payload {
                Ok(payload) => {
                    if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    error!(worker_id, error = %err, "failed to encode bridge message");
                    break;
                }
            }
        }
    });

    while let Some(message) = connection::recv_worker_message(
        worker_id,
        heartbeat_timeout,
        &mut ws_rx,
        read_cipher.as_mut(),
    )
    .await
    {
        handle_worker_bridge_message(&state, message).await;
    }

    write_task.abort();
    state.inner.workers.lock().await.remove(&worker_id);
    fail_all_pending(
        &state,
        ResponseError {
            request_id: String::new(),
            status: StatusCode::BAD_GATEWAY.as_u16(),
            code: "worker_disconnected".to_string(),
            message: "worker disconnected".to_string(),
        },
    )
    .await;
    info!(worker_id, "worker disconnected");
}

pub(super) async fn handle_config_snapshot(state: &AppState, snapshot: ConfigSnapshot) {
    let version = snapshot.version;
    let routes = snapshot
        .keys
        .into_iter()
        .map(|route| (route.key_hash.clone(), route))
        .collect::<HashMap<_, _>>();
    let route_count = routes.len();
    let relay_ip_policy = match ip_acl::compile_policy(&snapshot.relay_ip_policy) {
        Ok(policy) => policy,
        Err(err) => {
            warn!(error = %err, "ignoring invalid relay ip policy from worker snapshot");
            state.inner.relay_ip_policy.lock().await.clone()
        }
    };
    *state.inner.routes.lock().await = routes;
    *state.inner.relay_ip_policy.lock().await = relay_ip_policy;
    *state.inner.config_version.lock().await = Some(version);
    info!(version, route_count, "relay config snapshot applied");
}

async fn register_worker(state: &AppState, worker_id: usize, tx: WorkerSender) -> bool {
    let mut workers = state.inner.workers.lock().await;
    if !workers.is_empty() {
        warn!(
            worker_id,
            "worker connection rejected: worker already connected"
        );
        return false;
    }
    workers.insert(worker_id, tx);
    true
}

async fn handle_worker_bridge_message(state: &AppState, message: BridgeMessage) {
    match message {
        BridgeMessage::ApprovalPending(pending) => mark_approval_pending(state, pending).await,
        BridgeMessage::ResponseStart(start) => handle_response_start(state, start).await,
        BridgeMessage::ResponseChunk(chunk) => handle_response_chunk(state, chunk).await,
        BridgeMessage::ResponseEnd(end) => handle_response_end(state, end).await,
        BridgeMessage::ResponseError(err) => {
            handle_response_error(state, err.clone()).await;
            handle_mcp_response_error(state, err.clone()).await;
            handle_realtime_session_error(state, err).await;
        }
        BridgeMessage::RealtimeServerEvent(event) => {
            handle_realtime_server_event(state, event).await
        }
        BridgeMessage::RealtimeSessionClose(close) => {
            handle_realtime_session_close(state, close).await
        }
        BridgeMessage::McpResponseStart(start) => handle_mcp_response_start(state, start).await,
        BridgeMessage::McpResponseChunk(chunk) => handle_mcp_response_chunk(state, chunk).await,
        BridgeMessage::McpResponseEnd(end) => handle_mcp_response_end(state, end).await,
        BridgeMessage::ConfigSnapshot(snapshot) => handle_config_snapshot(state, snapshot).await,
        BridgeMessage::Pong | BridgeMessage::Ping => debug!("worker heartbeat"),
        BridgeMessage::RealtimeSessionStart(_) | BridgeMessage::RealtimeClientEvent(_) => {
            warn!("worker sent unexpected realtime request")
        }
        BridgeMessage::RequestStart(_)
        | BridgeMessage::RequestChunk(_)
        | BridgeMessage::RequestEnd(_) => warn!("worker sent unexpected request"),
        BridgeMessage::McpRequestStart(_)
        | BridgeMessage::McpRequestChunk(_)
        | BridgeMessage::McpRequestEnd(_) => warn!("worker sent unexpected mcp request"),
    }
}

async fn mark_approval_pending(state: &AppState, pending: ApprovalPending) {
    let mut requests = state.inner.pending.lock().await;
    if let Some(entry) = requests.get_mut(&pending.request_id) {
        entry.awaiting_approval = true;
    }
}

#[cfg(test)]
mod tests {
    use super::super::{response_forward::fail_all_pending, state::test_state};
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn fail_all_pending_uses_approval_interrupted_for_flagged_waiters() {
        let state = test_state();
        let (start_tx, start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(1);
        state.inner.pending.lock().await.insert(
            "req-1".to_string(),
            super::super::state::PendingRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                awaiting_approval: true,
            },
        );

        fail_all_pending(
            &state,
            ResponseError {
                request_id: String::new(),
                status: StatusCode::BAD_GATEWAY.as_u16(),
                code: "worker_disconnected".to_string(),
                message: "worker disconnected".to_string(),
            },
        )
        .await;

        let err = start_rx.await.unwrap().unwrap_err();
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE.as_u16());
        assert_eq!(err.code, "approval_interrupted");
    }

    #[tokio::test]
    async fn choose_worker_returns_none_without_workers() {
        let state = test_state();
        assert!(
            super::super::response_forward::choose_worker(&state)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn register_worker_rejects_second_worker() {
        let state = test_state();
        let (first_tx, _first_rx) = mpsc::channel(1);
        let (second_tx, _second_rx) = mpsc::channel(1);

        assert!(register_worker(&state, 1, first_tx).await);
        assert!(!register_worker(&state, 2, second_tx).await);
        assert_eq!(state.inner.workers.lock().await.len(), 1);
    }
}
