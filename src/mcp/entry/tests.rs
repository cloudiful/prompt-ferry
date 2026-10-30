use super::*;
use futures::StreamExt;
use rmcp::transport::streamable_http_server::session::{
    SessionState, SessionStore, SessionStoreError,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

#[allow(dead_code)]
mod db_harness {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/db_harness.rs"
    ));
}

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
        selected_credential: None,
    }
}

fn test_pool() -> sqlx::PgPool {
    db_harness::test_db_url::lazy_test_pool()
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

// ---------------------------------------------------------------------------
// Shared session-store test doubles
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

// ---------------------------------------------------------------------------
// Cross-version entry coverage: legacy-shaped upstream, modern/legacy peers
// ---------------------------------------------------------------------------

mod legacy_shape {
    use std::borrow::Cow;

    use rmcp::{
        ErrorData, ServerHandler,
        model::{
            CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult,
            CompleteRequestParams, CompleteResult, CompletionInfo, ContentBlock, ErrorCode,
            GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
            InitializeResult, ListToolsResult, PaginatedRequestParams, PromptMessage,
            ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
            ResourceContents, Role, ServerCapabilities, ServerConfig, Tool,
        },
        service::{RequestContext, RoleServer},
        transport::streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        },
    };
    use serde_json::json;

    /// Upstream that negotiated `2025-11-25`, so rmcp strips the SEP-2322
    /// `resultType` from its ordinary results exactly like the built-in MiniMax
    /// bridge and other pre-`2026-07-28` upstreams.
    #[derive(Clone, Copy)]
    pub(super) struct LegacyShapeUpstream;

    impl ServerHandler for LegacyShapeUpstream {
        fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
            Cow::Borrowed(&[ProtocolVersion::V_2025_11_25])
        }

        fn get_info(&self) -> ServerConfig {
            let capabilities = ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .enable_completions()
                .build();
            InitializeResult::new(capabilities)
                .with_server_info(Implementation::new("legacy-shape-upstream", "1.0"))
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, rmcp::ErrorData> {
            Ok(ListToolsResult::with_all_items(vec![
                Tool::new(
                    "legacy_echo",
                    "echoes a fixed legacy payload",
                    json!({"type": "object"}).as_object().unwrap().clone(),
                ),
                Tool::new(
                    "legacy_structured",
                    "returns structured content with an error flag",
                    json!({"type": "object"}).as_object().unwrap().clone(),
                ),
                Tool::new(
                    "legacy_error",
                    "returns a JSON-RPC error",
                    json!({"type": "object"}).as_object().unwrap().clone(),
                ),
            ]))
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, rmcp::ErrorData> {
            match request.name.as_ref() {
                "legacy_structured" => {
                    let mut result =
                        CallToolResult::success(vec![ContentBlock::text("legacy-structured")]);
                    result.structured_content = Some(json!({"answer": 42}));
                    result.is_error = Some(true);
                    Ok(result.into())
                }
                "legacy_error" => Err(ErrorData::new(
                    ErrorCode(-32001),
                    "legacy tool failed",
                    Some(json!({"detail": "boom"})),
                )),
                _ => Ok(
                    CallToolResult::success(vec![ContentBlock::text("legacy-tool-result")]).into(),
                ),
            }
        }

        async fn read_resource(
            &self,
            _request: ReadResourceRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<ReadResourceResponse, rmcp::ErrorData> {
            let mut result = ReadResourceResult::new(vec![ResourceContents::text(
                "legacy-resource",
                "legacy://resource",
            )]);
            result.ttl_ms = Some(60);
            result.cache_scope = Some(CacheScope::Public);
            Ok(ReadResourceResponse::Complete(result))
        }

        async fn get_prompt(
            &self,
            _request: GetPromptRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<GetPromptResponse, rmcp::ErrorData> {
            Ok(GetPromptResponse::Complete(GetPromptResult::new(vec![
                PromptMessage::new_text(Role::User, "legacy-prompt"),
            ])))
        }

        async fn complete(
            &self,
            _request: CompleteRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CompleteResult, rmcp::ErrorData> {
            let completion = CompletionInfo::with_all_values(vec!["legacy-completion".to_string()])
                .expect("single completion value");
            Ok(CompleteResult::new(completion))
        }
    }

    pub(super) async fn spawn() -> String {
        let service = StreamableHttpService::new(
            || Ok(LegacyShapeUpstream),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default().disable_allowed_hosts(),
        );
        let router = axum::Router::new().nest_service("/mcp", service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}/mcp")
    }
}

async fn sqlite_storage_with_http_server(url: &str) -> (McpRuntimeStorage, McpCatalogCache) {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    use crate::{
        db::{ConfigRepository, McpServerInput},
        relay_secrets::RelaySecretManager,
        standalone_config::StandaloneConfigStore,
    };

    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-entry-cross-version-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let store = std::sync::Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let manager = RelaySecretManager::from_base64(&STANDARD.encode([9_u8; 32])).expect("manager");
    let repository = ConfigRepository::sqlite(store, manager);
    repository
        .create_mcp_server(
            uuid::Uuid::new_v4(),
            McpServerInput {
                scope: "admin".to_string(),
                owner_user_id: None,
                source_endpoint_id: None,
                name: "legacy-shape".to_string(),
                aggregate_naming_mode: "qualified_only".to_string(),
                transport: "http".to_string(),
                provider_kind: None,
                url: Some(url.to_string()),
                command: None,
                args: serde_json::json!([]),
                env_json: serde_json::json!({}),
                bearer_tokens_json: serde_json::json!([]),
                http_headers_json: serde_json::json!({}),
                auth_mode: "none".to_string(),
                basic_username: None,
                basic_password: None,
                proxy_url: None,
                tool_filter_mode: "blacklist".to_string(),
                allowed_tools: serde_json::json!([]),
                disabled_tools: serde_json::json!([]),
                disabled_resources: serde_json::json!([]),
                enabled: true,
                timeout_ms: 30_000,
                lifecycle_policy: "auto".to_string(),
                lifecycle_manual_protocol_version: None,
            },
        )
        .await
        .expect("create MCP server");
    (
        McpRuntimeStorage::from_repository(repository),
        McpCatalogCache::new(),
    )
}

fn v2_request_body(method: &str, id: i64, mut params: Value) -> Vec<u8> {
    params["_meta"] = serde_json::from_str(MCP_V2_META).expect("protocol meta");
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))
    .expect("request body")
}

fn legacy_request_body(method: &str, id: i64, params: Value) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))
    .expect("request body")
}

