//! Issue #1032: the embeddings forwarding path.
//!
//! The upstream double is a loopback socket; no provider account is
//! contacted. The tests prove the embeddings request is forwarded verbatim
//! (with only `model`/`dimensions` forced), that an incompatible or streaming
//! request is refused before any upstream contact, and that a redaction
//! conflict is fail-closed with no upstream request.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::super::{ForwardOutcome, RouteForwardRequest, forward_route_request};
use super::{spawn_bridge_log, test_route, wait_for_count};
use crate::config::NativeApi;
use crate::protocol::BridgeMessage;
use crate::worker::runtime::RequestPromptLog;
use crate::worker::runtime::context::{
    RequestExecutionContext, RequestExecutionContextParams, RouteExecutionContext,
};
use crate::worker::runtime::request_assembly::BufferedBridgeRequest;

// The upstream deliberately reports nonzero completion/output tokens: an
// embedding must still bill input only.
const EMBEDDINGS_RESPONSE: &[u8] = br#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.5,0.25]}],"model":"qwen/qwen3-embedding-8b","usage":{"prompt_tokens":4,"completion_tokens":3,"output_tokens":3,"total_tokens":7}}"#;

/// A loopback upstream that records each request line and body and answers
/// with the fixed embeddings response.
async fn spawn_recording_embeddings_upstream() -> (
    SocketAddr,
    Arc<AtomicUsize>,
    Arc<tokio::sync::Mutex<Vec<(String, Vec<u8>)>>>,
) {
    let count = Arc::new(AtomicUsize::new(0));
    let recorded = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let counter = count.clone();
    let log = recorded.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            let log = log.clone();
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = vec![0u8; 65536];
                let read = sock.read(&mut buf).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..read]).to_string();
                let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
                let path = head
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_string();
                log.lock().await.push((path, body.as_bytes().to_vec()));
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Request-Id: embedding-upstream-id\r\nX-Ratelimit-Remaining-Requests: 17\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    EMBEDDINGS_RESPONSE.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.write_all(EMBEDDINGS_RESPONSE).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count, recorded)
}

fn embeddings_request(path: &str, body: &str) -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: path.to_string(),
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(1),
        client_key_hash: None,
        request_user_agent: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    }
}

/// The request context the runtime builds for a bridge request: the request id
/// is the bridge id, so a usage record lands under `request.request_id`.
fn request_ctx_for(
    runtime_state: &crate::worker::runtime::WorkerRuntimeState,
    request: &BufferedBridgeRequest,
) -> RequestExecutionContext {
    RequestExecutionContext::new(RequestExecutionContextParams {
        request_id: uuid::Uuid::parse_str(&request.request_id).expect("request id"),
        started: std::time::Instant::now(),
        request_model: Some("gpt-test".to_string()),
        client_key_id: None,
        client_key_label: None,
        user_id: Some(1),
        owner_worker_id: runtime_state.worker_instance_id(),
        request_prompt_log: RequestPromptLog::default(),
    })
}

/// Initialize a request exactly as the runtime does, so the prompt log (and its
/// raw capture) and the logging flags are real.
async fn initialize(
    services: &crate::worker::runtime::context::RuntimeServices,
    request: &BufferedBridgeRequest,
) -> crate::worker::runtime::ai::request_init::InitializedRequest {
    crate::worker::runtime::ai::request_init::initialize_request(request, services)
        .await
        .expect("initialize request")
}

async fn forward(
    services: &crate::worker::runtime::context::RuntimeServices,
    route: &crate::db::RouteConfig,
    request: &BufferedBridgeRequest,
) -> anyhow::Result<ForwardOutcome> {
    let mut request_ctx = request_ctx_for(&services.runtime_state, request);
    Box::pin(forward_route_request(RouteForwardRequest {
        services,
        request,
        request_ctx: &mut request_ctx,
        route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    }))
    .await
}

fn embeddings_route(base_url: &str) -> crate::db::RouteConfig {
    let mut route = test_route(base_url, NativeApi::Embeddings);
    route.upstream_model = Some("qwen/qwen3-embedding-8b".to_string());
    route.embedding_dimensions_override = Some(7);
    route
}

