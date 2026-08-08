use super::*;
use futures::StreamExt;
use rmcp::transport::streamable_http_server::session::{
    SessionState, SessionStore, SessionStoreError,
};
use sqlx::postgres::PgPoolOptions;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

#[derive(Default)]
struct TestSessionStore {
    states: RwLock<HashMap<String, SessionState>>,
    deleted: RwLock<Vec<String>>,
}

#[async_trait::async_trait]
impl SessionStore for TestSessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        Ok(self.states.read().await.get(session_id).cloned())
    }

    async fn store(&self, session_id: &str, state: &SessionState) -> Result<(), SessionStoreError> {
        self.states
            .write()
            .await
            .insert(session_id.to_string(), state.clone());
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        self.states.write().await.remove(session_id);
        self.deleted.write().await.push(session_id.to_string());
        Ok(())
    }
}

fn test_store() -> Arc<TestSessionStore> {
    Arc::new(TestSessionStore::default())
}

fn dyn_store(store: &Arc<TestSessionStore>) -> Arc<dyn SessionStore> {
    store.clone()
}

fn named_request<'a>(
    server_name: &'a str,
    method: &'a str,
    path: &'a str,
    headers: &'a [(String, String)],
    body: &'a [u8],
) -> McpRequestContext<'a> {
    let mut context = request(method, path, headers, body);
    context.server_name = Some(server_name);
    context
}

fn request<'a>(
    method: &'a str,
    path: &'a str,
    headers: &'a [(String, String)],
    body: &'a [u8],
) -> McpRequestContext<'a> {
    McpRequestContext {
        user_id: None,
        server_name: None,
        method,
        path,
        headers,
        body,
    }
}

fn test_pool() -> sqlx::PgPool {
    PgPoolOptions::new()
        .connect_lazy("postgres://postgres:postgres@127.0.0.1/prompt_ferry_test")
        .unwrap()
}

async fn collect_response(
    response: McpTransportResponse,
) -> (u16, String, Vec<(String, String)>, Vec<u8>) {
    match response {
        McpTransportResponse::Buffered {
            status,
            content_type,
            headers,
            body,
            ..
        } => (status, content_type, headers, body),
        McpTransportResponse::Streaming {
            status,
            content_type,
            headers,
            mut stream,
            ..
        } => {
            let mut body = Vec::new();
            while let Some(chunk) = stream.next().await {
                body.extend_from_slice(&chunk.unwrap());
            }
            (status, content_type, headers, body)
        }
    }
}

async fn read_first_chunk(
    response: McpTransportResponse,
) -> (u16, String, Vec<(String, String)>, Vec<u8>) {
    match response {
        McpTransportResponse::Buffered {
            status,
            content_type,
            headers,
            body,
            ..
        } => (status, content_type, headers, body),
        McpTransportResponse::Streaming {
            status,
            content_type,
            headers,
            mut stream,
            ..
        } => {
            let chunk = stream.next().await.unwrap().unwrap().to_vec();
            (status, content_type, headers, chunk)
        }
    }
}

fn last_sse_json(body: &[u8]) -> Value {
    let text = String::from_utf8_lossy(body);
    let payload = text
        .replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|event| {
            let data = event
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>();
            (!data.is_empty()).then(|| data.join("\n"))
        })
        .filter(|payload| !payload.trim().is_empty())
        .last()
        .unwrap();
    serde_json::from_str(&payload).unwrap()
}

#[tokio::test]
async fn aggregate_initialize_returns_session_header() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (status, content_type, headers, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(content_type, "text/event-stream");
    assert!(headers.iter().any(|(name, _)| name == "mcp-session-id"));
    let value = last_sse_json(&body);
    assert_eq!(
        value["result"]["protocolVersion"].as_str(),
        Some("2025-11-25")
    );
}

