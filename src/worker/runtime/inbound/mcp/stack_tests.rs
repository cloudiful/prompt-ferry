use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;
use crate::{
    config::WorkerConfig,
    db::{ConfigRepository, McpServerInput},
    mcp::{McpRuntimeState, McpRuntimeStorage},
    protocol::BridgeMessage,
    relay_secrets::RelaySecretManager,
    standalone_config::{StandaloneConfig, StandaloneConfigStore},
    worker::runtime::{
        WorkerRuntimeState,
        context::{BridgeSender, ResponseLimits, RuntimeServices},
        request_assembly::BufferedMcpRequest,
        standalone::StandaloneRuntimeState,
    },
};

/// Hard bound on the inlined MCP handler futures. When exceeded, split the
/// handler further instead of raising the thread stack.
const FUTURE_SIZE_GUARD_BYTES: usize = 64 * 1024;
/// Stack regression: the handler must survive a small worker stack that a
/// multi-hundred-KB inlined future would overflow.
const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;

fn minimal_request() -> BufferedMcpRequest {
    BufferedMcpRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        server_name: None,
        method: "POST".to_string(),
        path: "/mcp".to_string(),
        headers: Vec::new(),
        body: br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.to_vec(),
        user_id: Some(1),
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    }
}

fn test_services(out_tx: BridgeSender) -> RuntimeServices {
    RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
}

fn database_path() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("prompt-ferry-mcp-preparation-{suffix}.sqlite"))
}

#[test]
fn handler_and_execution_futures_stay_bounded() {
    let (out_tx, _control_rx, _data_rx) = BridgeSender::channel();
    let services = test_services(out_tx);

    let handler = handle_mcp_request(minimal_request(), &services);
    assert!(
        std::mem::size_of_val(&handler) <= FUTURE_SIZE_GUARD_BYTES,
        "MCP handler future is {} bytes, exceeding the {} byte guard",
        std::mem::size_of_val(&handler),
        FUTURE_SIZE_GUARD_BYTES,
    );
    drop(handler);

    let execution = execution::execute_mcp_request(minimal_request(), &services);
    assert!(
        std::mem::size_of_val(&execution) <= FUTURE_SIZE_GUARD_BYTES,
        "MCP execution future is {} bytes, exceeding the {} byte guard",
        std::mem::size_of_val(&execution),
        FUTURE_SIZE_GUARD_BYTES,
    );
    drop(execution);
}

#[test]
fn mcp_handler_completes_on_bounded_worker_stack() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_stack_size(WORKER_STACK_BYTES)
        .enable_all()
        .build()
        .expect("failed to build single-worker test runtime");
    runtime.block_on(async {
        let (out_tx, _control_rx, mut data_rx) = BridgeSender::channel();
        let services = test_services(out_tx);
        let request = minimal_request();
        let request_id = request.request_id.clone();
        runtime
            .spawn(async move {
                handle_mcp_request(request, &services).await;
            })
            .await
            .expect("MCP handler task must complete without aborting");
        let mut saw_start = false;
        while let Ok(data) = data_rx.try_recv() {
            if let BridgeMessage::McpResponseStart(start) = data.message {
                saw_start = true;
                assert_eq!(start.request_id, request_id);
                assert_eq!(
                    start.status,
                    reqwest::StatusCode::SERVICE_UNAVAILABLE.as_u16()
                );
            }
        }
        assert!(saw_start, "handler must emit an MCP response start");
    });
}

fn mcp_input(name: &str) -> McpServerInput {
    McpServerInput {
        scope: "admin".to_string(),
        owner_user_id: None,
        source_endpoint_id: None,
        name: name.to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "stdio".to_string(),
        provider_kind: None,
        url: None,
        command: Some("mcpd".to_string()),
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
    }
}

