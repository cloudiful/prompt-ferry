mod admin;
mod ai;
mod mcp;

use crate::{
    auth::{bearer_token, client_token, error_response},
    bridge_wire, ip_acl,
    keys::hash_client_key,
    protocol::ClientRoute,
};

use super::{
    request_compression::capture_request_compression,
    state::{AppState, RemoteAddr},
};
use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::{Extensions, HeaderMap, StatusCode, Version, header},
    middleware,
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use std::net::IpAddr;
use tower_http::compression::{
    CompressionLayer,
    predicate::{DefaultPredicate, NotForContentType, Predicate},
};
use tower_http::cors::CorsLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tracing::{info, warn};

use self::{
    admin::proxy_admin_ui,
    ai::{
        create_realtime_client_secret_handler, proxy_anthropic_messages, proxy_chat,
        proxy_conversations, proxy_models, proxy_realtime, proxy_responses,
    },
    mcp::{proxy_mcp_root, proxy_mcp_server},
};

pub(super) fn response_compression_layer()
-> CompressionLayer<impl Predicate + Clone + Send + 'static> {
    let predicate = DefaultPredicate::new()
        .and(NotForContentType::SSE)
        .and(skip_websocket_upgrade);
    CompressionLayer::new().compress_when(predicate)
}

fn skip_websocket_upgrade(
    status: StatusCode,
    _version: Version,
    headers: &HeaderMap,
    _extensions: &Extensions,
) -> bool {
    if status == StatusCode::SWITCHING_PROTOCOLS {
        return false;
    }
    if headers.contains_key(header::UPGRADE) {
        return false;
    }
    if headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"))
    {
        return false;
    }
    true
}

