mod cancellation;
mod mcp;

use super::{
    REQUEST_STREAM_BUFFER, RealtimeInboundMessage, RequestTransferStats, ai::process_request,
    ai::realtime::start_realtime_session, collect_request_chunks, forward_request_chunk,
    safe_error, send_worker_shutdown_mcp_response, send_worker_shutdown_response,
};
use crate::protocol::{ApprovalPending, BridgeMessage, ResponseError};
use reqwest::StatusCode;
use tokio::sync::{mpsc, oneshot};

use super::context::{RuntimeServices, is_bridge_send_error};
use super::request_assembly::{BufferedBridgeRequest, PendingIncomingRequest, RequestCancellation};

pub(super) async fn handle_relay_bridge_message(
    message: BridgeMessage,
    config: &crate::config::WorkerConfig,
    services: &RuntimeServices,
) {
    match message {
        BridgeMessage::RequestStart(request) => {
            let Some(active_guard) = services.runtime_state.try_track_request() else {
                send_worker_shutdown_response(&services.out_tx, &request.request_id);
                return;
            };
            let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>(REQUEST_STREAM_BUFFER);
            let (end_tx, end_rx) = oneshot::channel::<RequestTransferStats>();
            let cancellation = RequestCancellation::default();
            services.runtime_state.pending_requests.lock().await.insert(
                request.request_id.clone(),
                PendingIncomingRequest {
                    chunk_tx,
                    end_tx: Some(end_tx),
                    cancellation: cancellation.clone(),
                },
            );
            services
                .runtime_state
                .request_cancellations
                .lock()
                .await
                .insert(request.request_id.clone(), cancellation.clone());
            let config = config.clone();
            let services = services.clone();
            tokio::spawn(async move {
                let _active_guard = active_guard;
                let (body, stats) = collect_request_chunks(
                    &services.runtime_state.pending_requests,
                    &request.request_id,
                    chunk_rx,
                    end_rx,
                )
                .await;
                if cancellation.is_cancelled() {
                    services
                        .runtime_state
                        .request_cancellations
                        .lock()
                        .await
                        .remove(&request.request_id);
                    return;
                }
                let request_id = request.request_id.clone();
                let request = BufferedBridgeRequest::from_parts(request, body, stats);
                tokio::select! {
                    _ = cancellation.cancelled() => {}
                    result = process_request(request.clone(), &config, &services) => {
                        if let Err(err) = result {
                            let redact_enabled = services.admin_state().is_some_and(|state| {
                                state
                                    .redaction_enabled
                                    .load(std::sync::atomic::Ordering::SeqCst)
                            });
                            let error_code = if is_bridge_send_error(&err) {
                                "relay_bridge_error"
                            } else {
                                "upstream_error"
                            };
                            let _ = services
                                .out_tx
                                .send(BridgeMessage::ResponseError(ResponseError {
                                    request_id: request_id.clone(),
                                    status: StatusCode::BAD_GATEWAY.as_u16(),
                                    code: error_code.to_string(),
                                    message: safe_error(&err, redact_enabled, request.user_id),
                                }));
                        }
                    }
                }
                services
                    .runtime_state
                    .request_cancellations
                    .lock()
                    .await
                    .remove(&request_id);
            });
        }
        BridgeMessage::RequestChunk(chunk) => {
            forward_request_chunk(
                &services.runtime_state.pending_requests,
                chunk.request_id,
                chunk.data,
            )
            .await;
        }
        BridgeMessage::RequestEnd(end) => {
            finish_request_transfer(
                &services.runtime_state.pending_requests,
                &end.request_id,
                RequestTransferStats {
                    http_request_compressed_bytes: end.http_request_compressed_bytes,
                    http_request_decompressed_bytes: end.http_request_decompressed_bytes,
                    http_request_compression_ratio: end.http_request_compression_ratio,
                },
            )
            .await;
        }
        BridgeMessage::RequestCancel(cancel) => {
            cancellation::cancel_request(
                &services.runtime_state.pending_requests,
                &services.runtime_state.request_cancellations,
                services.admin_state(),
                &cancel.request_id,
                &cancel.reason,
                cancel.response_started,
            )
            .await;
        }
        BridgeMessage::McpRequestStart(request) => {
            let Some(active_guard) = services.runtime_state.try_track_request() else {
                send_worker_shutdown_mcp_response(&services.out_tx, &request.request_id);
                return;
            };
            let (chunk_tx, chunk_rx) = mpsc::channel::<Vec<u8>>(REQUEST_STREAM_BUFFER);
            let (end_tx, end_rx) = oneshot::channel::<RequestTransferStats>();
            let cancellation = RequestCancellation::default();
            services
                .runtime_state
                .pending_mcp_requests
                .lock()
                .await
                .insert(
                    request.request_id.clone(),
                    PendingIncomingRequest {
                        chunk_tx,
                        end_tx: Some(end_tx),
                        cancellation: cancellation.clone(),
                    },
                );
            services
                .runtime_state
                .mcp_request_cancellations
                .lock()
                .await
                .insert(request.request_id.clone(), cancellation.clone());
            let services = services.clone();
            tokio::spawn(async move {
                let _active_guard = active_guard;
                let (body, stats) = collect_request_chunks(
                    &services.runtime_state.pending_mcp_requests,
                    &request.request_id,
                    chunk_rx,
                    end_rx,
                )
                .await;
                if cancellation.is_cancelled() {
                    services
                        .runtime_state
                        .mcp_request_cancellations
                        .lock()
                        .await
                        .remove(&request.request_id);
                    return;
                }
                let request_id = request.request_id.clone();
                let request =
                    super::request_assembly::BufferedMcpRequest::from_parts(request, body, stats);
                tokio::select! {
                    _ = cancellation.cancelled() => {}
                    _ = mcp::handle_mcp_request(request, &services) => {}
                }
                services
                    .runtime_state
                    .mcp_request_cancellations
                    .lock()
                    .await
                    .remove(&request_id);
            });
        }
        BridgeMessage::McpRequestChunk(chunk) => {
            forward_request_chunk(
                &services.runtime_state.pending_mcp_requests,
                chunk.request_id,
                chunk.data,
            )
            .await;
        }
        BridgeMessage::McpRequestEnd(end) => {
            finish_request_transfer(
                &services.runtime_state.pending_mcp_requests,
                &end.request_id,
                RequestTransferStats {
                    http_request_compressed_bytes: end.http_request_compressed_bytes,
                    http_request_decompressed_bytes: end.http_request_decompressed_bytes,
                    http_request_compression_ratio: end.http_request_compression_ratio,
                },
            )
            .await;
        }
        BridgeMessage::McpRequestCancel(cancel) => {
            cancellation::cancel_request(
                &services.runtime_state.pending_mcp_requests,
                &services.runtime_state.mcp_request_cancellations,
                services.admin_state(),
                &cancel.request_id,
                &cancel.reason,
                cancel.response_started,
            )
            .await;
        }
        BridgeMessage::RealtimeSessionStart(request) => {
            let Some(active_guard) = services.runtime_state.try_track_request() else {
                send_worker_shutdown_response(&services.out_tx, &request.request_id);
                return;
            };
            let (event_tx, event_rx) =
                mpsc::channel::<RealtimeInboundMessage>(super::REALTIME_INBOUND_BUFFER);
            services
                .runtime_state
                .pending_realtime_sessions
                .lock()
                .await
                .insert(request.request_id.clone(), event_tx);
            let config = config.clone();
            let services = services.clone();
            tokio::spawn(async move {
                let _active_guard = active_guard;
                start_realtime_session(request, event_rx, &config, &services).await;
            });
        }
        BridgeMessage::RealtimeClientEvent(event) => {
            let sender = services
                .runtime_state
                .pending_realtime_sessions
                .lock()
                .await
                .get(&event.request_id)
                .cloned();
            if let Some(sender) = sender {
                let _ = sender
                    .send(RealtimeInboundMessage::Event(event.event_json))
                    .await;
            }
        }
        BridgeMessage::RealtimeSessionClose(close) => {
            let sender = services
                .runtime_state
                .pending_realtime_sessions
                .lock()
                .await
                .remove(&close.request_id);
            if let Some(sender) = sender {
                let _ = sender
                    .send(RealtimeInboundMessage::Close {
                        code: close.code,
                        reason: close.reason,
                    })
                    .await;
            }
        }
        BridgeMessage::Ping => {
            let _ = services.out_tx.send(BridgeMessage::Pong);
        }
        BridgeMessage::Pong => {}
        BridgeMessage::ApprovalPending(ApprovalPending { .. })
        | BridgeMessage::RealtimeServerEvent(_)
        | BridgeMessage::ResponseStart(_)
        | BridgeMessage::ResponseChunk(_)
        | BridgeMessage::ResponseEnd(_)
        | BridgeMessage::ResponseError(_)
        | BridgeMessage::McpResponseStart(_)
        | BridgeMessage::McpResponseChunk(_)
        | BridgeMessage::McpResponseEnd(_)
        | BridgeMessage::ConfigSnapshot(_) => warn_unexpected(),
    }
}

async fn finish_request_transfer(
    pending_requests: &std::sync::Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<String, super::request_assembly::PendingIncomingRequest>,
        >,
    >,
    request_id: &str,
    stats: RequestTransferStats,
) {
    if let Some(mut pending) = pending_requests.lock().await.remove(request_id)
        && let Some(end_tx) = pending.end_tx.take()
    {
        let _ = end_tx.send(stats);
    }
}

fn warn_unexpected() {
    tracing::warn!("relay sent unexpected response message");
}
