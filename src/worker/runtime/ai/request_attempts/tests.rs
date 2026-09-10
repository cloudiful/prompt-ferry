use super::super::super::{
    RequestExecutionContext, WorkerRuntimeState,
    context::{BridgeSender, ResponseLimits, RuntimeServices},
    request_assembly::BufferedBridgeRequest,
};
use super::*;
use crate::{
    config::NativeApi, db::RouteConfig, protocol::BridgeMessage,
    worker::runtime::prompt_log::RequestPromptLog,
};
use axum::{
    Router,
    http::{StatusCode, header},
    response::Response,
    routing::post,
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const RESPONSES_JSON_BODY: &str = r#"{"id":"resp_1","object":"response","status":"completed","output":[],"usage":{"total_tokens":5}}"#;

fn test_services(out_tx: BridgeSender) -> RuntimeServices {
    RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
}

fn test_request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"gpt-test","input":"hello"}"#.to_vec(),
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

fn test_route(base_url: &str, native_api: NativeApi) -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: base_url.to_string(),
        api_key: "test-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api,
        upstream_model: None,
        route_selection_reason: crate::db::RouteSelectionReason::Default,
        provider: crate::db::EndpointProvider::Generic,
        service_tier: crate::db::MinimaxServiceTier::Standard,
    }
}

fn test_request_ctx(runtime_state: &WorkerRuntimeState) -> RequestExecutionContext {
    RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-test".to_string()),
        None,
        None,
        Some(1),
        runtime_state.worker_instance_id(),
        RequestPromptLog::default(),
    )
}

async fn forward_test_request(
    services: &RuntimeServices,
    route: &RouteConfig,
) -> anyhow::Result<ForwardOutcome> {
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    Box::pin(forward_route_request(RouteForwardRequest {
        services,
        request: &request,
        request_ctx: &request_ctx,
        route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    }))
    .await
}

fn spawn_bridge_log() -> (BridgeSender, Arc<tokio::sync::Mutex<Vec<BridgeMessage>>>) {
    let (out_tx, mut control_rx, mut data_rx) = BridgeSender::channel();
    let log = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let log_control = log.clone();
    let log_data = log.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                message = control_rx.recv() => {
                    let Some(message) = message else { break };
                    log_control.lock().await.push(message);
                }
                data = data_rx.recv() => {
                    let Some(data) = data else { break };
                    log_data.lock().await.push(data.message);
                }
            }
        }
    });
    (out_tx, log)
}

async fn wait_for_count(count: &AtomicUsize, expected: usize) {
    for _ in 0..200 {
        if count.load(Ordering::SeqCst) >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("upstream request count did not reach {expected}");
}

async fn wait_for_bridge(log: &Arc<tokio::sync::Mutex<Vec<BridgeMessage>>>, expected: usize) {
    for _ in 0..200 {
        if log.lock().await.len() >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("bridge message count did not reach {expected}");
}

async fn spawn_eof_before_headers_upstream(fail_first_n: usize) -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                if n < fail_first_n {
                    let _ = sock.shutdown().await;
                    return;
                }
                let body = RESPONSES_JSON_BODY;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

async fn spawn_truncated_body_upstream(fail_first_n: usize) -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                if n < fail_first_n {
                    let prefix = r#"{"id":"resp_1","object":"respon"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{}",
                        prefix
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.shutdown().await;
                    return;
                }
                let body = RESPONSES_JSON_BODY;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

async fn spawn_broken_sse_upstream() -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        counter.fetch_add(1, Ordering::SeqCst);
        let mut request_buf = [0u8; 4096];
        let _ = stream.read(&mut request_buf).await;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        write_chunk(
            &mut stream,
            b"event: response.created\r\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\r\n\r\n",
        )
        .await;
        write_chunk(
            &mut stream,
            b"event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\r\n\r\n",
        )
        .await;
        stream.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        stream
            .write_all(b"40\r\ndata: {\"type\":\"response.completed\"")
            .await
            .unwrap();
        stream.flush().await.unwrap();
    });
    (addr, count)
}

