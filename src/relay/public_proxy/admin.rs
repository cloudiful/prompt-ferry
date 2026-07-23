use crate::protocol::BridgeRequestStart;

use super::super::{
    request_compression::HttpRequestCompressionContext,
    response_forward::{
        bridge_error_response, choose_worker, release_response_bytes, remove_pending,
        request_deadline_unix_ms,
    },
    router::drain_body_then,
    state::{AppState, PendingRequest, RESPONSE_STREAM_BUFFER, RemoteAddr},
};
use super::{ai::stream_request_body, enforce_public_ip_policy};
use axum::{
    body::Body,
    extract::{ConnectInfo, Extension, State},
    http::{HeaderMap, Method, StatusCode, Uri, header},
    response::Response,
};
use bytes::Bytes;
use std::{net::IpAddr, time::Duration};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub(super) async fn proxy_admin_ui(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    uri: Uri,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    proxy_request(
        state,
        peer_addr.0.ip(),
        headers,
        compression,
        method,
        uri.to_string(),
        body,
    )
    .await
}

async fn proxy_request(
    state: AppState,
    peer_ip: IpAddr,
    headers: HeaderMap,
    compression: HttpRequestCompressionContext,
    method: Method,
    path: String,
    body: Body,
) -> Response {
    if let Err(response) = enforce_public_ip_policy(&state, peer_ip, &headers).await {
        return response;
    }

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
    let (chunk_tx, chunk_rx) = mpsc::channel(RESPONSE_STREAM_BUFFER);
    state.inner.pending.lock().await.insert(
        request_id.clone(),
        PendingRequest {
            start_tx: Some(start_tx),
            chunk_tx,
            worker_id: selection.worker_id,
            worker: selection.sender.clone(),
            queued_bytes: 0,
            awaiting_approval: false,
        },
    );
    let worker = selection.sender;

    let bridge_request = BridgeRequestStart {
        request_id: request_id.clone(),
        method: method.to_string(),
        path,
        headers: forwarded_request_headers(&headers),
        request_deadline_unix_ms: request_deadline_unix_ms(&state.config),
        user_id: None,
        route_id: None,
        client_key_hash: None,
        request_user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        http_request_content_encoding: compression.content_encoding.clone(),
        http_request_compressed: compression.compressed,
        http_request_compressed_bytes: compression.compressed_bytes,
    };

    if let Err(response) = stream_request_body(&worker, bridge_request, compression, body).await {
        remove_pending(&state, &request_id).await;
        return response;
    }

    let timeout = Duration::from_secs(state.config.request_timeout_seconds);
    let start = match tokio::time::timeout(timeout, start_rx).await {
        Ok(Ok(Ok(start))) => start,
        Ok(Ok(Err(err))) => {
            remove_pending(&state, &request_id).await;
            return bridge_error_response(err);
        }
        Ok(Err(_)) => {
            remove_pending(&state, &request_id).await;
            return crate::auth::error_response(
                StatusCode::BAD_GATEWAY,
                "worker_response_closed",
                "worker response channel closed",
            );
        }
        Err(_) => {
            remove_pending(&state, &request_id).await;
            return crate::auth::error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "request_timeout",
                "timed out waiting for worker response",
            );
        }
    };

    let status = StatusCode::from_u16(start.status).unwrap_or(StatusCode::BAD_GATEWAY);
    let stream_state = state.clone();
    let stream_request_id = request_id.clone();
    let stream = async_stream::stream! {
        let mut chunk_rx = chunk_rx;
        while let Some(item) = chunk_rx.recv().await {
            match item {
                Ok(chunk) => {
                    let data = chunk.data;
                    release_response_bytes(&stream_state, &stream_request_id, data.len()).await;
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(data));
                }
                Err(err) => {
                    let body = serde_json::json!({
                        "error": {
                            "code": err.code,
                            "message": err.message,
                        }
                    })
                    .to_string();
                    yield Ok(Bytes::from(body));
                    break;
                }
            }
        }
        remove_pending(&stream_state, &stream_request_id).await;
    };

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = status;
    if let Some(content_type) = start.content_type
        && let Ok(value) = content_type.parse()
    {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    append_response_headers(response.headers_mut(), start.headers);
    response
}

fn forwarded_request_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| (!is_hop_by_hop_request_header(name)).then_some((name, value)))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn append_response_headers(target: &mut HeaderMap, headers: Vec<(String, String)>) {
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(name.as_str()),
            header::HeaderValue::from_str(&value),
        ) {
            target.append(name, value);
        }
    }
}

fn is_hop_by_hop_request_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "connection"
            | "content-length"
            | "content-encoding"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}
