use crate::{
    protocol::{
        BridgeMessage, BridgeRequestChunk, BridgeRequestEnd, BridgeRequestStart,
        RealtimeClientEventMessage, RealtimeSessionClose, RealtimeSessionStart,
    },
    realtime::{
        RealtimeSessionClaims, create_relay_client_secret, parse_client_event,
        verify_relay_client_secret,
    },
    relay_secrets::RelaySecretManager,
    worker_admin_types::{RealtimeClientSecretRequest, RealtimeClientSecretResponse},
};

use super::super::{
    request_compression::{HttpRequestCompressionContext, HttpRequestTransferStats},
    response_forward::{
        PendingCleanup, bridge_error_response, choose_worker, release_response_bytes,
        remove_pending, remove_realtime_pending, request_deadline_unix_ms,
    },
    response_pump::{spawn_realtime_response_pump, spawn_response_pump},
    router::drain_body_then,
    state::{AppState, PendingRequest, RemoteAddr, WorkerSender},
};
use super::{DownstreamStreamDiag, authorize_client, enforce_public_ip_policy};
use axum::{
    Json,
    body::Body,
    extract::{ConnectInfo, Extension, Query, State, WebSocketUpgrade, ws::Message},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::{net::IpAddr, time::Duration};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::super::state::PendingRealtimeSession;
use super::{
    admin::is_hop_by_hop_request_header, chat_sse_error_event, responses_sse_error_event,
    retryable_outward_code, sse_error_event,
};

fn forwarded_ai_request_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| {
            !is_hop_by_hop_request_header(name)
                && !matches!(name.as_str(), "authorization" | "cookie")
        })
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

pub(super) async fn proxy_models(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    headers: HeaderMap,
) -> Response {
    proxy_request(
        state,
        peer_addr.0.ip(),
        headers,
        compression,
        Method::GET,
        "/v1/models",
        Body::empty(),
    )
    .await
}

pub(super) async fn proxy_chat(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(
        state,
        peer_addr.0.ip(),
        headers,
        compression,
        Method::POST,
        "/v1/chat/completions",
        body,
    )
    .await
}

pub(super) async fn proxy_responses(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(
        state,
        peer_addr.0.ip(),
        headers,
        compression,
        Method::POST,
        "/v1/responses",
        body,
    )
    .await
}

