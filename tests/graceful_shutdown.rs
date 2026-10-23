//! Graceful-shutdown integration tests.
//!
//! These spawn the real `prompt-ferry serve` binary and send it SIGTERM while a
//! loopback upstream holds an accepted round open. The signal therefore reaches
//! the production relay and worker paths, including the bridge session and the
//! ordered writer flush that the in-process bridge fixtures cannot exercise.
//!
//! Every release barrier waits for the production `worker shutdown admission
//! closed` log line: kill delivery alone does not prove the signal handler ran.

#[path = "support/graceful_shutdown.rs"]
mod support;

use std::{
    io,
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures::StreamExt;
use reqwest::StatusCode;
use support::{CLIENT_TOKEN, Server, Upstream, UpstreamMode};
use tokio::sync::mpsc;

const CHAT_URL: &str = "/v1/chat/completions";
const MCP_URL: &str = "/mcp";
const ADMISSION_CLOSED: &str = "worker shutdown admission closed";
const NORMAL_DRAIN_SECONDS: u64 = 5;
const NORMAL_RELAY_DRAIN_SECONDS: u64 = 7;

fn chat_body(model: &str, stream: bool) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "stream": stream,
        "messages": [{"role": "user", "content": "graceful shutdown"}],
    })
}

async fn ready_client(server: &Server) -> reqwest::Client {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("build client");
    let url = format!("http://{}{CHAT_URL}", server.public_addr);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let probe = client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .json(&chat_body(UpstreamMode::Immediate.header_value(), true))
            .send()
            .await;
        if let Ok(response) = probe
            && response.status() == StatusCode::OK
        {
            let _ = response.text().await;
            return client;
        }
        if Instant::now() >= deadline {
            panic!(
                "relay/worker did not become ready; logs:\n{}",
                server.logs().join("\n")
            );
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn wait_admitted(upstream: &Upstream, server: &Server) {
    if tokio::time::timeout(Duration::from_secs(20), upstream.wait_admitted())
        .await
        .is_err()
    {
        panic!(
            "gated request never reached the upstream (hits={}); logs:\n{}",
            upstream.hits(),
            server.logs().join("\n")
        );
    }
}

/// Send SIGTERM and wait for the worker's own admission-closed log line before
/// any barrier is released, so a response can never complete before the signal
/// was actually processed.
async fn signal_and_observe_shutdown(server: &Server) {
    server.signal_term();
    server
        .wait_for_log(ADMISSION_CLOSED, Duration::from_secs(5))
        .await;
}

#[tokio::test]
async fn active_sse_round_completes_after_sigterm() {
    let upstream = Upstream::start().await;
    let mut server =
        Server::start(&upstream, NORMAL_DRAIN_SECONDS, NORMAL_RELAY_DRAIN_SECONDS).await;
    let client = ready_client(&server).await;
    let url = format!("http://{}{CHAT_URL}", server.public_addr);

    let request = tokio::spawn(async move {
        client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .json(&chat_body(
                UpstreamMode::StreamingGated.header_value(),
                true,
            ))
            .send()
            .await
    });

    let response = tokio::time::timeout(Duration::from_secs(10), request)
        .await
        .expect("response headers arrive")
        .expect("request task ran")
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream();
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("first SSE event arrives")
        .expect("SSE stream open")
        .expect("first SSE event reads");
    let mut body = String::from_utf8_lossy(&first).into_owned();
    assert!(body.contains("\"hello\""), "body={body}");

    signal_and_observe_shutdown(&server).await;
    upstream.release();

    while !body.contains("[DONE]") {
        let next = tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .expect("terminal SSE event arrives after shutdown")
            .expect("SSE stream open")
            .expect("terminal SSE event reads");
        body.push_str(&String::from_utf8_lossy(&next));
    }
    assert!(body.contains("\"hello\""), "body={body}");
    assert!(body.contains("data: [DONE]"), "body={body}");

    let status = server.wait_exit(Duration::from_secs(10)).await;
    assert!(status.success(), "shutdown exit status: {status}");
    assert!(
        !server.log_contains("drain deadline reached"),
        "an accepted SSE round must drain, not hit the deadline"
    );
}

#[tokio::test]
async fn delayed_headers_non_stream_round_completes_after_sigterm() {
    let upstream = Upstream::start().await;
    let mut server =
        Server::start(&upstream, NORMAL_DRAIN_SECONDS, NORMAL_RELAY_DRAIN_SECONDS).await;
    let client = ready_client(&server).await;
    let url = format!("http://{}{CHAT_URL}", server.public_addr);

    let request = tokio::spawn(async move {
        client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .json(&chat_body(UpstreamMode::GatedJson.header_value(), false))
            .send()
            .await
    });

    wait_admitted(&upstream, &server).await;
    signal_and_observe_shutdown(&server).await;
    upstream.release();

    let response = tokio::time::timeout(Duration::from_secs(10), request)
        .await
        .expect("response headers arrive")
        .expect("request task ran")
        .expect("request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("read response body");
    assert!(body.contains("\"response-1\""), "body={body}");

    let status = server.wait_exit(Duration::from_secs(10)).await;
    assert!(status.success(), "shutdown exit status: {status}");
    assert!(!server.log_contains("drain deadline reached"));
}

#[tokio::test]
async fn request_body_admitted_before_sigterm_can_finish_afterwards() {
    let upstream = Upstream::start().await;
    let mut server =
        Server::start(&upstream, NORMAL_DRAIN_SECONDS, NORMAL_RELAY_DRAIN_SECONDS).await;
    let client = ready_client(&server).await;
    let url = format!("http://{}{CHAT_URL}", server.public_addr);
    let hits_before = upstream.hits();

    let (body_tx, body_rx) = mpsc::channel::<Result<Bytes, io::Error>>(4);
    let mut body_rx = body_rx;
    let body_stream = futures::stream::poll_fn(move |cx| body_rx.poll_recv(cx));
    let request = tokio::spawn(async move {
        client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .body(reqwest::Body::wrap_stream(body_stream))
            .send()
            .await
    });

    body_tx
        .send(Ok(Bytes::from_static(
            b"{\"model\":\"gated-json\",\"input\":\"",
        )))
        .await
        .expect("send first body chunk");
    // Let the relay forward the admitted RequestStart before the signal; the
    // body itself is still incomplete, so completion cannot happen early.
    tokio::time::sleep(Duration::from_millis(300)).await;
    signal_and_observe_shutdown(&server).await;
    upstream.release();
    body_tx
        .send(Ok(Bytes::from_static(b"finished-after-sigterm\"}")))
        .await
        .expect("send rest of body");
    drop(body_tx);

    let response = tokio::time::timeout(Duration::from_secs(10), request)
        .await
        .expect("request completes")
        .expect("request task ran")
        .expect("admitted request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(upstream.hits(), hits_before + 1, "upstream was contacted");
    let _ = response.text().await;

    let status = server.wait_exit(Duration::from_secs(10)).await;
    assert!(status.success(), "shutdown exit status: {status}");
}

#[tokio::test]
async fn client_cancel_during_drain_releases_the_guard_without_upstream_completion() {
    let upstream = Upstream::start().await;
    let mut server =
        Server::start(&upstream, NORMAL_DRAIN_SECONDS, NORMAL_RELAY_DRAIN_SECONDS).await;
    let client = ready_client(&server).await;
    let url = format!("http://{}{CHAT_URL}", server.public_addr);

    let request = tokio::spawn(async move {
        client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .json(&chat_body(
                UpstreamMode::StreamingGated.header_value(),
                true,
            ))
            .send()
            .await
    });
    wait_admitted(&upstream, &server).await;
    signal_and_observe_shutdown(&server).await;
    // The upstream stays blocked for the rest of the test: only the client
    // cancellation may release the worker's drain guard.
    request.abort();
    let _ = request.await;

    let status = server.wait_exit(Duration::from_secs(8)).await;
    assert!(status.success(), "shutdown exit status: {status}");
    assert!(
        !server.log_contains("drain deadline reached"),
        "a cancelled request must release its drain guard; logs:\n{}",
        server.logs().join("\n")
    );
}

#[tokio::test]
async fn zero_work_shutdown_exits_promptly() {
    let upstream = Upstream::start().await;
    let mut server =
        Server::start(&upstream, NORMAL_DRAIN_SECONDS, NORMAL_RELAY_DRAIN_SECONDS).await;
    let _client = ready_client(&server).await;

    let started = Instant::now();
    signal_and_observe_shutdown(&server).await;
    let status = server.wait_exit(Duration::from_secs(5)).await;
    assert!(status.success(), "shutdown exit status: {status}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "zero-work shutdown took {:?}",
        started.elapsed()
    );
    assert!(!server.log_contains("drain deadline reached"));
}

#[tokio::test]
async fn never_ending_upstream_hits_the_bounded_drain_deadline() {
    let upstream = Upstream::start().await;
    let mut server = Server::start(&upstream, 1, 3).await;
    let client = ready_client(&server).await;
    let url = format!("http://{}{CHAT_URL}", server.public_addr);

    let request = tokio::spawn(async move {
        client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .json(&chat_body(UpstreamMode::NeverEnding.header_value(), true))
            .send()
            .await
    });
    wait_admitted(&upstream, &server).await;
    signal_and_observe_shutdown(&server).await;

    let started = Instant::now();
    let status = server.wait_exit(Duration::from_secs(8)).await;
    assert!(status.success(), "shutdown exit status: {status}");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "forced shutdown took {:?}",
        started.elapsed()
    );
    assert!(
        server.log_contains("worker shutdown drain deadline reached"),
        "deadline should be logged distinctly; logs:\n{}",
        server.logs().join("\n")
    );
    request.abort();
    let _ = request.await;
}

#[tokio::test]
async fn admitted_mcp_round_completes_after_sigterm() {
    let upstream = Upstream::start().await;
    let mut server =
        Server::start(&upstream, NORMAL_DRAIN_SECONDS, NORMAL_RELAY_DRAIN_SECONDS).await;
    let client = ready_client(&server).await;
    let url = format!("http://{}{MCP_URL}", server.public_addr);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "graceful-shutdown", "version": "1.0"}
        }
    })
    .to_string();
    let split = body.len() / 2;
    let (first, rest) = body.split_at(split);
    let first = first.to_string();
    let rest = rest.to_string();

    let (body_tx, body_rx) = mpsc::channel::<Result<Bytes, io::Error>>(4);
    let mut body_rx = body_rx;
    let body_stream = futures::stream::poll_fn(move |cx| body_rx.poll_recv(cx));
    let request = tokio::spawn(async move {
        client
            .post(&url)
            .bearer_auth(CLIENT_TOKEN)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .body(reqwest::Body::wrap_stream(body_stream))
            .send()
            .await
    });

    body_tx
        .send(Ok(Bytes::from(first)))
        .await
        .expect("send first MCP body chunk");
    // Let the relay forward the admitted MCP RequestStart before the signal;
    // the JSON-RPC body is still incomplete at this point.
    tokio::time::sleep(Duration::from_millis(300)).await;
    signal_and_observe_shutdown(&server).await;
    body_tx
        .send(Ok(Bytes::from(rest)))
        .await
        .expect("send rest of MCP body");
    drop(body_tx);

    let response = tokio::time::timeout(Duration::from_secs(10), request)
        .await
        .expect("MCP response arrives")
        .expect("request task ran")
        .expect("admitted MCP request succeeds");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("read MCP response body");
    assert!(body.contains("\"jsonrpc\""), "body={body}");
    assert!(
        body.contains("\"result\"") || body.contains("serverInfo"),
        "body={body}"
    );

    let status = server.wait_exit(Duration::from_secs(10)).await;
    assert!(status.success(), "shutdown exit status: {status}");
    assert!(!server.log_contains("drain deadline reached"));
}