#[tokio::test]
async fn an_embeddings_request_is_forwarded_verbatim_with_only_model_and_dimensions_forced() {
    let (addr, count, recorded) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request(
        "/v1/embeddings",
        r#"{"model":"caller-model","input":"hello","dimensions":3,"custom_provider_field":{"x":1}}"#,
    );

    let outcome = forward(&services, &route, &request).await.expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "embeddings must be handled, got {outcome:?}"
    );
    wait_for_count(&count, 1).await;

    let recorded = recorded.lock().await.clone();
    assert_eq!(recorded.len(), 1);
    let (path, body) = &recorded[0];
    assert_eq!(path, "/v1/embeddings");
    let body: serde_json::Value = serde_json::from_slice(body).expect("forwarded body json");
    assert_eq!(body["model"], "qwen/qwen3-embedding-8b");
    assert_eq!(
        body["dimensions"], 7,
        "the target override wins over the caller"
    );
    assert_eq!(body["input"], "hello");
    assert_eq!(
        body["custom_provider_field"]["x"], 1,
        "unknown provider fields survive"
    );
    // No chat transformation leaks into the body.
    assert!(body.get("stream").is_none());
    assert!(body.get("service_tier").is_none());
    assert!(body.get("reasoning_effort").is_none());
    assert!(body.get("max_tokens").is_none());

    // The caller receives the upstream body unmodified.
    super::wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("response start");
    assert_eq!(start.status, 200);
}

#[tokio::test]
async fn reviewer_buffered_embeddings_preserve_upstream_headers() {
    let (addr, _, _) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request("/v1/embeddings", r#"{"model":"m","input":"hello"}"#);
    assert!(matches!(
        forward(&services, &route, &request).await.expect("forward"),
        ForwardOutcome::Handled
    ));
    super::wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("response start");
    assert!(
        start.headers.contains(&(
            "x-request-id".to_string(),
            "embedding-upstream-id".to_string()
        )),
        "headers: {:?}",
        start.headers
    );
}

#[tokio::test]
async fn reviewer_truncated_embedding_response_never_delivers_partial_body() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let upstream = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = [0; 8192];
            socket.read(&mut request).await.expect("request");
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n{\"data\":[")
                .await
                .expect("partial response");
            socket.shutdown().await.expect("close response");
        }
    });
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request("/v1/embeddings", r#"{"model":"m","input":"hello"}"#);
    let outcome = forward(&services, &route, &request).await.expect("forward");
    upstream.abort();
    let _ = upstream.await;
    let ForwardOutcome::TransportError { error, .. } = outcome else {
        panic!("truncated body must fail, got {outcome:?}");
    };
    assert!(format!("{error:#}").contains("failed reading upstream response"));
    assert!(bridge_log.lock().await.is_empty());
}

#[tokio::test]
async fn reviewer_oversize_embedding_response_never_delivers_partial_body() {
    let (addr, _, _) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let mut services = super::test_services(out_tx);
    services.response_limits.max_upstream_response_bytes = 8;
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request("/v1/embeddings", r#"{"model":"m","input":"hello"}"#);
    let outcome = forward(&services, &route, &request).await.expect("forward");
    let ForwardOutcome::TransportError { error, .. } = outcome else {
        panic!("oversize body must fail, got {outcome:?}");
    };
    assert!(format!("{error:#}").contains("upstream_response_too_large"));
    assert!(bridge_log.lock().await.is_empty());
}

#[tokio::test]
async fn a_streaming_embeddings_request_is_rejected_before_any_upstream_contact() {
    let (addr, count, _recorded) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request(
        "/v1/embeddings",
        r#"{"model":"m","input":"hello","stream":true}"#,
    );

    let outcome = forward(&services, &route, &request).await.expect("forward");
    let ForwardOutcome::CompatError(err) = outcome else {
        panic!("a streaming embeddings request must be rejected, got {outcome:?}");
    };
    assert_eq!(err.code, "embeddings_stream_not_supported");
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "a rejected request must not contact the upstream"
    );
    drop(bridge_log);
}