async fn sqlite_services(
    name: &str,
) -> (
    RuntimeServices,
    StandaloneRuntimeState,
    PathBuf,
    tokio::sync::mpsc::Receiver<super::super::super::bridge::BridgeData>,
) {
    let _redaction_guard = crate::redact_test_support::lock_async().await;
    let path = database_path();
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let manager = RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
    let repository = ConfigRepository::sqlite(store.clone(), manager.clone());
    repository
        .create_mcp_server(uuid::Uuid::new_v4(), mcp_input(name))
        .await
        .expect("create SQLite MCP server");
    let config = WorkerConfig {
        standalone_database_path: path.to_string_lossy().to_string(),
        ..WorkerConfig::default()
    };
    let catalog_cache =
        crate::mcp::McpCatalogCache::from_config_with_sqlite(&config, Some(store.pool().clone()))
            .await;
    let mcp_runtime = McpRuntimeState::sqlite(
        &config,
        McpRuntimeStorage::from_repository(repository),
        store.pool().clone(),
        catalog_cache,
    )
    .await;
    let standalone = StandaloneRuntimeState::new(store, manager, StandaloneConfig::default())
        .with_mcp_runtime(mcp_runtime);
    let (out_tx, _control_rx, _data_rx) = BridgeSender::channel();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
    .with_standalone_state(standalone.clone());
    (services, standalone, path, _data_rx)
}