pub(super) async fn proxy_conversations(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = enforce_public_ip_policy(&state, peer_addr.0.ip(), &headers).await {
        return response;
    }
    if let Err(response) = authorize_client(&state, &headers).await {
        return response;
    }

    Json(serde_json::json!({
        "id": format!("conv_{}", Uuid::new_v4().simple()),
        "object": "conversation",
        "created_at": Utc::now().timestamp(),
        "metadata": {},
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
pub(super) struct RealtimeQuery {
    pub model: Option<String>,
    pub client_secret: Option<String>,
}

pub(super) async fn create_realtime_client_secret_handler(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    headers: HeaderMap,
    Json(body): Json<RealtimeClientSecretRequest>,
) -> Response {
    if let Err(response) = enforce_public_ip_policy(&state, peer_addr.0.ip(), &headers).await {
        return response;
    }
    let route = match authorize_client(&state, &headers).await {
        Ok(route) => route,
        Err(response) => return response,
    };
    let manager = match relay_secret_manager(&state) {
        Ok(manager) => manager,
        Err(err) => {
            return crate::auth::error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "realtime_secret_unavailable",
                &err,
            );
        }
    };
    let expires_after = body.expires_after.as_ref().map(|value| value.seconds);
    let secret = match create_relay_client_secret(
        &manager,
        RealtimeSessionClaims {
            version: 0,
            user_id: route.as_ref().map(|route| route.user_id),
            route_id: route.as_ref().map(|route| route.route_id.clone()),
            client_key_hash: route.as_ref().map(|route| route.key_hash.clone()),
            model: body
                .session
                .get("model")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            session: body.session.clone(),
            expires_at: 0,
        },
        expires_after,
    ) {
        Ok(secret) => secret,
        Err(err) => {
            return crate::auth::error_response(
                StatusCode::BAD_REQUEST,
                "invalid_realtime_session",
                &err.to_string(),
            );
        }
    };
    Json(RealtimeClientSecretResponse {
        value: secret.value,
        expires_at: secret.expires_at,
        session: secret.session,
    })
    .into_response()
}

pub(super) async fn proxy_realtime(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Query(query): Query<RealtimeQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if let Err(response) = enforce_public_ip_policy(&state, peer_addr.0.ip(), &headers).await {
        return response;
    }
    let model = match query
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(model) => model.to_string(),
        None => {
            return crate::auth::error_response(
                StatusCode::BAD_REQUEST,
                "missing_model",
                "model query parameter is required",
            );
        }
    };
    let auth =
        match authorize_realtime_client(&state, &headers, query.client_secret.as_deref()).await {
            Ok(auth) => auth,
            Err(response) => return response,
        };
    let selection = match choose_worker(&state).await {
        Some(selection) => selection,
        None => {
            return crate::auth::error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "no_worker",
                "no worker is connected",
            );
        }
    };
    let request_id = Uuid::new_v4().to_string();
    let request_user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let (event_tx, event_rx) = mpsc::channel(state.config.response_stream_buffer);
    let (forward_tx, forward_rx) = mpsc::unbounded_channel();
    let event_tx_for_pump = event_tx.clone();
    state.inner.pending_realtime_sessions.lock().await.insert(
        request_id.clone(),
        PendingRealtimeSession {
            event_tx,
            forward_tx,
            worker_id: selection.worker_id,
            worker: selection.sender.clone(),
            queued_bytes: 0,
            response_started: false,
        },
    );
    spawn_realtime_response_pump(
        state.clone(),
        request_id.clone(),
        selection.worker_id,
        selection.sender.clone(),
        forward_rx,
        event_tx_for_pump,
        Duration::from_millis(state.config.response_stream_backpressure_timeout_ms),
    );
    let start = RealtimeSessionStart {
        request_id: request_id.clone(),
        model,
        path: "/v1/realtime".to_string(),
        user_id: auth.user_id,
        route_id: auth.route_id,
        client_key_hash: auth.client_key_hash,
        request_user_agent,
    };
    let state_clone = state.clone();
    let worker = selection.sender;
    ws.on_upgrade(move |socket| async move {
        handle_realtime_socket(state_clone, worker, request_id, start, socket, event_rx).await;
    })
}

async fn proxy_request(
    state: AppState,
    peer_ip: IpAddr,
    headers: HeaderMap,
    compression: HttpRequestCompressionContext,
    method: Method,
    path: &'static str,
    body: Body,
) -> Response {
    if let Err(response) = enforce_public_ip_policy(&state, peer_ip, &headers).await {
        return response;
    }
    let route = match authorize_client(&state, &headers).await {
        Ok(route) => route,
        Err(response) => return response,
    };

    let request_id = Uuid::new_v4().to_string();
    let selection = match choose_worker(&state).await {
        Some(selection) => selection,
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

    let (start_tx, start_rx) = oneshot::channel();
    let (chunk_tx, chunk_rx) = mpsc::channel(state.config.response_stream_buffer);
    let (forward_tx, forward_rx) = mpsc::unbounded_channel();
    let chunk_tx_for_pump = chunk_tx.clone();

    state.inner.pending.lock().await.insert(
        request_id.clone(),
        PendingRequest {
            start_tx: Some(start_tx),
            chunk_tx,
            forward_tx,
            worker_id: selection.worker_id,
            worker: selection.sender.clone(),
            queued_bytes: 0,
            response_started: false,
            awaiting_approval: false,
        },
    );
    spawn_response_pump(
        state.clone(),
        request_id.clone(),
        selection.worker_id,
        selection.sender.clone(),
        forward_rx,
        chunk_tx_for_pump,
        Duration::from_millis(state.config.response_stream_backpressure_timeout_ms),
    );
    let worker = selection.sender;

    let route_user_id = route.as_ref().map(|route| route.user_id);
    let route_id = route.as_ref().map(|route| route.route_id.clone());
    let client_key_hash = route.as_ref().map(|route| route.key_hash.clone());
    let request_user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let bridge_request = BridgeRequestStart {
        request_id: request_id.clone(),
        method: method.to_string(),
        path: path.to_string(),
        headers: forwarded_ai_request_headers(&headers),
        request_deadline_unix_ms: request_deadline_unix_ms(&state.config),
        user_id: route_user_id,
        route_id,
        client_key_hash,
        request_user_agent,
        http_request_content_encoding: compression.content_encoding.clone(),
        http_request_compressed: compression.compressed,
        http_request_compressed_bytes: compression.compressed_bytes,
    };

    if let Err(response) = stream_request_body(&worker, bridge_request, compression, body).await {
        remove_pending(&state, &request_id).await;
        return response;
    }

    let timeout = Duration::from_secs(state.config.request_timeout_seconds);
    let mut cleanup = PendingCleanup::ai(state.clone(), request_id.clone());
    let start = match tokio::time::timeout(timeout, start_rx).await {
        Ok(Ok(Ok(start))) => start,
        Ok(Ok(Err(err))) => {
            remove_pending(&state, &request_id).await;
            cleanup.disarm();
            return bridge_error_response(err);
        }
        Ok(Err(_)) => {
            remove_pending(&state, &request_id).await;
            cleanup.disarm();
            return crate::auth::error_response(
                StatusCode::BAD_GATEWAY,
                "worker_response_closed",
                "worker response channel closed",
            );
        }
        Err(_) => {
            remove_pending(&state, &request_id).await;
            cleanup.disarm();
            return crate::auth::error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "request_timeout",
                "timed out waiting for worker response",
            );
        }
    };

    let status = StatusCode::from_u16(start.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = start.content_type.unwrap_or_else(|| {
        if method == Method::GET {
            "application/json".to_string()
        } else {
            "text/event-stream".to_string()
        }
    });
    let is_event_stream = content_type.contains("text/event-stream");
    let is_responses_stream = path == "/v1/responses" && is_event_stream;
    let is_chat_stream = path == "/v1/chat/completions" && is_event_stream;

    let stream_state = state.clone();
    let stream_request_id = request_id.clone();
    let stream_path = path.to_string();
    let stream_content_type = content_type.clone();
    let stream = async_stream::stream! {
        let mut cleanup = cleanup;
        let mut chunk_rx = chunk_rx;
        let mut diag = DownstreamStreamDiag::new(
            "ai",
            stream_request_id.clone(),
            stream_path,
            status.as_u16(),
            stream_content_type,
        );
        while let Some(item) = chunk_rx.recv().await {
            match item {
                Ok(chunk) => {
                    let data = chunk.data;
                    release_response_bytes(&stream_state, &stream_request_id, data.len()).await;
                    diag.record_chunk(data.len());
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(data));
                }
                Err(err) => {
                    let outward_code = retryable_outward_code(err.status, &err.code);
                    let body = if is_event_stream {
                        if is_responses_stream {
                            responses_sse_error_event(outward_code, &err.message)
                        } else if is_chat_stream {
                            chat_sse_error_event(outward_code, &err.message)
                        } else {
                            sse_error_event(outward_code, &err.message)
                        }
                    } else {
                        serde_json::json!({
                            "error": {
                                "code": outward_code,
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
        remove_pending(&stream_state, &stream_request_id).await;
        cleanup.disarm();
        diag.mark_completed();
        diag.finish();
    };

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    if let Ok(value) = content_type.parse() {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    for (name, value) in start.headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(name.as_str()),
            header::HeaderValue::from_str(&value),
        ) {
            response.headers_mut().append(name, value);
        }
    }
    response
}

pub(super) async fn stream_request_body(
    worker: &WorkerSender,
    start: BridgeRequestStart,
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
                    let _ = send_request_end(
                        worker,
                        &request_id,
                        compression.final_stats(decompressed_bytes),
                    )
                    .await;
                }
                return Err(crate::auth::error_response(
                    StatusCode::BAD_REQUEST,
                    "request_body_read_failed",
                    &format!("failed to read request body: {err}"),
                ));
            }
        };
        if !started {
            send_request_start(worker, start.clone()).await?;
            started = true;
        }
        decompressed_bytes =
            decompressed_bytes.saturating_add(i64::try_from(chunk.len()).unwrap_or(i64::MAX));
        if worker
            .send(BridgeMessage::RequestChunk(BridgeRequestChunk {
                request_id: request_id.clone(),
                data: chunk.to_vec(),
            }))
            .await
            .is_err()
        {
            return Err(crate::auth::error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker_disconnected",
                "worker disconnected while streaming request body",
            ));
        }
    }
    if !started {
        send_request_start(worker, start).await?;
    }
    send_request_end(
        worker,
        &request_id,
        compression.final_stats(decompressed_bytes),
    )
    .await?;
    Ok(())
}

async fn send_request_start(
    worker: &WorkerSender,
    start: BridgeRequestStart,
) -> Result<(), Response> {
    if worker
        .send(BridgeMessage::RequestStart(start))
        .await
        .is_err()
    {
        return Err(crate::auth::error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker_disconnected",
            "worker disconnected before request was sent",
        ));
    }
    Ok(())
}

async fn send_request_end(
    worker: &WorkerSender,
    request_id: &str,
    stats: HttpRequestTransferStats,
) -> Result<(), Response> {
    if worker
        .send(BridgeMessage::RequestEnd(BridgeRequestEnd {
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
            "worker disconnected before request finished",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::{BridgeMessage, ResponseChunk, ResponseError, ResponseStart},
        relay::{
            request_compression::HttpRequestCompressionContext,
            response_forward::{
                handle_response_chunk, handle_response_end, handle_response_error,
                handle_response_start,
            },
            state::test_state,
        },
    };
    use axum::{
        body::{Body, to_bytes},
        http::{HeaderMap, Method, StatusCode, header},
    };
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn streaming_ai_error_body_stays_sse_framed() {
        let mut state = test_state();
        state.config.client_token = "test-token".to_string();

        let (worker_tx, mut worker_rx) = mpsc::channel(8);
        state.inner.workers.lock().await.insert(1, worker_tx);

        let state_for_worker = state.clone();
        tokio::spawn(async move {
            let mut request_id = None;
            while let Some(message) = worker_rx.recv().await {
                match message {
                    BridgeMessage::RequestStart(start) => request_id = Some(start.request_id),
                    BridgeMessage::RequestEnd(end) => {
                        let request_id = request_id.unwrap_or(end.request_id);
                        handle_response_start(
                            &state_for_worker,
                            ResponseStart {
                                request_id: request_id.clone(),
                                status: StatusCode::OK.as_u16(),
                                content_type: Some("text/event-stream".to_string()),
                                headers: Vec::new(),
                            },
                        )
                        .await;
                        handle_response_chunk(
                            &state_for_worker,
                            ResponseChunk {
                                request_id: request_id.clone(),
                                data: b"data: {\"type\":\"response.created\"}\n\n".to_vec(),
                            },
                        )
                        .await;
                        handle_response_error(
                            &state_for_worker,
                            ResponseError {
                                request_id,
                                status: StatusCode::BAD_GATEWAY.as_u16(),
                                code: "upstream_stream_error".to_string(),
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

        let response = proxy_request(
            state,
            "127.0.0.1".parse().unwrap(),
            headers,
            HttpRequestCompressionContext::default(),
            Method::POST,
            "/v1/responses",
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
            "data: {\"type\":\"response.created\"}\n\nevent: error\ndata: {\"code\":\"server_error\",\"message\":\"stream broke\",\"param\":null,\"sequence_number\":0,\"type\":\"error\"}\n\n"
        );
        let error_json = text
            .split("event: error\ndata: ")
            .nth(1)
            .and_then(|value| value.split_once("\n\n"))
            .map(|(value, _)| value)
            .expect("Responses error event");
        let error_event: async_openai::types::responses::ResponseStreamEvent =
            serde_json::from_str(error_json).expect("standard Responses error event");
        assert!(matches!(
            error_event,
            async_openai::types::responses::ResponseStreamEvent::ResponseError(_)
        ));
    }

    #[tokio::test]
    async fn chat_stream_error_body_uses_ai_sdk_error_envelope() {
        let mut state = test_state();
        state.config.client_token = "test-token".to_string();

        let (worker_tx, mut worker_rx) = mpsc::channel(8);
        state.inner.workers.lock().await.insert(1, worker_tx);

        let state_for_worker = state.clone();
        tokio::spawn(async move {
            let mut request_id = None;
            while let Some(message) = worker_rx.recv().await {
                match message {
                    BridgeMessage::RequestStart(start) => request_id = Some(start.request_id),
                    BridgeMessage::RequestEnd(end) => {
                        let request_id = request_id.unwrap_or(end.request_id);
                        handle_response_start(
                            &state_for_worker,
                            ResponseStart {
                                request_id: request_id.clone(),
                                status: StatusCode::OK.as_u16(),
                                content_type: Some("text/event-stream".to_string()),
                                headers: Vec::new(),
                            },
                        )
                        .await;
                        handle_response_chunk(
                            &state_for_worker,
                            ResponseChunk {
                                request_id: request_id.clone(),
                                data:
                                    b"data: {\"choices\":[{\"delta\":{\"content\":\"part\"}}]}\n\n"
                                        .to_vec(),
                            },
                        )
                        .await;
                        handle_response_error(
                            &state_for_worker,
                            ResponseError {
                                request_id,
                                status: StatusCode::BAD_GATEWAY.as_u16(),
                                code: "upstream_stream_error".to_string(),
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

        let response = proxy_request(
            state,
            "127.0.0.1".parse().unwrap(),
            headers,
            HttpRequestCompressionContext::default(),
            Method::POST,
            "/v1/chat/completions",
            Body::empty(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(
            text,
            "data: {\"choices\":[{\"delta\":{\"content\":\"part\"}}]}\n\ndata: {\"error\":{\"code\":\"server_error\",\"message\":\"stream broke\",\"param\":null,\"type\":\"server_error\"}}\n\n"
        );
        let error_json = text
            .split("data: ")
            .nth(2)
            .and_then(|value| value.strip_suffix("\n\n"))
            .expect("Chat error event");
        let error_event: serde_json::Value =
            serde_json::from_str(error_json).expect("Chat error event JSON");
        assert_eq!(error_event["error"]["type"], "server_error");
        assert_eq!(error_event["error"]["code"], "server_error");
        assert_eq!(error_event["error"]["message"], "stream broke");
    }

    #[tokio::test]
    async fn non_server_error_code_is_preserved() {
        let mut state = test_state();
        state.config.client_token = "test-token".to_string();

        let (worker_tx, mut worker_rx) = mpsc::channel(8);
        state.inner.workers.lock().await.insert(1, worker_tx);

        let state_for_worker = state.clone();
        tokio::spawn(async move {
            let mut request_id = None;
            while let Some(message) = worker_rx.recv().await {
                match message {
                    BridgeMessage::RequestStart(start) => request_id = Some(start.request_id),
                    BridgeMessage::RequestEnd(end) => {
                        let request_id = request_id.unwrap_or(end.request_id);
                        handle_response_start(
                            &state_for_worker,
                            ResponseStart {
                                request_id: request_id.clone(),
                                status: StatusCode::OK.as_u16(),
                                content_type: Some("text/event-stream".to_string()),
                                headers: Vec::new(),
                            },
                        )
                        .await;
                        handle_response_error(
                            &state_for_worker,
                            ResponseError {
                                request_id,
                                status: StatusCode::BAD_REQUEST.as_u16(),
                                code: "invalid_request".to_string(),
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

        let response = proxy_request(
            state,
            "127.0.0.1".parse().unwrap(),
            headers,
            HttpRequestCompressionContext::default(),
            Method::POST,
            "/v1/chat/completions",
            Body::empty(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(
            text,
            "data: {\"error\":{\"code\":\"invalid_request\",\"message\":\"stream broke\",\"param\":null,\"type\":\"invalid_request\"}}\n\n"
        );
    }

    #[tokio::test]
    async fn streaming_ai_proxy_drains_buffered_chunks_before_channel_close() {
        let mut state = test_state();
        state.config.client_token = "test-token".to_string();

        let (worker_tx, mut worker_rx) = mpsc::channel(8);
        state.inner.workers.lock().await.insert(1, worker_tx);

        let state_for_worker = state.clone();
        tokio::spawn(async move {
            let mut request_id = None;
            while let Some(message) = worker_rx.recv().await {
                match message {
                    BridgeMessage::RequestStart(start) => request_id = Some(start.request_id),
                    BridgeMessage::RequestEnd(end) => {
                        let request_id = request_id.unwrap_or(end.request_id);
                        handle_response_start(
                            &state_for_worker,
                            ResponseStart {
                                request_id: request_id.clone(),
                                status: StatusCode::OK.as_u16(),
                                content_type: Some("text/event-stream".to_string()),
                                headers: Vec::new(),
                            },
                        )
                        .await;
                        for chunk in [
                            b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n".to_vec(),
                            b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"output_index\":0}\n\n".to_vec(),
                            b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"feat\"}\n\n".to_vec(),
                            b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n".to_vec(),
                        ] {
                            handle_response_chunk(
                                &state_for_worker,
                                ResponseChunk {
                                    request_id: request_id.clone(),
                                    data: chunk,
                                },
                            )
                            .await;
                        }
                        handle_response_end(
                            &state_for_worker,
                            crate::protocol::ResponseEnd { request_id },
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

        let response = proxy_request(
            state,
            "127.0.0.1".parse().unwrap(),
            headers,
            HttpRequestCompressionContext::default(),
            Method::POST,
            "/v1/responses",
            Body::empty(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(
            text,
            concat!(
                "event: response.created\n",
                "data: {\"type\":\"response.created\"}\n\n",
                "event: response.output_item.added\n",
                "data: {\"type\":\"response.output_item.added\",\"output_index\":0}\n\n",
                "event: response.output_text.delta\n",
                "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"feat\"}\n\n",
                "event: response.completed\n",
                "data: {\"type\":\"response.completed\"}\n\n"
            )
        );
    }
}

#[derive(Debug, Clone)]
struct RealtimeAuthContext {
    user_id: Option<i64>,
    route_id: Option<String>,
    client_key_hash: Option<String>,
}

fn relay_secret_manager(state: &AppState) -> Result<RelaySecretManager, String> {
    let key = state.config.bridge_encryption_key.trim();
    if key.is_empty() {
        return Err(
            "relay bridge_encryption_key is required for Realtime client secrets".to_string(),
        );
    }
    RelaySecretManager::from_base64(key).map_err(|err| err.to_string())
}

async fn authorize_realtime_client(
    state: &AppState,
    headers: &HeaderMap,
    client_secret: Option<&str>,
) -> Result<RealtimeAuthContext, Response> {
    if let Some(client_secret) = client_secret {
        let manager = relay_secret_manager(state).map_err(|err| {
            crate::auth::error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "realtime_secret_unavailable",
                &err,
            )
        })?;
        let claims = verify_relay_client_secret(&manager, client_secret).map_err(|err| {
            crate::auth::error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_realtime_client_secret",
                &err.to_string(),
            )
        })?;
        return Ok(RealtimeAuthContext {
            user_id: claims.user_id,
            route_id: claims.route_id,
            client_key_hash: claims.client_key_hash,
        });
    }
    let route = authorize_client(state, headers).await?;
    Ok(RealtimeAuthContext {
        user_id: route.as_ref().map(|route| route.user_id),
        route_id: route.as_ref().map(|route| route.route_id.clone()),
        client_key_hash: route.as_ref().map(|route| route.key_hash.clone()),
    })
}

async fn handle_realtime_socket(
    state: AppState,
    worker: WorkerSender,
    request_id: String,
    start: RealtimeSessionStart,
    socket: axum::extract::ws::WebSocket,
    mut event_rx: mpsc::Receiver<
        Result<crate::relay::state::QueuedRealtimeEvent, crate::protocol::ResponseError>,
    >,
) {
    let mut cleanup = PendingCleanup::realtime(state.clone(), request_id.clone());
    if worker
        .send(BridgeMessage::RealtimeSessionStart(start))
        .await
        .is_err()
    {
        remove_realtime_pending(&state, &request_id).await;
        cleanup.disarm();
        return;
    }
    let (mut ws_tx, mut ws_rx) = socket.split();
    loop {
        tokio::select! {
            incoming = ws_rx.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(err) = parse_client_event(&text) {
                            let _ = ws_tx.send(Message::Text(serde_json::json!({"type":"error","error":{"type":"invalid_request_error","code":"invalid_realtime_event","message":err.to_string(),"param":null,"event_id":null},"event_id":request_id}).to_string().into())).await;
                            break;
                        }
                        if worker.send(BridgeMessage::RealtimeClientEvent(RealtimeClientEventMessage { request_id: request_id.clone(), event_json: text.to_string() })).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {
                        let _ = ws_tx.send(Message::Text(serde_json::json!({"type":"error","error":{"type":"invalid_request_error","code":"binary_frames_not_supported","message":"binary realtime frames are not supported","param":null,"event_id":null},"event_id":request_id}).to_string().into())).await;
                        break;
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let _ = worker.send(BridgeMessage::RealtimeSessionClose(RealtimeSessionClose {
                            request_id: request_id.clone(),
                            code: frame.as_ref().map(|frame| frame.code),
                            reason: frame.as_ref().map(|frame| frame.reason.to_string()),
                            response_started: true,
                        })).await;
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = ws_tx.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Err(_)) | None => break,
                }
            }
            outbound = event_rx.recv() => {
                match outbound {
                    Some(Ok(event)) => {
                        let event_json = event.event.event_json;
                        crate::relay::response_forward::release_realtime_event_bytes(
                            &state,
                            &request_id,
                            event_json.len(),
                        ).await;
                        if ws_tx.send(Message::Text(event_json.into())).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(err)) => {
                        let _ = ws_tx.send(Message::Text(serde_json::json!({"type":"error","error":{"type":"server_error","code":err.code,"message":err.message,"param":null,"event_id":null},"event_id":request_id}).to_string().into())).await;
                        break;
                    }
                    None => break,
                }
            }
        }
    }
    remove_realtime_pending(&state, &request_id).await;
    cleanup.disarm();
}
