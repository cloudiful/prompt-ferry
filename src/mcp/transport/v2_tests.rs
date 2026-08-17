use std::borrow::Cow;

use axum::{Router, extract::State, response::IntoResponse};
use chrono::Utc;
use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, InitializeResult,
        InputRequiredResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
        ServerCapabilities, ServerInfo, Tool,
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

const ECHO_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "text":    { "type": "string",  "x-mcp-header": "X-Echo-Text" },
    "count":   { "type": "integer", "x-mcp-header": "X-Echo-Count" },
    "flag":    { "type": "boolean", "x-mcp-header": "X-Echo-Flag" },
    "unicode": { "type": "string",  "x-mcp-header": "X-Echo-Unicode" }
  }
}"#;

#[derive(Default)]
struct V2TestServer {
    /// Tool names whose next call is rejected with -32020 (one-shot),
    /// simulating a server whose SEP-2243 schema cache was stale. Shared
    /// across service instances: once the "stale" call was rejected and the
    /// client retried, the server has refreshed its catalog.
    flaky_first_call: Option<std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>>,
}

fn echo_tool() -> Tool {
    Tool::new(
        "echo",
        "echoes the text argument",
        serde_json::from_str::<serde_json::Map<String, Value>>(ECHO_SCHEMA).unwrap(),
    )
}

impl ServerHandler for V2TestServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![
            echo_tool(),
            Tool::new(
                "mrtr",
                "requires input before completing",
                json!({"type": "object", "properties": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            Tool::new(
                "flaky-cache",
                "rejects the first call with a header mismatch",
                json!({"type": "object", "properties": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        ]))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        match name {
            "echo" => Some(echo_tool()),
            "mrtr" | "flaky-cache" => Some(Tool::new(
                name.to_string(),
                "mock tool",
                json!({"type": "object", "properties": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            )),
            _ => None,
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.as_ref();
        if name == "flaky-cache"
            && self
                .flaky_first_call
                .as_ref()
                .is_some_and(|set| set.lock().unwrap().remove("flaky-cache"))
        {
            return Err(ErrorData::header_mismatch(
                "Mcp-Param-* headers did not match the request body (stale cache)",
                None,
            ));
        }
        if name == "mrtr" {
            if request.input_responses.is_none() && request.request_state.is_none() {
                return Ok(InputRequiredResult::from_request_state("round-1-state").into());
            }
            return Ok(CallToolResult::success(vec![ContentBlock::text("mrtr-complete")]).into());
        }
        let mut echoed = request.arguments.clone().unwrap_or_default();
        echoed.insert("name".to_string(), json!(request.name.as_ref().to_string()));
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&echoed).unwrap_or_default(),
        )])
        .into())
    }

    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        InitializeResult::new(capabilities)
            .with_server_info(rmcp::model::Implementation::new("v2-test-upstream", "1.0"))
    }
}

/// Upstream that only speaks the legacy 2025-11-25 protocol and therefore
/// never requires SEP-2243 standard headers.
struct LegacyV2TestServer;

impl ServerHandler for LegacyV2TestServer {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[ProtocolVersion::V_2025_11_25])
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![echo_tool()]))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        (name == "echo").then(echo_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let mut echoed = request.arguments.clone().unwrap_or_default();
        echoed.insert("name".to_string(), json!(request.name.as_ref().to_string()));
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&echoed).unwrap_or_default(),
        )])
        .into())
    }

    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        InitializeResult::new(capabilities)
            .with_server_info(rmcp::model::Implementation::new("legacy-v2-test", "1.0"))
    }
}