#[tokio::test]
async fn sqlite_mcp_request_uses_unified_server_configuration() {
    let (services, standalone, path, mut _data_rx) = sqlite_services("local").await;
    let request = minimal_request();
    let mut request = request;
    request.server_name = Some("local".to_string());
    let request_id = uuid::Uuid::parse_str(&request.request_id).expect("request ID");

    let execution = preparation::build_request_context(request, &services)
        .await
        .expect("SQLite MCP request should pass MCP admission");
    let execution = preparation::resolve_server_and_quota(execution, &services)
        .await
        .expect("SQLite MCP server should resolve through ConfigRepository");
    assert_eq!(
        execution.server.as_ref().map(|server| server.name.as_str()),
        Some("local")
    );
    assert!(
        standalone
            .recent_usage()
            .iter()
            .all(|summary| summary.error_code.is_none())
    );
    assert_eq!(request_id, execution.request_ctx.request_id);

    let mut aggregate_request = minimal_request();
    aggregate_request.body =
        br#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{}}"#.to_vec();
    handle_mcp_request(aggregate_request, &services).await;
    let summary = standalone
        .recent_usage()
        .into_iter()
        .next_back()
        .expect("aggregate request summary");
    assert_eq!(summary.state, "completed", "{summary:?}");
    assert!(summary.error_code.is_none(), "{summary:?}");

    drop(execution);
    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// Issue #1179 P2: the persisted record of a builtin MiniMax tool call.
//
// The builtin provider runs inside the rmcp session task, so the record can
// only carry the selected key and the actual outcome when the request
// tracker crosses that boundary.
// ---------------------------------------------------------------------------

/// The builtin transport stub: it answers the MiniMax API in process.
struct StubBuiltinTransport {
    /// How many attempts report an exhausted quota before one succeeds.
    failures_before_success: usize,
    /// How long every attempt takes, so a tool outcome can happen after the
    /// response headers.
    delay: std::time::Duration,
    /// The bearer secret of every attempt, in order.
    attempts: std::sync::Mutex<Vec<String>>,
    /// Signalled when an attempt starts.
    started: tokio::sync::Notify,
    /// When set, every attempt fails with a transport exception that repeats
    /// the credential and the query, the way a real connection error can.
    transport_error: std::sync::atomic::AtomicBool,
}

impl StubBuiltinTransport {
    fn attempts(&self) -> Vec<String> {
        self.attempts
            .lock()
            .expect("the attempt log is not poisoned")
            .clone()
    }

    fn fail_with_transport_error(&self) {
        self.transport_error
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl crate::mcp::MiniMaxTransport for StubBuiltinTransport {
    async fn send(
        &self,
        request: crate::mcp::MiniMaxRequest<'_>,
    ) -> anyhow::Result<crate::mcp::MiniMaxResponse> {
        let attempt = {
            let mut attempts = self
                .attempts
                .lock()
                .expect("the attempt log is not poisoned");
            attempts.push(request.secret.to_string());
            attempts.len()
        };
        self.started.notify_waiters();
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if self
            .transport_error
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(anyhow::anyhow!(
                "connection reset for {} while searching \"rust\"",
                request.secret
            ));
        }
        let exhausted = attempt <= self.failures_before_success;
        Ok(crate::mcp::MiniMaxResponse {
            status: reqwest::StatusCode::OK,
            payload: if exhausted {
                serde_json::json!({
                    "base_resp": {
                        "status_code": 2056,
                        // The provider echoes every configured credential —
                        // including the disabled row and the legacy inline
                        // value — and the query.
                        "status_msg": "keys sk-first sk-second sk-inline sk-disabled for query rust are spent"
                    }
                })
            } else {
                serde_json::json!({ "base_resp": { "status_code": 0 }, "organic": [] })
            },
            retry_after: None,
        })
    }
}

/// The endpoint key ids and secrets of the seeded MiniMax endpoint.
struct MinimaxKeys {
    first: (uuid::Uuid, String),
    second: (uuid::Uuid, String),
}

impl MinimaxKeys {
    fn key_id_of(&self, secret: &str) -> uuid::Uuid {
        if secret == self.first.1 {
            self.first.0
        } else {
            self.second.0
        }
    }

    fn label_of(&self, secret: &str) -> &'static str {
        if secret == self.first.1 {
            "first"
        } else {
            "second"
        }
    }
}

/// The standalone services with a MiniMax endpoint, its builtin MCP server
/// and the stub transport that answers the MiniMax API in process.
async fn sqlite_services_with_builtin_minimax_delayed(
    failures_before_success: usize,
    delay: std::time::Duration,
) -> (
    RuntimeServices,
    StandaloneRuntimeState,
    PathBuf,
    MinimaxKeys,
    Arc<StubBuiltinTransport>,
    tokio::sync::mpsc::Receiver<super::super::super::bridge::BridgeData>,
) {
    let _redaction_guard = crate::redact_test_support::lock_async().await;
    let path = database_path();
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let manager = RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
    let endpoint_id = uuid::Uuid::new_v4();
    let first_key = uuid::Uuid::new_v4();
    let second_key = uuid::Uuid::new_v4();
    let disabled_key = uuid::Uuid::new_v4();
    let now = chrono::Utc::now();
    let endpoint = crate::standalone_config::ProviderEndpointConfig {
        endpoint_id,
        name: "minimax".to_string(),
        provider: crate::standalone_config::EndpointProvider::Minimax,
        provider_region: Some(crate::standalone_config::EndpointRegion::Global),
        service_tier: None,
        base_url: "https://api.minimax.io".to_string(),
        native_api: crate::config::NativeApi::Responses,
        native_api_source: crate::config::NativeApiSource::Manual,
        key_lb_enabled: true,
        enabled: true,
        mcp_enabled: true,
        created_at: now,
        updated_at: now,
        api_key: "sk-inline".to_string(),
        api_keys: vec![
            crate::standalone_config::EndpointApiKeyConfig {
                key_id: first_key,
                endpoint_id,
                key_label: "first".to_string(),
                api_key: "sk-first".to_string(),
                position: 0,
                enabled: true,
                created_at: now,
                updated_at: now,
            },
            crate::standalone_config::EndpointApiKeyConfig {
                key_id: second_key,
                endpoint_id,
                key_label: "second".to_string(),
                api_key: "sk-second".to_string(),
                position: 1,
                enabled: true,
                created_at: now,
                updated_at: now,
            },
            crate::standalone_config::EndpointApiKeyConfig {
                key_id: disabled_key,
                endpoint_id,
                key_label: "disabled".to_string(),
                api_key: "sk-disabled".to_string(),
                position: 2,
                enabled: false,
                created_at: now,
                updated_at: now,
            },
        ],
        proxy_url: None,
        active_windows: None,
    };
    store
        .save_endpoint(&manager, &endpoint)
        .await
        .expect("save the MiniMax endpoint");
    let repository = ConfigRepository::sqlite(store.clone(), manager.clone());
    repository
        .create_mcp_server(
            uuid::Uuid::new_v4(),
            McpServerInput {
                scope: "admin".to_string(),
                owner_user_id: None,
                source_endpoint_id: Some(endpoint_id),
                name: "minimax-mcp".to_string(),
                aggregate_naming_mode: "qualified_only".to_string(),
                transport: "builtin_minimax".to_string(),
                provider_kind: None,
                url: None,
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
        .expect("create the builtin MiniMax MCP server");
    let transport = Arc::new(StubBuiltinTransport {
        failures_before_success,
        delay,
        attempts: std::sync::Mutex::new(Vec::new()),
        started: tokio::sync::Notify::new(),
        transport_error: std::sync::atomic::AtomicBool::new(false),
    });
    let config = WorkerConfig {
        standalone_database_path: path.to_string_lossy().to_string(),
        ..WorkerConfig::default()
    };
    let catalog_cache =
        crate::mcp::McpCatalogCache::from_config_with_sqlite(&config, Some(store.pool().clone()))
            .await;
    let storage =
        McpRuntimeStorage::from_repository(repository).with_builtin_transport(transport.clone());
    let mcp_runtime =
        McpRuntimeState::sqlite(&config, storage, store.pool().clone(), catalog_cache).await;
    let standalone = StandaloneRuntimeState::new(store, manager, StandaloneConfig::default())
        .with_mcp_runtime(mcp_runtime);
    let (out_tx, _control_rx, data_rx) = BridgeSender::channel();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
    .with_standalone_state(standalone.clone());
    (
        services,
        standalone,
        path,
        MinimaxKeys {
            first: (first_key, "sk-first".to_string()),
            second: (second_key, "sk-second".to_string()),
        },
        transport,
        data_rx,
    )
}

fn minimax_search_request() -> BufferedMcpRequest {
    let mut request = minimal_request();
    request.server_name = Some("minimax-mcp".to_string());
    request.headers = vec![
        ("Mcp-Protocol-Version".to_string(), "2026-07-28".to_string()),
        ("Mcp-Method".to_string(), "tools/call".to_string()),
        ("Mcp-Name".to_string(), "web_search".to_string()),
    ];
    request.body = br#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"web_search","arguments":{"query":"rust"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"stack-test","version":"1.0"}}}}"#.to_vec();
    request
}

// ---------------------------------------------------------------------------
// Issue #1179 P2 repair: the evidence is the durable SQLite ledger and the
// bytes the client actually receives, never the in-memory cache.
// ---------------------------------------------------------------------------

/// The persisted summary of one request, read back from a reopened store.
///
/// Reopening proves the row reached SQLite instead of only the bounded
/// in-memory cache, and the request id selects exactly this request.
async fn durable_summary(
    path: &PathBuf,
    request_id: &str,
) -> crate::standalone_config::StandaloneUsageSummaryRecord {
    let wanted = uuid::Uuid::parse_str(request_id).expect("the request id");
    let store = StandaloneConfigStore::open(path)
        .await
        .expect("reopen the durable store");
    store
        .list_usage_summaries(1024)
        .await
        .expect("read the durable usage ledger")
        .into_iter()
        // The request family writes its admission row first and the settlement
        // row last, so the newest row of the request is the settled one.
        .rfind(|record| record.request_id == wanted && record.event_kind == "request")
        .expect("the request is recorded in the durable usage ledger")
}

/// The response body the worker sent to the client.
fn client_body(
    data_rx: &mut tokio::sync::mpsc::Receiver<super::super::super::bridge::BridgeData>,
) -> String {
    let mut body = Vec::new();
    while let Ok(data) = data_rx.try_recv() {
        match data.message {
            BridgeMessage::McpResponseChunk(chunk) => body.extend_from_slice(&chunk.data),
            BridgeMessage::McpResponseEnd(_) => break,
            _ => {}
        }
    }
    String::from_utf8_lossy(&body).to_string()
}

#[tokio::test]
async fn a_builtin_minimax_quota_rejection_is_recorded_as_a_failed_request() {
    let (services, _standalone, path, keys, transport, _data_rx) =
        sqlite_services_with_builtin_minimax_delayed(usize::MAX, std::time::Duration::ZERO).await;
    let request = minimax_search_request();
    let request_id = request.request_id.clone();

    handle_mcp_request(request, &services).await;

    let attempts = transport
        .attempts
        .lock()
        .expect("the attempt log is not poisoned")
        .clone();
    assert!(!attempts.is_empty(), "the builtin called the upstream");
    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "failed", "{summary:?}");
    assert_eq!(summary.ok, Some(false), "{summary:?}");
    assert!(summary.error_code.is_some(), "{summary:?}");
    // The provider echoed the credential and the query in its status message;
    // the persisted record keeps only the fixed diagnostic.
    let message = summary.error_message.clone().unwrap_or_default();
    assert_eq!(
        message, "the provider reported an exhausted plan quota (2056)",
        "{summary:?}"
    );
    assert!(!message.contains("sk-first"), "{message}");
    assert!(!message.contains("rust"), "{message}");
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.key_id_of(attempts.last().expect("an attempt"))),
        "the record must carry the last attempted key: {summary:?}"
    );
    assert!(summary.endpoint_key_label.is_some(), "{summary:?}");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_builtin_minimax_failover_is_recorded_as_a_successful_request() {
    let (services, _standalone, path, keys, transport, _data_rx) =
        sqlite_services_with_builtin_minimax_delayed(1, std::time::Duration::ZERO).await;
    let request = minimax_search_request();
    let request_id = request.request_id.clone();

    handle_mcp_request(request, &services).await;

    let attempts = transport
        .attempts
        .lock()
        .expect("the attempt log is not poisoned")
        .clone();
    assert_eq!(attempts.len(), 2, "the call switched to another key");
    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "completed", "{summary:?}");
    assert_eq!(summary.ok, Some(true), "{summary:?}");
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.key_id_of(attempts.last().expect("an attempt"))),
        "the record must carry the final key: {summary:?}"
    );

    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// Issue #1179 P2 repair: the settlement happens after the response stream.