async fn write_chunk(stream: &mut tokio::net::TcpStream, body: &[u8]) {
    stream
        .write_all(format!("{:X}\r\n", body.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
    stream.write_all(b"\r\n").await.unwrap();
}

#[tokio::test]
async fn retries_connection_close_before_headers_and_succeeds() {
    let (addr, count) = spawn_eof_before_headers_upstream(1).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 2).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let starts = messages
        .iter()
        .filter(|message| matches!(message, BridgeMessage::ResponseStart(_)))
        .count();
    let ends = messages
        .iter()
        .filter(|message| matches!(message, BridgeMessage::ResponseEnd(_)))
        .count();
    let errors = messages
        .iter()
        .filter(|message| matches!(message, BridgeMessage::ResponseError(_)))
        .count();
    assert_eq!(starts, 1, "downstream should receive exactly one response");
    assert_eq!(ends, 1);
    assert_eq!(errors, 0);
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "upstream should be requested twice"
    );
}

#[tokio::test]
async fn exhausts_attempts_when_connection_closes_before_headers() {
    let (addr, count) = spawn_eof_before_headers_upstream(3).await;
    let (out_tx, _bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    match outcome {
        ForwardOutcome::TransportError {
            error,
            terminal_recorded,
        } => {
            assert!(!terminal_recorded);
            assert!(error.to_string().contains("upstream request failed"));
        }
        other => panic!("expected transport error, got {other:?}"),
    }
    wait_for_count(&count, 3).await;
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn does_not_retry_http_bad_gateway() {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let app = Router::new().route(
        "/v1/responses",
        post(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                StatusCode::BAD_GATEWAY
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_bridge(&bridge_log, 3).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retries_truncated_non_stream_body_and_succeeds() {
    let (addr, count) = spawn_truncated_body_upstream(1).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 2).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let starts = messages
        .iter()
        .filter(|message| matches!(message, BridgeMessage::ResponseStart(_)))
        .count();
    let ends = messages
        .iter()
        .filter(|message| matches!(message, BridgeMessage::ResponseEnd(_)))
        .count();
    let chunks = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.as_slice()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(starts, 1, "partial response must not leak downstream");
    assert_eq!(ends, 1);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0], RESPONSES_JSON_BODY.as_bytes());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn exhausts_attempts_for_truncated_non_stream_body() {
    let (addr, count) = spawn_truncated_body_upstream(3).await;
    let (out_tx, _bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(
        outcome,
        ForwardOutcome::TransportError {
            terminal_recorded: false,
            ..
        }
    ));
    wait_for_count(&count, 3).await;
    assert_eq!(count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn does_not_retry_committed_stream_failure() {
    let (addr, count) = spawn_broken_sse_upstream().await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    match outcome {
        ForwardOutcome::TransportError {
            error,
            terminal_recorded,
        } => {
            assert!(
                terminal_recorded,
                "committed stream failure must be marked as already recorded"
            );
            assert!(
                error
                    .to_string()
                    .contains("failed reading upstream response")
            );
        }
        other => panic!("expected transport error, got {other:?}"),
    }
    wait_for_bridge(&bridge_log, 2).await;
    let messages = bridge_log.lock().await;
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, BridgeMessage::ResponseStart(_))),
        "stream response must have started downstream"
    );
    drop(messages);
    for _ in 0..200 {
        let has_error = bridge_log
            .lock()
            .await
            .iter()
            .any(|message| matches!(message, BridgeMessage::ResponseError(_)));
        if has_error {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let messages = bridge_log.lock().await;
    let errors = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseError(error) => Some(error.code.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(errors, vec!["upstream_stream_error"]);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "stream failure must not retry"
    );
}

#[tokio::test]
async fn stops_retrying_when_request_is_cancelled_during_backoff() {
    let (addr, count) = spawn_eof_before_headers_upstream(3).await;
    let (out_tx, _bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let runtime_state = services.runtime_state.clone();
    let request = test_request();
    let request_ctx = test_request_ctx(&runtime_state);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);
    let cancellation = runtime_state
        .test_register_request_cancellation(&request.request_id)
        .await;

    let wait = forward_route_request(RouteForwardRequest {
        services: &services,
        request: &request,
        request_ctx: &request_ctx,
        route: &route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    });
    tokio::pin!(wait);
    loop {
        if count.load(Ordering::SeqCst) >= 1 {
            cancellation.cancel();
            break;
        }
        if tokio::time::timeout(Duration::from_millis(10), &mut wait)
            .await
            .is_ok()
        {
            panic!("request finished before first attempt was observed");
        }
    }
    let outcome = wait.await.expect("forward");
    assert!(matches!(outcome, ForwardOutcome::TransportError { .. }));
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "cancelled request must not start a retry attempt"
    );
}

#[tokio::test]
async fn does_not_retry_oversized_non_stream_response() {
    let (addr, count) = spawn_eof_before_headers_upstream(0).await;
    let (out_tx, _bridge_log) = spawn_bridge_log();
    let services = RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits {
            max_upstream_response_bytes: 16,
            ..ResponseLimits::default()
        },
    );
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    match outcome {
        ForwardOutcome::TransportError { error, .. } => {
            assert!(error.to_string().contains("upstream_response_too_large"));
        }
        other => panic!("expected transport error, got {other:?}"),
    }
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn does_not_retry_stateless_responses_to_chat_on_adapter_error() {
    // Stateless /v1/responses→Chat direct path reaches the chat upstream exactly
    // once; an invalid chat payload is handled without retry. Stateful rejection
    // (previous_response_id/conversation → invalid_responses_continuation) is covered
    // by wrapper unit tests and bridge tests and is not masked here.
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from("this is not a chat completion"))
                    .unwrap()
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (out_tx, _bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Chat);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "stateless responses→chat adapter error must be handled without retry, got {outcome:?}"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn does_not_retry_when_relay_bridge_is_closed() {
    let (addr, count) = spawn_eof_before_headers_upstream(0).await;
    let (out_tx, control_rx, data_rx) = BridgeSender::channel();
    drop(control_rx);
    drop(data_rx);
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::TransportError { .. }));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

fn glm_test_route(base_url: &str, native_api: NativeApi) -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: base_url.to_string(),
        api_key: "test-key".to_string(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api,
        upstream_model: None,
        route_selection_reason: crate::db::RouteSelectionReason::Default,
        provider: crate::db::EndpointProvider::Glm,
        service_tier: crate::db::MinimaxServiceTier::Standard,
    }
}

#[tokio::test]
async fn glm_responses_on_v1_base_does_not_fail_fast() {
    // The documented GLM Responses base (`.../api/v1`) must pass
    // through the validator; the forward path then proceeds to the
    // URL composer and (in this test, with no upstream bound) hits
    // a transport error, not a CompatError.
    let (addr, _count) = spawn_eof_before_headers_upstream(0).await;
    let (out_tx, _bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(
        !matches!(outcome, ForwardOutcome::CompatError(_)),
        "GLM Responses on /api/v1 must not trip the fail-fast, got {outcome:?}",
    );
}

const GLM_ENVELOPE_404_BODY: &[u8] =
    br#"{"code":500,"msg":"404 NOT_FOUND","success":false,"data":null}"#;

async fn spawn_fixed_body_upstream(body: &'static [u8]) -> (SocketAddr, Arc<AtomicUsize>) {
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
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.write_all(body).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

#[tokio::test]
async fn glm_responses_envelope_body_surfaces_bad_gateway_to_bridge() {
    // Issue #241: a Zhipu 2xx envelope body
    // (`{"code":500,"msg":"404 NOT_FOUND","success":false}`) used to be
    // recorded as an empty success because the runtime HTTP path
    // trusted the status code. The envelope check now fails the
    // request loudly: the forwarder sends a 502 with the envelope
    // message and records a failed usage event, instead of passing
    // the envelope bytes to the client.
    let (addr, count) = spawn_fixed_body_upstream(GLM_ENVELOPE_404_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "envelope body must be handled (not retried) as a 502 response, got {outcome:?}",
    );
    wait_for_count(&count, 1).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "envelope must not trigger retry"
    );
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(
        start.status, 502,
        "envelope must surface as 502 (Bad Gateway), got {}",
        start.status,
    );
    let chunk: Vec<u8> = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.clone()),
            _ => None,
        })
        .next()
        .expect("bridge must receive a response chunk");
    let payload: serde_json::Value =
        serde_json::from_slice(&chunk).expect("envelope error response must be JSON");
    assert_eq!(
        payload["error"]["code"], "glm_envelope_error",
        "error code must identify the GLM envelope failure, got {payload}",
    );
    let message = payload["error"]["message"]
        .as_str()
        .expect("error message must be a string");
    assert!(
        message.contains("404 NOT_FOUND"),
        "message must surface the envelope reason, got {message}",
    );
    assert!(
        message.contains("500"),
        "message must surface the envelope code, got {message}",
    );
}

