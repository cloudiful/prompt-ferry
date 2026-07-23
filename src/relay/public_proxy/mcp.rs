use crate::protocol::{BridgeMessage, McpRequestChunk, McpRequestEnd, McpRequestStart};

use super::super::{
    request_compression::{HttpRequestCompressionContext, HttpRequestTransferStats},
    response_forward::{bridge_error_response, choose_worker},
    router::drain_body_then,
    state::{AppState, PendingMcpRequest, RESPONSE_STREAM_BUFFER, RemoteAddr, WorkerSender},
};
use super::{
    DownstreamStreamDiag, authorize_client, enforce_public_ip_policy, header_value, sse_error_event,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, Extension, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::Response,
};
use bytes::Bytes;
use futures::StreamExt;
use std::{net::IpAddr, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tracing::info;
use uuid::Uuid;

pub(super) async fn proxy_mcp_root(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    proxy_mcp_request(
        state,
        peer_addr.0.ip(),
        headers,
        compression,
        method,
        None,
        body,
    )
    .await
}

pub(super) async fn proxy_mcp_server(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    axum::extract::Path(server): axum::extract::Path<String>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    proxy_mcp_request(
        state,
        peer_addr.0.ip(),
        headers,
        compression,
        method,
        Some(server),
        body,
    )
    .await
}

async fn proxy_mcp_request(
    state: AppState,
    peer_ip: IpAddr,
    headers: HeaderMap,
    compression: HttpRequestCompressionContext,
    method: Method,
    server_name: Option<String>,
    body: Body,
) -> Response {
    let request_path = server_name
        .as_deref()
        .map(|server| format!("/mcp/{server}"))
        .unwrap_or_else(|| "/mcp".to_string());
    let accept = header_value(&headers, header::ACCEPT);
    let content_type = header_value(&headers, header::CONTENT_TYPE);
    let user_agent = header_value(&headers, header::USER_AGENT);
    let mcp_session_id = headers
        .get("mcp-session-id")
        .or_else(|| headers.get("MCP-Session-Id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mcp_protocol_version = headers
        .get("mcp-protocol-version")
        .or_else(|| headers.get("MCP-Protocol-Version"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    info!(
        category = "mcp_diag",
        peer_ip = %peer_ip,
        method = %method,
        path = %request_path,
        server_name = server_name.as_deref().unwrap_or("-"),
        accept = accept.as_deref().unwrap_or("-"),
        content_type = content_type.as_deref().unwrap_or("-"),
        mcp_session_id = mcp_session_id.as_deref().unwrap_or("-"),
        mcp_protocol_version = mcp_protocol_version.as_deref().unwrap_or("-"),
        user_agent = user_agent.as_deref().unwrap_or("-"),
        "mcp request received"
    );

    if let Err(response) = enforce_public_ip_policy(&state, peer_ip, &headers).await {
        return response;
    }
    let route = match authorize_client(&state, &headers).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let worker = match choose_worker(&state).await {
        Some(worker) => worker,
        None => {
            return drain_body_then(
                body,
                crate::auth::error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "no_worker",
                    "no worker is connected",
                ),
            )
            .await;
        }
    };
    let request_id = Uuid::new_v4().to_string();
    let (start_tx, start_rx) = oneshot::channel();
    let (chunk_tx, chunk_rx) = mpsc::channel(RESPONSE_STREAM_BUFFER);
    state.inner.pending_mcp.lock().await.insert(
        request_id.clone(),
        PendingMcpRequest {
            start_tx: Some(start_tx),
            chunk_tx,
        },
    );
    let request_headers = headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let start = McpRequestStart {
        request_id: request_id.clone(),
        server_name: server_name.clone(),
        method: method.to_string(),
        path: request_path.clone(),
        headers: request_headers,
        user_id: route.map(|route| route.user_id),
        http_request_content_encoding: compression.content_encoding.clone(),
        http_request_compressed: compression.compressed,
        http_request_compressed_bytes: compression.compressed_bytes,
    };
    if let Err(response) = stream_mcp_body(&worker, start, compression, body).await {
        state.inner.pending_mcp.lock().await.remove(&request_id);
        return response;
    }
    let timeout = Duration::from_secs(state.config.request_timeout_seconds);
    let start = match tokio::time::timeout(timeout, start_rx).await {
        Ok(Ok(Ok(start))) => start,
        Ok(Ok(Err(err))) => {
            state.inner.pending_mcp.lock().await.remove(&request_id);
            return bridge_error_response(err);
        }
        Ok(Err(_)) => {
            state.inner.pending_mcp.lock().await.remove(&request_id);
            return crate::auth::error_response(
                StatusCode::BAD_GATEWAY,
                "mcp_response_closed",
                "worker MCP response channel closed",
            );
        }
        Err(_) => {
            state.inner.pending_mcp.lock().await.remove(&request_id);
            return crate::auth::error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "mcp_timeout",
                "timed out waiting for MCP response",
            );
        }
    };

    let status = StatusCode::from_u16(start.status).unwrap_or(StatusCode::BAD_GATEWAY);
    info!(
        category = "mcp_diag",
        peer_ip = %peer_ip,
        method = %method,
        path = %request_path,
        server_name = server_name.as_deref().unwrap_or("-"),
        status = status.as_u16(),
        response_content_type = start.content_type.as_deref().unwrap_or("-"),
        "mcp response started"
    );
    let content_type = start.content_type.unwrap_or_else(|| {
        if method == Method::GET {
            "text/event-stream".to_string()
        } else {
            "application/json".to_string()
        }
    });
    let stream_state = state.clone();
    let stream_request_id = request_id.clone();
    let is_event_stream = content_type.contains("text/event-stream");
    let stream_path = request_path.clone();
    let stream_content_type = content_type.clone();
    let stream = async_stream::stream! {
        let mut chunk_rx = chunk_rx;
        let mut diag = DownstreamStreamDiag::new(
            "mcp",
            stream_request_id.clone(),
            stream_path,
            status.as_u16(),
            stream_content_type,
        );
        let mut emitted_chunk = false;
        while let Some(item) = chunk_rx.recv().await {
            match item {
                Ok(data) => {
                    diag.record_chunk(data.len());
                    emitted_chunk = true;
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(data));
                }
                Err(err) => {
                    if is_event_stream && emitted_chunk {
                        diag.mark_error("response_error", &err.code, &err.message);
                        break;
                    }
                    let body = if is_event_stream {
                        sse_error_event(&err.code, &err.message)
                    } else {
                        serde_json::json!({
                            "error": {
                                "code": err.code,
                                "message": err.message,
                            }
                        })
                        .to_string()
                        .into_bytes()
                    };
                    diag.mark_error("response_error", &err.code, &err.message);
                    diag.record_chunk(body.len());
                    yield Ok(Bytes::from(body));
                    break;
                }
            }
        }
        stream_state.inner.pending_mcp.lock().await.remove(&stream_request_id);
        diag.mark_completed();
        diag.finish();
    };
    let mut out = Response::new(Body::from_stream(stream));
    *out.status_mut() = status;
    if let Ok(content_type) = content_type.parse() {
        out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    }
    for (name, value) in start.headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(name.as_str()),
            header::HeaderValue::from_str(&value),
        ) {
            out.headers_mut().append(name, value);
        }
    }
    out
}