//
// rmcp answers a streaming MCP request with the response headers first and
// delivers the tool outcome as a later event. The record must read the
// request tracker at that final settlement, never at the response headers.
// ---------------------------------------------------------------------------

/// The search request, negotiated over server-sent events.
fn minimax_search_request_over_sse() -> BufferedMcpRequest {
    minimax_search_request()
}

/// Drive one request to settlement and return the content type the worker
/// reported for the response.
async fn drive_to_settlement(
    request: BufferedMcpRequest,
    services: RuntimeServices,
    transport: &Arc<StubBuiltinTransport>,
    data_rx: &mut tokio::sync::mpsc::Receiver<super::super::super::bridge::BridgeData>,
) -> Option<String> {
    let task = tokio::spawn(async move {
        handle_mcp_request(request, &services).await;
    });
    if tokio::time::timeout(
        std::time::Duration::from_secs(10),
        transport.started.notified(),
    )
    .await
    .is_err()
    {
        panic!("the stub transport never ran; the request ended early");
    }

    let mut content_type = None;
    while let Some(data) = data_rx.recv().await {
        if let BridgeMessage::McpResponseStart(start) = data.message {
            content_type = start.content_type;
            break;
        }
    }
    while let Some(data) = data_rx.recv().await {
        if let BridgeMessage::McpResponseEnd(_) = data.message {
            break;
        }
    }
    task.await.expect("the MCP request task completes");
    content_type
}