#[tokio::test]
async fn glm_responses_normal_body_passes_envelope_check() {
    // A normal Responses body must pass the envelope check unchanged
    // and reach the client as the original 2xx payload — the helper
    // is a no-op for OpenAI-shape responses (no `success`/`code`).
    let (addr, count) = spawn_fixed_body_upstream(RESPONSES_JSON_BODY.as_bytes()).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(start.status, 200, "normal body must keep HTTP 200");
    let chunk: Vec<u8> = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.clone()),
            _ => None,
        })
        .next()
        .expect("bridge must receive a response chunk");
    assert_eq!(
        chunk,
        RESPONSES_JSON_BODY.as_bytes(),
        "normal body must be forwarded verbatim to the client",
    );
}

const CHAT_COMPLETIONS_JSON_BODY: &[u8] =
    br#"{"id":"chatcmpl-1","object":"chat.completion","choices":[],"usage":{"total_tokens":7}}"#;

fn chat_completions_request(stream: bool) -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/chat/completions".to_string(),
        headers: Vec::new(),
        body: if stream {
            br#"{"model":"gpt-test","messages":[{"role":"user","content":"hi"}],"stream":true}"#
                .to_vec()
        } else {
            br#"{"model":"gpt-test","messages":[{"role":"user","content":"hi"}],"stream":false}"#
                .to_vec()
        },
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