async fn stream_mcp_body(
    worker: &WorkerSender,
    start: McpRequestStart,
    compression: HttpRequestCompressionContext,
    body: Body,
) -> Result<(), Response> {
    let mut stream = body.into_data_stream();
    let request_id = start.request_id.clone();
    let mut started = false;
    let mut decompressed_bytes = 0i64;
    while let Some(next) = stream.next().await {
        let chunk = match next {
            Ok(chunk) => chunk,
            Err(err) => {
                if started {
                    let _ = send_mcp_request_end(
                        worker,
                        &request_id,
                        compression.final_stats(decompressed_bytes),
                    )
                    .await;
                }
                return Err(crate::auth::error_response(
                    StatusCode::BAD_REQUEST,
                    "request_body_read_failed",
                    &format!("failed to read MCP request body: {err}"),
                ));
            }
        };
        if !started {
            send_mcp_request_start(worker, start.clone()).await?;
            started = true;
        }
        decompressed_bytes =
            decompressed_bytes.saturating_add(i64::try_from(chunk.len()).unwrap_or(i64::MAX));
        if worker
            .send(BridgeMessage::McpRequestChunk(McpRequestChunk {
                request_id: request_id.clone(),
                data: chunk.to_vec(),
            }))
            .await
            .is_err()
        {
            return Err(crate::auth::error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker_disconnected",
                "worker disconnected while streaming MCP request body",
            ));
        }
    }
    if !started {
        send_mcp_request_start(worker, start).await?;
    }
    send_mcp_request_end(
        worker,
        &request_id,
        compression.final_stats(decompressed_bytes),
    )
    .await?;
    Ok(())
}

async fn send_mcp_request_start(
    worker: &WorkerSender,
    start: McpRequestStart,
) -> Result<(), Response> {
    if worker
        .send(BridgeMessage::McpRequestStart(start))
        .await
        .is_err()
    {
        return Err(crate::auth::error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_disconnected",
            "worker disconnected before MCP request was sent",
        ));
    }
    Ok(())
}

