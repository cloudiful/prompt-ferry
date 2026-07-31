use crate::{
    auth::error_response,
    config::RelayConfig,
    protocol::{
        BridgeMessage, BridgeRequestCancel, McpRequestCancel, McpResponseChunk, McpResponseEnd,
        McpResponseStart, RealtimeServerEventMessage, RealtimeSessionClose, ResponseChunk,
        ResponseEnd, ResponseError, ResponseStart,
    },
};
use axum::{http::StatusCode, response::Response};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

use super::response_queue::{EnqueueResult, try_enqueue};
use super::state::{
    AppState, QueuedRealtimeEvent, QueuedResponseChunk, WorkerSelection, WorkerSender,
};

pub(crate) async fn handle_response_start(state: &AppState, start: ResponseStart) {
    let mut pending = state.inner.pending.lock().await;
    if let Some(entry) = pending.get_mut(&start.request_id)
        && let Some(tx) = entry.start_tx.take()
    {
        let _ = tx.send(Ok(start));
    }
}

pub(crate) async fn handle_response_chunk(state: &AppState, chunk: ResponseChunk) {
    let request_id = chunk.request_id.clone();
    let queued = QueuedResponseChunk { data: chunk.data };
    let chunk_bytes = queued.data.len();
    let removal = {
        let mut pending = state.inner.pending.lock().await;
        let Some(entry) = pending.get_mut(&request_id) else {
            return;
        };
        let result = try_enqueue(
            &entry.chunk_tx,
            &mut entry.queued_bytes,
            Ok(queued),
            chunk_bytes,
            state.config.response_stream_max_bytes,
        );
        if result == EnqueueResult::Enqueued {
            None
        } else {
            Some((
                pending.remove(&request_id).expect("pending entry exists"),
                result,
            ))
        }
    };
    if let Some((entry, result)) = removal {
        warn!(
            category = "relay_bridge_diag",
            request_id = %request_id,
            worker_id = entry.worker_id,
            queue_result = ?result,
            queued_bytes = entry.queued_bytes,
            chunk_bytes,
            queue_capacity = state.config.response_stream_buffer,
            max_queued_bytes = state.config.response_stream_max_bytes,
            "failed to enqueue response chunk"
        );
        release_worker(state, entry.worker_id).await;
        match result {
            EnqueueResult::Closed => {
                send_request_cancel(&entry.worker, &request_id, "request_cancelled");
            }
            EnqueueResult::BytesLimit | EnqueueResult::Full => {
                let err = backpressure_error(&request_id);
                send_request_cancel(&entry.worker, &request_id, &err.code);
                send_response_error(entry.start_tx, entry.chunk_tx, err);
            }
            EnqueueResult::Enqueued => unreachable!(),
        }
    }
}

pub(crate) async fn handle_response_end(state: &AppState, end: ResponseEnd) {
    finish_pending(state, &end.request_id).await;
}

pub(crate) async fn handle_response_error(state: &AppState, err: ResponseError) {
    let entry = state.inner.pending.lock().await.remove(&err.request_id);
    if let Some(mut entry) = entry {
        release_worker(state, entry.worker_id).await;
        if let Some(tx) = entry.start_tx.take() {
            let _ = tx.send(Err(err));
        } else {
            let _ = entry.chunk_tx.try_send(Err(err));
        }
    }
}

pub(crate) async fn handle_mcp_response_start(state: &AppState, start: McpResponseStart) {
    let mut pending = state.inner.pending_mcp.lock().await;
    if let Some(entry) = pending.get_mut(&start.request_id)
        && let Some(tx) = entry.start_tx.take()
    {
        let _ = tx.send(Ok(start));
    }
}