#[tokio::test]
async fn a_chatgpt_subscription_endpoint_rejects_embeddings() {
    // Issue #1032: a subscription endpoint is rejected before any backend
    // contact.
    let (addr, count, _recorded) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let mut route = embeddings_route(&format!("http://{addr}"));
    route.has_oauth_token = true;
    let request = embeddings_request("/v1/embeddings", r#"{"model":"m","input":"x"}"#);

    let outcome = forward(&services, &route, &request).await.expect("forward");
    let ForwardOutcome::CompatError(err) = outcome else {
        panic!("a subscription embeddings request must be rejected, got {outcome:?}");
    };
    assert_eq!(err.code, "embeddings_not_supported_for_subscription");
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(bridge_log);
}

#[tokio::test]
async fn a_chat_request_to_an_embeddings_target_is_rejected() {
    let (addr, count, _recorded) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request(
        "/v1/chat/completions",
        r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#,
    );

    let outcome = forward(&services, &route, &request).await.expect("forward");
    let ForwardOutcome::CompatError(err) = outcome else {
        panic!("a chat request to an embeddings target must be rejected, got {outcome:?}");
    };
    assert_eq!(err.code, "unsupported_upstream");
    assert_eq!(count.load(Ordering::SeqCst), 0);
    drop(bridge_log);
}

#[tokio::test(flavor = "current_thread")]
async fn a_redaction_conflict_is_fail_closed_before_any_upstream_contact() {
    let _guard = crate::redact_test_support::domain_redaction_async().await;
    let (addr, count, _recorded) = spawn_recording_embeddings_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));

    // A text input the effective policy would rewrite is refused.
    let request = embeddings_request(
        "/v1/embeddings",
        r#"{"model":"m","input":"contact a.example.com"}"#,
    );
    let outcome = forward(&services, &route, &request).await.expect("forward");
    let ForwardOutcome::CompatError(err) = outcome else {
        panic!("a redaction conflict must be refused, got {outcome:?}");
    };
    assert_eq!(err.code, "embedding_input_redaction_conflict");
    assert!(
        !err.message.contains("a.example.com"),
        "the refusal must not echo the input: {}",
        err.message
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);

    // A token-only input has no text to redact and is forwarded.
    let request = embeddings_request("/v1/embeddings", r#"{"model":"m","input":[1,2,3]}"#);
    let outcome = forward(&services, &route, &request).await.expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "a token-only input must be forwarded, got {outcome:?}"
    );
    wait_for_count(&count, 1).await;
    drop(bridge_log);
}

/// Issue #1032: a configured valid target dimension must never hide a
/// malformed caller dimension. Every malformed shape is refused with an
/// explicit 400 before any upstream contact, even though the target would
/// otherwise force a valid value.
#[tokio::test]
async fn a_malformed_caller_dimension_is_rejected_even_with_a_valid_target_override() {
    for dimensions in ["0", "-1", "1.5", "true", "\"7\"", "2147483648"] {
        let (addr, count, _recorded) = spawn_recording_embeddings_upstream().await;
        let (out_tx, bridge_log) = spawn_bridge_log();
        let services = super::test_services(out_tx);
        // The target forces a valid dimension; the caller value must still be
        // validated on its own.
        let route = embeddings_route(&format!("http://{addr}"));
        assert_eq!(route.embedding_dimensions_override, Some(7));
        let request = embeddings_request(
            "/v1/embeddings",
            &format!(r#"{{"model":"m","input":"hello","dimensions":{dimensions}}}"#),
        );

        let outcome = forward(&services, &route, &request).await.expect("forward");
        let ForwardOutcome::CompatError(err) = outcome else {
            panic!("dimensions={dimensions} must be rejected, got {outcome:?}");
        };
        assert_eq!(
            err.code, "invalid_embeddings_request",
            "dimensions={dimensions}"
        );
        assert_eq!(
            err.status,
            http::StatusCode::BAD_REQUEST,
            "dimensions={dimensions}"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "dimensions={dimensions} must not contact the upstream"
        );
        drop(bridge_log);
    }
}

/// A loopback upstream that answers with a chat completion carrying assistant
/// text and usage.
async fn spawn_chat_json_upstream() -> SocketAddr {
    const BODY: &[u8] = br#"{"id":"chatcmpl-1","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"chat-answer-text"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}"#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = vec![0u8; 65536];
                let _ = sock.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    BODY.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.write_all(BODY).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    addr
}

/// Decode every raw payload object under the local object store root.
///
/// Each object is `PFR1` + a format byte + a zstd-compressed JSON envelope, so
/// the payload content is readable without a credential.
fn raw_store_objects(root: &std::path::Path) -> Vec<String> {
    let mut objects = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if !bytes.starts_with(b"PFR1") || bytes.len() < 5 {
                continue;
            }
            if let Ok(json) = zstd::stream::decode_all(&bytes[5..]) {
                objects.push(String::from_utf8_lossy(&json).to_string());
            }
        }
    }
    objects
}