#[tokio::test]
async fn a_delayed_sse_tool_failure_is_recorded_with_the_selected_key() {
    let (services, _standalone, path, keys, transport, mut data_rx) =
        sqlite_services_with_builtin_minimax_delayed(
            usize::MAX,
            std::time::Duration::from_millis(200),
        )
        .await;
    let request = minimax_search_request_over_sse();
    let request_id = request.request_id.clone();

    let content_type = drive_to_settlement(request, services, &transport, &mut data_rx).await;
    assert_eq!(
        content_type.as_deref(),
        Some("text/event-stream"),
        "the delayed tool answers over the real SSE path"
    );

    let attempts = transport.attempts();
    let last = attempts.last().expect("an attempt");
    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "failed", "{summary:?}");
    assert_eq!(summary.ok, Some(false), "{summary:?}");
    assert_eq!(
        summary.error_code.as_deref(),
        Some("mcp_quota_exhausted"),
        "{summary:?}"
    );
    assert_eq!(
        summary.error_message.as_deref(),
        Some("the provider reported an exhausted plan quota (2056)"),
        "the durable row keeps the fixed message: {summary:?}"
    );
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.key_id_of(last)),
        "the record must carry the key of the delayed attempt: {summary:?}"
    );
    assert_eq!(
        summary.endpoint_key_label.as_deref(),
        Some(keys.label_of(last)),
        "the record must carry the label of the delayed attempt: {summary:?}"
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_delayed_sse_failover_is_recorded_with_the_final_key() {
    let (services, _standalone, path, keys, transport, mut data_rx) =
        sqlite_services_with_builtin_minimax_delayed(1, std::time::Duration::from_millis(150))
            .await;
    let request = minimax_search_request_over_sse();
    let request_id = request.request_id.clone();

    let content_type = drive_to_settlement(request, services, &transport, &mut data_rx).await;
    assert_eq!(
        content_type.as_deref(),
        Some("text/event-stream"),
        "the delayed tool answers over the real SSE path"
    );

    let attempts = transport.attempts();
    assert_eq!(attempts.len(), 2, "the call switched to another key");
    let last = attempts.last().expect("an attempt");
    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "completed", "{summary:?}");
    assert_eq!(summary.ok, Some(true), "{summary:?}");
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.key_id_of(last)),
        "the record must carry the final key of the delayed failover: {summary:?}"
    );
    assert_eq!(
        summary.endpoint_key_label.as_deref(),
        Some(keys.label_of(last)),
        "the record must carry the final label: {summary:?}"
    );
    assert_eq!(
        summary.error_code, None,
        "a settled success keeps no stale error code: {summary:?}"
    );
    assert_eq!(
        summary.error_message, None,
        "a settled success keeps no stale error message: {summary:?}"
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn the_client_protocol_error_never_repeats_a_configured_credential_or_the_query() {
    // The provider echoes every configured credential — the enabled keys, the
    // disabled row and the legacy inline value — and the query. The client
    // receives a fixed diagnostic instead, and the record keeps only that.
    let (services, _standalone, path, keys, transport, mut data_rx) =
        sqlite_services_with_builtin_minimax_delayed(usize::MAX, std::time::Duration::ZERO).await;
    let request = minimax_search_request();
    let request_id = request.request_id.clone();

    handle_mcp_request(request, &services).await;

    let body = client_body(&mut data_rx);
    assert!(!body.is_empty(), "the client receives a protocol body");
    for leaked in ["sk-first", "sk-second", "sk-inline", "sk-disabled", "rust"] {
        assert!(
            !body.contains(leaked),
            "the client protocol body repeats `{leaked}`: {body}"
        );
    }
    assert!(
        body.contains("2056"),
        "the known provider code stays visible: {body}"
    );

    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "failed", "{summary:?}");
    let message = summary.error_message.clone().unwrap_or_default();
    assert_eq!(
        message, "the provider reported an exhausted plan quota (2056)",
        "{summary:?}"
    );
    for leaked in ["sk-first", "sk-second", "sk-inline", "sk-disabled", "rust"] {
        assert!(!message.contains(leaked), "{message}");
    }
    let attempts = transport.attempts();
    assert_eq!(
        attempts.len(),
        2,
        "each enabled key is tried once and the disabled row never is"
    );
    assert!(
        attempts.iter().all(|secret| secret != "sk-disabled"),
        "a disabled configured key is never selected: {attempts:?}"
    );
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.key_id_of(attempts.last().expect("an attempt"))),
        "the record still names the tried key: {summary:?}"
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_transport_exception_never_reaches_the_client_protocol_error() {
    let (services, _standalone, path, _keys, transport, mut data_rx) =
        sqlite_services_with_builtin_minimax_delayed(0, std::time::Duration::ZERO).await;
    transport.fail_with_transport_error();
    let request = minimax_search_request();
    let request_id = request.request_id.clone();

    handle_mcp_request(request, &services).await;

    let body = client_body(&mut data_rx);
    for leaked in [
        "connection reset",
        "sk-first",
        "sk-second",
        "sk-inline",
        "sk-disabled",
        "rust",
    ] {
        assert!(
            !body.contains(leaked),
            "the client protocol body repeats `{leaked}`: {body}"
        );
    }
    assert!(
        body.contains("MiniMax MCP upstream call failed"),
        "the client receives the fixed transport diagnostic: {body}"
    );

    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "failed", "{summary:?}");
    assert_eq!(summary.error_code.as_deref(), Some("mcp_call_failed"));

    let _ = std::fs::remove_file(path);
}