async fn send_mcp_request_end(
    worker: &WorkerSender,
    request_id: &str,
    stats: HttpRequestTransferStats,
) -> Result<(), Response> {
    if worker
        .send(BridgeMessage::McpRequestEnd(McpRequestEnd {
            request_id: request_id.to_string(),
            http_request_compressed_bytes: stats.compressed_bytes,
            http_request_decompressed_bytes: stats.decompressed_bytes,
            http_request_compression_ratio: stats.compression_ratio,
        }))
        .await
        .is_err()
    {
        return Err(crate::auth::error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_disconnected",
            "worker disconnected before MCP request finished",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::{BridgeMessage, McpResponseChunk, McpResponseStart, ResponseError},
        relay::{
            request_compression::HttpRequestCompressionContext,
            response_forward::{handle_mcp_response_error, handle_mcp_response_start},
            state::test_state,
        },
    };
    use axum::{
        body::{Body, to_bytes},
        http::{HeaderMap, Method, StatusCode, header},
    };
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn streaming_mcp_error_body_stays_sse_framed() {
        let mut state = test_state();
        state.config.client_token = "test-token".to_string();

        let (worker_tx, mut worker_rx) = mpsc::channel(8);
        state.inner.workers.lock().await.insert(1, worker_tx);

        let state_for_worker = state.clone();
        tokio::spawn(async move {
            let mut request_id = None;
            while let Some(message) = worker_rx.recv().await {
                match message {
                    BridgeMessage::McpRequestStart(start) => request_id = Some(start.request_id),
                    BridgeMessage::McpRequestEnd(end) => {
                        let request_id = request_id.unwrap_or(end.request_id);
                        handle_mcp_response_start(
                            &state_for_worker,
                            McpResponseStart {
                                request_id: request_id.clone(),
                                status: StatusCode::OK.as_u16(),
                                content_type: Some("text/event-stream".to_string()),
                                headers: Vec::new(),
                            },
                        )
                        .await;
                        handle_mcp_response_error(
                            &state_for_worker,
                            ResponseError {
                                request_id,
                                status: StatusCode::BAD_GATEWAY.as_u16(),
                                code: "mcp_stream_error".to_string(),
                                message: "stream broke".to_string(),
                            },
                        )
                        .await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer test-token".parse().unwrap());

        let response = proxy_mcp_request(
            state,
            "127.0.0.1".parse().unwrap(),
            headers,
            HttpRequestCompressionContext::default(),
            Method::GET,
            None,
            Body::empty(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(
            text,
            "data: {\"error\":{\"code\":\"mcp_stream_error\",\"message\":\"stream broke\"}}\n\n"
        );
    }

    #[tokio::test]
    async fn streaming_mcp_error_is_suppressed_after_first_event() {
        let mut state = test_state();
        state.config.client_token = "test-token".to_string();

        let (worker_tx, mut worker_rx) = mpsc::channel(8);
        state.inner.workers.lock().await.insert(1, worker_tx);

        let state_for_worker = state.clone();
        tokio::spawn(async move {
            let mut request_id = None;
            while let Some(message) = worker_rx.recv().await {
                match message {
                    BridgeMessage::McpRequestStart(start) => request_id = Some(start.request_id),
                    BridgeMessage::McpRequestEnd(end) => {
                        let request_id = request_id.unwrap_or(end.request_id);
                        handle_mcp_response_start(
                            &state_for_worker,
                            McpResponseStart {
                                request_id: request_id.clone(),
                                status: StatusCode::OK.as_u16(),
                                content_type: Some("text/event-stream".to_string()),
                                headers: Vec::new(),
                            },
                        )
                        .await;
                        state_for_worker
                            .inner
                            .pending_mcp
                            .lock()
                            .await
                            .get(&request_id)
                            .expect("pending request");
                        crate::relay::response_forward::handle_mcp_response_chunk(
                            &state_for_worker,
                            McpResponseChunk {
                                request_id: request_id.clone(),
                                data: b"data: {\"jsonrpc\":\"2.0\",\"method\":\"first\"}\n\n"
                                    .to_vec(),
                            },
                        )
                        .await;
                        handle_mcp_response_error(
                            &state_for_worker,
                            ResponseError {
                                request_id,
                                status: StatusCode::BAD_GATEWAY.as_u16(),
                                code: "mcp_stream_error".to_string(),
                                message: "stream broke".to_string(),
                            },
                        )
                        .await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer test-token".parse().unwrap());

        let response = proxy_mcp_request(
            state,
            "127.0.0.1".parse().unwrap(),
            headers,
            HttpRequestCompressionContext::default(),
            Method::GET,
            None,
            Body::empty(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(text, "data: {\"jsonrpc\":\"2.0\",\"method\":\"first\"}\n\n");
    }
}