/// A unique scratch directory for the local raw object store of one test.
fn raw_store_dir(label: &str) -> std::path::PathBuf {
    let base = std::env::var("PROMPT_FERRY_TEST_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(format!("issue-1032-raw-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create raw store dir");
    dir
}

#[allow(dead_code)]
mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}

/// Build admin-backed runtime services with a local raw payload store and the
/// given content-logging mode.
fn recording_services(
    pool: &sqlx::PgPool,
    dir: &std::path::Path,
    mode: crate::worker_admin_types::RequestContentLoggingMode,
) -> crate::worker::runtime::context::RuntimeServices {
    let config = crate::raw_payload_store::RawObjectStoreConfig {
        backend: crate::raw_payload_store::RawObjectStoreBackend::Local,
        local_dir: dir.to_string_lossy().to_string(),
        ..crate::raw_payload_store::RawObjectStoreConfig::default()
    };
    let raw_store = std::sync::Arc::new(
        config
            .build_store()
            .expect("local raw object store builds")
            .expect("local raw object store is enabled"),
    );
    let catalog_cache = crate::mcp::McpCatalogCache::new();
    let admin_state =
        crate::worker_admin_state::AdminState::new(crate::worker_admin_state::AdminStateInit {
            pool: pool.clone(),
            lease_pool: pool.clone(),
            replay_cache: crate::replay_cache::ReplayCache::for_tests(),
            configured_relays: Vec::new(),
            managed_mode: false,
            relay_secret_manager: None,
            redaction_enabled: false,
            model_route_whitelist_enabled: true,
            request_content_logging: crate::worker_admin_types::RequestContentLoggingResponse {
                mode,
                raw_retention_days: 3,
            },
            usage_retention: crate::worker_admin_types::UsageRetentionSettings::default(),
            raw_payload_store: Some(raw_store),
            stream_delta_batching: crate::db::StreamDeltaBatchingSettings::default(),
            llm_review_settings: crate::llm_review::LlmReviewSettings::default(),
            mcp_catalog_cache: catalog_cache.clone(),
            mcp_catalog_service: crate::mcp::McpCatalogService::new(pool.clone(), catalog_cache),
            mcp_session_store: None,
            mcp_allowed_origins: Vec::new(),
            endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
                std::time::Duration::from_secs(60),
            ),
        });
    let (out_tx, bridge_log) = spawn_bridge_log();
    // The bridge log is kept alive for the whole process; a dropped receiver
    // would fail the response pump.
    std::mem::forget(bridge_log);
    crate::worker::runtime::context::RuntimeServices::new(
        Some(admin_state),
        out_tx,
        reqwest::Client::new(),
        crate::worker::runtime::WorkerRuntimeState::default(),
        crate::worker::runtime::context::ResponseLimits::default(),
    )
}

async fn test_pool() -> sqlx::PgPool {
    test_pg_target::required_pool(2)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "this test needs the confirmed dedicated test database; run it through \
                 scripts/ci/with-test-env.sh: {error}"
            )
        })
}