fn v2_headers_with(method: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut headers = v2_headers(method);
    for (name, value) in extra {
        headers.push((name.to_string(), value.to_string()));
    }
    headers
}

fn named_request<'a>(
    method: &'a str,
    path: &'a str,
    server_name: &'a str,
    headers: &'a [(String, String)],
    body: &'a [u8],
) -> McpRequestContext<'a> {
    McpRequestContext {
        user_id: None,
        server_name: Some(server_name),
        method,
        path,
        headers,
        body,
        selected_credential: None,
    }
}

async fn initialize_legacy_session(
    storage: &McpRuntimeStorage,
    cache: &McpCatalogCache,
    server_name: Option<&str>,
) -> String {
    let (status, _, headers, _) = collect_response(
        handle_stream_with_storage(
            storage,
            cache,
            McpRequestContext {
                user_id: None,
                server_name,
                method: "POST",
                path: "/mcp",
                headers: &[],
                body: br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                selected_credential: None,
            },
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(status, 200);
    headers
        .iter()
        .find(|(name, _)| name == "mcp-session-id")
        .map(|(_, value)| value.clone())
        .expect("legacy initialize must establish a session")
}

#[tokio::test]
async fn entry_2026_peer_receives_complete_result_type_from_legacy_shape_upstream() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &v2_headers_with("tools/call", &[("mcp-name", "legacy_echo")]),
                &v2_request_body(
                    "tools/call",
                    2,
                    serde_json::json!({"name": "legacy_echo", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert_eq!(
        value["result"]["content"][0]["text"].as_str(),
        Some("legacy-tool-result")
    );

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &v2_headers_with("resources/read", &[("mcp-name", "legacy://resource")]),
                &v2_request_body(
                    "resources/read",
                    3,
                    serde_json::json!({"uri": "legacy://resource"}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert_eq!(value["result"]["ttlMs"], serde_json::json!(60));
    assert_eq!(value["result"]["cacheScope"].as_str(), Some("public"));

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &v2_headers_with("prompts/get", &[("mcp-name", "legacy_prompt")]),
                &v2_request_body(
                    "prompts/get",
                    4,
                    serde_json::json!({"name": "legacy_prompt", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &v2_headers("completion/complete"),
                &v2_request_body(
                    "completion/complete",
                    5,
                    serde_json::json!({
                        "ref": {"type": "ref/prompt", "name": "legacy_prompt"},
                        "argument": {"name": "arg", "value": "leg"},
                    }),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert_eq!(
        value["result"]["completion"]["values"][0].as_str(),
        Some("legacy-completion")
    );
}

#[tokio::test]
async fn entry_aggregate_2026_peer_receives_complete_result_type() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers_with("tools/call", &[("mcp-name", "legacy-shape__legacy_echo")]),
                &v2_request_body(
                    "tools/call",
                    6,
                    serde_json::json!({"name": "legacy-shape__legacy_echo", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert_eq!(
        value["result"]["content"][0]["text"].as_str(),
        Some("legacy-tool-result")
    );
}

#[tokio::test]
async fn entry_2025_peer_receives_no_modern_result_type_or_cache_metadata() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;
    let session_id = initialize_legacy_session(&storage, &cache, Some("legacy-shape")).await;
    let headers = vec![
        ("mcp-session-id".to_string(), session_id),
        ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
    ];

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &headers,
                &legacy_request_body(
                    "tools/call",
                    2,
                    serde_json::json!({"name": "legacy_echo", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert!(
        value["result"].get("resultType").is_none(),
        "legacy peer must not receive resultType: {value}"
    );

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &headers,
                &legacy_request_body(
                    "resources/read",
                    3,
                    serde_json::json!({"uri": "legacy://resource"}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert!(
        value["result"].get("resultType").is_none(),
        "legacy peer must not receive resultType: {value}"
    );
    assert!(
        value["result"].get("ttlMs").is_none(),
        "legacy peer must not receive modern cache metadata: {value}"
    );
    assert!(
        value["result"].get("cacheScope").is_none(),
        "legacy peer must not receive modern cache metadata: {value}"
    );
    assert_eq!(
        value["result"]["contents"][0]["text"].as_str(),
        Some("legacy-resource")
    );

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &headers,
                &legacy_request_body(
                    "prompts/get",
                    4,
                    serde_json::json!({"name": "legacy_prompt", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert!(
        value["result"].get("resultType").is_none(),
        "legacy peer must not receive resultType: {value}"
    );
    assert_eq!(
        value["result"]["messages"][0]["content"]["text"].as_str(),
        Some("legacy-prompt")
    );

    let (status, _, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &headers,
                &legacy_request_body(
                    "completion/complete",
                    5,
                    serde_json::json!({
                        "ref": {"type": "ref/prompt", "name": "legacy_prompt"},
                        "argument": {"name": "arg", "value": "leg"},
                    }),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = last_sse_json(&body);
    assert!(
        value["result"].get("resultType").is_none(),
        "legacy peer must not receive resultType: {value}"
    );
    assert_eq!(
        value["result"]["completion"]["values"][0].as_str(),
        Some("legacy-completion")
    );
}

fn entry_response_json(content_type: &str, body: &[u8]) -> Value {
    if content_type.starts_with("text/event-stream") {
        last_sse_json(body)
    } else {
        serde_json::from_slice(body).expect("JSON response body")
    }
}

#[tokio::test]
async fn entry_2026_peer_preserves_structured_content_and_is_error() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &v2_headers_with("tools/call", &[("mcp-name", "legacy_structured")]),
                &v2_request_body(
                    "tools/call",
                    20,
                    serde_json::json!({"name": "legacy_structured", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert_eq!(value["id"], serde_json::json!(20));
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert_eq!(
        value["result"]["content"][0]["text"].as_str(),
        Some("legacy-structured")
    );
    assert_eq!(
        value["result"]["structuredContent"],
        serde_json::json!({"answer": 42})
    );
    assert_eq!(value["result"]["isError"], serde_json::json!(true));
}

#[tokio::test]
async fn entry_2026_peer_must_preserve_upstream_jsonrpc_error_object() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &v2_headers_with("tools/call", &[("mcp-name", "legacy_error")]),
                &v2_request_body(
                    "tools/call",
                    21,
                    serde_json::json!({"name": "legacy_error", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    let value = entry_response_json(&content_type, &body);
    assert_eq!(
        value["id"],
        serde_json::json!(21),
        "actual downstream response: {value}"
    );
    assert_eq!(
        value["error"]["code"],
        serde_json::json!(-32001),
        "actual downstream response: {value}"
    );
    assert_eq!(
        value["error"]["message"].as_str(),
        Some("legacy tool failed")
    );
    assert_eq!(
        value["error"]["data"],
        serde_json::json!({"detail": "boom"})
    );
    assert!(
        value.get("result").is_none(),
        "error must not carry a result"
    );
    assert_eq!(status, 200);
}

#[tokio::test]
async fn entry_aggregate_2026_peer_gets_complete_results_for_all_methods() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;

    let resource_uri = "mcp://legacy-shape/legacy-resource";
    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers_with("resources/read", &[("mcp-name", resource_uri)]),
                &v2_request_body(
                    "resources/read",
                    30,
                    serde_json::json!({"uri": resource_uri}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
    assert_eq!(value["result"]["ttlMs"], serde_json::json!(60));
    assert_eq!(value["result"]["cacheScope"].as_str(), Some("public"));

    let prompt_name = "legacy-shape__legacy_prompt";
    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers_with("prompts/get", &[("mcp-name", prompt_name)]),
                &v2_request_body(
                    "prompts/get",
                    31,
                    serde_json::json!({"name": prompt_name, "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers("completion/complete"),
                &v2_request_body(
                    "completion/complete",
                    32,
                    serde_json::json!({
                        "ref": {"type": "ref/prompt", "name": prompt_name},
                        "argument": {"name": "arg", "value": "leg"},
                    }),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert_eq!(value["result"]["resultType"].as_str(), Some("complete"));
}

#[tokio::test]
async fn entry_aggregate_2025_peer_gets_degraded_results_for_all_methods() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;
    let session_id = initialize_legacy_session(&storage, &cache, None).await;
    let headers = vec![
        ("mcp-session-id".to_string(), session_id),
        ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
    ];

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &headers,
                &legacy_request_body(
                    "resources/read",
                    40,
                    serde_json::json!({"uri": "mcp://legacy-shape/legacy-resource"}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert!(value["result"].get("resultType").is_none());
    assert!(value["result"].get("ttlMs").is_none());
    assert!(value["result"].get("cacheScope").is_none());

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &headers,
                &legacy_request_body(
                    "prompts/get",
                    41,
                    serde_json::json!({
                        "name": "legacy-shape__legacy_prompt",
                        "arguments": {},
                    }),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert!(value["result"].get("resultType").is_none());

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &headers,
                &legacy_request_body(
                    "completion/complete",
                    42,
                    serde_json::json!({
                        "ref": {"type": "ref/prompt", "name": "legacy-shape__legacy_prompt"},
                        "argument": {"name": "arg", "value": "leg"},
                    }),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    let value = entry_response_json(&content_type, &body);
    assert!(value["result"].get("resultType").is_none());
}

fn assert_preserved_upstream_error(value: &Value, id: i64) {
    assert_eq!(
        value["id"],
        serde_json::json!(id),
        "actual downstream response: {value}"
    );
    assert_eq!(
        value["error"]["code"],
        serde_json::json!(-32001),
        "actual downstream response: {value}"
    );
    assert_eq!(
        value["error"]["message"].as_str(),
        Some("legacy tool failed"),
        "actual downstream response: {value}"
    );
    assert_eq!(
        value["error"]["data"],
        serde_json::json!({"detail": "boom"}),
        "actual downstream response: {value}"
    );
    assert!(
        value.get("result").is_none(),
        "error must not carry a result: {value}"
    );
}

#[tokio::test]
async fn entry_2025_named_peer_preserves_upstream_jsonrpc_error_object() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;
    let session_id = initialize_legacy_session(&storage, &cache, Some("legacy-shape")).await;
    let headers = vec![
        ("mcp-session-id".to_string(), session_id),
        ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
    ];

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            named_request(
                "POST",
                "/mcp",
                "legacy-shape",
                &headers,
                &legacy_request_body(
                    "tools/call",
                    22,
                    serde_json::json!({"name": "legacy_error", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    assert_preserved_upstream_error(&entry_response_json(&content_type, &body), 22);
}

#[tokio::test]
async fn entry_2026_aggregate_peer_preserves_upstream_jsonrpc_error_object() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &v2_headers_with("tools/call", &[("mcp-name", "legacy-shape__legacy_error")]),
                &v2_request_body(
                    "tools/call",
                    23,
                    serde_json::json!({"name": "legacy-shape__legacy_error", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    assert_preserved_upstream_error(&entry_response_json(&content_type, &body), 23);
}

#[tokio::test]
async fn entry_2025_aggregate_peer_preserves_upstream_jsonrpc_error_object() {
    let url = legacy_shape::spawn().await;
    let (storage, cache) = sqlite_storage_with_http_server(&url).await;
    let session_id = initialize_legacy_session(&storage, &cache, None).await;
    let headers = vec![
        ("mcp-session-id".to_string(), session_id),
        ("mcp-protocol-version".to_string(), "2025-11-25".to_string()),
    ];

    let (status, content_type, _, body) = collect_response(
        handle_stream_with_storage(
            &storage,
            &cache,
            request(
                "POST",
                "/mcp",
                &headers,
                &legacy_request_body(
                    "tools/call",
                    24,
                    serde_json::json!({"name": "legacy-shape__legacy_error", "arguments": {}}),
                ),
            ),
            None,
            &[],
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(
        status,
        200,
        "unexpected body: {}",
        String::from_utf8_lossy(&body)
    );
    assert_preserved_upstream_error(&entry_response_json(&content_type, &body), 24);
}