async fn spawn_v2_upstream() -> String {
    let service = StreamableHttpService::new(
        || Ok(V2TestServer::default()),
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

/// Upstream whose session service factory shares one flaky-state set, so the
/// "reject once per tool" behavior survives per-session service instances.
async fn spawn_v2_upstream_with_flaky_tool() -> String {
    let flaky_first_call =
        std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
    flaky_first_call
        .lock()
        .unwrap()
        .insert("flaky-cache".to_string());
    let service = StreamableHttpService::new(
        move || {
            Ok(V2TestServer {
                flaky_first_call: Some(flaky_first_call.clone()),
            })
        },
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

async fn spawn_legacy_v2_upstream() -> String {
    let service = StreamableHttpService::new(
        || Ok(LegacyV2TestServer),
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
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
        lifecycle_learned_mode: None,
        lifecycle_learned_protocol_version: None,
        lifecycle_learned_for_updated_at: None,
        lifecycle_learned_at: None,
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
        None,
    )
    .await
    .unwrap();

    let tools = response["result"]["tools"].as_array().expect("tools list");
    assert_eq!(tools.len(), 3);
    assert!(tools.iter().any(|tool| tool["name"] == "echo"));
}

#[tokio::test]
async fn client_falls_back_to_legacy_when_discover_probe_rejected() {
    let url = spawn_legacy_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
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

/// A mock of https://mcp.grep.app (gh-grep), which rejects `server/discover`
/// probes for protocol versions it does not declare with an HTTP 400 that has
/// NO `Content-Type` header and a JSON-RPC `-32000` body whose message names
/// the supported versions (e.g. `2025-06-18`). It answers `server/discover`
/// with `-32601 Method not found` for supported versions and only serves the
/// legacy `initialize` flow.
async fn spawn_gh_grep_upstream() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let initialize_versions = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = initialize_versions.clone();

    async fn handle(
        headers: http::HeaderMap,
        body: axum::body::Body,
        observed: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) -> axum::response::Response {
        let supported = ["2025-06-18", "2025-03-26", "2024-11-05", "2024-10-07"];
        let probe_version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok());
        if probe_version.is_some_and(|version| !supported.contains(&version)) {
            observed
                .lock()
                .unwrap()
                .push(format!("rejected:{probe_version:?}"));
            // gh-grep answers without any Content-Type header; axum adds one
            // for `String` bodies, so build the response manually.
            return axum::response::Response::builder()
                .status(axum::http::StatusCode::BAD_REQUEST)
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","error":{"code":-32000,"message":"Bad Request: Unsupported protocol version (supported versions: 2025-06-18, 2025-03-26, 2024-11-05, 2024-10-07)"},"id":null}"#,
                ))
                .unwrap();
        }
        let body = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default().to_string();
        let data = match method.as_str() {
            "server/discover" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "Method not found"}
            }),
            "initialize" => {
                let requested = request["params"]["protocolVersion"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                observed.lock().unwrap().push(requested);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {"tools": {"listChanged": true}},
                        "serverInfo": {"name": "mcp-typescript server on vercel", "version": "0.1.0"}
                    }
                })
            }
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "grep-echo",
                        "description": "legacy grep tool",
                        "inputSchema": {"type": "object"}
                    }]
                }
            }),
            _ => {
                return axum::http::StatusCode::ACCEPTED.into_response();
            }
        };
        (
            [("content-type", "text/event-stream")],
            format!("event: message\ndata: {data}\n\n"),
        )
            .into_response()
    }

    let router = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |headers, body| {
            let observed = observed.clone();
            async move { handle(headers, body, observed).await }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), initialize_versions)
}

#[tokio::test]
async fn client_falls_back_to_legacy_with_negotiated_version_for_grep_style_http_400() {
    let (url, initialize_versions) = spawn_gh_grep_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();

    let tools = response["result"]["tools"].as_array().expect("tools list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str(), Some("grep-echo"));
    let versions = initialize_versions.lock().unwrap();
    assert!(
        versions.iter().any(|version| version == "2025-06-18"),
        "legacy initialize must negotiate the highest version the upstream supports, got {versions:?}"
    );
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
        None,
    )
    .await
    .unwrap();

    let echoed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["text"], "hello");
}

#[tokio::test]
async fn tool_call_sends_all_annotated_param_headers() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": {
                    "text": "ascii",
                    "count": 42,
                    "flag": true,
                    "unicode": "café"
                }
            }
        }),
        None,
        None,
    )
    .await
    .unwrap();

    // The upstream validates Mcp-Param-* against the body (missing or wrong
    // values would be rejected with -32020 before the handler runs), so a
    // successful echo proves every annotated argument travelled as a header:
    // plain ASCII, Base64-wrapped Unicode, integer, and boolean forms.
    let echoed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["text"], "ascii");
    assert_eq!(echoed["count"], 42);
    assert_eq!(echoed["flag"], true);
    assert_eq!(echoed["unicode"], "café");
}