/// Issue #1032: the actual record writer must never receive an embedding
/// vector.
///
/// With content logging and raw payload capture enabled and a real local
/// object store, the embeddings request must produce an AI record (metadata +
/// content), usage and a charge row with input-only accounting, while no raw
/// payload object carries a vector. The input has no redaction match, so the
/// body must reach the upstream byte-for-byte. A chat request through the same
/// services proves capture was genuinely on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedding_vectors_never_reach_the_record_writer() {
    let pool = test_pool().await;
    let dir = raw_store_dir("embedding");
    let services = recording_services(
        &pool,
        &dir,
        crate::worker_admin_types::RequestContentLoggingMode::NormalizedAndRaw,
    );

    let (embeddings_addr, count, recorded) = spawn_recording_embeddings_upstream().await;
    // No dimension override, so the body must reach the upstream unchanged.
    let mut route = test_route(&format!("http://{embeddings_addr}"), NativeApi::Embeddings);
    // The billing charge references the endpoint row, so use a real one.
    let endpoint_id: uuid::Uuid =
        sqlx::query_scalar("SELECT endpoint_id FROM provider_endpoints LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("the dedicated test database has an endpoint row");
    route.route_id = endpoint_id;
    let body =
        r#"{"model":"qwen/qwen3-embedding-8b","input":"plain text with no redaction match"}"#;
    let request = embeddings_request("/v1/embeddings", body);
    let request_id = request.request_id.clone();
    let mut initialized = initialize(&services, &request).await;
    assert!(
        initialized.raw_content_logging_enabled,
        "raw payload capture must be on for this test"
    );
    let outcome = Box::pin(forward_route_request(RouteForwardRequest {
        services: &services,
        request: &request,
        request_ctx: &mut initialized.request_ctx,
        route: &route,
        method: &initialized.method,
        redact_content: initialized.redact_content,
        content_logging_enabled: initialized.content_logging_enabled,
        raw_content_logging_enabled: initialized.raw_content_logging_enabled,
    }))
    .await
    .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled), "{outcome:?}");
    wait_for_count(&count, 1).await;

    let forwarded = recorded.lock().await.clone();
    assert_eq!(forwarded.len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&forwarded[0].1),
        body,
        "a no-match input must be forwarded byte-for-byte"
    );

    let event_id: i64 =
        sqlx::query_scalar("SELECT event_id FROM request_records WHERE request_id = $1")
            .bind(uuid::Uuid::parse_str(&request_id).expect("request id"))
            .fetch_one(&pool)
            .await
            .expect("an AI record is written");
    let (input_tokens, output_tokens, total_tokens, request_state): (
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
    ) = sqlx::query_as(
        "SELECT input_tokens, output_tokens, total_tokens, request_state FROM request_records WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("the record carries usage");
    assert_eq!(input_tokens, Some(4));
    assert_eq!(
        output_tokens,
        Some(0),
        "a reported completion/output count is never billed for an embedding"
    );
    assert_eq!(total_tokens, Some(7), "the reported total is preserved");
    assert_eq!(request_state, "completed");

    // The content half exists but carries no response prompt (there is no text
    // response and, for embeddings, never any response content).
    let response_prompt: Option<String> = sqlx::query_scalar(
        "SELECT response_prompt FROM request_record_content WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("the content row is written");
    assert!(response_prompt.is_none());

    // The existing billing pipeline runs for this record and bills input only.
    let (usage_status, pricing_status, charge_input, charge_output): (
        String,
        String,
        Option<i64>,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT usage_status, pricing_status, input_tokens, output_tokens FROM usage_charges WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("a charge row is written");
    assert_eq!(usage_status, "known");
    assert_eq!(charge_input, Some(4));
    assert_eq!(charge_output, Some(0));
    assert!(
        pricing_status == "unpriced" || pricing_status == "priced",
        "unexpected pricing status {pricing_status}"
    );

    // The charge lines are written by the existing billing pipeline: the input
    // meter carries the reported input tokens and the output meter is zero.
    let charge_id: i64 =
        sqlx::query_scalar("SELECT charge_id FROM usage_charges WHERE event_id = $1")
            .bind(event_id)
            .fetch_one(&pool)
            .await
            .expect("charge id");
    let lines: Vec<(String, i64)> =
        sqlx::query_as("SELECT meter, token_count FROM usage_charge_lines WHERE charge_id = $1")
            .bind(charge_id)
            .fetch_all(&pool)
            .await
            .expect("charge lines");
    let meter = |name: &str| {
        lines
            .iter()
            .find(|(meter, _)| meter == name)
            .map(|(_, count)| *count)
            .unwrap_or(0)
    };
    assert_eq!(meter("input"), 4, "the input meter is billed: {lines:?}");
    assert_eq!(meter("output"), 0, "no output tokens are billed: {lines:?}");

    // No assistant artifact is written for an embeddings request.
    let artifacts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM request_record_assistant_artifacts WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_one(&pool)
    .await
    .expect("artifact lookup");
    assert_eq!(artifacts, 0, "embeddings have no assistant artifact");

    // The raw object carries the request body but never a vector. The chat
    // control below proves capture is genuinely on.
    let objects = raw_store_objects(&dir);
    assert!(!objects.is_empty(), "raw capture must be active");
    assert!(
        objects
            .iter()
            .any(|text| text.contains("plain text with no redaction match")),
        "the request body reaches the raw store"
    );
    let leaked = objects
        .iter()
        .filter(|text| text.contains("0.5") || text.contains("\"embedding\""))
        .count();
    assert_eq!(
        leaked, 0,
        "an embedding vector reached the raw payload store"
    );

    let chat_addr = spawn_chat_json_upstream().await;
    let chat_route = test_route(&format!("http://{chat_addr}"), NativeApi::Chat);
    let chat_request = embeddings_request(
        "/v1/chat/completions",
        r#"{"model":"gpt-test","messages":[{"role":"user","content":"hi"}]}"#,
    );
    let mut chat_initialized = initialize(&services, &chat_request).await;
    let outcome = Box::pin(forward_route_request(RouteForwardRequest {
        services: &services,
        request: &chat_request,
        request_ctx: &mut chat_initialized.request_ctx,
        route: &chat_route,
        method: &chat_initialized.method,
        redact_content: chat_initialized.redact_content,
        content_logging_enabled: chat_initialized.content_logging_enabled,
        raw_content_logging_enabled: chat_initialized.raw_content_logging_enabled,
    }))
    .await
    .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled), "{outcome:?}");
    assert!(
        raw_store_objects(&dir)
            .iter()
            .any(|text| text.contains("chat-answer-text")),
        "the chat control must persist its raw response body"
    );

    sqlx::query("DELETE FROM usage_charge_lines WHERE charge_id = $1")
        .bind(charge_id)
        .execute(&pool)
        .await
        .expect("remove the fixture charge lines");
    sqlx::query("DELETE FROM usage_charges WHERE event_id = $1")
        .bind(event_id)
        .execute(&pool)
        .await
        .expect("remove the fixture charge row");
    let _ = std::fs::remove_dir_all(&dir);
    pool.close().await;
}

