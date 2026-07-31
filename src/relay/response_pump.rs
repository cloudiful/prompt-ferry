use std::time::Duration;

use tokio::sync::mpsc;
use tracing::warn;

use super::{
    response_forward::{
        backpressure_error, finish_mcp_pending, finish_pending, finish_realtime_pending,
        release_worker, send_mcp_request_cancel, send_mcp_response_error, send_realtime_close,
        send_request_cancel, send_response_error,
    },
    response_queue::{EnqueueResult, send_with_backpressure},
    state::{
        AppState, ForwardedRealtimeItem, ForwardedResponseItem, QueuedRealtimeEvent,
        QueuedResponseChunk, WorkerSender,
    },
};
use crate::protocol::ResponseError;

pub(crate) fn spawn_response_pump(
    state: AppState,
    request_id: String,
    worker_id: usize,
    worker: WorkerSender,
    mut forward_rx: mpsc::UnboundedReceiver<ForwardedResponseItem>,
    chunk_tx: mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    timeout: Duration,
) {
    tokio::spawn(async move {
        let mut forwarded_chunks = 0usize;
        let mut forwarded_bytes = 0usize;
        while let Some(item) = forward_rx.recv().await {
            let is_error = matches!(&item, ForwardedResponseItem::Error(_));
            let bytes = match &item {
                ForwardedResponseItem::Chunk(chunk) => chunk.data.len(),
                ForwardedResponseItem::Error(_) => 0,
            };
            let result = send_with_backpressure(
                &chunk_tx,
                match item {
                    ForwardedResponseItem::Chunk(chunk) => Ok(chunk),
                    ForwardedResponseItem::Error(error) => Err(error),
                },
                timeout,
            )
            .await;
            match result {
                EnqueueResult::Enqueued => {
                    if !is_error {
                        forwarded_chunks += 1;
                        forwarded_bytes += bytes;
                    } else {
                        finish_pending(&state, &request_id).await;
                        return;
                    }
                }
                EnqueueResult::Closed => {
                    abort_response(
                        &state,
                        &request_id,
                        worker_id,
                        &worker,
                        "downstream_closed",
                        forwarded_chunks,
                        forwarded_bytes,
                    )
                    .await;
                    return;
                }
                EnqueueResult::Full | EnqueueResult::BytesLimit => {
                    abort_response(
                        &state,
                        &request_id,
                        worker_id,
                        &worker,
                        "bridge_backpressure_full",
                        forwarded_chunks,
                        forwarded_bytes,
                    )
                    .await;
                    return;
                }
            }
        }
    });
}

pub(crate) fn spawn_mcp_response_pump(
    state: AppState,
    request_id: String,
    worker_id: usize,
    worker: WorkerSender,
    mut forward_rx: mpsc::UnboundedReceiver<ForwardedResponseItem>,
    chunk_tx: mpsc::Sender<Result<QueuedResponseChunk, ResponseError>>,
    timeout: Duration,
) {
    tokio::spawn(async move {
        let mut forwarded_chunks = 0usize;
        let mut forwarded_bytes = 0usize;
        while let Some(item) = forward_rx.recv().await {
            let is_error = matches!(&item, ForwardedResponseItem::Error(_));
            let bytes = match &item {
                ForwardedResponseItem::Chunk(chunk) => chunk.data.len(),
                ForwardedResponseItem::Error(_) => 0,
            };
            let result = send_with_backpressure(
                &chunk_tx,
                match item {
                    ForwardedResponseItem::Chunk(chunk) => Ok(chunk),
                    ForwardedResponseItem::Error(error) => Err(error),
                },
                timeout,
            )
            .await;
            match result {
                EnqueueResult::Enqueued => {
                    if !is_error {
                        forwarded_chunks += 1;
                        forwarded_bytes += bytes;
                    } else {
                        finish_mcp_pending(&state, &request_id).await;
                        return;
                    }
                }
                EnqueueResult::Closed => {
                    abort_mcp_response(
                        &state,
                        &request_id,
                        worker_id,
                        &worker,
                        "downstream_closed",
                        forwarded_chunks,
                        forwarded_bytes,
                    )
                    .await;
                    return;
                }
                EnqueueResult::Full | EnqueueResult::BytesLimit => {
                    abort_mcp_response(
                        &state,
                        &request_id,
                        worker_id,
                        &worker,
                        "bridge_backpressure_full",
                        forwarded_chunks,
                        forwarded_bytes,
                    )
                    .await;
                    return;
                }
            }
        }
    });
}

pub(crate) fn spawn_realtime_response_pump(
    state: AppState,
    request_id: String,
    worker_id: usize,
    worker: WorkerSender,
    mut forward_rx: mpsc::UnboundedReceiver<ForwardedRealtimeItem>,
    event_tx: mpsc::Sender<Result<QueuedRealtimeEvent, crate::protocol::ResponseError>>,
    timeout: Duration,
) {
    tokio::spawn(async move {
        let mut forwarded_chunks = 0usize;
        let mut forwarded_bytes = 0usize;
        while let Some(item) = forward_rx.recv().await {
            let is_error = matches!(&item, ForwardedRealtimeItem::Error(_));
            let bytes = match &item {
                ForwardedRealtimeItem::Event(event) => event.event.event_json.len(),
                ForwardedRealtimeItem::Error(_) => 0,
            };
            let result = send_with_backpressure(
                &event_tx,
                match item {
                    ForwardedRealtimeItem::Event(event) => Ok(event),
                    ForwardedRealtimeItem::Error(error) => Err(error),
                },
                timeout,
            )
            .await;
            match result {
                EnqueueResult::Enqueued => {
                    if !is_error {
                        forwarded_chunks += 1;
                        forwarded_bytes += bytes;
                    } else {
                        finish_realtime_pending(&state, &request_id).await;
                        return;
                    }
                }
                EnqueueResult::Closed => {
                    abort_realtime_response(
                        &state,
                        &request_id,
                        worker_id,
                        &worker,
                        "downstream_closed",
                        forwarded_chunks,
                        forwarded_bytes,
                    )
                    .await;
                    return;
                }
                EnqueueResult::Full | EnqueueResult::BytesLimit => {
                    abort_realtime_response(
                        &state,
                        &request_id,
                        worker_id,
                        &worker,
                        "bridge_backpressure_full",
                        forwarded_chunks,
                        forwarded_bytes,
                    )
                    .await;
                    return;
                }
            }
        }
    });
}