async fn forward_chat_completions_request(
    services: &RuntimeServices,
    route: &RouteConfig,
    stream: bool,
) -> anyhow::Result<ForwardOutcome> {
    let request = chat_completions_request(stream);
    let request_ctx = test_request_ctx(&services.runtime_state);
    Box::pin(forward_route_request(RouteForwardRequest {
        services,
        request: &request,
        request_ctx: &request_ctx,
        route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    }))
    .await
}

async fn assert_envelope_502_on_bridge(
    bridge_log: &Arc<tokio::sync::Mutex<Vec<BridgeMessage>>>,
    upstream_count: &AtomicUsize,
    expected_count: usize,
) {
    wait_for_count(upstream_count, expected_count).await;
    assert_eq!(
        upstream_count.load(Ordering::SeqCst),
        expected_count,
        "envelope must not trigger retry",
    );
    wait_for_bridge(bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(
        start.status, 502,
        "envelope must surface as 502 (Bad Gateway), got {}",
        start.status,
    );
    let chunk: Vec<u8> = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.clone()),
            _ => None,
        })
        .next()
        .expect("bridge must receive a response chunk");
    let payload: serde_json::Value =
        serde_json::from_slice(&chunk).expect("envelope error response must be JSON");
    assert_eq!(
        payload["error"]["code"], "glm_envelope_error",
        "error code must identify the GLM envelope failure, got {payload}",
    );
    let message = payload["error"]["message"]
        .as_str()
        .expect("error message must be a string");
    assert!(
        message.contains("404 NOT_FOUND"),
        "message must surface the envelope reason, got {message}",
    );
    assert!(
        message.contains("500"),
        "message must surface the envelope code, got {message}",
    );
}