pub(crate) async fn handle_mcp_response_chunk(state: &AppState, chunk: McpResponseChunk) {
    let request_id = chunk.request_id.clone();
    let queued = QueuedResponseChunk { data: chunk.data };
    let chunk_bytes = queued.data.len();
    let removal = {
        let mut pending = state.inner.pending_mcp.lock().await;
        let Some(entry) = pending.get_mut(&request_id) else {
            warn!(
                category = "mcp_bridge_diag",
                request_id = %request_id,
                "discarded MCP response chunk because relay request is no longer pending"
            );
            return;
        };
        let result = try_enqueue(
            &entry.chunk_tx,
            &mut entry.queued_bytes,
            Ok(queued),
            chunk_bytes,
            state.config.response_stream_max_bytes,
        );
        if result == EnqueueResult::Enqueued {
            None
        } else {
            Some((
                pending
                    .remove(&request_id)
                    .expect("pending MCP entry exists"),
                result,
            ))
        }
    };
    if let Some((entry, result)) = removal {
        warn!(
            category = "mcp_bridge_diag",
            request_id = %request_id,
            worker_id = entry.worker_id,
            queued_bytes = entry.queued_bytes,
            chunk_bytes,
            queue_result = ?result,
            queue_capacity = state.config.response_stream_buffer,
            max_queued_bytes = state.config.response_stream_max_bytes,
            "removed pending MCP request while forwarding a response chunk"
        );
        release_worker(state, entry.worker_id).await;
        match result {
            EnqueueResult::Closed => {
                send_mcp_request_cancel(&entry.worker, &request_id, "request_cancelled");
            }
            EnqueueResult::BytesLimit | EnqueueResult::Full => {
                let err = backpressure_error(&request_id);
                send_mcp_request_cancel(&entry.worker, &request_id, &err.code);
                send_mcp_response_error(entry.start_tx, entry.chunk_tx, err);
            }
            EnqueueResult::Enqueued => unreachable!(),
        }
    }
}

pub(crate) async fn handle_mcp_response_end(state: &AppState, end: McpResponseEnd) {
    finish_mcp_pending(state, &end.request_id).await;
}

pub(crate) async fn handle_mcp_response_error(state: &AppState, err: ResponseError) {
    let entry = state.inner.pending_mcp.lock().await.remove(&err.request_id);
    if let Some(mut entry) = entry {
        warn!(
            category = "mcp_bridge_diag",
            request_id = %err.request_id,
            worker_id = entry.worker_id,
            status = err.status,
            error_code = %err.code,
            "worker returned an MCP response error"
        );
        release_worker(state, entry.worker_id).await;
        if let Some(tx) = entry.start_tx.take() {
            let _ = tx.send(Err(err));
        } else {
            let _ = entry.chunk_tx.try_send(Err(err));
        }
    }
}

pub(crate) async fn handle_realtime_server_event(
    state: &AppState,
    event: RealtimeServerEventMessage,
) {
    let request_id = event.request_id.clone();
    let queued = QueuedRealtimeEvent { event };
    let event_bytes = queued.event.event_json.len();
    let removal = {
        let mut pending = state.inner.pending_realtime_sessions.lock().await;
        let Some(entry) = pending.get_mut(&request_id) else {
            return;
        };
        let result = try_enqueue(
            &entry.event_tx,
            &mut entry.queued_bytes,
            Ok(queued),
            event_bytes,
            state.config.response_stream_max_bytes,
        );
        if result == EnqueueResult::Enqueued {
            None
        } else {
            Some((
                pending
                    .remove(&request_id)
                    .expect("pending realtime entry exists"),
                result,
            ))
        }
    };
    if let Some((entry, result)) = removal {
        warn!(
            category = "relay_bridge_diag",
            request_id = %request_id,
            worker_id = entry.worker_id,
            queue_result = ?result,
            queued_bytes = entry.queued_bytes,
            chunk_bytes = event_bytes,
            queue_capacity = state.config.response_stream_buffer,
            max_queued_bytes = state.config.response_stream_max_bytes,
            "failed to enqueue realtime server event"
        );
        release_worker(state, entry.worker_id).await;
        match result {
            EnqueueResult::Closed => {
                send_realtime_close(&entry.worker, &request_id, "request_cancelled");
            }
            EnqueueResult::BytesLimit | EnqueueResult::Full => {
                let err = backpressure_error(&request_id);
                send_realtime_close(&entry.worker, &request_id, &err.code);
                let _ = entry.event_tx.try_send(Err(err));
            }
            EnqueueResult::Enqueued => unreachable!(),
        }
    }
}

pub(crate) async fn handle_realtime_session_close(state: &AppState, close: RealtimeSessionClose) {
    finish_realtime_pending(state, &close.request_id).await;
}

pub(crate) async fn handle_realtime_session_error(state: &AppState, err: ResponseError) {
    let entry = state
        .inner
        .pending_realtime_sessions
        .lock()
        .await
        .remove(&err.request_id);
    if let Some(entry) = entry {
        release_worker(state, entry.worker_id).await;
        let _ = entry.event_tx.try_send(Err(err));
    }
}

