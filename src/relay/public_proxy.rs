mod admin;
mod ai;
mod mcp;

use crate::{
    auth::{bearer_token, error_response},
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
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::Response,
    routing::{any, get, post},
};
use std::net::IpAddr;
use tower_http::cors::CorsLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tracing::{info, warn};

use self::{
    admin::proxy_admin_ui,
    ai::{
        create_realtime_client_secret_handler, proxy_chat, proxy_conversations, proxy_models,
        proxy_realtime, proxy_responses,
    },
    mcp::{proxy_mcp_root, proxy_mcp_server},
};

pub(super) fn public_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(public_healthz))
        .route("/v1/models", get(proxy_models))
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
    let routes = state.inner.routes.lock().await;
    if routes.is_empty() {
        drop(routes);
        let token = match bearer_token(headers) {
            Ok(token) => token,
            Err(response) => {
                warn!("client auth failed: missing or invalid bearer authorization");
                return Err(*response);
            }
        };
        if state.config.client_token.is_empty() {
            warn!("client auth failed: relay client_token is not configured");
            return Err(error_response(
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
            return Err(error_response(
                StatusCode::FORBIDDEN,
                "forbidden",
                "invalid token",
            ));
        }
        Ok(None)
    } else {
        let token = match bearer_token(headers) {
            Ok(token) => token,
            Err(response) => {
                warn!(
                    route_count = routes.len(),
                    "client auth failed: missing or invalid bearer authorization"
                );
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
                Err(error_response(
                    StatusCode::FORBIDDEN,
                    "forbidden",
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
    let policy = state.inner.relay_ip_policy.lock().await.clone();
    if policy.allowed_cidrs.is_empty() {
        return Ok(());
    }
    let Some(client_ip) = ip_acl::resolve_client_ip(peer_ip, headers, &policy.trusted_proxy_cidrs)
    else {
        warn!(%peer_ip, "relay public request denied: client ip could not be resolved");
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "ip_not_allowed",
            "client IP is not allowed",
        ));
    };
    if ip_acl::contains_ip(&policy.allowed_cidrs, client_ip) {
        return Ok(());
    }
    warn!(%peer_ip, %client_ip, "relay public request denied by ip whitelist");
    Err(error_response(
        StatusCode::FORBIDDEN,
        "ip_not_allowed",
        "client IP is not allowed",
    ))
}