pub(super) fn public_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(public_healthz))
        .route("/v1/models", get(proxy_models))
        .route(
            "/v1/messages",
            post(proxy_anthropic_messages).layer(DefaultBodyLimit::max(
                bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/v1/chat/completions",
            post(proxy_chat).layer(DefaultBodyLimit::max(
                bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/v1/responses",
            post(proxy_responses).layer(DefaultBodyLimit::max(
                bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/v1/conversations",
            post(proxy_conversations).layer(DefaultBodyLimit::max(
                bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
            )),
        )
        .route("/v1/realtime", get(proxy_realtime))
        .route(
            "/v1/realtime/client_secrets",
            post(create_realtime_client_secret_handler).layer(DefaultBodyLimit::max(
                bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
            )),
        )
        .route(
            "/mcp",
            get(proxy_mcp_root)
                .post(proxy_mcp_root)
                .delete(proxy_mcp_root),
        )
        .route(
            "/mcp/{server}",
            get(proxy_mcp_server)
                .post(proxy_mcp_server)
                .delete(proxy_mcp_server),
        )
        .fallback(any(proxy_admin_ui))
        .layer(RequestDecompressionLayer::new())
        .layer(CorsLayer::permissive())
        .layer(middleware::from_fn(capture_request_compression))
        .layer(response_compression_layer())
        .with_state(state)
}

fn header_value(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn sse_error_event(code: &str, message: &str) -> Vec<u8> {
    let payload = serde_json::json!({
        "error": {
            "code": code,
            "message": message,
        }
    });
    format!(
        "data: {}\n\n",
        serde_json::to_string(&payload).expect("SSE error payload should serialize")
    )
    .into_bytes()
}

pub(super) fn responses_sse_error_event(code: &str, message: &str) -> Vec<u8> {
    let payload = serde_json::json!({
        "type": "error",
        "sequence_number": 0,
        "code": code,
        "message": message,
        "param": null,
    });
    format!(
        "event: error\ndata: {}\n\n",
        serde_json::to_string(&payload).expect("Responses SSE error payload should serialize")
    )
    .into_bytes()
}

pub(super) fn anthropic_sse_error_event(code: &str, message: &str) -> Vec<u8> {
    let payload = serde_json::json!({
        "type": "error",
        "error": {
            "type": code,
            "message": message,
        },
    });
    format!(
        "event: error\ndata: {}\n\n",
        serde_json::to_string(&payload).expect("Anthropic SSE error payload should serialize")
    )
    .into_bytes()
}

pub(super) fn anthropic_error_response(
    status: StatusCode,
    error_type: &str,
    message: &str,
) -> Response {
    (
        status,
        axum::Json(serde_json::json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
            },
        })),
    )
        .into_response()
}

pub(super) fn chat_sse_error_event(code: &str, message: &str) -> Vec<u8> {
    let payload = serde_json::json!({
        "error": {
            "type": code,
            "code": code,
            "message": message,
            "param": null,
        }
    });
    format!(
        "data: {}\n\n",
        serde_json::to_string(&payload).expect("Chat SSE error payload should serialize")
    )
    .into_bytes()
}

/// Map an internal error code to the outward-facing code for AI responses.
/// Transport-level failures (HTTP status >= 500) are surfaced as `server_error`
/// so OpenAI-compatible clients such as OpenCode classify the error as
/// retryable; the internal code is kept in diagnostics and usage records.
/// Applies to streaming SSE error events and non-stream JSON error bodies.
/// Non-5xx codes are passed through unchanged.
pub(super) fn retryable_outward_code(status: u16, code: &str) -> &str {
    if status >= 500 { "server_error" } else { code }
}

struct DownstreamStreamDiag {
    kind: &'static str,
    request_id: String,
    path: String,
    status: u16,
    content_type: String,
    emitted_chunks: usize,
    emitted_bytes: usize,
    terminal_reason: Option<String>,
    terminal_error_code: Option<String>,
    terminal_error_message: Option<String>,
    finished: bool,
}

impl DownstreamStreamDiag {
    fn new(
        kind: &'static str,
        request_id: String,
        path: String,
        status: u16,
        content_type: String,
    ) -> Self {
        Self {
            kind,
            request_id,
            path,
            status,
            content_type,
            emitted_chunks: 0,
            emitted_bytes: 0,
            terminal_reason: None,
            terminal_error_code: None,
            terminal_error_message: None,
            finished: false,
        }
    }

    fn record_chunk(&mut self, len: usize) {
        self.emitted_chunks += 1;
        self.emitted_bytes += len;
    }

    fn mark_completed(&mut self) {
        self.terminal_reason
            .get_or_insert_with(|| "completed".to_string());
    }

    fn mark_error(&mut self, reason: &str, code: &str, message: &str) {
        self.terminal_reason = Some(reason.to_string());
        self.terminal_error_code = Some(code.to_string());
        self.terminal_error_message = Some(message.to_string());
    }

    fn finish(&mut self) {
        if self.finished {
            return;
        }
        info!(
            category = "stream_diag",
            kind = self.kind,
            request_id = %self.request_id,
            path = %self.path,
            status = self.status,
            content_type = %self.content_type,
            emitted_chunks = self.emitted_chunks,
            emitted_bytes = self.emitted_bytes,
            terminal_reason = self.terminal_reason.as_deref().unwrap_or(""),
            terminal_error_code = self.terminal_error_code.as_deref().unwrap_or(""),
            terminal_error_message = self.terminal_error_message.as_deref().unwrap_or(""),
            "downstream relay stream finished"
        );
        self.finished = true;
    }
}

impl Drop for DownstreamStreamDiag {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        warn!(
            category = "stream_diag",
            kind = self.kind,
            request_id = %self.request_id,
            path = %self.path,
            status = self.status,
            content_type = %self.content_type,
            emitted_chunks = self.emitted_chunks,
            emitted_bytes = self.emitted_bytes,
            terminal_reason = self.terminal_reason.as_deref().unwrap_or("downstream_stream_dropped"),
            terminal_error_code = self.terminal_error_code.as_deref().unwrap_or(""),
            terminal_error_message = self.terminal_error_message.as_deref().unwrap_or(""),
            "downstream relay stream dropped before normal completion"
        );
    }
}

async fn public_healthz(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = enforce_public_ip_policy(&state, peer_addr.0.ip(), &headers).await {
        return response;
    }
    Response::new(Body::from("ok"))
}

async fn authorize_client(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<ClientRoute>, Response> {
    authorize_client_with_format(state, headers, false).await
}

pub(super) async fn authorize_anthropic_client(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<ClientRoute>, Response> {
    authorize_client_with_format(state, headers, true).await
}

async fn authorize_client_with_format(
    state: &AppState,
    headers: &HeaderMap,
    anthropic_format: bool,
) -> Result<Option<ClientRoute>, Response> {
    let auth_error = |status, code, message| {
        if anthropic_format {
            anthropic_error_response(status, code, message)
        } else {
            error_response(status, code, message)
        }
    };
    let routes = state.inner.routes.lock().await;
    if routes.is_empty() {
        drop(routes);
        let token = match if anthropic_format {
            client_token(headers)
        } else {
            bearer_token(headers)
        } {
            Ok(token) => token,
            Err(response) => {
                warn!("client auth failed: missing or invalid bearer authorization");
                if anthropic_format {
                    return Err(anthropic_error_response(
                        response.status(),
                        "authentication_error",
                        "missing or invalid client authentication",
                    ));
                }
                return Err(*response);
            }
        };
        if state.config.client_token.is_empty() {
            warn!("client auth failed: relay client_token is not configured");
            return Err(auth_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auth_not_configured",
                "authentication token is not configured",
            ));
        }
        if token != state.config.client_token {
            warn!(
                mode = "legacy_client_token",
                token_len = token.len(),
                token_hash_prefix = %token_hash_prefix(token),
                "client auth failed: invalid token"
            );
            return Err(auth_error(
                StatusCode::FORBIDDEN,
                if anthropic_format {
                    "authentication_error"
                } else {
                    "forbidden"
                },
                "invalid token",
            ));
        }
        Ok(None)
    } else {
        let token = match if anthropic_format {
            client_token(headers)
        } else {
            bearer_token(headers)
        } {
            Ok(token) => token,
            Err(response) => {
                warn!(
                    route_count = routes.len(),
                    "client auth failed: missing or invalid bearer authorization"
                );
                if anthropic_format {
                    return Err(anthropic_error_response(
                        response.status(),
                        "authentication_error",
                        "missing or invalid client authentication",
                    ));
                }
                return Err(*response);
            }
        };
        let key_hash = hash_client_key(token);
        match routes.get(&key_hash).cloned() {
            Some(route) => Ok(Some(route)),
            None => {
                warn!(
                    mode = "managed_client_key",
                    route_count = routes.len(),
                    token_len = token.len(),
                    token_prefix = %token.chars().take(12).collect::<String>(),
                    key_hash_prefix = %key_hash.chars().take(12).collect::<String>(),
                    "client auth failed: invalid client key"
                );
                Err(auth_error(
                    StatusCode::FORBIDDEN,
                    if anthropic_format {
                        "authentication_error"
                    } else {
                        "forbidden"
                    },
                    "invalid client key",
                ))
            }
        }
    }
}

fn token_hash_prefix(token: &str) -> String {
    hash_client_key(token).chars().take(12).collect()
}

async fn enforce_public_ip_policy(
    state: &AppState,
    peer_ip: IpAddr,
    headers: &HeaderMap,
) -> Result<(), Response> {
    enforce_public_ip_policy_with_format(state, peer_ip, headers, false).await
}

pub(super) async fn enforce_public_ip_policy_for(
    state: &AppState,
    peer_ip: IpAddr,
    headers: &HeaderMap,
    anthropic_format: bool,
) -> Result<(), Response> {
    enforce_public_ip_policy_with_format(state, peer_ip, headers, anthropic_format).await
}

async fn enforce_public_ip_policy_with_format(
    state: &AppState,
    peer_ip: IpAddr,
    headers: &HeaderMap,
    anthropic_format: bool,
) -> Result<(), Response> {
    let policy = state.inner.relay_ip_policy.lock().await.clone();
    if policy.allowed_cidrs.is_empty() {
        return Ok(());
    }
    let Some(client_ip) = ip_acl::resolve_client_ip(peer_ip, headers, &policy.trusted_proxy_cidrs)
    else {
        warn!(%peer_ip, "relay public request denied: client ip could not be resolved");
        return Err(public_ip_error(anthropic_format));
    };
    if ip_acl::contains_ip(&policy.allowed_cidrs, client_ip) {
        return Ok(());
    }
    warn!(%peer_ip, %client_ip, "relay public request denied by ip whitelist");
    Err(public_ip_error(anthropic_format))
}

fn public_ip_error(anthropic_format: bool) -> Response {
    if anthropic_format {
        anthropic_error_response(
            StatusCode::FORBIDDEN,
            "permission_error",
            "client IP is not allowed",
        )
    } else {
        error_response(
            StatusCode::FORBIDDEN,
            "ip_not_allowed",
            "client IP is not allowed",
        )
    }
}

#[cfg(test)]
mod response_compression_tests {
    use super::response_compression_layer;
    use axum::{
        Json, Router,
        body::Body,
        http::{Request, StatusCode, header},
        response::{IntoResponse, Response},
        routing::get,
    };
    use tower::ServiceExt;

    fn test_app() -> Router {
        async fn json_handler() -> Response {
            Json(serde_json::json!({
                "data": "x".repeat(512),
                "message": "compressible json payload for response compression test",
            }))
            .into_response()
        }

        async fn sse_handler() -> Response {
            let body = format!("data: {}\n\n", "x".repeat(512));
            (
                [(header::CONTENT_TYPE, "text/event-stream")],
                Body::from(body),
            )
                .into_response()
        }

        async fn upgrade_handler() -> Response {
            (
                StatusCode::SWITCHING_PROTOCOLS,
                [
                    (header::UPGRADE, "websocket"),
                    (header::CONNECTION, "Upgrade"),
                ],
                Body::from("x".repeat(512)),
            )
                .into_response()
        }

        Router::new()
            .route("/json", get(json_handler))
            .route("/sse", get(sse_handler))
            .route("/ws", get(upgrade_handler))
            .layer(response_compression_layer())
    }

    #[tokio::test]
    async fn json_compresses_with_gzip_when_client_advertises_support() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/json")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("gzip"),
            "JSON should gain Content-Encoding: gzip",
        );
    }

    #[tokio::test]
    async fn json_compresses_with_brotli_when_client_advertises_support() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/json")
                    .header(header::ACCEPT_ENCODING, "br")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("br"),
            "JSON should gain Content-Encoding: br",
        );
    }

    #[tokio::test]
    async fn json_stays_uncompressed_without_accept_encoding() {
        let response = test_app()
            .oneshot(Request::builder().uri("/json").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers().get(header::CONTENT_ENCODING).is_none(),
            "JSON should stay uncompressed without Accept-Encoding",
        );
    }

    #[tokio::test]
    async fn sse_content_type_stays_uncompressed() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/sse")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers().get(header::CONTENT_ENCODING).is_none(),
            "text/event-stream must stay uncompressed",
        );
    }

    #[tokio::test]
    async fn websocket_upgrade_stays_uncompressed() {
        let response = test_app()
            .oneshot(
                Request::builder()
                    .uri("/ws")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        assert!(
            response.headers().get(header::CONTENT_ENCODING).is_none(),
            "websocket upgrade must stay uncompressed",
        );
    }
}