#[tokio::test]
async fn tool_call_omits_headers_for_missing_annotated_params() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {"name": "echo", "arguments": {"text": "only-text"}}
        }),
        None,
        None,
    )
    .await
    .unwrap();

    // count/flag/unicode are absent from the body, so no Mcp-Param-* headers
    // may be sent for them; the upstream would reject unexpected headers
    // with -32020, so success proves the omission is correct.
    let echoed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["text"], "only-text");
    assert_eq!(echoed.get("count"), None);
}

#[tokio::test]
async fn rewritten_aggregate_tool_name_regenerates_mcp_headers() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    // The aggregate path rewrites `github__list_issues` to the upstream name
    // before calling the outbound client; the client must regenerate
    // Mcp-Name (list_issues) and Mcp-Param-* from the rewritten request
    // instead of copying downstream headers. The upstream rejects stale
    // Mcp-Name values with -32020, so success proves regeneration.
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "tools/call",
            "params": {
                "name": "list_issues",
                "arguments": {"text": "rewritten", "count": 7, "flag": false}
            }
        }),
        None,
        None,
    )
    .await
    .unwrap();

    let echoed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["name"], "list_issues");
    assert_eq!(echoed["text"], "rewritten");
    assert_eq!(echoed["count"], 7);
    assert_eq!(echoed["flag"], false);
}

#[tokio::test]
async fn header_mismatch_relists_and_retries_once() {
    let url = spawn_v2_upstream_with_flaky_tool().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": {"name": "flaky-cache", "arguments": {}}
        }),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        response["result"]["content"][0]["text"].as_str(),
        Some("{\"name\":\"flaky-cache\"}")
    );
}

#[tokio::test]
async fn input_required_result_round_trips_request_state() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {"name": "mrtr", "arguments": {}}
        }),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        response["result"]["resultType"].as_str(),
        Some("input_required")
    );
    assert_eq!(
        response["result"]["requestState"].as_str(),
        Some("round-1-state")
    );
}

#[tokio::test]
async fn input_responses_and_request_state_reach_upstream() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "tools/call",
            "params": {
                "name": "mrtr",
                "arguments": {},
                "inputResponses": {"q1": {"action": "accept", "content": {"x": 1}}},
                "requestState": "round-1-state"
            }
        }),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(
        response["result"]["content"][0]["text"].as_str(),
        Some("mrtr-complete")
    );
}

#[tokio::test]
async fn legacy_2025_11_25_upstream_does_not_require_standard_headers() {
    let url = spawn_legacy_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "tools/call",
            "params": {
                "name": "echo",
                "arguments": {"text": "legacy", "count": 1, "flag": true, "unicode": "héllo"}
            }
        }),
        None,
        None,
    )
    .await
    .unwrap();

    let echoed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["text"], "legacy");
    assert_eq!(echoed["unicode"], "héllo");
}

#[tokio::test]
async fn resources_templates_list_is_forwarded() {
    let url = spawn_v2_upstream().await;
    let server = v2_server(&url);
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 10, "method": "resources/templates/list"}),
        None,
        None,
    )
    .await
    .unwrap();

    assert!(
        response["result"]["resourceTemplates"].is_array(),
        "expected resourceTemplates array: {response}"
    );
}

/// Upstream that rejects bearer tokens by order: the first token always gets
/// 401, the second 429, and only the third is accepted — exercising token
/// failover across the whole enabled set, including handshake-stage auth
/// failures.
async fn spawn_token_failover_upstream() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>)
{
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let service = StreamableHttpService::new(
        || Ok(V2TestServer::default()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default().disable_allowed_hosts(),
    );
    async fn auth_middleware(
        State(seen): State<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
        request: axum::http::Request<axum::body::Body>,
        next: axum::middleware::Next,
    ) -> axum::response::Response {
        let token = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or("")
            .to_string();
        seen.lock().unwrap().push(token.clone());
        match token.as_str() {
            "first-token" => (
                axum::http::StatusCode::UNAUTHORIZED,
                "first token is rejected",
            )
                .into_response(),
            "second-token" => (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "second token is throttled",
            )
                .into_response(),
            _ => next.run(request).await,
        }
    }
    let router =
        Router::new()
            .nest_service("/mcp", service)
            .layer(axum::middleware::from_fn_with_state(
                seen.clone(),
                auth_middleware,
            ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), seen)
}

#[tokio::test]
async fn bearer_token_failover_tries_all_tokens_and_recovers_on_third() {
    let (url, seen) = spawn_token_failover_upstream().await;
    let mut server = v2_server(&url);
    server.bearer_tokens_json = json!([
        {"token": "first-token", "enabled": true},
        {"token": "second-token", "enabled": true},
        {"token": "third-token", "enabled": true},
    ]);

    let response = call(
        &server,
        json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "tools/call",
            "params": {"name": "echo", "arguments": {"text": "failover"}}
        }),
        None,
        None,
    )
    .await
    .unwrap();

    let echoed: Value =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["text"], "failover");

    let seen = seen.lock().unwrap().clone();
    assert!(
        seen.iter().any(|token| token == "first-token"),
        "first token must be attempted: {seen:?}"
    );
    assert!(
        seen.iter().any(|token| token == "second-token"),
        "second token must be attempted: {seen:?}"
    );
    assert!(
        seen.iter().any(|token| token == "third-token"),
        "third token must be attempted: {seen:?}"
    );
}

