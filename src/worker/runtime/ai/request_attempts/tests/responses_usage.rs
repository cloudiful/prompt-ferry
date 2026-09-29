//! Request-usage sink coverage for the sniffed Responses event stream.
//!
//! The bridge-level framing tests prove framing recovery and byte-exact
//! replay, but their `RuntimeServices` has no usage sink. This test drives the
//! same sniffed fixture through a standalone SQLite usage recorder and asserts
//! the parsed terminal usage actually reaches the request record (issue #599
//! R2f.3 acceptance).

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::super::{ForwardOutcome, RouteForwardRequest, forward_route_request};
use super::{
    responses_framing::spawn_split_event_stream_upstream, spawn_bridge_log, test_request,
    test_request_ctx, test_route, wait_for_count,
};
use crate::{
    config::NativeApi,
    relay_secrets::RelaySecretManager,
    standalone_config::{StandaloneConfig, StandaloneConfigStore},
    worker::runtime::{
        WorkerRuntimeState,
        context::{BridgeSender, ResponseLimits, RuntimeServices},
        standalone::StandaloneRuntimeState,
    },
};

/// A fresh SQLite file inside a unique temp directory, so the test owns the
/// whole path and can clean up the store plus any WAL sidecars afterwards.
fn database_path() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("prompt-ferry-r2f3-usage-{suffix}"));
    std::fs::create_dir_all(&dir).expect("temp usage dir");
    dir.join("usage.sqlite")
}

async fn standalone_services(
    out_tx: BridgeSender,
    path: &Path,
) -> (RuntimeServices, StandaloneRuntimeState) {
    let store = Arc::new(
        StandaloneConfigStore::open(path)
            .await
            .expect("standalone store"),
    );
    let standalone = StandaloneRuntimeState::new(
        store,
        RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("secret manager"),
        StandaloneConfig::default(),
    );
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
    .with_standalone_state(standalone.clone());
    (services, standalone)
}

#[tokio::test]
async fn sniffed_event_stream_usage_reaches_the_request_usage_sink() {
    let _redaction_guard = crate::redact_test_support::lock_async().await;
    let (addr, count) = spawn_split_event_stream_upstream(None).await;
    let (out_tx, _bridge_log) = spawn_bridge_log();
    let path = database_path();
    let (services, standalone) = standalone_services(out_tx, &path).await;
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);
    let request = test_request();
    let mut request_ctx = test_request_ctx(&services.runtime_state);

    let outcome = Box::pin(forward_route_request(RouteForwardRequest {
        services: &services,
        request: &request,
        request_ctx: &mut request_ctx,
        route: &route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    }))
    .await
    .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 1).await;

    let recorded = standalone.recent_usage();
    assert_eq!(
        recorded.len(),
        1,
        "the sniffed stream must record exactly one terminal usage event",
    );
    let terminal = &recorded[0];
    assert_eq!(terminal.event_kind, "request");
    assert_eq!(terminal.state, "completed");
    assert_eq!(terminal.status, Some(200));
    assert_eq!(
        terminal.input_tokens,
        Some(120),
        "input tokens must reach the request-usage sink",
    );
    assert_eq!(terminal.output_tokens, Some(20));
    assert_eq!(terminal.total_tokens, Some(140));

    if let Some(dir) = path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}
