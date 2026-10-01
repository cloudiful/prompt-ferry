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
