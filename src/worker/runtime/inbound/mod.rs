mod cancellation;
mod mcp;

use super::{
    REQUEST_STREAM_BUFFER, RealtimeInboundMessage, RequestTransferStats, ai::process_request,
    ai::realtime::start_realtime_session, collect_request_chunks, forward_request_chunk,
    safe_error, send_worker_shutdown_mcp_response, send_worker_shutdown_response,
};
use crate::protocol::{
    ApprovalPending, BridgeMessage, McpResponseChunk, McpResponseEnd, McpResponseStart,
    ResponseError,
};
use reqwest::StatusCode;
use tokio::sync::{mpsc, oneshot};

use super::context::{RuntimeServices, is_bridge_send_error};
use super::request_assembly::{
    BufferedBridgeRequest, ChunkCollection, PendingIncomingRequest, RequestCancellation,
};

pub(super) async fn handle_relay_bridge_message(
    message: BridgeMessage,
    config: &crate::config::WorkerConfig,
    services: &RuntimeServices,
) {
    match message {
        BridgeMessage::RequestStart(request) => {
            let Some(active_guard) = services.runtime_state.try_track_request() else {
                send_worker_shutdown_response(&services.out_tx, &request.request_id).await;
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
                    crate::bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
                )
                .await;
                let ChunkCollection::Complete(body) = body else {
                    send_too_large_response(&services.out_tx, &request.request_id).await;
                    services
                        .runtime_state
                        .request_cancellations
                        .lock()
                        .await
                        .remove(&request.request_id);
                    return;
                };
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
                            }) || services
                                .standalone_state()
                                .is_some_and(|state| state.redaction_enabled());
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
                                }))
                                .await;
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
            cancellation::cancel_request(cancellation::CancelRequestParams {
                pending_requests: &services.runtime_state.pending_requests,
                cancellations: &services.runtime_state.request_cancellations,
                admin_state: services.admin_state(),
                standalone_state: services.standalone_state(),
                category: crate::db::RequestRecordCategory::Ai,
                request_id: &cancel.request_id,
                reason: &cancel.reason,
                response_started: cancel.response_started,
            })
            .await;
        }
        BridgeMessage::McpRequestStart(request) => {
            let Some(active_guard) = services.runtime_state.try_track_request() else {
                send_worker_shutdown_mcp_response(&services.out_tx, &request.request_id).await;
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
                    crate::mcp::MAX_MCP_REQUEST_BODY_BYTES,
                )
                .await;
                let ChunkCollection::Complete(body) = body else {
                    send_mcp_too_large_response(&services.out_tx, &request.request_id).await;
                    services
                        .runtime_state
                        .mcp_request_cancellations
                        .lock()
                        .await
                        .remove(&request.request_id);
                    return;
                };
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
            cancellation::cancel_request(cancellation::CancelRequestParams {
                pending_requests: &services.runtime_state.pending_mcp_requests,
                cancellations: &services.runtime_state.mcp_request_cancellations,
                admin_state: services.admin_state(),
                standalone_state: services.standalone_state(),
                category: crate::db::RequestRecordCategory::Mcp,
                request_id: &cancel.request_id,
                reason: &cancel.reason,
                response_started: cancel.response_started,
            })
            .await;
        }
        BridgeMessage::RealtimeSessionStart(request) => {
            let Some(active_guard) = services.runtime_state.try_track_request() else {
                send_worker_shutdown_response(&services.out_tx, &request.request_id).await;
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
                Box::pin(start_realtime_session(
                    request, event_rx, &config, &services,
                ))
                .await;
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
            let _ = services.out_tx.send(BridgeMessage::Pong).await;
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

async fn send_too_large_response(out_tx: &super::context::BridgeSender, request_id: &str) {
    let _ = out_tx
        .send(BridgeMessage::ResponseError(ResponseError {
            request_id: request_id.to_string(),
            status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
            code: "request_too_large".to_string(),
            message: "request body exceeds the maximum allowed size".to_string(),
        }))
        .await;
}

async fn send_mcp_too_large_response(out_tx: &super::context::BridgeSender, request_id: &str) {
    let body = serde_json::json!({
        "error": {
            "code": "request_too_large",
            "message": "request body exceeds the maximum allowed size",
        }
    })
    .to_string();
    let _ = out_tx
        .send(BridgeMessage::McpResponseStart(McpResponseStart {
            request_id: request_id.to_string(),
            status: StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
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

fn warn_unexpected() {
    tracing::warn!("relay sent unexpected response message");
}

/// Close the long-lived realtime sessions on admission shutdown through the
/// same close signal the relay path uses. They are not accepted inference
/// rounds, so they must not hold the drain open: every send shares one deadline
/// and a saturated queue is abandoned rather than awaited individually.
pub(super) async fn close_realtime_sessions(
    state: &super::WorkerRuntimeState,
    deadline: tokio::time::Instant,
) {
    let senders = {
        let sessions = state.pending_realtime_sessions.lock().await;
        sessions.values().cloned().collect::<Vec<_>>()
    };
    let sends = senders.into_iter().map(|sender| async move {
        let _ = tokio::time::timeout_at(
            deadline,
            sender.send(RealtimeInboundMessage::Close {
                code: Some(1001),
                reason: Some("worker is shutting down".to_string()),
            }),
        )
        .await;
    });
    futures::future::join_all(sends).await;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::WorkerRuntimeState;
    use super::super::context::{BridgeSender, ResponseLimits, RuntimeServices};
    use super::handle_relay_bridge_message;
    use crate::protocol::{
        BridgeMessage, BridgeRequestStart, McpRequestCancel, McpRequestEnd, McpRequestStart,
        McpResponseStart, ResponseError,
    };
    use reqwest::StatusCode;

    fn request_start(request_id: &str) -> BridgeMessage {
        BridgeMessage::RequestStart(BridgeRequestStart {
            request_id: request_id.to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            headers: Vec::new(),
            request_deadline_unix_ms: 0,
            user_id: None,
            route_id: None,
            client_key_hash: None,
            request_user_agent: None,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
        })
    }

    fn mcp_start(request_id: &str) -> BridgeMessage {
        BridgeMessage::McpRequestStart(McpRequestStart {
            request_id: request_id.to_string(),
            server_name: None,
            method: "POST".to_string(),
            path: "/mcp".to_string(),
            headers: Vec::new(),
            user_id: None,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
        })
    }

    #[tokio::test]
    async fn ai_start_after_admission_shutdown_is_refused_without_a_guard() {
        let runtime_state = WorkerRuntimeState::default();
        runtime_state.control.begin_shutdown();
        let (sender, mut control_rx, _data_rx) = BridgeSender::channel();
        let services = RuntimeServices::new(
            None,
            sender,
            reqwest::Client::new(),
            runtime_state,
            ResponseLimits::default(),
        );

        handle_relay_bridge_message(
            request_start("refused-ai"),
            &crate::config::WorkerConfig::default(),
            &services,
        )
        .await;

        let message = tokio::time::timeout(Duration::from_secs(1), control_rx.recv())
            .await
            .expect("refusal arrives")
            .expect("control channel open");
        let BridgeMessage::ResponseError(ResponseError {
            request_id,
            status,
            code,
            ..
        }) = message
        else {
            panic!("unexpected refusal message: {message:?}");
        };
        assert_eq!(request_id, "refused-ai");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE.as_u16());
        assert_eq!(code, "worker_shutting_down");
        assert_eq!(
            services
                .runtime_state
                .control
                .wait_for_drain(Duration::from_millis(10))
                .await,
            0,
            "a refused start must not hold a drain guard"
        );
    }

    #[tokio::test]
    async fn mcp_start_after_admission_shutdown_is_refused_without_a_guard() {
        let runtime_state = WorkerRuntimeState::default();
        runtime_state.control.begin_shutdown();
        let (sender, _control_rx, mut data_rx) = BridgeSender::channel();
        let services = RuntimeServices::new(
            None,
            sender,
            reqwest::Client::new(),
            runtime_state,
            ResponseLimits::default(),
        );

        handle_relay_bridge_message(
            mcp_start("refused-mcp"),
            &crate::config::WorkerConfig::default(),
            &services,
        )
        .await;

        let start = tokio::time::timeout(Duration::from_secs(1), data_rx.recv())
            .await
            .expect("refusal arrives")
            .expect("data channel open");
        let BridgeMessage::McpResponseStart(McpResponseStart {
            request_id, status, ..
        }) = start.message
        else {
            panic!("unexpected MCP refusal");
        };
        assert_eq!(request_id, "refused-mcp");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE.as_u16());
        assert_eq!(
            services
                .runtime_state
                .control
                .wait_for_drain(Duration::from_millis(10))
                .await,
            0,
            "a refused MCP start must not hold a drain guard"
        );
    }

    #[tokio::test]
    async fn saturated_realtime_queues_share_one_bounded_close_budget() {
        let runtime_state = WorkerRuntimeState::default();
        let mut stalled_receivers = Vec::new();
        for index in 0..3 {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            sender
                .send(super::RealtimeInboundMessage::Event("queued".to_string()))
                .await
                .expect("fill the realtime queue");
            runtime_state
                .pending_realtime_sessions
                .lock()
                .await
                .insert(format!("stalled-realtime-{index}"), sender);
            // Keep each stalled consumer alive without draining its queue.
            stalled_receivers.push(receiver);
        }

        let started = tokio::time::Instant::now();
        super::close_realtime_sessions(&runtime_state, started + Duration::from_millis(30)).await;
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "N saturated realtime queues must not multiply the shared deadline: {:?}",
            started.elapsed()
        );
        drop(stalled_receivers);
    }

    #[tokio::test]
    async fn admitted_mcp_round_holds_the_drain_guard_until_it_finishes() {
        let runtime_state = WorkerRuntimeState::default();
        let (sender, _control_rx, _data_rx) = BridgeSender::channel();
        let services = RuntimeServices::new(
            None,
            sender,
            reqwest::Client::new(),
            runtime_state,
            ResponseLimits::default(),
        );

        handle_relay_bridge_message(
            mcp_start("admitted-mcp"),
            &crate::config::WorkerConfig::default(),
            &services,
        )
        .await;
        assert_eq!(
            services
                .runtime_state
                .control
                .wait_for_drain(Duration::from_millis(30))
                .await,
            1,
            "an admitted MCP round must hold the drain guard"
        );

        handle_relay_bridge_message(
            BridgeMessage::McpRequestCancel(McpRequestCancel {
                request_id: "admitted-mcp".to_string(),
                reason: "client_cancel".to_string(),
                response_started: false,
            }),
            &crate::config::WorkerConfig::default(),
            &services,
        )
        .await;
        handle_relay_bridge_message(
            BridgeMessage::McpRequestEnd(McpRequestEnd {
                request_id: "admitted-mcp".to_string(),
                http_request_compressed_bytes: None,
                http_request_decompressed_bytes: None,
                http_request_compression_ratio: None,
            }),
            &crate::config::WorkerConfig::default(),
            &services,
        )
        .await;

        let remaining = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let remaining = services
                    .runtime_state
                    .control
                    .wait_for_drain(Duration::from_millis(10))
                    .await;
                if remaining == 0 {
                    return remaining;
                }
            }
        })
        .await
        .expect("cancelled MCP round releases its guard");
        assert_eq!(remaining, 0);
    }
}