#[tokio::test]
async fn glm_chat_passthrough_envelope_fails_loudly_for_stream_false() {
    // Issue #241 P1: guarding only the ChatToResponses + Responses
    // non-stream forwarders was not enough — a dead-route 200
    // envelope on `POST /v1/chat/completions` + GLM Chat native
    // (response_adapter == Passthrough, non-SSE) used to fall through
    // to `forward_streaming_response` / the buffered restore path and
    // reach the client as HTTP 200 with the envelope bytes (the
    // original live Codex streaming failure). The centralized
    // preflight in `forward_upstream_response` now buffers the JSON
    // body and surfaces 502 glm_envelope_error before any forwarder
    // sees the envelope bytes. Pin `stream: false` first.
    let (addr, count) = spawn_fixed_body_upstream(GLM_ENVELOPE_404_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Chat);

    let outcome = forward_chat_completions_request(&services, &route, false)
        .await
        .expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "envelope body must be handled (not retried) as a 502 response, got {outcome:?}",
    );
    assert_envelope_502_on_bridge(&bridge_log, &count, 1).await;
}

#[tokio::test]
async fn glm_chat_passthrough_envelope_fails_loudly_for_stream_true() {
    // Same live failure as the `stream: false` case but with the
    // client requesting `stream: true`. The runtime preflight runs
    // off the upstream's `Content-Type` (not the client's `stream`
    // flag), so both client shapes must surface 502 instead of
    // silently emitting the envelope bytes as the first SSE /
    // non-SSE chunk.
    let (addr, count) = spawn_fixed_body_upstream(GLM_ENVELOPE_404_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Chat);

    let outcome = forward_chat_completions_request(&services, &route, true)
        .await
        .expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "envelope body must be handled (not retried) as a 502 response, got {outcome:?}",
    );
    assert_envelope_502_on_bridge(&bridge_log, &count, 1).await;
}

#[tokio::test]
async fn glm_chat_passthrough_normal_body_is_forwarded_verbatim() {
    // Happy-path regression for the centralized preflight: a
    // normal Chat body on the Passthrough + non-SSE + GLM Chat +
    // JSON path must reach the client unchanged (HTTP 200, body
    // verbatim) — the preflight is a no-op for OpenAI-shape
    // responses (no `success`/`code`).
    let (addr, count) = spawn_fixed_body_upstream(CHAT_COMPLETIONS_JSON_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Chat);

    let outcome = forward_chat_completions_request(&services, &route, false)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(start.status, 200, "normal Chat body must keep HTTP 200");
    let chunk: Vec<u8> = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.clone()),
            _ => None,
        })
        .next()
        .expect("bridge must receive a response chunk");
    assert_eq!(
        chunk, CHAT_COMPLETIONS_JSON_BODY,
        "normal Chat body must be forwarded verbatim to the client",
    );
}

