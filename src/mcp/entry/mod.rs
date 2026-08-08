use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use anyhow::anyhow;
use bytes::Bytes;
use futures::{StreamExt, stream::BoxStream};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService,
    session::{
        SessionStore,
        local::{LocalSessionManager, SessionConfig},
    },
};
use serde_json::{Value, json};

use crate::db::McpServer;

use super::{
    MAX_MCP_REQUEST_BODY_BYTES, McpCatalogCache,
    server::{ProxyService, RequestScope},
    service,
};

mod request;
mod response;

use request::{build_rmcp_request, has_mcp_session_id};
use response::{json_response, normalize_response};

const MAX_MCP_BODY_BYTES: usize = 8 * 1024 * 1024;
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
const SSE_KEEP_ALIVE: Duration = Duration::from_secs(15);
const SSE_RETRY: Duration = Duration::from_secs(3);

type McpResponseStream = BoxStream<'static, anyhow::Result<Bytes>>;

static RMCP_SESSIONS: LazyLock<Arc<LocalSessionManager>> = LazyLock::new(|| {
    let mut manager = LocalSessionManager::default();
    let mut session_config = SessionConfig::default();
    session_config.keep_alive = Some(SESSION_TTL);
    session_config.sse_retry = Some(SSE_RETRY);
    manager.session_config = session_config;
    Arc::new(manager)
});

/// Reusable Streamable HTTP services, one per external session store.
///
/// rmcp shares session restore coordination (`pending_restores`) and the
/// SEP-2243 tool schema cache per `StreamableHttpService` instance, so a new
/// service per HTTP request would defeat both. The worker constructs one
/// service for its `mcp_session_store` and reuses it for the whole lifecycle;
/// requests only clone it. Tests use their own store instances (or `None`),
/// so they never share a service.
static MCP_HTTP_SERVICES: LazyLock<
    Mutex<HashMap<String, StreamableHttpService<ProxyService, LocalSessionManager>>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub enum McpTransportResponse {
    Buffered {
        status: u16,
        content_type: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        selected_token_slot: Option<i16>,
    },
    Streaming {
        status: u16,
        content_type: String,
        headers: Vec<(String, String)>,
        stream: McpResponseStream,
        selected_token_slot: Option<i16>,
    },
}

pub struct McpRequestContext<'a> {
    pub user_id: Option<i64>,
    pub server_name: Option<&'a str>,
    pub method: &'a str,
    pub path: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

/// Buffered handler used by tests and admin tooling. Bypasses Origin
/// validation (no allowlist is available here); production traffic goes
/// through [`handle_stream_with_session_store`], which enforces it.
pub async fn handle(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    request: McpRequestContext<'_>,
) -> anyhow::Result<(u16, String, Vec<(String, String)>, Vec<u8>)> {
    match handle_stream_with_session_store(pool, cache, request, None, &[]).await? {
        McpTransportResponse::Buffered {
            status,
            content_type,
            headers,
            body,
            ..
        } => Ok((status, content_type, headers, body)),
        McpTransportResponse::Streaming {
            status,
            content_type,
            headers,
            mut stream,
            ..
        } => {
            let mut body = Vec::new();
            while let Some(chunk) = stream.next().await {
                body.extend_from_slice(&chunk?);
            }
            Ok((status, content_type, headers, body))
        }
    }
}

/// Streaming handler used by tests. Bypasses Origin validation (no allowlist
/// is available here); production traffic goes through
/// [`handle_stream_with_session_store`], which enforces it.
pub async fn handle_stream(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    request: McpRequestContext<'_>,
) -> anyhow::Result<McpTransportResponse> {
    handle_stream_with_session_store(pool, cache, request, None, &[]).await
}