#[tokio::test]
async fn aggregate_initialized_requires_session_and_returns_accepted() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (_, _, headers, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    let (status, _, _, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[
                    ("mcp-session-id".to_string(), session_id),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 202);
}

#[tokio::test]
async fn get_root_stream_returns_event_stream_with_valid_session() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (_, _, headers, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    let response = handle_stream(
        &pool,
        &cache,
        request(
            "GET",
            "/mcp",
            &[
                ("accept".to_string(), "text/event-stream".to_string()),
                ("mcp-session-id".to_string(), session_id),
                ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
            ],
            &[],
        ),
    )
    .await
    .unwrap();
    let McpTransportResponse::Streaming {
        status,
        content_type,
        ..
    } = response
    else {
        panic!("expected streaming response");
    };

    assert_eq!(status, 200);
    assert_eq!(content_type, "text/event-stream");
}

#[tokio::test]
async fn get_root_stream_starts_with_sse_retry_priming_event() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (_, _, headers, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    let (_, _, _, body) = read_first_chunk(
        handle_stream(
            &pool,
            &cache,
            request(
                "GET",
                "/mcp",
                &[
                    ("accept".to_string(), "text/event-stream".to_string()),
                    ("mcp-session-id".to_string(), session_id),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                &[],
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    let text = String::from_utf8_lossy(&body).replace("\r\n", "\n");
    assert!(
        text.contains("data: \n") && text.contains("id: 0\n") && text.contains("retry: 3000\n"),
        "unexpected first sse chunk: {text:?}"
    );
}

#[tokio::test]
async fn get_root_without_session_returns_legacy_sse_unsupported() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();

    let (status, content_type, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "GET",
                "/mcp",
                &[("accept".to_string(), "text/event-stream".to_string())],
                &[],
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 405);
    assert_eq!(content_type, "application/json");
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        value["error"]["code"].as_str(),
        Some("unsupported_transport")
    );
}

#[tokio::test]
async fn get_root_stream_accepts_last_event_id_for_resume() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (_, _, headers, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    let response = handle_stream(
        &pool,
        &cache,
        request(
            "GET",
            "/mcp",
            &[
                ("accept".to_string(), "text/event-stream".to_string()),
                ("mcp-session-id".to_string(), session_id),
                ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ("last-event-id".to_string(), "event-1".to_string()),
            ],
            &[],
        ),
    )
    .await
    .unwrap();
    let McpTransportResponse::Streaming { status, .. } = response else {
        panic!("expected streaming response");
    };

    assert_eq!(status, 200);
}

#[tokio::test]
async fn delete_root_session_closes_existing_session() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (_, _, headers, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    let (status, _, _, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "DELETE",
                "/mcp",
                &[
                    ("mcp-session-id".to_string(), session_id.clone()),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                &[],
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 202);

    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[
                    ("mcp-session-id".to_string(), session_id),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 404);
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"].as_str(), Some("session_not_found"));
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Session not found")
    );
}

#[tokio::test]
async fn initialize_persists_session_state_to_store() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let store = test_store();
    let (_, _, headers, _) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
            Some(dyn_store(&store)),
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    assert!(store.states.read().await.contains_key(&session_id));
}

#[tokio::test]
async fn missing_session_restores_from_store() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let store = test_store();
    let session_id = uuid::Uuid::new_v4().to_string();
    store.states.write().await.insert(
        session_id.clone(),
        SessionState::new(rmcp::model::InitializeRequestParams::new(
            rmcp::model::ClientCapabilities::default(),
            rmcp::model::Implementation::new("test-client", "0.0.0"),
        )),
    );

    let (status, _, _, _) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[
                    ("mcp-session-id".to_string(), session_id),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            ),
            Some(dyn_store(&store)),
            &[],
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 200);
}

#[tokio::test]
async fn store_miss_returns_session_not_found_code() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let store = test_store();
    let (status, _, _, body) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[
                    (
                        "mcp-session-id".to_string(),
                        uuid::Uuid::new_v4().to_string(),
                    ),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            ),
            Some(dyn_store(&store)),
            &[],
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 404);
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"].as_str(), Some("session_not_found"));
}

