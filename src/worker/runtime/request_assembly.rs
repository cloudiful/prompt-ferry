use crate::protocol::{
    BridgeMessage, BridgeRequestStart, McpRequestStart, McpResponseChunk, McpResponseEnd,
    McpResponseStart, ResponseError,
};
use reqwest::StatusCode;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, mpsc, oneshot};

#[derive(Debug)]
pub(super) struct PendingIncomingRequest {
    pub(super) chunk_tx: mpsc::Sender<Vec<u8>>,
    pub(super) end_tx: Option<oneshot::Sender<RequestTransferStats>>,
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

pub(super) async fn collect_request_chunks(
    pending: &Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    request_id: &str,
    mut chunk_rx: mpsc::Receiver<Vec<u8>>,
    end_rx: oneshot::Receiver<RequestTransferStats>,
) -> (Vec<u8>, RequestTransferStats) {
    let mut body = Vec::new();
    while let Some(chunk) = chunk_rx.recv().await {
        body.extend_from_slice(&chunk);
    }
    pending.lock().await.remove(request_id);
    let stats = end_rx.await.unwrap_or_default();
    (body, stats)
}

pub(super) fn send_worker_shutdown_response(
    out_tx: &mpsc::UnboundedSender<BridgeMessage>,
    request_id: &str,
) {
    let _ = out_tx.send(BridgeMessage::ResponseError(ResponseError {
        request_id: request_id.to_string(),
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        code: "worker_shutting_down".to_string(),
        message: "worker is shutting down and no longer accepts new requests".to_string(),
    }));
}

pub(super) fn send_worker_shutdown_mcp_response(
    out_tx: &mpsc::UnboundedSender<BridgeMessage>,
    request_id: &str,
) {
    let body = serde_json::json!({
        "error": {
            "code": "worker_shutting_down",
            "message": "worker is shutting down and no longer accepts new requests",
        }
    })
    .to_string();
    let _ = out_tx.send(BridgeMessage::McpResponseStart(McpResponseStart {
        request_id: request_id.to_string(),
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        content_type: Some("application/json".to_string()),
        headers: Vec::new(),
    }));
    let _ = out_tx.send(BridgeMessage::McpResponseChunk(McpResponseChunk {
        request_id: request_id.to_string(),
        data: body.into_bytes(),
    }));
    let _ = out_tx.send(BridgeMessage::McpResponseEnd(McpResponseEnd {
        request_id: request_id.to_string(),
    }));
}
