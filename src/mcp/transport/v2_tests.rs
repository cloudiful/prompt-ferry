use axum::{Router, response::IntoResponse};
use chrono::Utc;
use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, InitializeResult,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    service::RoleServer,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{Value, json};

use crate::db::McpServer;

use super::call;

struct V2TestServer;

impl ServerHandler for V2TestServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![Tool::new(
            "echo",
            "echoes the text argument",
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
                .as_object()
                .unwrap()
                .clone(),
        )]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let text = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into())
    }

    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        InitializeResult::new(capabilities)
            .with_server_info(rmcp::model::Implementation::new("v2-test-upstream", "1.0"))
    }
}

async fn spawn_v2_upstream() -> String {
    let service = StreamableHttpService::new(
        || Ok(V2TestServer),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().disable_allowed_hosts(),
    );
    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}/mcp")
}

fn v2_server(url: &str) -> McpServer {
    McpServer {
        server_id: uuid::Uuid::new_v4(),
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "v2-upstream".to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        url: Some(url.to_string()),
        command: None,
        args: json!([]),
        env_json: json!({}),
        bearer_tokens_json: json!([]),
        http_headers_json: json!({}),
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: json!([]),
        disabled_tools: json!([]),
        disabled_resources: json!([]),
        daily_max_requests: None,
        monthly_max_requests: None,
        enabled: true,
        timeout_ms: 30_000,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn client_negotiates_2026_07_28_and_lists_tools() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
    )
    .await
    .unwrap();

    let tools = response["result"]["tools"].as_array().expect("tools list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str(), Some("echo"));
}

#[tokio::test]
async fn client_falls_back_to_legacy_when_discover_probe_rejected() {
    let url = spawn_legacy_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
    )
    .await
    .unwrap();

    let tools = response["result"]["tools"].as_array().expect("tools list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str(), Some("legacy-echo"));
}

/// A mock of an rmcp <= 2.2.0 upstream: the `2026-07-28` protocol probe is
/// rejected at the HTTP layer with 400 (as old SDKs did against unknown
/// `MCP-Protocol-Version` headers) and only the legacy `initialize` flow and
/// JSON-RPC requests are served.
async fn spawn_legacy_upstream() -> String {
    async fn handle(headers: http::HeaderMap, body: axum::body::Body) -> axum::response::Response {
        let probe_version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok());
        if probe_version == Some("2026-07-28") {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "Unsupported MCP-Protocol-Version: 2026-07-28",
            )
                .into_response();
        }
        let body = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default().to_string();
        match method.as_str() {
            "initialize" => axum::Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "serverInfo": {"name": "legacy-mock", "version": "1.0"}
                }
            }))
            .into_response(),
            "notifications/initialized" => axum::http::StatusCode::ACCEPTED.into_response(),
            "tools/list" => axum::Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "legacy-echo",
                        "description": "legacy tool",
                        "inputSchema": {"type": "object"}
                    }]
                }
            }))
            .into_response(),
            _ => axum::Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "Method not found"}
            }))
            .into_response(),
        }
    }

    let router = axum::Router::new().route("/mcp", axum::routing::post(handle));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}/mcp")
}

#[tokio::test]
async fn client_calls_tool_on_2026_07_28_upstream() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {"name": "echo", "arguments": {"text": "hello"}}
        }),
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        response["result"]["content"][0]["text"].as_str(),
        Some("hello")
    );
}