/// Issue #1032: a fail-closed redaction refusal still writes a failed usage
/// record, makes no outbound request, and never echoes the input.
#[tokio::test(flavor = "current_thread")]
async fn a_redaction_conflict_records_a_failed_usage_row_without_echo_or_outbound_request() {
    let _guard = crate::redact_test_support::domain_redaction_async().await;
    let pool = test_pool().await;
    let dir = raw_store_dir("conflict");
    let services = recording_services(
        &pool,
        &dir,
        crate::worker_admin_types::RequestContentLoggingMode::NormalizedOnly,
    );

    let (addr, count, _recorded) = spawn_recording_embeddings_upstream().await;
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request(
        "/v1/embeddings",
        r#"{"model":"m","input":"contact a.example.com"}"#,
    );
    let request_id = request.request_id.clone();
    // Drive the real refusal path (`respond_with_client_error`), which is the
    // writer of the failed usage record in the runtime flow.
    let mut initialized = initialize(&services, &request).await;
    assert!(initialized.content_logging_enabled);
    let outcome = Box::pin(forward_route_request(RouteForwardRequest {
        services: &services,
        request: &request,
        request_ctx: &mut initialized.request_ctx,
        route: &route,
        method: &initialized.method,
        redact_content: initialized.redact_content,
        content_logging_enabled: initialized.content_logging_enabled,
        raw_content_logging_enabled: initialized.raw_content_logging_enabled,
    }))
    .await
    .expect("forward");
    let ForwardOutcome::CompatError(err) = outcome else {
        panic!("a redaction conflict must be refused, got {outcome:?}");
    };
    assert_eq!(err.code, "embedding_input_redaction_conflict");
    assert!(
        !err.message.contains("a.example.com"),
        "the refusal must not echo the input"
    );
    let code = err.code;
    let status = err.status;
    let message = err.message.clone();
    let route_ctx = RouteExecutionContext::new(&route);
    Box::pin(
        crate::worker::runtime::ai::errors::respond_with_client_error(
            &services,
            &request,
            &initialized.request_ctx,
            &route_ctx,
            err,
        ),
    )
    .await
    .expect("client error response");
    assert_eq!(status, http::StatusCode::BAD_REQUEST);
    assert_eq!(code, "embedding_input_redaction_conflict");
    assert!(!message.contains("a.example.com"));
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "a refused request must not contact the upstream"
    );

    let (request_state, error_code): (String, Option<String>) = sqlx::query_as(
        "SELECT request_state, error_code FROM request_records WHERE request_id = $1",
    )
    .bind(uuid::Uuid::parse_str(&request_id).expect("request id"))
    .fetch_one(&pool)
    .await
    .expect("a failed usage record is written");
    assert_eq!(request_state, "failed");
    assert_eq!(
        error_code.as_deref(),
        Some("embedding_input_redaction_conflict")
    );

    let _ = std::fs::remove_dir_all(&dir);
    pool.close().await;
}

