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
        responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForcePassthrough,
        chat_reasoning_replay_policy: crate::db::ChatReasoningReplayPolicy::Auto,
        route_selection_reason: crate::db::RouteSelectionReason::Default,
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
    forward_route_request(RouteForwardRequest {
        services,
        request: &request,
        request_ctx: &request_ctx,
        route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    })
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
async fn does_not_retry_adapter_translation_failure() {
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
    assert!(matches!(outcome, ForwardOutcome::Handled));
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