pub async fn handle_stream_with_session_store(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    request: McpRequestContext<'_>,
    session_store: Option<Arc<dyn SessionStore>>,
    allowed_origins: &[String],
) -> anyhow::Result<McpTransportResponse> {
    let McpRequestContext {
        user_id,
        server_name,
        method,
        path,
        headers,
        body,
    } = request;
    super::transport::with_tracked_token_slot(async {
        if !matches!(
            method.to_ascii_uppercase().as_str(),
            "GET" | "POST" | "DELETE"
        ) {
            return Err(anyhow!("unsupported MCP HTTP method: {method}"));
        }

        if method.eq_ignore_ascii_case("GET") && !has_mcp_session_id(headers) {
            return Ok(McpTransportResponse::Buffered {
                status: 405,
                content_type: "application/json".to_string(),
                headers: Vec::new(),
                body: serde_json::to_vec(&json!({
                    "error": {
                        "code": "unsupported_transport",
                        "message": "Legacy SSE transport is not supported; initialize with streamable HTTP first"
                    }
                }))?,
                selected_token_slot: None,
            });
        }

        let request = build_rmcp_request(
            method,
            path,
            headers,
            body,
            RequestScope {
                user_id,
                server_name: server_name.map(str::to_string),
                conversation_id: None,
                pool: pool.clone(),
                cache: cache.clone(),
            },
        )?;

        let http_service = reusable_http_service(session_store.clone(), allowed_origins);

        let response = normalize_response(http_service.handle(request).await).await?;
        if session_store.is_none() && response_status(&response) == 404 {
            // Without a shared session store each worker only knows its own
            // sessions; a 404 here may mean the session lives on another
            // worker (session drift), not that it was deleted.
            tracing::warn!(
                category = "mcp_session_drift",
                path,
                "MCP session not found locally and no shared session store is configured; the session may live on another worker"
            );
        }
        let selected_token_slot = super::transport::tracked_token_slot();
        Ok(match response {
            McpTransportResponse::Buffered {
                status,
                content_type,
                headers,
                body,
                ..
            } => McpTransportResponse::Buffered {
                status,
                content_type,
                headers,
                body,
                selected_token_slot,
            },
            McpTransportResponse::Streaming {
                status,
                content_type,
                headers,
                stream,
                ..
            } => McpTransportResponse::Streaming {
                status,
                content_type,
                headers,
                stream,
                selected_token_slot,
            },
        })
    })
    .await
}

/// Get (or lazily build) the shared `StreamableHttpService` for a session
/// store. The store's `Arc` pointer identifies the worker's store instance;
/// `None` (local sessions only) maps to a shared service as well. The origin
/// allowlist is part of the identity because it is baked into the service
/// config.
fn reusable_http_service(
    session_store: Option<Arc<dyn SessionStore>>,
    allowed_origins: &[String],
) -> StreamableHttpService<ProxyService, LocalSessionManager> {
    let key = format!(
        "{}|{}",
        session_store
            .as_ref()
            .map(|store| Arc::as_ptr(store) as *const () as usize)
            .unwrap_or(0),
        allowed_origins.join(",")
    );
    let mut services = MCP_HTTP_SERVICES
        .lock()
        .expect("mcp http service registry lock");
    if let Some(service) = services.get(&key) {
        return service.clone();
    }
    let service = StreamableHttpService::new(|| Ok(ProxyService::new()), RMCP_SESSIONS.clone(), {
        let mut config = StreamableHttpServerConfig::default()
            .with_sse_keep_alive(Some(SSE_KEEP_ALIVE))
            .with_sse_retry(Some(SSE_RETRY))
            .disable_allowed_hosts()
            .with_max_request_body_bytes(MAX_MCP_REQUEST_BODY_BYTES)
            .with_stateless_protocol_metadata_required(true);
        config.session_store = session_store;
        if !allowed_origins.is_empty() {
            config = config.with_allowed_origins(allowed_origins.iter().cloned());
        }
        config
    });
    services.insert(key, service.clone());
    service
}

fn response_status(response: &McpTransportResponse) -> u16 {
    match response {
        McpTransportResponse::Buffered { status, .. }
        | McpTransportResponse::Streaming { status, .. } => *status,
    }
}

pub async fn inspect_server(server: &McpServer) -> anyhow::Result<(Value, Value, Value)> {
    let snapshot = service::fetch_server_snapshot(server).await?;
    let (tools, resources, prompts) = service::snapshot_to_test_values(&snapshot);
    Ok((
        json_response(json!("admin-test-tools"), tools),
        json_response(json!("admin-test-resources"), resources),
        json_response(json!("admin-test-prompts"), prompts),
    ))
}

#[cfg(test)]
mod tests;