fn request_id_from_body(body: &[u8]) -> Value {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|request| request.get("id").cloned())
        .unwrap_or(Value::Null)
}

fn legacy_probe_response(id: Value, method: &str) -> axum::response::Response {
    match method {
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
            "result": {"tools": [{
                "name": "legacy-echo",
                "description": "legacy tool",
                "inputSchema": {"type": "object"}
            }]}
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

fn discover_success_response(id: Value, supported: &[String]) -> axum::response::Response {
    axum::Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "resultType": "complete",
            "supportedVersions": supported,
            "capabilities": {},
            "ttlMs": 0,
            "cacheScope": "public"
        }
    }))
    .into_response()
}

fn json_rpc_error_response(
    id: Value,
    status: axum::http::StatusCode,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> axum::response::Response {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    (
        status,
        [(http::header::CONTENT_TYPE, "application/json")],
        json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string(),
    )
        .into_response()
}

/// The non-standard rejection some gateways/SDKs emit for the `2026-07-28`
/// probe: a generic outer error (`-32000 Bad Request`) whose `data` carries
/// the real unsupported-version message and supported-version list.
fn nonstandard_rejection_body() -> Value {
    json!({
        "error": {
            "code": -32000,
            "message": "Bad Request",
            "data": {
                "message": "Unsupported protocol version: 2026-07-28",
                "supported": ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]
            }
        }
    })
}

/// (JSON-RPC method, `mcp-protocol-version` header) pairs seen by a mock.
/// Version is `-` when the request carried no protocol header.
type SeenVersions = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;

/// Mock of an upstream that rejects the `2026-07-28` discover probe with a
/// non-standard nested JSON-RPC error (HTTP 400 + `-32000 Bad Request`) and
/// otherwise only speaks the legacy `initialize` flow.
async fn spawn_nonstandard_rejection_upstream() -> (String, SeenVersions) {
    async fn handle(
        seen: SeenVersions,
        headers: http::HeaderMap,
        body: axum::body::Body,
    ) -> axum::response::Response {
        let probe_version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let body = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let id = request_id_from_body(&body);
        let request: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
        let method = request["method"].as_str().unwrap_or_default().to_string();
        seen.lock()
            .unwrap()
            .push((method.clone(), probe_version.clone()));
        if probe_version == "2026-07-28" {
            return json_rpc_error_response(
                id,
                axum::http::StatusCode::BAD_REQUEST,
                -32000,
                "Bad Request",
                Some(nonstandard_rejection_body()),
            );
        }
        legacy_probe_response(id, &method)
    }

    let seen: SeenVersions = Default::default();
    let state = seen.clone();
    let router = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |headers: http::HeaderMap, body: axum::body::Body| {
            handle(state.clone(), headers, body)
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), seen)
}

#[tokio::test]
async fn client_falls_back_when_discover_rejected_with_nonstandard_jsonrpc_error() {
    let (url, seen) = spawn_nonstandard_rejection_upstream().await;
    let server = v2_server(&url);

    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    let tools = response["result"]["tools"].as_array().expect("tools list");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"].as_str(), Some("legacy-echo"));

    // The second call reuses the learned legacy lifecycle and must not probe
    // 2026-07-28 again.
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(response["result"]["tools"].is_array());

    let seen = seen.lock().unwrap();
    let probes_2026 = seen
        .iter()
        .filter(|(method, version)| method == "server/discover" && version == "2026-07-28")
        .count();
    assert_eq!(
        probes_2026, 1,
        "the rejected probe must not be replayed per request: {seen:?}"
    );
}

