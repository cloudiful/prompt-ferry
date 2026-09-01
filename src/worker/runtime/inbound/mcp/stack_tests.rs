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
            match data.message {
                BridgeMessage::McpResponseStart(start) => {
                    saw_start = true;
                    assert_eq!(start.request_id, request_id);
                    assert_eq!(
                        start.status,
                        reqwest::StatusCode::SERVICE_UNAVAILABLE.as_u16()
                    );
                }
                _ => {}
            }
        }
        assert!(saw_start, "handler must emit an MCP response start");
    });
}

fn mcp_input(name: &str, daily_max_requests: Option<i32>) -> McpServerInput {
    McpServerInput {
        scope: "admin".to_string(),
        owner_user_id: None,
        source_endpoint_id: None,
        name: name.to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "stdio".to_string(),
        url: None,
        command: Some("mcpd".to_string()),
        args: serde_json::json!([]),
        env_json: serde_json::json!({}),
        bearer_tokens_json: serde_json::json!([]),
        http_headers_json: serde_json::json!({}),
        auth_mode: "none".to_string(),
        basic_username: None,
        basic_password: None,
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: serde_json::json!([]),
        disabled_tools: serde_json::json!([]),
        disabled_resources: serde_json::json!([]),
        daily_max_requests,
        monthly_max_requests: None,
        enabled: true,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
    }
}

async fn sqlite_services(
    name: &str,
    daily_max_requests: Option<i32>,
) -> (
    RuntimeServices,
    StandaloneRuntimeState,
    PathBuf,
    tokio::sync::mpsc::Receiver<super::super::super::bridge::BridgeData>,
) {
    let _redaction_guard = crate::redact_test_support::lock();
    let path = database_path();
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let manager = RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
    let repository = ConfigRepository::sqlite(store.clone(), manager.clone());
    repository
        .create_mcp_server(uuid::Uuid::new_v4(), mcp_input(name, daily_max_requests))
        .await
        .expect("create SQLite MCP server");
    let config = WorkerConfig {
        standalone_database_path: path.to_string_lossy().to_string(),
        ..WorkerConfig::default()
    };
    let mcp_runtime = McpRuntimeState::sqlite(
        &config,
        McpRuntimeStorage::from_repository(repository),
        store.pool().clone(),
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
    let (services, standalone, path, mut _data_rx) = sqlite_services("local", None).await;
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
        .rev()
        .next()
        .expect("aggregate request summary");
    assert_eq!(summary.state, "completed", "{summary:?}");
    assert!(summary.error_code.is_none(), "{summary:?}");

    drop(execution);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn sqlite_mcp_request_budget_returns_precise_capability_error() {
    let (services, standalone, path, _data_rx) = sqlite_services("limited", Some(1)).await;
    let mut request = minimal_request();
    request.server_name = Some("limited".to_string());
    let request_id = request.request_id.clone();

    let execution = preparation::build_request_context(request, &services)
        .await
        .expect("request context");
    assert!(
        preparation::resolve_server_and_quota(execution, &services)
            .await
            .is_none()
    );

    let summary = standalone
        .recent_usage()
        .into_iter()
        .rev()
        .find(|summary| summary.request_id.to_string() == request_id)
        .expect("quota rejection summary");
    assert_eq!(
        summary.error_code.as_deref(),
        Some("sqlite_mcp_quota_unavailable")
    );
    let _ = std::fs::remove_file(path);
    drop(standalone);
}