#[tokio::test]
async fn delete_session_removes_session_state_from_store() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let store = test_store();
    let (_, _, headers, _) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
            Some(dyn_store(&store)),
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();

    let (status, _, _, _) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "DELETE",
                "/mcp",
                &[
                    ("mcp-session-id".to_string(), session_id.clone()),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                &[],
            ),
            Some(dyn_store(&store)),
            &[],
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 202);
    assert!(!store.states.read().await.contains_key(&session_id));
    assert!(store.deleted.read().await.contains(&session_id));
}

const MCP_V2_META: &str = r#"{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"stateless-test","version":"1.0"}}"#;

fn v2_body(method: &str, id: i64) -> Vec<u8> {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"_meta":{MCP_V2_META}}}}}"#
    )
    .into_bytes()
}

fn v2_headers(method: &str) -> Vec<(String, String)> {
    vec![
        ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
        ("Mcp-Method".to_string(), method.to_string()),
    ]
}

#[tokio::test]
async fn stateless_discover_advertises_2026_07_28_protocol() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let body = v2_body("server/discover", 1);
    let (status, content_type, headers, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request("POST", "/mcp", &v2_headers("server/discover"), &body),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(content_type, "text/event-stream");
    assert!(
        !headers.iter().any(|(name, _)| name == "mcp-session-id"),
        "stateless discover must not create a session"
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    let versions = value["result"]["supportedVersions"].as_array().unwrap();
    assert!(versions.iter().any(|v| v.as_str() == Some("2026-07-28")));
    assert!(versions.iter().any(|v| v.as_str() == Some("2025-11-25")));
    assert!(value["result"]["capabilities"]["tools"].is_object());
    assert!(value["result"]["capabilities"]["resources"].is_object());
    assert!(value["result"]["capabilities"]["prompts"].is_object());
}

#[tokio::test]
async fn stateless_request_is_served_without_session_header() {
    let Ok(url) = std::env::var("PROMPT_FERRY_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping stateless templates/list DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set"
        );
        return;
    };
    let pool = PgPoolOptions::new().connect_lazy(&url).unwrap();
    let cache = McpCatalogCache::new();
    let body = v2_body("resources/templates/list", 2);
    let (status, content_type, headers, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers("resources/templates/list"),
                &body,
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 200);
    assert_eq!(content_type, "text/event-stream");
    assert!(
        !headers.iter().any(|(name, _)| name == "mcp-session-id"),
        "stateless requests must not receive a session header"
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert!(value["result"]["resourceTemplates"].is_array());
}

#[tokio::test]
async fn stateless_legacy_client_still_gets_initialize_session() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let (_, _, headers, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    assert!(headers.iter().any(|(name, _)| name == "mcp-session-id"));
    let value = last_sse_json(&body);
    assert_eq!(
        value["result"]["protocolVersion"].as_str(),
        Some("2025-11-25")
    );
    assert_eq!(
        value["result"].get("resultType"),
        None,
        "legacy initialize result must not carry resultType"
    );
}

#[tokio::test]
async fn stateless_tools_list_aggregates_visible_servers() {
    let Ok(url) = std::env::var("PROMPT_FERRY_TEST_DATABASE_URL") else {
        eprintln!(
            "skipping stateless tools/list DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set"
        );
        return;
    };
    let pool = PgPoolOptions::new().connect_lazy(&url).unwrap();
    let cache = McpCatalogCache::new();
    let body = v2_body("tools/list", 3);
    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request("POST", "/mcp", &v2_headers("tools/list"), &body),
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 200);
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert!(value["result"]["tools"].is_array());
}

// ---------------------------------------------------------------------------
// Reusable service + end-to-end SEP-2243 / MRTR tests
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SlowMissingSessionStore;

#[async_trait::async_trait]
impl SessionStore for SlowMissingSessionStore {
    async fn load(&self, _session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        // Simulate a slow shared store so concurrent restores of the same
        // unknown session overlap.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(None)
    }

    async fn store(
        &self,
        _session_id: &str,
        _state: &SessionState,
    ) -> Result<(), SessionStoreError> {
        Ok(())
    }