#[tokio::test]
async fn glm_responses_to_chat_envelope_fails_loudly() {
    // Issue #241 P1: the centralized preflight only
    // covers the Passthrough branch; the ResponsesToChat branch
    // (`POST /v1/chat/completions` + GLM Responses native) gets
    // its own envelope check at the top of
    // `forward_non_stream_responses_to_chat_response`. Without
    // it, the translator mapped a Zhipu envelope's missing
    // `output` to `[]` and synthesized a 200 Chat payload with
    // null content, silently recording an empty success.
    let (addr, count) = spawn_fixed_body_upstream(GLM_ENVELOPE_404_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = glm_test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_chat_completions_request(&services, &route, false)
        .await
        .expect("forward");
    assert!(
        matches!(outcome, ForwardOutcome::Handled),
        "ResponsesToChat envelope must be handled (not retried) as a 502 response, got {outcome:?}",
    );
    assert_envelope_502_on_bridge(&bridge_log, &count, 1).await;
}

async fn spawn_quota_then_success_upstream(
    quota_body: &'static [u8],
) -> (
    SocketAddr,
    Arc<AtomicUsize>,
    Arc<tokio::sync::Mutex<Vec<String>>>,
) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let auths = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let auth_log = auths.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                break;
            };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let auth_log = auth_log.clone();
            tokio::spawn(async move {
                let mut sock = sock;
                let mut buf = [0u8; 8192];
                let read = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..read]);
                let authorization = head
                    .lines()
                    .find_map(|line| line.strip_prefix("authorization: "))
                    .map(|value| value.trim().to_string())
                    .unwrap_or_default();
                auth_log.lock().await.push(authorization);
                if n == 0 {
                    let response = format!(
                        "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        quota_body.len()
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.write_all(quota_body).await;
                } else {
                    let body = RESPONSES_JSON_BODY.as_bytes();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(response.as_bytes()).await;
                    let _ = sock.write_all(body).await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count, auths)
}

fn quota_failover_route(base_url: &str) -> RouteConfig {
    let endpoint_id = uuid::Uuid::new_v4();
    let primary_key_id = uuid::Uuid::new_v4();
    let secondary_key_id = uuid::Uuid::new_v4();
    let key =
        |key_id: uuid::Uuid, label: &str, secret: &str, position: i32| crate::db::EndpointApiKey {
            key_id,
            endpoint_id,
            key_label: label.to_string(),
            api_key: secret.to_string(),
            position,
            enabled: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
    RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: base_url.to_string(),
        api_key: "key-a".to_string(),
        endpoint_key_id: Some(primary_key_id),
        endpoint_key_label: Some("primary".to_string()),
        api_keys: vec![
            key(primary_key_id, "primary", "key-a", 0),
            key(secondary_key_id, "secondary", "key-b", 1),
        ],
        key_lb_enabled: true,
        native_api: NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: crate::db::RouteSelectionReason::Default,
        provider: crate::db::EndpointProvider::Generic,
        service_tier: crate::db::MinimaxServiceTier::Standard,
    }
}

#[tokio::test]
async fn non_stream_quota_exhaustion_retries_with_another_endpoint_key() {
    const QUOTA_BODY: &[u8] = br#"{"error":{"message":"insufficient_quota"}}"#;
    let (addr, count, auths) = spawn_quota_then_success_upstream(QUOTA_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = quota_failover_route(&format!("http://{addr}"));

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 2).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        2,
        "non-stream quota exhaustion must retry once with the alternate key",
    );

    let auths = auths.lock().await;
    assert_eq!(auths.len(), 2, "expected exactly two upstream requests");
    assert_eq!(auths[0], "Bearer key-a");
    assert_eq!(auths[1], "Bearer key-b");

    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(
        start.status, 200,
        "retry must surface the successful response"
    );
}

#[tokio::test]
async fn non_stream_quota_exhaustion_without_an_alternate_key_surfaces_the_error() {
    const QUOTA_BODY: &[u8] = br#"{"error":{"message":"insufficient_quota"}}"#;
    let (addr, count, _auths) = spawn_quota_then_success_upstream(QUOTA_BODY).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let mut route = quota_failover_route(&format!("http://{addr}"));
    route.api_keys.truncate(1);

    let outcome = forward_test_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 1).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "a single key cannot fail over and must not be retried",
    );

    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(
        start.status, 429,
        "quota exhaustion without an alternate key must surface as 429",
    );
}