pub(crate) async fn choose_worker(state: &AppState) -> Option<WorkerSelection> {
    let workers = state.inner.workers.lock().await;
    let mut loads = state.inner.worker_loads.lock().await;
    workers
        .iter()
        .min_by_key(|(worker_id, _)| {
            (
                loads.get(worker_id).copied().unwrap_or_default(),
                **worker_id,
            )
        })
        .map(|(worker_id, sender)| {
            *loads.entry(*worker_id).or_default() += 1;
            WorkerSelection {
                worker_id: *worker_id,
                sender: sender.clone(),
            }
        })
}

pub(crate) async fn remove_pending(state: &AppState, request_id: &str) {
    let entry = state.inner.pending.lock().await.remove(request_id);
    if let Some(entry) = entry {
        release_worker(state, entry.worker_id).await;
        send_request_cancel(&entry.worker, request_id, "request_cancelled");
    }
}

pub(crate) async fn remove_mcp_pending(state: &AppState, request_id: &str) {
    let entry = state.inner.pending_mcp.lock().await.remove(request_id);
    if let Some(entry) = entry {
        debug!(
            category = "mcp_bridge_diag",
            request_id,
            worker_id = entry.worker_id,
            "removed pending MCP request after downstream stream ended"
        );
        release_worker(state, entry.worker_id).await;
        send_mcp_request_cancel(&entry.worker, request_id, "request_cancelled");
    }
}

pub(crate) async fn remove_realtime_pending(state: &AppState, request_id: &str) {
    let entry = state
        .inner
        .pending_realtime_sessions
        .lock()
        .await
        .remove(request_id);
    if let Some(entry) = entry {
        release_worker(state, entry.worker_id).await;
        send_realtime_close(&entry.worker, request_id, "request_cancelled");
    }
}

pub(crate) async fn finish_pending(state: &AppState, request_id: &str) {
    if let Some(entry) = state.inner.pending.lock().await.remove(request_id) {
        release_worker(state, entry.worker_id).await;
    }
}

pub(crate) async fn finish_mcp_pending(state: &AppState, request_id: &str) {
    if let Some(entry) = state.inner.pending_mcp.lock().await.remove(request_id) {
        release_worker(state, entry.worker_id).await;
    }
}

pub(crate) async fn finish_realtime_pending(state: &AppState, request_id: &str) {
    if let Some(entry) = state
        .inner
        .pending_realtime_sessions
        .lock()
        .await
        .remove(request_id)
    {
        release_worker(state, entry.worker_id).await;
    }
}

pub(crate) async fn release_response_bytes(state: &AppState, request_id: &str, bytes: usize) {
    if let Some(entry) = state.inner.pending.lock().await.get_mut(request_id) {
        entry.queued_bytes = entry.queued_bytes.saturating_sub(bytes);
    }
}

pub(crate) async fn release_mcp_response_bytes(state: &AppState, request_id: &str, bytes: usize) {
    if let Some(entry) = state.inner.pending_mcp.lock().await.get_mut(request_id) {
        entry.queued_bytes = entry.queued_bytes.saturating_sub(bytes);
    }
}

pub(crate) async fn release_realtime_event_bytes(state: &AppState, request_id: &str, bytes: usize) {
    if let Some(entry) = state
        .inner
        .pending_realtime_sessions
        .lock()
        .await
        .get_mut(request_id)
    {
        entry.queued_bytes = entry.queued_bytes.saturating_sub(bytes);
    }
}