async fn abort_response(
    state: &AppState,
    request_id: &str,
    worker_id: usize,
    worker: &WorkerSender,
    reason: &str,
    forwarded_chunks: usize,
    forwarded_bytes: usize,
) {
    let entry = state.inner.pending.lock().await.remove(request_id);
    let Some(entry) = entry else { return };
    let response_started = entry.response_started;
    release_worker(state, entry.worker_id).await;
    send_request_cancel(worker, request_id, reason, response_started);
    if reason != "downstream_closed" {
        send_response_error(
            entry.start_tx,
            entry.chunk_tx,
            backpressure_error(request_id),
        );
    }
    warn!(
        category = "relay_bridge_diag",
        request_id,
        worker_id,
        queue_result = reason,
        response_started,
        forwarded_chunks,
        forwarded_bytes,
        "response forwarding pump stopped"
    );
}

async fn abort_mcp_response(
    state: &AppState,
    request_id: &str,
    worker_id: usize,
    worker: &WorkerSender,
    reason: &str,
    forwarded_chunks: usize,
    forwarded_bytes: usize,
) {
    let entry = state.inner.pending_mcp.lock().await.remove(request_id);
    let Some(entry) = entry else { return };
    let response_started = entry.response_started;
    release_worker(state, entry.worker_id).await;
    send_mcp_request_cancel(worker, request_id, reason, response_started);
    if reason != "downstream_closed" {
        send_mcp_response_error(
            entry.start_tx,
            entry.chunk_tx,
            backpressure_error(request_id),
        );
    }
    warn!(
        category = "relay_bridge_diag",
        request_id,
        worker_id,
        queue_result = reason,
        response_started,
        forwarded_chunks,
        forwarded_bytes,
        "MCP response forwarding pump stopped"
    );
}

async fn abort_realtime_response(
    state: &AppState,
    request_id: &str,
    worker_id: usize,
    worker: &WorkerSender,
    reason: &str,
    forwarded_chunks: usize,
    forwarded_bytes: usize,
) {
    let entry = state
        .inner
        .pending_realtime_sessions
        .lock()
        .await
        .remove(request_id);
    let Some(entry) = entry else { return };
    let response_started = entry.response_started;
    release_worker(state, entry.worker_id).await;
    send_realtime_close(worker, request_id, reason, response_started);
    if reason != "downstream_closed" {
        let _ = entry.event_tx.try_send(Err(backpressure_error(request_id)));
    }
    warn!(
        category = "relay_bridge_diag",
        request_id,
        worker_id,
        queue_result = reason,
        response_started,
        forwarded_chunks,
        forwarded_bytes,
        "realtime response forwarding pump stopped"
    );
}

#[cfg(test)]
mod tests {
    use super::spawn_response_pump;
    use crate::{
        protocol::BridgeMessage,
        relay::state::{ForwardedResponseItem, PendingRequest, QueuedResponseChunk, test_state},
    };
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};

    #[tokio::test]
    async fn sustained_downstream_backpressure_times_out_per_request() {
        let mut state = test_state();
        state.config.response_stream_backpressure_timeout_ms = 10;
        let (worker, mut worker_rx) = mpsc::channel(4);
        let (start_tx, _start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(1);
        let (forward_tx, forward_rx) = mpsc::unbounded_channel();
        state.inner.pending.lock().await.insert(
            "request-1".to_string(),
            PendingRequest {
                start_tx: Some(start_tx),
                chunk_tx: chunk_tx.clone(),
                forward_tx: forward_tx.clone(),
                worker_id: 7,
                worker: worker.clone(),
                queued_bytes: 0,
                response_started: true,
                awaiting_approval: false,
            },
        );

        spawn_response_pump(
            state.clone(),
            "request-1".to_string(),
            7,
            worker,
            forward_rx,
            chunk_tx,
            Duration::from_millis(10),
        );
        forward_tx
            .send(ForwardedResponseItem::Chunk(QueuedResponseChunk {
                data: vec![1],
            }))
            .expect("first chunk queued");
        forward_tx
            .send(ForwardedResponseItem::Chunk(QueuedResponseChunk {
                data: vec![2],
            }))
            .expect("second chunk queued");

        tokio::time::sleep(Duration::from_millis(40)).await;

        assert!(!state.inner.pending.lock().await.contains_key("request-1"));
        assert_eq!(
            worker_rx.recv().await,
            Some(BridgeMessage::RequestCancel(
                crate::protocol::BridgeRequestCancel {
                    request_id: "request-1".to_string(),
                    reason: "bridge_backpressure_full".to_string(),
                    response_started: true,
                }
            ))
        );
    }
}