// ---------------------------------------------------------------------------
// Issue #1179 P2 repair: the worker streaming path proves the ordering.
//
// The rmcp in-process server resolves the SSE response after the handler
// result, so a delayed tool call there cannot prove that the response starts
// before the tool outcome exists. This test drives the real worker streaming
// settlement with a controlled chunk stream instead: the response start is
// observed while the tracker is still empty, then the tool outcome is recorded
// and the stream ends, and only the final settlement may read the tracker.
// ---------------------------------------------------------------------------

/// One `UsageLog` exactly as the recorder received it.
#[derive(Debug, Clone)]
struct SubmittedLog {
    request_id: uuid::Uuid,
    endpoint_key_id: Option<uuid::Uuid>,
    endpoint_key_label: Option<String>,
    token_slot: Option<i16>,
    state: crate::db::RequestRecordState,
    ok: Option<bool>,
    error_code: Option<String>,
    error_message: Option<String>,
}

#[tokio::test]
async fn the_streaming_settlement_records_the_key_selected_after_the_response_start() {
    let (mut services, _standalone, path, keys, _transport, mut data_rx) =
        sqlite_services_with_builtin_minimax_delayed(usize::MAX, std::time::Duration::ZERO).await;
    // Observe the log the settlement actually submits, not a re-derived one.
    let submitted: std::sync::Arc<std::sync::Mutex<Vec<SubmittedLog>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = submitted.clone();
    services.observe_submitted_usage_log(move |log| {
        observed
            .lock()
            .expect("the submitted log list is not poisoned")
            .push(SubmittedLog {
                request_id: log.request_id,
                endpoint_key_id: log.endpoint_key_id,
                endpoint_key_label: log.endpoint_key_label.clone(),
                token_slot: log.mcp_bearer_token_slot,
                state: log.request_state,
                ok: log.ok,
                error_code: log.error_code.clone(),
                error_message: log.error_message.clone(),
            });
    });
    let request = minimax_search_request();
    let request_id = uuid::Uuid::parse_str(&request.request_id).expect("the request id");
    let request_ctx = crate::worker::runtime::RequestExecutionContext::for_mcp(
        request_id,
        std::time::Instant::now(),
        Some(1),
        uuid::Uuid::new_v4(),
        crate::worker::runtime::prompt_log::RequestPromptLog::default(),
    );
    let metadata = crate::mcp::targeting::McpRequestMetadata {
        server_name: Some("minimax-mcp".to_string()),
        protocol_method: Some("tools/call".to_string()),
        operation_name: Some("web_search".to_string()),
        // A stale slot captured with the response must not beat the tracker.
        selected_token_slot: Some(3),
        request_raw_json: None,
    };
    let request_content_logging = crate::worker_admin_types::RequestContentLoggingResponse {
        mode: crate::worker_admin_types::RequestContentLoggingMode::Off,
        raw_retention_days: 3,
    };
    let tracker = crate::mcp::McpCallTracker::new();
    let context = crate::worker::runtime::mcp_support::McpResponseContext {
        request: &request,
        request_ctx: &request_ctx,
        metadata: &metadata,
        request_content_logging: &request_content_logging,
        redact_content: false,
        upstream_redacted_request_json: None,
        upstream_restore_session: None,
        selected_token_slot: Some(1),
        tracker: Some(&tracker),
        server: None,
        services: &services,
    };

    let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<anyhow::Result<bytes::Bytes>>(4);
    chunk_tx
        .send(Ok(bytes::Bytes::from_static(
            b"event: message\ndata: {}\n\n",
        )))
        .await
        .expect("the response chunk is queued");
    let stream = Box::pin(futures::stream::unfold(
        chunk_rx,
        |mut receiver| async move { receiver.recv().await.map(|item| (item, receiver)) },
    ));

    let handler = super::streaming::handle_streaming_transport_response(
        &context,
        200,
        "text/event-stream".to_string(),
        Vec::new(),
        stream,
    );
    let driver = async {
        let start = loop {
            let data = data_rx.recv().await.expect("the bridge stays open");
            if let BridgeMessage::McpResponseStart(start) = data.message {
                break start;
            }
        };
        assert_eq!(start.status, 200);
        assert_eq!(start.content_type.as_deref(), Some("text/event-stream"));
        assert!(
            tracker.snapshot().endpoint_key_id.is_none(),
            "the response starts before any tool outcome exists"
        );

        // The tool runs after the headers and settles on the second key.
        tracker.record_endpoint_key(Some(keys.second.0), Some("second".to_string()));
        tracker.record_token_slot(Some(2));
        tracker.record_failure(crate::mcp::call_record::McpCallFailure::QuotaExhausted {
            provider_code: 2056,
        });
        drop(chunk_tx);
        while let Some(data) = data_rx.recv().await {
            if let BridgeMessage::McpResponseEnd(_) = data.message {
                break;
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(handler, driver);
    })
    .await
    .expect("the streaming settlement completes");

    // The actual submitted terminal log: everything the request-log model can
    // carry, including the final slot SQLite has no column for.
    {
        let logs = submitted
            .lock()
            .expect("the submitted log list is not poisoned");
        let terminal = logs
            .iter()
            .rfind(|log| log.request_id == request_id)
            .expect("the settlement submitted one log for this request");
        assert_eq!(terminal.state, crate::db::RequestRecordState::Failed);
        assert_eq!(terminal.ok, Some(false));
        assert_eq!(terminal.endpoint_key_id, Some(keys.second.0));
        assert_eq!(terminal.endpoint_key_label.as_deref(), Some("second"));
        assert_eq!(
            terminal.token_slot,
            Some(2),
            "the settlement submits the tracker's final slot, not the captured one"
        );
        assert_eq!(terminal.error_code.as_deref(), Some("mcp_quota_exhausted"));
        assert_eq!(
            terminal.error_message.as_deref(),
            Some("the provider reported an exhausted plan quota (2056)")
        );
    }

    // The durable ledger stores the same state, key, code and message; it has
    // no slot column, so the slot proof is the submitted log above.
    let summary = durable_summary(&path, &request.request_id).await;
    assert_eq!(summary.state, "failed", "{summary:?}");
    assert_eq!(
        summary.error_code.as_deref(),
        Some("mcp_quota_exhausted"),
        "{summary:?}"
    );
    assert_eq!(
        summary.error_message.as_deref(),
        Some("the provider reported an exhausted plan quota (2056)"),
        "{summary:?}"
    );
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.second.0),
        "the record names the key selected after the response start: {summary:?}"
    );
    assert_eq!(summary.endpoint_key_label.as_deref(), Some("second"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_disconnect_settles_the_transport_failure_with_the_selected_key() {
    let (services, _standalone, path, keys, transport, data_rx) =
        sqlite_services_with_builtin_minimax_delayed(
            usize::MAX,
            std::time::Duration::from_millis(200),
        )
        .await;
    let request = minimax_search_request_over_sse();
    let request_id = request.request_id.clone();

    let task = tokio::spawn(async move {
        handle_mcp_request(request, &services).await;
    });
    transport.started.notified().await;
    // The relay is gone before the worker can send the response start.
    drop(data_rx);
    task.await.expect("the MCP request task completes");

    let attempts = transport.attempts();
    let summary = durable_summary(&path, &request_id).await;
    assert_eq!(summary.state, "failed", "{summary:?}");
    assert_eq!(
        summary.error_code.as_deref(),
        Some("mcp_stream_disconnected"),
        "{summary:?}"
    );
    assert_eq!(
        summary.endpoint_key_id,
        Some(keys.key_id_of(attempts.last().expect("an attempt"))),
        "the disconnected record keeps the selected key: {summary:?}"
    );

    let _ = std::fs::remove_file(path);
}
