use std::{
    sync::{Arc, LazyLock},
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
    McpCatalogCache,
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

pub async fn handle(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    request: McpRequestContext<'_>,
) -> anyhow::Result<(u16, String, Vec<(String, String)>, Vec<u8>)> {
    match handle_stream_with_session_store(pool, cache, request, None).await? {
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

pub async fn handle_stream(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    request: McpRequestContext<'_>,
) -> anyhow::Result<McpTransportResponse> {
    handle_stream_with_session_store(pool, cache, request, None).await
}

pub async fn handle_stream_with_session_store(
    pool: &sqlx::PgPool,
    cache: &McpCatalogCache,
    request: McpRequestContext<'_>,
    session_store: Option<Arc<dyn SessionStore>>,
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
            },
        )?;

        let http_service = StreamableHttpService::new(
            {
                let pool = pool.clone();
                let cache = cache.clone();
                move || Ok(ProxyService::new(pool.clone(), cache.clone()))
            },
            RMCP_SESSIONS.clone(),
            {
                let mut config = StreamableHttpServerConfig::default()
                    .with_sse_keep_alive(Some(SSE_KEEP_ALIVE))
                    .with_sse_retry(Some(SSE_RETRY))
                    .disable_allowed_hosts();
                config.session_store = session_store;
                config
            },
        );

        let response = normalize_response(http_service.handle(request).await).await?;
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
