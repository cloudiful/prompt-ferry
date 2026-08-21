use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::*;
use crate::{
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

#[tokio::test]
async fn standalone_mcp_rejection_records_one_terminal_failure_summary() {
    let _redaction_guard = crate::redact_test_support::lock();
    let path = database_path();
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let standalone = StandaloneRuntimeState::new(
        store,
        RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager"),
        StandaloneConfig::default(),
    );
    let (out_tx, _control_rx, _data_rx) = BridgeSender::channel();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
    .with_standalone_state(standalone.clone());
    let request = minimal_request();
    let request_id = uuid::Uuid::parse_str(&request.request_id).expect("request ID");

    assert!(
        preparation::build_request_context(request, &services)
            .await
            .is_none()
    );

    let summaries = standalone.recent_usage();
    assert_eq!(summaries.len(), 1);
    let summary = &summaries[0];
    assert_eq!(summary.request_id, request_id);
    assert_eq!(summary.state, "failed");
    assert_eq!(summary.path, "/mcp");
    assert_eq!(
        summary.error_code.as_deref(),
        Some("standalone_mcp_unavailable")
    );

    drop(services);
    drop(standalone);
    let _ = std::fs::remove_file(path);
}
