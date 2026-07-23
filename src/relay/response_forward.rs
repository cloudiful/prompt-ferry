use crate::{
    auth::error_response,
    config::RelayConfig,
    protocol::{
        McpResponseChunk, McpResponseEnd, McpResponseStart, RealtimeServerEventMessage,
        RealtimeSessionClose, ResponseChunk, ResponseEnd, ResponseError, ResponseStart,
    },
};
use axum::{http::StatusCode, response::Response};
use std::time::{SystemTime, UNIX_EPOCH};

use super::state::{AppState, WorkerSender};

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
    let tx = {
        let pending = state.inner.pending.lock().await;
        pending.get(&request_id).map(|entry| entry.chunk_tx.clone())
    };
    if let Some(tx) = tx
        && tx.send(Ok(chunk.data)).await.is_err()
    {
        remove_pending(state, &request_id).await;
    }
}

pub(crate) async fn handle_response_end(state: &AppState, end: ResponseEnd) {
    remove_pending(state, &end.request_id).await;
}

pub(crate) async fn handle_response_error(state: &AppState, err: ResponseError) {
    let mut pending = state.inner.pending.lock().await;
    if let Some(mut entry) = pending.remove(&err.request_id) {
        if let Some(tx) = entry.start_tx.take() {
            let _ = tx.send(Err(err));
        } else {
            drop(pending);
            let _ = entry.chunk_tx.send(Err(err)).await;
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
    let tx = {
        let pending = state.inner.pending_mcp.lock().await;
        pending.get(&request_id).map(|entry| entry.chunk_tx.clone())
    };
    if let Some(tx) = tx
        && tx.send(Ok(chunk.data)).await.is_err()
    {
        state.inner.pending_mcp.lock().await.remove(&request_id);
    }
}

pub(crate) async fn handle_mcp_response_end(state: &AppState, end: McpResponseEnd) {
    state.inner.pending_mcp.lock().await.remove(&end.request_id);
}

pub(crate) async fn handle_mcp_response_error(state: &AppState, err: ResponseError) {
    let mut pending = state.inner.pending_mcp.lock().await;
    if let Some(mut entry) = pending.remove(&err.request_id) {
        if let Some(tx) = entry.start_tx.take() {
            let _ = tx.send(Err(err));
        } else {
            drop(pending);
            let _ = entry.chunk_tx.send(Err(err)).await;
        }
    }
}

pub(crate) async fn handle_realtime_server_event(
    state: &AppState,
    event: RealtimeServerEventMessage,
) {
    let request_id = event.request_id.clone();
    let tx = {
        let pending = state.inner.pending_realtime_sessions.lock().await;
        pending.get(&request_id).map(|entry| entry.event_tx.clone())
    };
    if let Some(tx) = tx
        && tx.send(Ok(event)).await.is_err()
    {
        state
            .inner
            .pending_realtime_sessions
            .lock()
            .await
            .remove(&request_id);
    }
}

pub(crate) async fn handle_realtime_session_close(state: &AppState, close: RealtimeSessionClose) {
    state
        .inner
        .pending_realtime_sessions
        .lock()
        .await
        .remove(&close.request_id);
}

pub(crate) async fn handle_realtime_session_error(state: &AppState, err: ResponseError) {
    let mut pending = state.inner.pending_realtime_sessions.lock().await;
    if let Some(entry) = pending.remove(&err.request_id) {
        drop(pending);
        let _ = entry.event_tx.send(Err(err)).await;
    }
}

pub(crate) async fn choose_worker(state: &AppState) -> Option<WorkerSender> {
    state.inner.workers.lock().await.values().next().cloned()
}

pub(crate) async fn remove_pending(state: &AppState, request_id: &str) {
    state.inner.pending.lock().await.remove(request_id);
}

pub(crate) async fn fail_all_pending(state: &AppState, mut err: ResponseError) {
    let drained = {
        let mut pending = state.inner.pending.lock().await;
        pending.drain().collect::<Vec<_>>()
    };
    for (request_id, mut entry) in drained {
        err.request_id = request_id;
        if entry.awaiting_approval {
            err.status = StatusCode::SERVICE_UNAVAILABLE.as_u16();
            err.code = "approval_interrupted".to_string();
            err.message = "approval wait was interrupted".to_string();
        } else {
            err.status = StatusCode::BAD_GATEWAY.as_u16();
            err.code = "worker_disconnected".to_string();
            err.message = "worker disconnected".to_string();
        }
        if let Some(tx) = entry.start_tx.take() {
            let _ = tx.send(Err(err.clone()));
        } else {
            let _ = entry.chunk_tx.send(Err(err.clone())).await;
        }
    }

    let realtime_drained = {
        let mut realtime_pending = state.inner.pending_realtime_sessions.lock().await;
        realtime_pending.drain().collect::<Vec<_>>()
    };
    for (request_id, entry) in realtime_drained {
        err.request_id = request_id;
        err.status = StatusCode::BAD_GATEWAY.as_u16();
        err.code = "worker_disconnected".to_string();
        err.message = "worker disconnected".to_string();
        let _ = entry.event_tx.send(Err(err.clone())).await;
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