pub(crate) async fn fail_pending_for_worker(
    state: &AppState,
    worker_id: usize,
    mut err: ResponseError,
) {
    let drained = {
        let mut pending = state.inner.pending.lock().await;
        let request_ids = pending
            .iter()
            .filter(|(_, entry)| entry.worker_id == worker_id)
            .map(|(request_id, _)| request_id.clone())
            .collect::<Vec<_>>();
        request_ids
            .into_iter()
            .filter_map(|request_id| pending.remove(&request_id).map(|entry| (request_id, entry)))
            .collect::<Vec<_>>()
    };
    for (request_id, mut entry) in drained {
        err.request_id = request_id;
        if entry.awaiting_approval {
            err.status = StatusCode::SERVICE_UNAVAILABLE.as_u16();
            err.code = "approval_interrupted".to_string();
            err.message = "approval wait was interrupted".to_string();
        }
        send_response_error(entry.start_tx.take(), entry.chunk_tx, err.clone());
    }

    let drained = {
        let mut pending = state.inner.pending_mcp.lock().await;
        let request_ids = pending
            .iter()
            .filter(|(_, entry)| entry.worker_id == worker_id)
            .map(|(request_id, _)| request_id.clone())
            .collect::<Vec<_>>();
        request_ids
            .into_iter()
            .filter_map(|request_id| pending.remove(&request_id).map(|entry| (request_id, entry)))
            .collect::<Vec<_>>()
    };
    if !drained.is_empty() {
        warn!(
            category = "mcp_bridge_diag",
            worker_id,
            pending_mcp_requests = drained.len(),
            error_code = %err.code,
            "failing pending MCP requests after worker bridge disconnect"
        );
    }
    for (request_id, mut entry) in drained {
        err.request_id = request_id;
        send_mcp_response_error(entry.start_tx.take(), entry.chunk_tx, err.clone());
    }

    let drained = {
        let mut pending = state.inner.pending_realtime_sessions.lock().await;
        let request_ids = pending
            .iter()
            .filter(|(_, entry)| entry.worker_id == worker_id)
            .map(|(request_id, _)| request_id.clone())
            .collect::<Vec<_>>();
        request_ids
            .into_iter()
            .filter_map(|request_id| pending.remove(&request_id).map(|entry| (request_id, entry)))
            .collect::<Vec<_>>()
    };
    for (request_id, entry) in drained {
        err.request_id = request_id;
        let _ = entry.event_tx.try_send(Err(err.clone()));
    }
}

async fn release_worker(state: &AppState, worker_id: usize) {
    let mut loads = state.inner.worker_loads.lock().await;
    if let Some(load) = loads.get_mut(&worker_id) {
        *load = load.saturating_sub(1);
    }
}

fn send_response_error(
    start_tx: Option<tokio::sync::oneshot::Sender<Result<ResponseStart, ResponseError>>>,
    chunk_tx: tokio::sync::mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    err: ResponseError,
) {
    if let Some(tx) = start_tx {
        let _ = tx.send(Err(err));
    } else {
        let _ = chunk_tx.try_send(Err(err));
    }
}

fn send_mcp_response_error(
    start_tx: Option<tokio::sync::oneshot::Sender<Result<McpResponseStart, ResponseError>>>,
    chunk_tx: tokio::sync::mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    err: ResponseError,
) {
    if let Some(tx) = start_tx {
        let _ = tx.send(Err(err));
    } else {
        let _ = chunk_tx.try_send(Err(err));
    }
}

fn send_request_cancel(worker: &WorkerSender, request_id: &str, reason: &str) {
    let _ = worker.try_send(BridgeMessage::RequestCancel(BridgeRequestCancel {
        request_id: request_id.to_string(),
        reason: reason.to_string(),
    }));
}

fn send_mcp_request_cancel(worker: &WorkerSender, request_id: &str, reason: &str) {
    let _ = worker.try_send(BridgeMessage::McpRequestCancel(McpRequestCancel {
        request_id: request_id.to_string(),
        reason: reason.to_string(),
    }));
}

fn send_realtime_close(worker: &WorkerSender, request_id: &str, reason: &str) {
    let _ = worker.try_send(BridgeMessage::RealtimeSessionClose(RealtimeSessionClose {
        request_id: request_id.to_string(),
        code: None,
        reason: Some(reason.to_string()),
    }));
}

fn backpressure_error(request_id: &str) -> ResponseError {
    ResponseError {
        request_id: request_id.to_string(),
        status: StatusCode::BAD_GATEWAY.as_u16(),
        code: "bridge_backpressure".to_string(),
        message: "relay response queue exceeded its configured limit".to_string(),
    }
}

pub(crate) fn request_deadline_unix_ms(config: &RelayConfig) -> i64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    now.saturating_add((config.request_timeout_seconds as i64).saturating_mul(1_000))
}

pub(crate) fn bridge_error_response(err: ResponseError) -> Response {
    let status = StatusCode::from_u16(err.status).unwrap_or(StatusCode::BAD_GATEWAY);
    error_response(status, &err.code, &err.message)
}

#[cfg(test)]
mod tests {
    use super::{handle_response_chunk, remove_mcp_pending};
    use crate::{
        protocol::{BridgeMessage, ResponseChunk},
        relay::state::{PendingMcpRequest, PendingRequest, test_state},
    };
    use tokio::sync::{mpsc, oneshot};