    async fn delete(&self, _session_id: &str) -> Result<(), SessionStoreError> {
        Ok(())
    }
}

#[tokio::test]
async fn concurrent_restores_of_unknown_session_share_one_restore() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let store: Arc<dyn SessionStore> = Arc::new(SlowMissingSessionStore);
    let session_id = uuid::Uuid::new_v4().to_string();
    let headers = vec![
        ("mcp-session-id".to_string(), session_id.clone()),
        ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
    ];
    let headers_2 = vec![
        ("mcp-session-id".to_string(), session_id.clone()),
        ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
    ];

    let (first, second) = tokio::join!(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &headers,
                br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            ),
            Some(store.clone()),
            &[],
        ),
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &headers_2,
                br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            ),
            Some(store),
            &[],
        ),
    );

    // Both requests must resolve identically to session_not_found; the shared
    // pending_restores coordination must not let one of them fail with 500.
    for response in [first.unwrap(), second.unwrap()] {
        let (status, _, _, body) = collect_response(response).await;
        assert_eq!(status, 404, "expected session_not_found, got {status}");
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"].as_str(), Some("session_not_found"));
    }
}

struct EntryUpstream;

const ENTRY_ECHO_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "text": { "type": "string", "x-mcp-header": "X-Echo-Text" }
  }
}"#;

impl rmcp::ServerHandler for EntryUpstream {
    async fn list_resource_templates(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::ListResourceTemplatesResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListResourceTemplatesResult::with_all_items(
            vec![rmcp::model::ResourceTemplate::new(
                "git://{owner}/{repo}/issues",
                "issues of a repo",
            )],
        ))
    }

    async fn complete(
        &self,
        request: rmcp::model::CompleteRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::CompleteResult, rmcp::ErrorData> {
        let target = match &request.r#ref {
            rmcp::model::Reference::Prompt(prompt) => prompt.name.clone(),
            rmcp::model::Reference::Resource(resource) => resource.uri.clone(),
            _ => return Err(rmcp::ErrorData::invalid_params("unsupported ref", None)),
        };
        let completion = rmcp::model::CompletionInfo::new(vec![
            format!("{target}-suggestion"),
            format!("{target}-alt"),
        ])
        .expect("two values");
        Ok(rmcp::model::CompleteResult::new(completion))
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        Ok(rmcp::model::ListToolsResult::with_all_items(vec![
            rmcp::model::Tool::new(
                "list_issues",
                "lists issues",
                serde_json::from_str::<serde_json::Map<String, Value>>(ENTRY_ECHO_SCHEMA).unwrap(),
            ),
            rmcp::model::Tool::new(
                "mrtr",
                "requires input before completing",
                serde_json::json!({"type": "object", "properties": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        ]))
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        match name {
            "list_issues" => Some(rmcp::model::Tool::new(
                "list_issues",
                "lists issues",
                serde_json::from_str::<serde_json::Map<String, Value>>(ENTRY_ECHO_SCHEMA).unwrap(),
            )),
            "mrtr" => Some(rmcp::model::Tool::new(
                "mrtr",
                "requires input",
                serde_json::json!({"type": "object", "properties": {}})
                    .as_object()
                    .unwrap()
                    .clone(),
            )),
            _ => None,
        }
    }

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        if request.name.as_ref() == "mrtr" {
            if request.input_responses.is_none() && request.request_state.is_none() {
                return Ok(
                    rmcp::model::InputRequiredResult::from_request_state("entry-round-1").into(),
                );
            }
            return Ok(rmcp::model::CallToolResult::success(vec![
                rmcp::model::ContentBlock::text("entry-mrtr-complete"),
            ])
            .into());
        }
        let mut echoed = request.arguments.clone().unwrap_or_default();
        echoed.insert(
            "name".to_string(),
            serde_json::json!(request.name.as_ref().to_string()),
        );
        Ok(
            rmcp::model::CallToolResult::success(vec![rmcp::model::ContentBlock::text(
                serde_json::to_string(&echoed).unwrap_or_default(),
            )])
            .into(),
        )
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::InitializeResult::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new("entry-upstream", "1.0"))
    }
}