#[tokio::test]
async fn lifecycle_cache_reprobes_after_server_config_update() {
    let (url, seen) = spawn_nonstandard_rejection_upstream().await;
    let mut server = v2_server(&url);

    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(response["result"]["tools"].is_array());

    server.updated_at += chrono::Duration::seconds(1);
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(response["result"]["tools"].is_array());

    let seen = seen.lock().unwrap();
    let probes_2026 = seen
        .iter()
        .filter(|(method, version)| method == "server/discover" && version == "2026-07-28")
        .count();
    assert_eq!(
        probes_2026, 2,
        "a bumped updated_at must invalidate the learned lifecycle: {seen:?}"
    );
}

/// Mock of an upstream that rejects the `2026-07-28` probe with a standard
/// `-32022` error carrying its supported-version list, then serves a
/// successful `server/discover` for any other requested version.
async fn spawn_32022_discover_upstream(supported: Vec<String>) -> (String, SeenVersions) {
    async fn handle(
        seen: SeenVersions,
        supported: std::sync::Arc<Vec<String>>,
        headers: http::HeaderMap,
        body: axum::body::Body,
    ) -> axum::response::Response {
        let probe_version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let body = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let id = request_id_from_body(&body);
        let request: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
        let method = request["method"].as_str().unwrap_or_default().to_string();
        seen.lock()
            .unwrap()
            .push((method.clone(), probe_version.clone()));
        if probe_version == "2026-07-28" {
            return json_rpc_error_response(
                id,
                axum::http::StatusCode::OK,
                -32022,
                "Unsupported protocol version",
                Some(json!({
                    "requested": "2026-07-28",
                    "supported": supported,
                })),
            );
        }
        match method.as_str() {
            "server/discover" => discover_success_response(id, &supported),
            "tools/list" => axum::Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"tools": [{
                    "name": "negotiated-echo",
                    "description": "negotiated tool",
                    "inputSchema": {"type": "object"}
                }]}
            }))
            .into_response(),
            _ => json_rpc_error_response(
                id,
                axum::http::StatusCode::OK,
                -32601,
                "Method not found",
                None,
            ),
        }
    }

    let seen: SeenVersions = Default::default();
    let supported = std::sync::Arc::new(supported);
    let seen_state = seen.clone();
    let supported_state = supported.clone();
    let router = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |headers: http::HeaderMap, body: axum::body::Body| {
            handle(seen_state.clone(), supported_state.clone(), headers, body)
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), seen)
}

