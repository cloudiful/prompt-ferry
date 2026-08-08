use crate::protocol::{
    BridgeMessage, BridgeRequestStart, McpRequestStart, McpResponseChunk, McpResponseEnd,
    McpResponseStart, ResponseError,
};
use reqwest::StatusCode;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Mutex, Notify, mpsc, oneshot};

#[derive(Clone, Debug, Default)]
pub(super) struct RequestCancellation {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl RequestCancellation {
    pub(super) fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::Release) {
            self.notify.notify_waiters();
        }
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(super) async fn cancelled(&self) {
        while !self.is_cancelled() {
            self.notify.notified().await;
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingIncomingRequest {
    pub(super) chunk_tx: mpsc::Sender<Vec<u8>>,
    pub(super) end_tx: Option<oneshot::Sender<RequestTransferStats>>,
    pub(super) cancellation: RequestCancellation,
}

#[derive(Debug, Clone, Default)]
pub(super) struct RequestTransferStats {
    pub(super) http_request_compressed_bytes: Option<i64>,
    pub(super) http_request_decompressed_bytes: Option<i64>,
    pub(super) http_request_compression_ratio: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct BufferedBridgeRequest {
    pub(super) request_id: String,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
    pub(super) request_deadline_unix_ms: i64,
    pub(super) user_id: Option<i64>,
    pub(super) client_key_hash: Option<String>,
    pub(super) request_user_agent: Option<String>,
    pub(super) http_request_content_encoding: Option<String>,
    pub(super) http_request_compressed: bool,
    pub(super) http_request_compressed_bytes: Option<i64>,
    pub(super) http_request_decompressed_bytes: Option<i64>,
    pub(super) http_request_compression_ratio: Option<f64>,
}

impl BufferedBridgeRequest {
    pub(super) fn from_parts(
        start: BridgeRequestStart,
        body: Vec<u8>,
        stats: RequestTransferStats,
    ) -> Self {
        let decompressed_bytes = stats
            .http_request_decompressed_bytes
            .or_else(|| i64::try_from(body.len()).ok());
        let compressed_bytes = stats
            .http_request_compressed_bytes
            .or(start.http_request_compressed_bytes);
        let compression_ratio = stats.http_request_compression_ratio.or_else(|| {
            if start.http_request_compressed {
                compressed_bytes
                    .zip(decompressed_bytes)
                    .filter(|(compressed, _)| *compressed > 0)
                    .map(|(compressed, decompressed)| decompressed as f64 / compressed as f64)
            } else {
                None
            }
        });
        Self {
            request_id: start.request_id,
            method: start.method,
            path: start.path,
            headers: start.headers,
            body,
            request_deadline_unix_ms: start.request_deadline_unix_ms,
            user_id: start.user_id,
            client_key_hash: start.client_key_hash,
            request_user_agent: start.request_user_agent,
            http_request_content_encoding: start.http_request_content_encoding,
            http_request_compressed: start.http_request_compressed,
            http_request_compressed_bytes: compressed_bytes,
            http_request_decompressed_bytes: decompressed_bytes,
            http_request_compression_ratio: compression_ratio,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct BufferedMcpRequest {
    pub(super) request_id: String,
    pub(super) server_name: Option<String>,
    pub(super) method: String,
    pub(super) path: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
    pub(super) user_id: Option<i64>,
    pub(super) http_request_content_encoding: Option<String>,
    pub(super) http_request_compressed: bool,
    pub(super) http_request_compressed_bytes: Option<i64>,
    pub(super) http_request_decompressed_bytes: Option<i64>,
    pub(super) http_request_compression_ratio: Option<f64>,
}

impl BufferedMcpRequest {
    pub(super) fn from_parts(
        start: McpRequestStart,
        body: Vec<u8>,
        stats: RequestTransferStats,
    ) -> Self {
        let decompressed_bytes = stats
            .http_request_decompressed_bytes
            .or_else(|| i64::try_from(body.len()).ok());
        let compressed_bytes = stats
            .http_request_compressed_bytes
            .or(start.http_request_compressed_bytes);
        let compression_ratio = stats.http_request_compression_ratio.or_else(|| {
            if start.http_request_compressed {
                compressed_bytes
                    .zip(decompressed_bytes)
                    .filter(|(compressed, _)| *compressed > 0)
                    .map(|(compressed, decompressed)| decompressed as f64 / compressed as f64)
            } else {
                None
            }
        });
        Self {
            request_id: start.request_id,
            server_name: start.server_name,
            method: start.method,
            path: start.path,
            headers: start.headers,
            body,
            user_id: start.user_id,
            http_request_content_encoding: start.http_request_content_encoding,
            http_request_compressed: start.http_request_compressed,
            http_request_compressed_bytes: compressed_bytes,
            http_request_decompressed_bytes: decompressed_bytes,
            http_request_compression_ratio: compression_ratio,
        }
    }
}

pub(super) async fn forward_request_chunk(
    pending: &Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    request_id: String,
    data: Vec<u8>,
) {
    let sender = pending
        .lock()
        .await
        .get(&request_id)
        .map(|pending| pending.chunk_tx.clone());
    if let Some(sender) = sender {
        let _ = sender.send(data).await;
    }
}

/// Outcome of collecting a bridged request body.
pub(super) enum ChunkCollection {
    Complete(Vec<u8>),
    /// The assembled body exceeded `limit`; the caller should respond 413.
    TooLarge,
}

pub(super) async fn collect_request_chunks(
    pending: &Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    request_id: &str,
    mut chunk_rx: mpsc::Receiver<Vec<u8>>,
    end_rx: oneshot::Receiver<RequestTransferStats>,
    limit: usize,
) -> (ChunkCollection, RequestTransferStats) {
    let mut body = Vec::new();
    while let Some(chunk) = chunk_rx.recv().await {
        if body.len().saturating_add(chunk.len()) > limit {
            pending.lock().await.remove(request_id);
            let stats = end_rx.await.unwrap_or_default();
            return (ChunkCollection::TooLarge, stats);
        }
        body.extend_from_slice(&chunk);
    }
    pending.lock().await.remove(request_id);
    let stats = end_rx.await.unwrap_or_default();
    (ChunkCollection::Complete(body), stats)
}

pub(super) async fn send_worker_shutdown_response(
    out_tx: &super::context::BridgeSender,
    request_id: &str,
) {
    let _ = out_tx
        .send(BridgeMessage::ResponseError(ResponseError {
            request_id: request_id.to_string(),
            status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            code: "worker_shutting_down".to_string(),
            message: "worker is shutting down and no longer accepts new requests".to_string(),
        }))
        .await;
}

pub(super) async fn send_worker_shutdown_mcp_response(
    out_tx: &super::context::BridgeSender,
    request_id: &str,
) {
    let body = serde_json::json!({
        "error": {
            "code": "worker_shutting_down",
            "message": "worker is shutting down and no longer accepts new requests",
        }
    })
    .to_string();
    let _ = out_tx
        .send(BridgeMessage::McpResponseStart(McpResponseStart {
            request_id: request_id.to_string(),
            status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .await;
    let _ = out_tx
        .send(BridgeMessage::McpResponseChunk(McpResponseChunk {
            request_id: request_id.to_string(),
            data: body.into_bytes(),
        }))
        .await;
    let _ = out_tx
        .send(BridgeMessage::McpResponseEnd(McpResponseEnd {
            request_id: request_id.to_string(),
        }))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collect_request_chunks_stops_at_limit_without_buffering_more() {
        let pending: Arc<Mutex<HashMap<String, PendingIncomingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (chunk_tx, chunk_rx) = mpsc::channel(4);
        let (end_tx, end_rx) = oneshot::channel();
        pending.lock().await.insert(
            "req-1".to_string(),
            PendingIncomingRequest {
                chunk_tx,
                end_tx: Some(end_tx),
                cancellation: RequestCancellation::default(),
            },
        );

        let pending_for_collector = pending.clone();
        let collector = tokio::spawn(async move {
            collect_request_chunks(&pending_for_collector, "req-1", chunk_rx, end_rx, 1024).await
        });
        forward_request_chunk(&pending, "req-1".to_string(), vec![b'a'; 600]).await;
        forward_request_chunk(&pending, "req-1".to_string(), vec![b'b'; 600]).await;

        let (collection, stats) = collector.await.unwrap();
        assert!(matches!(collection, ChunkCollection::TooLarge));
        assert_eq!(stats.http_request_decompressed_bytes, None);
        assert!(
            pending.lock().await.get("req-1").is_none(),
            "pending entry must be removed when the limit is hit"
        );
    }
}