async fn spawn_entry_upstream() -> String {
    let service = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        || Ok(EntryUpstream),
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
            .into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
            .disable_allowed_hosts(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}/mcp")
}

fn test_database_pool() -> Option<sqlx::PgPool> {
    std::env::var("PROMPT_FERRY_TEST_DATABASE_URL")
        .ok()
        .map(|url| PgPoolOptions::new().connect_lazy(&url).unwrap())
}

async fn insert_test_mcp_server(
    pool: &sqlx::PgPool,
    name: &str,
    url: &str,
) -> crate::db::McpServer {
    use crate::db::{McpServerInput, create_mcp_server};
    create_mcp_server(
        pool,
        McpServerInput {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: name.to_string(),
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
        },
    )
    .await
    .expect("insert test mcp server")
}

#[tokio::test]
async fn aggregate_tool_call_rewrites_name_and_regenerates_mcp_headers() {
    let Some(pool) = test_database_pool() else {
        eprintln!(
            "skipping aggregate tools/call DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set"
        );
        return;
    };
    let upstream_url = spawn_entry_upstream().await;
    let server_name = format!("github-{}", uuid::Uuid::new_v4().simple());
    insert_test_mcp_server(&pool, &server_name, &upstream_url).await;
    let cache = McpCatalogCache::new();

    let body = format!(
        r#"{{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{{"name":"{server_name}__list_issues","arguments":{{"text":"hello"}},"_meta":{MCP_V2_META}}}}}"#
    )
    .into_bytes();
    let headers = vec![
        ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
        ("Mcp-Method".to_string(), "tools/call".to_string()),
        (
            "Mcp-Name".to_string(),
            format!("{server_name}__list_issues"),
        ),
    ];
    let (status, _, _, body) = collect_response(
        handle_stream(&pool, &cache, request("POST", "/mcp", &headers, &body))
            .await
            .unwrap(),
    )
    .await;

    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    // The aggregate name was rewritten to the upstream name before the
    // outbound call; the upstream (which validates Mcp-Name and Mcp-Param-*
    // against the body) only accepts the regenerated headers, so a successful
    // result proves the rewrite happened end to end.
    let echoed: Value =
        serde_json::from_str(value["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["name"], "list_issues");
    assert_eq!(echoed["text"], "hello");
}

#[tokio::test]
async fn named_server_mrtr_round_trip_preserves_input_responses_and_request_state() {
    let Some(pool) = test_database_pool() else {
        eprintln!("skipping named-server MRTR DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set");
        return;
    };
    let upstream_url = spawn_entry_upstream().await;
    let server_name = format!("github-{}", uuid::Uuid::new_v4().simple());
    insert_test_mcp_server(&pool, &server_name, &upstream_url).await;
    let cache = McpCatalogCache::new();

    let first_body = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"mrtr","arguments":{{}},"_meta":{MCP_V2_META}}}}}"#
    )
    .into_bytes();
    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            named_request(
                &server_name,
                "POST",
                &format!("/mcp/{server_name}"),
                &[
                    ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
                    ("Mcp-Method".to_string(), "tools/call".to_string()),
                    ("Mcp-Name".to_string(), "mrtr".to_string()),
                ],
                &first_body,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(
        value["result"]["resultType"].as_str(),
        Some("input_required")
    );
    assert_eq!(
        value["result"]["requestState"].as_str(),
        Some("entry-round-1")
    );

    let second_body = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"mrtr","arguments":{{}},"inputResponses":{{"q1":{{"action":"accept","content":{{"x":1}}}}}},"requestState":"entry-round-1","_meta":{MCP_V2_META}}}}}"#
    )
    .into_bytes();
    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            named_request(
                &server_name,
                "POST",
                &format!("/mcp/{server_name}"),
                &[
                    ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
                    ("Mcp-Method".to_string(), "tools/call".to_string()),
                    ("Mcp-Name".to_string(), "mrtr".to_string()),
                ],
                &second_body,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(
        value["result"]["content"][0]["text"].as_str(),
        Some("entry-mrtr-complete")
    );
}

