//! Forwarding one HTTP request to a connected worker over the relay/worker
//! bridge.
//!
//! Both management entry points need this: the relay's public fallback, which
//! keeps the existing authenticated Worker-admin proxy contract, and the relay's
//! own management listener, which proxies the worker's business API while
//! serving its own control plane locally. Sharing the bridge request lifecycle
//! here keeps both on one implementation of streaming, back-pressure, cleanup,
//! and the timeout behaviour.

use crate::protocol::BridgeRequestStart;

use super::{
    public_proxy::ai::stream_request_body,
    request_compression::HttpRequestCompressionContext,
    response_forward::{
        PendingCleanup, bridge_error_response, choose_worker, release_response_bytes,
        remove_pending, request_deadline_unix_ms,
    },
    response_pump::spawn_response_pump,
    router::drain_body_then,
    state::{AppState, PendingRequest},
};
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use uuid::Uuid;

/// One request to hand to a worker.
///
/// Both management entry points authenticate their caller before reaching this,
/// and the public one additionally applies the relay's client IP policy, so the
/// forwarding core has no access-control decision left to make.
pub(crate) struct WorkerRequest<'a> {
    pub(crate) method: &'a Method,
    pub(crate) path: &'a str,
    pub(crate) headers: &'a HeaderMap,
    pub(crate) compression: HttpRequestCompressionContext,
    pub(crate) body: Body,
}

/// Hand the request to a connected worker and stream its response back.
///
/// With no worker connected the caller decides how that reads; this returns
/// `None` before any bridge state is touched so the management listener can
/// answer on its own instead of pretending a worker answered.
pub(crate) async fn forward_to_worker(
    state: &AppState,
    request: WorkerRequest<'_>,
    no_worker: impl FnOnce() -> Response,
) -> Response {
    let selection = match choose_worker(state).await {
        Some(selection) => selection,
        None => return drain_body_then(request.body, no_worker()).await,
    };

    let request_id = Uuid::new_v4().to_string();
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

    let bridge_request = BridgeRequestStart {
        request_id: request_id.clone(),
        method: request.method.to_string(),
        path: request.path.to_string(),
        headers: forwarded_request_headers(request.headers),
        request_deadline_unix_ms: request_deadline_unix_ms(&state.config),
        user_id: None,
        route_id: None,
        client_key_hash: None,
        request_user_agent: request
            .headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        http_request_content_encoding: request.compression.content_encoding.clone(),
        http_request_compressed: request.compression.compressed,
        http_request_compressed_bytes: request.compression.compressed_bytes,
    };

    if let Err(err) =
        stream_request_body(&worker, bridge_request, request.compression, request.body).await
    {
        remove_pending(state, &request_id).await;
        return err.into_response();
    }

    let timeout = Duration::from_secs(state.config.request_timeout_seconds);
    let mut cleanup = PendingCleanup::ai(state.clone(), request_id.clone());
    let start = match tokio::time::timeout(timeout, start_rx).await {
        Ok(Ok(Ok(start))) => start,
        Ok(Ok(Err(err))) => {
            remove_pending(state, &request_id).await;
            cleanup.disarm();
            return bridge_error_response(err);
        }
        Ok(Err(_)) => {
            remove_pending(state, &request_id).await;
            cleanup.disarm();
            return crate::auth::error_response(
                StatusCode::BAD_GATEWAY,
                "worker_response_closed",
                "worker response channel closed",
            );
        }
        Err(_) => {
            remove_pending(state, &request_id).await;
            cleanup.disarm();
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
        let mut cleanup = cleanup;
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
        cleanup.disarm();
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

pub(crate) fn forwarded_request_headers(headers: &HeaderMap) -> Vec<(String, String)> {
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

pub(super) fn is_hop_by_hop_request_header(name: &header::HeaderName) -> bool {
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

/// Report that no worker is connected without blaming the request.
///
/// The management listener answers with this when no worker has registered on
/// the bridge, which is the normal state of a relay-only host, so it is logged
/// at a level that does not drown the log: the caller learns the real state from
/// `GET /api/v1/relay/status`, which reports the same thing without a request
/// against it. The public fallback keeps its own wording so an existing client
/// contract is unchanged.
pub(crate) fn no_worker_connected() -> Response {
    debug!("relay management request needs a worker, but none is connected");
    crate::auth::error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "worker_not_connected",
        "no worker is connected to this relay",
    )
}

#[cfg(test)]
mod tests {
    use super::{WorkerRequest, forward_to_worker, no_worker_connected};
    use crate::relay::{
        public_proxy::enforce_public_ip_policy, request_compression::HttpRequestCompressionContext,
        state::test_state,
    };
    use axum::{
        body::{Body, to_bytes},
        http::{HeaderMap, Method, Request, StatusCode},
    };

    #[tokio::test]
    async fn a_relay_without_a_worker_answers_instead_of_failing() {
        let state = test_state();
        let mut headers = HeaderMap::new();
        headers.insert("x-requested-with", "relay-admin".parse().expect("header"));

        let response = forward_to_worker(
            &state,
            WorkerRequest {
                method: &Method::GET,
                path: "/api/v1/admin/endpoints",
                headers: &headers,
                compression: HttpRequestCompressionContext::default(),
                body: Body::empty(),
            },
            no_worker_connected,
        )
        .await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read the body");
        let body = std::str::from_utf8(&body).expect("JSON body");
        assert!(body.contains("worker_not_connected"), "got: {body}");
    }

    #[tokio::test]
    async fn hop_by_hop_headers_are_not_carried_to_the_worker() {
        let state = test_state();
        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/admin/endpoints")
            .header("host", "127.0.0.1:8790")
            .header("connection", "keep-alive")
            .header("x-request-id", "abc")
            .body(Body::empty())
            .expect("build a request");

        let forwarded = super::forwarded_request_headers(request.headers());

        assert_eq!(
            forwarded
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["x-request-id"],
            "only end-to-end headers reach the worker"
        );
        let _ = state;
    }

    #[tokio::test]
    async fn the_public_ip_policy_still_guards_the_public_caller() {
        let state = test_state();
        state.inner.relay_ip_policy.lock().await.allowed_cidrs =
            vec!["203.0.113.0/24".parse().expect("cidr")];
        let headers = HeaderMap::new();

        let denied =
            enforce_public_ip_policy(&state, "198.51.100.7".parse().expect("peer ip"), &headers)
                .await
                .expect_err("an address outside the whitelist must be denied");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let allowed =
            enforce_public_ip_policy(&state, "203.0.113.9".parse().expect("peer ip"), &headers)
                .await;
        assert!(
            allowed.is_ok(),
            "an address inside the whitelist must be served"
        );
    }
}