/// A loopback upstream that answers with a valid but large (>1 MiB) embeddings
/// response whose `usage` object trails the vector payload.
async fn spawn_large_embeddings_upstream() -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = vec![0u8; 65536];
                let _ = sock.read(&mut buf).await;
                let mut body = String::from(
                    r#"{"object":"list","model":"usage","metadata":{"usage":{"prompt_tokens":999,"total_tokens":999}},"data":[{"object":"embedding","index":0,"embedding":["#,
                );
                for _ in 0..110_000 {
                    body.push_str("0.1234567,");
                }
                body.push_str(
                    "0.7654321]}],\"usage\":{\"prompt_tokens\":4242,\"total_tokens\":4242}}",
                );
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(body.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

/// Issue #1032: a valid embedding batch larger than the general JSON capture
/// cap must still be accounted for, while its vectors never reach a log or a
/// raw payload artifact.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_large_embedding_batch_reports_usage_without_retaining_vectors() {
    let pool = test_pool().await;
    let dir = raw_store_dir("large-embedding");
    let services = recording_services(
        &pool,
        &dir,
        crate::worker_admin_types::RequestContentLoggingMode::NormalizedAndRaw,
    );

    let (addr, count) = spawn_large_embeddings_upstream().await;
    let mut route = test_route(&format!("http://{addr}"), NativeApi::Embeddings);
    route.upstream_model = Some("qwen/qwen3-embedding-8b".to_string());
    let endpoint_id: uuid::Uuid =
        sqlx::query_scalar("SELECT endpoint_id FROM provider_endpoints LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("the dedicated test database has an endpoint row");
    route.route_id = endpoint_id;
    let request = embeddings_request(
        "/v1/embeddings",
        r#"{"model":"qwen/qwen3-embedding-8b","input":"a large batch"}"#,
    );
    let request_id = request.request_id.clone();
    let mut initialized = initialize(&services, &request).await;
    assert!(initialized.raw_content_logging_enabled);
    let outcome = Box::pin(forward_route_request(RouteForwardRequest {
        services: &services,
        request: &request,
        request_ctx: &mut initialized.request_ctx,
        route: &route,
        method: &initialized.method,
        redact_content: initialized.redact_content,
        content_logging_enabled: initialized.content_logging_enabled,
        raw_content_logging_enabled: initialized.raw_content_logging_enabled,
    }))
    .await
    .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled), "{outcome:?}");
    wait_for_count(&count, 1).await;

    let (event_id, input_tokens, output_tokens, total_tokens): (i64, Option<i64>, Option<i64>, Option<i64>) =
        sqlx::query_as(
            "SELECT event_id, input_tokens, output_tokens, total_tokens FROM request_records WHERE request_id = $1",
        )
        .bind(uuid::Uuid::parse_str(&request_id).expect("request id"))
        .fetch_one(&pool)
        .await
        .expect("a record is written");
    assert_eq!(
        input_tokens,
        Some(4242),
        "a >1 MiB batch must still report its input"
    );
    assert_eq!(output_tokens, Some(0));
    assert_eq!(
        total_tokens,
        Some(4242),
        "a >1 MiB batch must still report its total"
    );

    // The vectors stay out of the raw payload store even though the body was
    // far larger than the general content-capture cap.
    let objects = raw_store_objects(&dir);
    assert!(!objects.is_empty(), "raw capture must be active");
    let leaked = objects
        .iter()
        .filter(|text| text.contains("0.1234567") || text.contains("\"embedding\""))
        .count();
    assert_eq!(
        leaked, 0,
        "a vector from a large batch reached the raw store"
    );

    sqlx::query(
        "DELETE FROM usage_charge_lines WHERE charge_id IN (SELECT charge_id FROM usage_charges WHERE event_id = $1)",
    )
    .bind(event_id)
    .execute(&pool)
    .await
    .expect("remove the fixture charge lines");
    sqlx::query("DELETE FROM usage_charges WHERE event_id = $1")
        .bind(event_id)
        .execute(&pool)
        .await
        .expect("remove the fixture charge row");
    let _ = std::fs::remove_dir_all(&dir);
    pool.close().await;
}