#[tokio::test]
async fn legacy_downstream_session_still_negotiates_2026_upstream() {
    let Some(pool) = test_database_pool() else {
        eprintln!("skipping legacy-downstream DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set");
        return;
    };
    let upstream_url = spawn_entry_upstream().await;
    let server_name = format!("github-{}", uuid::Uuid::new_v4().simple());
    insert_test_mcp_server(&pool, &server_name, &upstream_url).await;
    let cache = McpCatalogCache::new();

    // Legacy 2025-11-25 session: initialize, then tools/call without any
    // SEP-2243 headers or 2026 request meta.
    let (_, _, headers, _) = collect_response(
        handle_stream(
            &pool,
            &cache,
            named_request(
                &server_name,
                "POST",
                &format!("/mcp/{server_name}"),
                &[],
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    let session_id = headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .unwrap();
    let call_body = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_issues","arguments":{"text":"legacy"}}}"#;
    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            named_request(
                &server_name,
                "POST",
                &format!("/mcp/{server_name}"),
                &[
                    ("mcp-session-id".to_string(), session_id),
                    ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
                ],
                call_body,
            ),
        )
        .await
        .unwrap(),
    )
    .await;

    // The upstream connection negotiates 2026-07-28 on its own and regenerates
    // Mcp-Param-X-Echo-Text for the annotated argument; forcing the downstream
    // legacy version onto the upstream would break this call.
    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    let echoed: Value =
        serde_json::from_str(value["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(echoed["text"], "legacy");
}

#[tokio::test]
async fn named_server_resource_templates_are_namespaced_and_reversible() {
    let Some(pool) = test_database_pool() else {
        eprintln!("skipping templates DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set");
        return;
    };
    let upstream_url = spawn_entry_upstream().await;
    let server_name = format!("github-{}", uuid::Uuid::new_v4().simple());
    let server_row = insert_test_mcp_server(&pool, &server_name, &upstream_url).await;
    let cache = McpCatalogCache::new();
    cache
        .put(
            &server_row,
            crate::mcp::service::fetch_server_snapshot(&server_row)
                .await
                .expect("snapshot upstream"),
        )
        .await;

    let body = v2_body("resources/templates/list", 3);
    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            named_request(
                &server_name,
                "POST",
                &format!("/mcp/{server_name}"),
                &v2_headers("resources/templates/list"),
                &body,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    let templates = value["result"]["resourceTemplates"].as_array().unwrap();
    assert_eq!(templates.len(), 1);
    // Direct (named) servers expose the upstream catalog unchanged.
    let raw_template = templates[0]["uriTemplate"].as_str().unwrap();
    assert_eq!(raw_template, "git://{owner}/{repo}/issues");

    // Aggregate path: the same cached snapshot is served through /mcp with a
    // reversible, RFC 6570-preserving namespacing.
    let body = v2_body("resources/templates/list", 4);
    let (status, _, _, body) = collect_response(
        handle_stream(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers("resources/templates/list"),
                &body,
            ),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
    let value = last_sse_json(&body);
    let templates = value["result"]["resourceTemplates"].as_array().unwrap();
    assert_eq!(templates.len(), 1);
    let namespaced = templates[0]["uriTemplate"].as_str().unwrap();
    assert!(
        namespaced.contains("{owner}") && namespaced.contains("{repo}"),
        "RFC 6570 expressions must survive namespacing: {namespaced}"
    );
    let target = crate::mcp::targeting::parse_resource_template_target(namespaced)
        .unwrap()
        .expect("namespaced template must parse");
    assert_eq!(target.server_name, server_name);
    assert_eq!(target.upstream_name, "git://{owner}/{repo}/issues");
}

#[tokio::test]
async fn aggregate_completion_rewrites_prompt_reference() {
    let Some(pool) = test_database_pool() else {
        eprintln!("skipping completion DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set");
        return;
    };
    let upstream_url = spawn_entry_upstream().await;
    let server_name = format!("github-{}", uuid::Uuid::new_v4().simple());
    insert_test_mcp_server(&pool, &server_name, &upstream_url).await;
    let cache = McpCatalogCache::new();

    let body = format!(
        r#"{{"jsonrpc":"2.0","id":4,"method":"completion/complete","params":{{"ref":{{"type":"ref/prompt","name":"{server_name}__my-prompt"}},"argument":{{"name":"q","value":"pr"}},"_meta":{MCP_V2_META}}}}}"#
    )
    .into_bytes();
    let headers = vec![
        ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
        ("Mcp-Method".to_string(), "completion/complete".to_string()),
    ];
    let (status, _, _, body) = collect_response(
        handle_stream(&pool, &cache, request("POST", "/mcp", &headers, &body))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    let values = value["result"]["completion"]["values"].as_array().unwrap();
    assert_eq!(values[0], "my-prompt-suggestion");
    assert_eq!(values[1], "my-prompt-alt");
}

#[tokio::test]
async fn aggregate_completion_rewrites_resource_template_reference() {
    let Some(pool) = test_database_pool() else {
        eprintln!("skipping completion DB test: PROMPT_FERRY_TEST_DATABASE_URL is not set");
        return;
    };
    let upstream_url = spawn_entry_upstream().await;
    let server_name = format!("github-{}", uuid::Uuid::new_v4().simple());
    insert_test_mcp_server(&pool, &server_name, &upstream_url).await;
    let cache = McpCatalogCache::new();

    let body = format!(
        r#"{{"jsonrpc":"2.0","id":5,"method":"completion/complete","params":{{"ref":{{"type":"ref/resource","uri":"mcp://{server_name}/git%3A%2F%2F%7Bowner%7D%2F%7Brepo%7D%2Fissues"}},"argument":{{"name":"owner","value":"oct"}},"_meta":{MCP_V2_META}}}}}"#
    )
    .into_bytes();
    let headers = vec![
        ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
        ("Mcp-Method".to_string(), "completion/complete".to_string()),
    ];
    let (status, _, _, body) = collect_response(
        handle_stream(&pool, &cache, request("POST", "/mcp", &headers, &body))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "response body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    let values = value["result"]["completion"]["values"].as_array().unwrap();
    // The decoded upstream template (with RFC 6570 braces intact) is what the
    // upstream completion handler sees.
    assert_eq!(values[0], "git://{owner}/{repo}/issues-suggestion");
    assert_eq!(values[1], "git://{owner}/{repo}/issues-alt");
}

#[tokio::test]
async fn origin_not_in_allowlist_is_rejected_with_403() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let body = v2_body("tools/list", 6);
    let (status, _, _, body) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request(
                "POST",
                "/mcp",
                &[
                    ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
                    ("Origin".to_string(), "https://evil.example.com".to_string()),
                ],
                &body,
            ),
            None,
            &["https://app.example.com".to_string()],
        )
        .await
        .unwrap(),
    )
    .await;

    assert_eq!(status, 403);
    assert!(
        String::from_utf8_lossy(&body).contains("Origin"),
        "expected an origin error body: {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn origin_in_allowlist_and_missing_origin_are_accepted() {
    let pool = test_pool();
    let cache = McpCatalogCache::new();
    let allowed = vec!["https://app.example.com".to_string()];
    let body = v2_body("server/discover", 7);
    let mut headers = v2_headers("server/discover");
    headers.push(("Origin".to_string(), "https://app.example.com".to_string()));
    let (status, _, _, _) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request("POST", "/mcp", &headers, &body),
            None,
            &allowed,
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(status, 200);

    let (status, _, _, _) = collect_response(
        handle_stream_with_session_store(
            &pool,
            &cache,
            request("POST", "/mcp", &v2_headers("server/discover"), &body),
            None,
            &allowed,
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
}