    #[tokio::test]
    async fn downstream_mcp_stream_removal_cancels_the_worker_request() {
        let state = test_state();
        let (worker, mut worker_rx) = mpsc::channel(1);
        let (start_tx, _start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(1);
        state.inner.pending_mcp.lock().await.insert(
            "request-1".to_string(),
            PendingMcpRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                worker_id: 7,
                worker,
                queued_bytes: 0,
            },
        );

        remove_mcp_pending(&state, "request-1").await;

        assert!(
            !state
                .inner
                .pending_mcp
                .lock()
                .await
                .contains_key("request-1")
        );
        assert_eq!(
            worker_rx.recv().await,
            Some(BridgeMessage::McpRequestCancel(
                crate::protocol::McpRequestCancel {
                    request_id: "request-1".to_string(),
                    reason: "request_cancelled".to_string(),
                }
            ))
        );
    }

    #[tokio::test]
    async fn full_response_queue_cancels_with_backpressure_reason() {
        let mut state = test_state();
        state.config.response_stream_buffer = 1;
        state.config.response_stream_max_bytes = 1024;
        let (worker, mut worker_rx) = mpsc::channel(1);
        let (start_tx, _start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(1);
        state.inner.pending.lock().await.insert(
            "request-1".to_string(),
            PendingRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                worker_id: 7,
                worker,
                queued_bytes: 0,
                awaiting_approval: false,
            },
        );

        handle_response_chunk(
            &state,
            ResponseChunk {
                request_id: "request-1".to_string(),
                data: vec![1],
            },
        )
        .await;
        handle_response_chunk(
            &state,
            ResponseChunk {
                request_id: "request-1".to_string(),
                data: vec![2],
            },
        )
        .await;

        assert!(!state.inner.pending.lock().await.contains_key("request-1"));
        assert_eq!(
            worker_rx.recv().await,
            Some(BridgeMessage::RequestCancel(
                crate::protocol::BridgeRequestCancel {
                    request_id: "request-1".to_string(),
                    reason: "bridge_backpressure".to_string(),
                }
            ))
        );
    }

    #[tokio::test]
    async fn closed_response_queue_cancels_as_downstream_disconnect() {
        let state = test_state();
        let (worker, mut worker_rx) = mpsc::channel(1);
        let (start_tx, _start_rx) = oneshot::channel();
        let (chunk_tx, chunk_rx) = mpsc::channel(1);
        drop(chunk_rx);
        state.inner.pending.lock().await.insert(
            "request-1".to_string(),
            PendingRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                worker_id: 7,
                worker,
                queued_bytes: 0,
                awaiting_approval: false,
            },
        );

        handle_response_chunk(
            &state,
            ResponseChunk {
                request_id: "request-1".to_string(),
                data: vec![1],
            },
        )
        .await;

        assert!(!state.inner.pending.lock().await.contains_key("request-1"));
        assert_eq!(
            worker_rx.recv().await,
            Some(BridgeMessage::RequestCancel(
                crate::protocol::BridgeRequestCancel {
                    request_id: "request-1".to_string(),
                    reason: "request_cancelled".to_string(),
                }
            ))
        );
    }

    #[tokio::test]
    async fn response_byte_limit_returns_backpressure_error() {
        let mut state = test_state();
        state.config.response_stream_buffer = 4;
        state.config.response_stream_max_bytes = 1;
        let (worker, mut worker_rx) = mpsc::channel(1);
        let (start_tx, start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(4);
        state.inner.pending.lock().await.insert(
            "request-1".to_string(),
            PendingRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                worker_id: 7,
                worker,
                queued_bytes: 0,
                awaiting_approval: false,
            },
        );

        handle_response_chunk(
            &state,
            ResponseChunk {
                request_id: "request-1".to_string(),
                data: vec![1, 2],
            },
        )
        .await;

        let error = start_rx.await.unwrap().unwrap_err();
        assert_eq!(error.code, "bridge_backpressure");
        assert_eq!(
            worker_rx.recv().await,
            Some(BridgeMessage::RequestCancel(
                crate::protocol::BridgeRequestCancel {
                    request_id: "request-1".to_string(),
                    reason: "bridge_backpressure".to_string(),
                }
            ))
        );
    }
}