/// Issue #1032: the newly reachable buffered embeddings path forwards exactly
/// the existing allowlisted upstream headers and nothing else, including when
/// the upstream also sends secrets or hop-by-hop headers.
#[tokio::test]
async fn buffered_embeddings_forward_only_the_allowlisted_upstream_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    let upstream = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = socket.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     X-Request-Id: correlation-1\r\n\
                     X-Ratelimit-Remaining-Requests: 17\r\n\
                     Set-Cookie: session=secret\r\n\
                     Authorization: Bearer secret-token\r\n\
                     X-Custom-Secret: leaked\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n",
                    EMBEDDINGS_RESPONSE.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(EMBEDDINGS_RESPONSE).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = super::test_services(out_tx);
    let route = embeddings_route(&format!("http://{addr}"));
    let request = embeddings_request("/v1/embeddings", r#"{"model":"m","input":"hello"}"#);
    assert!(matches!(
        forward(&services, &route, &request).await.expect("forward"),
        ForwardOutcome::Handled
    ));
    upstream.abort();
    let _ = upstream.await;
    super::wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("response start");
    assert!(
        start
            .headers
            .contains(&("x-request-id".to_string(), "correlation-1".to_string())),
        "headers: {:?}",
        start.headers
    );
    assert!(
        start.headers.contains(&(
            "x-ratelimit-remaining-requests".to_string(),
            "17".to_string()
        )),
        "headers: {:?}",
        start.headers
    );
    for (name, _) in &start.headers {
        assert!(
            matches!(
                name.as_str(),
                "request-id" | "x-request-id" | "retry-after" | "content-type"
            ) || name.starts_with("anthropic-ratelimit-")
                || name.starts_with("x-ratelimit-"),
            "a non-allowlisted header was forwarded: {name}"
        );
    }
    for forbidden in ["set-cookie", "authorization", "x-custom-secret"] {
        assert!(
            !start.headers.iter().any(|(name, _)| name == forbidden),
            "forbidden header {forbidden} was forwarded: {:?}",
            start.headers
        );
    }
}