#[tokio::test]
async fn client_negotiates_newest_mutually_supported_version_from_32022() {
    let (url, seen) = spawn_32022_discover_upstream(
        ["2025-11-25", "2025-06-18"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    )
    .await;
    let server = v2_server(&url);

    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(response["result"]["tools"].is_array());

    let seen = seen.lock().unwrap();
    assert_eq!(
        &seen[..],
        &[
            ("server/discover".to_string(), "2026-07-28".to_string()),
            ("server/discover".to_string(), "2025-11-25".to_string()),
            ("tools/list".to_string(), "2025-11-25".to_string()),
        ],
        "must reject the newest probe, then negotiate 2025-11-25: {seen:?}"
    );
}

#[tokio::test]
async fn client_negotiates_older_protocol_when_it_is_the_only_overlap() {
    let (url, seen) = spawn_32022_discover_upstream(vec!["2025-06-18".to_string()]).await;
    let server = v2_server(&url);

    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert!(response["result"]["tools"].is_array());

    let seen = seen.lock().unwrap();
    assert_eq!(
        &seen[..],
        &[
            ("server/discover".to_string(), "2026-07-28".to_string()),
            ("server/discover".to_string(), "2025-06-18".to_string()),
            ("tools/list".to_string(), "2025-06-18".to_string()),
        ],
        "must fall back to an older draft when it is the only overlap: {seen:?}"
    );
}

#[tokio::test]
async fn client_does_not_fallback_on_unrelated_internal_error() {
    async fn handle(
        seen: SeenVersions,
        headers: http::HeaderMap,
        body: axum::body::Body,
    ) -> axum::response::Response {
        let probe_version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let body = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let id = request_id_from_body(&body);
        let request: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
        let method = request["method"].as_str().unwrap_or_default().to_string();
        seen.lock()
            .unwrap()
            .push((method.clone(), probe_version.clone()));
        json_rpc_error_response(
            id,
            axum::http::StatusCode::OK,
            -32603,
            "Internal server error",
            Some(json!({ "err": "boom" })),
        )
    }

    let seen: SeenVersions = Default::default();
    let state = seen.clone();
    let router = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |headers: http::HeaderMap, body: axum::body::Body| {
            handle(state.clone(), headers, body)
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let server = v2_server(&format!("http://{addr}/mcp"));

    let err = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("Internal server error"));

    let seen = seen.lock().unwrap();
    assert_eq!(
        &seen[..],
        &[("server/discover".to_string(), "2026-07-28".to_string())],
        "unrelated internal errors must not trigger the legacy fallback: {seen:?}"
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpstreamPhase {
    /// Legacy-only: non-standard rejection of the modern probe, legacy
    /// `initialize` works.
    LegacyOnly,
    /// Modern-only: legacy `initialize` is rejected with a protocol error,
    /// `server/discover` works.
    ModernOnly,
}

/// Mock whose protocol behavior can be switched between calls, to prove the
/// learned lifecycle cache self-heals when the upstream changes.
async fn spawn_phase_switch_upstream() -> (
    String,
    std::sync::Arc<std::sync::Mutex<UpstreamPhase>>,
    SeenVersions,
) {
    async fn handle(
        seen: SeenVersions,
        phase: std::sync::Arc<std::sync::Mutex<UpstreamPhase>>,
        headers: http::HeaderMap,
        body: axum::body::Body,
    ) -> axum::response::Response {
        let probe_version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let body = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let id = request_id_from_body(&body);
        let request: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
        let method = request["method"].as_str().unwrap_or_default().to_string();
        seen.lock()
            .unwrap()
            .push((method.clone(), probe_version.clone()));
        match *phase.lock().unwrap() {
            UpstreamPhase::LegacyOnly => {
                if probe_version == "2026-07-28" {
                    return json_rpc_error_response(
                        id,
                        axum::http::StatusCode::BAD_REQUEST,
                        -32000,
                        "Bad Request",
                        Some(nonstandard_rejection_body()),
                    );
                }
                legacy_probe_response(id, &method)
            }
            UpstreamPhase::ModernOnly => match method.as_str() {
                "server/discover" => discover_success_response(id, &["2026-07-28".to_string()]),
                "initialize" => json_rpc_error_response(
                    id,
                    axum::http::StatusCode::OK,
                    -32603,
                    "Unsupported protocol version: 2025-11-25",
                    None,
                ),
                "tools/list" => axum::Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"tools": [{
                        "name": "modern-echo",
                        "description": "modern tool",
                        "inputSchema": {"type": "object"}
                    }]}
                }))
                .into_response(),
                _ => json_rpc_error_response(
                    id,
                    axum::http::StatusCode::OK,
                    -32601,
                    "Method not found",
                    None,
                ),
            },
        }
    }

    let seen: SeenVersions = Default::default();
    let phase = std::sync::Arc::new(std::sync::Mutex::new(UpstreamPhase::LegacyOnly));
    let seen_state = seen.clone();
    let phase_state = phase.clone();
    let router = axum::Router::new().route(
        "/mcp",
        axum::routing::post(move |headers: http::HeaderMap, body: axum::body::Body| {
            handle(seen_state.clone(), phase_state.clone(), headers, body)
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}/mcp"), phase, seen)
}

#[tokio::test]
async fn cached_legacy_lifecycle_self_heals_after_upstream_upgrade() {
    let (url, phase, seen) = spawn_phase_switch_upstream().await;
    let server = v2_server(&url);

    // Phase 1: legacy-only server; the modern probe is rejected once, the
    // legacy lifecycle is learned and reused.
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        response["result"]["tools"][0]["name"].as_str(),
        Some("legacy-echo")
    );

    // Phase 2: the server upgrades to modern-only. The cached legacy
    // initialize is rejected with a protocol error; the client must probe
    // discover again and relearn.
    *phase.lock().unwrap() = UpstreamPhase::ModernOnly;
    let response = call(
        &server,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        response["result"]["tools"][0]["name"].as_str(),
        Some("modern-echo")
    );

    let seen = seen.lock().unwrap();
    let probes_2026 = seen
        .iter()
        .filter(|(method, version)| method == "server/discover" && version == "2026-07-28")
        .count();
    assert_eq!(
        probes_2026, 2,
        "the upgraded server must be rediscovered after the legacy fallback is rejected: {seen:?}"
    );
}
