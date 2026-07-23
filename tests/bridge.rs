use axum::{
    Router,
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use flate2::{Compression, write::GzEncoder};
use futures::{SinkExt, StreamExt};
use prompt_ferry::{
    bridge_wire,
    config::{self, BridgeEncryptionMode, NativeApi, WorkerTlsMode},
    protocol::{
        BridgeMessage, ConfigSnapshot, RelayIpPolicy, ResponseChunk, ResponseEnd, ResponseStart,
    },
    relay::{self, RelayHandle},
    worker,
};
use std::io::Write;
use std::net::SocketAddr;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

const BRIDGE_KEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";
const OTHER_BRIDGE_KEY: &str = "CAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAg=";

#[tokio::test]
async fn worker_serves_multiple_relays_concurrently() {
    let upstream_addr = spawn_upstream().await;
    let (relay_a_addr, worker_a_addr, relay_a_handle) = spawn_relay().await;
    let (relay_b_addr, worker_b_addr, relay_b_handle) = spawn_relay().await;

    let worker_config = config::WorkerConfig {
        relay_urls: vec![
            format!("ws://{worker_a_addr}/ws/worker"),
            format!("ws://{worker_b_addr}/ws/worker"),
        ],
        worker_token: "worker-token".to_string(),
        upstream_base_url: format!("http://{upstream_addr}"),
        upstream_api_key: "upstream-key".to_string(),
        upstream_native_api: NativeApi::Chat,
        connect_timeout_seconds: 5,
        ..config::WorkerConfig::default()
    };

    let mut worker_handle = tokio::spawn(async move { worker::run_embedded(worker_config).await });

    wait_for_worker(&relay_a_handle, &mut worker_handle).await;
    wait_for_worker(&relay_b_handle, &mut worker_handle).await;

    let ((), ()) = tokio::join!(
        assert_streaming_chat(relay_a_addr),
        assert_streaming_chat(relay_b_addr),
    );

    worker_handle.abort();
}

#[tokio::test]
async fn sdk_style_consumer_reads_chat_stream_until_done() {
    let upstream_addr = spawn_upstream().await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;

    let worker_config = worker_config(worker_addr, upstream_addr);

    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "fake",
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let chunks = collect_sse_data_lines(response).await;
    assert_eq!(
        chunks,
        vec!["{\"delta\":\"hello\"}".to_string(), "[DONE]".to_string()]
    );

    let mut assembled = String::new();
    for data in &chunks {
        if data == "[DONE]" {
            break;
        }
        let value: serde_json::Value = serde_json::from_str(data).unwrap();
        if let Some(delta) = value.get("delta").and_then(serde_json::Value::as_str) {
            assembled.push_str(delta);
        }
    }
    assert_eq!(assembled, "hello");

    worker_handle.abort();
}

#[tokio::test]
async fn relays_streaming_chat_completion_through_encrypted_worker() {
    let upstream_addr = spawn_upstream().await;
    let (relay_addr, worker_addr, relay_handle) =
        spawn_relay_with_encryption(BridgeEncryptionMode::Required, BRIDGE_KEY).await;

    let worker_config = config::WorkerConfig {
        bridge_encryption_mode: BridgeEncryptionMode::Required,
        bridge_encryption_key: BRIDGE_KEY.to_string(),
        ..worker_config(worker_addr, upstream_addr)
    };

    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;
    assert_streaming_chat(relay_addr).await;

    worker_handle.abort();
}

#[tokio::test]
async fn relays_large_request_through_encrypted_worker_bridge() {
    let upstream_addr = spawn_upstream().await;
    let (relay_addr, worker_addr, relay_handle) =
        spawn_relay_with_encryption(BridgeEncryptionMode::Required, BRIDGE_KEY).await;

    let worker_config = config::WorkerConfig {
        bridge_encryption_mode: BridgeEncryptionMode::Required,
        bridge_encryption_key: BRIDGE_KEY.to_string(),
        upstream_native_api: NativeApi::Responses,
        ..worker_config(worker_addr, upstream_addr)
    };
    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;
    assert_large_request_round_trip(relay_addr).await;

    worker_handle.abort();
}

#[tokio::test]
async fn relay_accepts_large_openai_compatible_request_bodies() {
    let (relay_addr, _worker_addr, _relay_handle) = spawn_relay().await;
    let client = reqwest::Client::new();
    let large_body = "a".repeat(3_000_000);

    for path in ["/v1/chat/completions", "/v1/responses"] {
        let response = client
            .post(format!("http://{relay_addr}{path}"))
            .bearer_auth("client-token")
            .body(large_body.clone())
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}

#[tokio::test]
async fn relay_accepts_gzip_responses_request_body() {
    let upstream_addr = spawn_upstream().await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;

    let worker_config = config::WorkerConfig {
        upstream_native_api: NativeApi::Responses,
        ..worker_config(worker_addr, upstream_addr)
    };
    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let body = gzip_bytes(
        serde_json::json!({
            "model": "fake",
            "input": "gzip me"
        })
        .to_string()
        .as_bytes(),
    );

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_ENCODING, "gzip")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let text = response.text().await.unwrap();
    assert!(text.contains("data: {\"delta\":\"hello\"}"), "body={text}");
    worker_handle.abort();
}

#[tokio::test]
async fn relay_rejects_unsupported_content_encoding() {
    let (relay_addr, _worker_addr, _relay_handle) = spawn_relay().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_ENCODING, "br")
        .body(r#"{"model":"fake","input":"hi"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn relay_rejects_invalid_gzip_body() {
    let upstream_addr = spawn_upstream().await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = config::WorkerConfig {
        upstream_native_api: NativeApi::Responses,
        ..worker_config(worker_addr, upstream_addr)
    };
    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_ENCODING, "gzip")
        .body("not gzip")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    worker_handle.abort();
}

#[tokio::test]
async fn relay_proxies_admin_api_requests_through_worker_bridge() {
    let (relay_addr, worker_addr, _relay_handle) = spawn_relay().await;
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();

    let bridge_task = tokio::spawn(async move {
        let mut pending_request_id = None;
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Binary(bytes) = message else {
                continue;
            };
            match bridge_wire::decode_message(&bytes).unwrap() {
                BridgeMessage::RequestStart(request) => {
                    assert_eq!(request.method, "GET");
                    assert_eq!(request.path, "/api/v1/auth/me?source=relay");
                    assert!(
                        request
                            .headers
                            .iter()
                            .any(|(name, value)| name == "cookie" && value == "pfy_session=abc")
                    );
                    assert!(request.headers.iter().all(|(name, _)| name != "host"));
                    pending_request_id = Some(request.request_id);
                }
                BridgeMessage::RequestEnd(end) => {
                    let request_id = pending_request_id
                        .take()
                        .expect("request start before request end");
                    assert_eq!(end.request_id, request_id);
                    for message in [
                        BridgeMessage::ResponseStart(ResponseStart {
                            request_id: request_id.clone(),
                            status: StatusCode::UNAUTHORIZED.as_u16(),
                            content_type: Some("application/json".to_string()),
                            headers: vec![
                                (
                                    "set-cookie".to_string(),
                                    "pfy_session=deleted; Path=/; Max-Age=0".to_string(),
                                ),
                                ("cache-control".to_string(), "no-store".to_string()),
                            ],
                        }),
                        BridgeMessage::ResponseChunk(ResponseChunk {
                            request_id: request_id.clone(),
                            data: br#"{"error":"login required"}"#.to_vec(),
                        }),
                        BridgeMessage::ResponseEnd(ResponseEnd { request_id }),
                    ] {
                        socket
                            .send(Message::Binary(
                                bridge_wire::encode_message(&message).unwrap().into(),
                            ))
                            .await
                            .unwrap();
                    }
                    socket.close(None).await.unwrap();
                    return;
                }
                _ => {}
            }
        }
        panic!("relay did not send admin proxy request");
    });

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{relay_addr}/api/v1/auth/me?source=relay"))
        .header(header::COOKIE, "pfy_session=abc")
        .send()
        .await
        .unwrap();

    let status = response.status();
    let response_headers = response.headers().clone();
    let response_text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::UNAUTHORIZED, "body={response_text}");
    assert_eq!(
        response_headers
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok()),
        Some("pfy_session=deleted; Path=/; Max-Age=0")
    );
    assert_eq!(
        response_headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(response_text, r#"{"error":"login required"}"#);
    bridge_task.await.unwrap();
}

#[tokio::test]
async fn relay_proxies_frontend_routes_through_worker_bridge() {
    let (relay_addr, worker_addr, _relay_handle) = spawn_relay().await;
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();

    let bridge_task = tokio::spawn(async move {
        let mut pending_request_id = None;
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Binary(bytes) = message else {
                continue;
            };
            match bridge_wire::decode_message(&bytes).unwrap() {
                BridgeMessage::RequestStart(request) => {
                    assert_eq!(request.method, "GET");
                    assert_eq!(request.path, "/settings/relays");
                    pending_request_id = Some(request.request_id);
                }
                BridgeMessage::RequestEnd(_) => {
                    let request_id = pending_request_id
                        .take()
                        .expect("request start before request end");
                    for message in [
                        BridgeMessage::ResponseStart(ResponseStart {
                            request_id: request_id.clone(),
                            status: StatusCode::OK.as_u16(),
                            content_type: Some("text/html; charset=utf-8".to_string()),
                            headers: vec![(
                                "cache-control".to_string(),
                                "public, max-age=60".to_string(),
                            )],
                        }),
                        BridgeMessage::ResponseChunk(ResponseChunk {
                            request_id: request_id.clone(),
                            data: b"<html>relay admin</html>".to_vec(),
                        }),
                        BridgeMessage::ResponseEnd(ResponseEnd { request_id }),
                    ] {
                        socket
                            .send(Message::Binary(
                                bridge_wire::encode_message(&message).unwrap().into(),
                            ))
                            .await
                            .unwrap();
                    }
                    socket.close(None).await.unwrap();
                    return;
                }
                _ => {}
            }
        }
        panic!("relay did not send frontend proxy request");
    });

    let response = reqwest::get(format!("http://{relay_addr}/settings/relays"))
        .await
        .unwrap();

    let status = response.status();
    let response_headers = response.headers().clone();
    let response_text = response.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "body={response_text}");
    assert_eq!(
        response_headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        response_headers
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("public, max-age=60")
    );
    assert_eq!(response_text, "<html>relay admin</html>");
    bridge_task.await.unwrap();
}

#[tokio::test]
async fn relay_mcp_no_longer_rejects_cross_origin_requests() {
    let (relay_addr, _worker_addr, _relay_handle) = spawn_relay().await;
    let client = reqwest::Client::new();
    let mcp = client
        .post(format!("http://{relay_addr}/mcp"))
        .bearer_auth("client-token")
        .header(header::ORIGIN, "https://example.com")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":"1","method":"initialize","params":{}}))
        .send()
        .await
        .unwrap();

    assert_eq!(mcp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn relay_mcp_initialize_returns_session_header_and_accepts_initialized() {
    let (relay_addr, worker_addr, _relay_handle) = spawn_relay().await;
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();

    let client = reqwest::Client::new();
    let response_task = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Binary(bytes) = message else {
                continue;
            };
            let BridgeMessage::McpRequestStart(request) =
                bridge_wire::decode_message(&bytes).unwrap()
            else {
                continue;
            };
            let mut body = Vec::new();
            while let Some(message) = socket.next().await {
                let message = message.unwrap();
                let Message::Binary(bytes) = message else {
                    continue;
                };
                match bridge_wire::decode_message(&bytes).unwrap() {
                    BridgeMessage::McpRequestChunk(chunk)
                        if chunk.request_id == request.request_id =>
                    {
                        body.extend_from_slice(&chunk.data);
                    }
                    BridgeMessage::McpRequestEnd(end) if end.request_id == request.request_id => {
                        break;
                    }
                    _ => {}
                }
            }
            let request_body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let method = request_body["method"].as_str().unwrap_or_default();
            if method == "initialize" {
                socket
                    .send(Message::Binary(
                        bridge_wire::encode_message(&BridgeMessage::McpResponseStart(
                            prompt_ferry::protocol::McpResponseStart {
                                request_id: request.request_id.clone(),
                                status: 200,
                                content_type: Some("application/json".to_string()),
                                headers: vec![(
                                    "mcp-session-id".to_string(),
                                    "test-session".to_string(),
                                )],
                            },
                        ))
                        .unwrap()
                        .into(),
                    ))
                    .await
                    .unwrap();
                socket
                    .send(Message::Binary(
                        bridge_wire::encode_message(&BridgeMessage::McpResponseChunk(
                            prompt_ferry::protocol::McpResponseChunk {
                                request_id: request.request_id.clone(),
                                data: br#"{"jsonrpc":"2.0","id":"1","result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"test","version":"1.0"}}}"#.to_vec(),
                            },
                        ))
                        .unwrap()
                        .into(),
                    ))
                    .await
                    .unwrap();
                socket
                    .send(Message::Binary(
                        bridge_wire::encode_message(&BridgeMessage::McpResponseEnd(
                            prompt_ferry::protocol::McpResponseEnd {
                                request_id: request.request_id,
                            },
                        ))
                        .unwrap()
                        .into(),
                    ))
                    .await
                    .unwrap();
            } else if method == "notifications/initialized" {
                assert!(
                    request
                        .headers
                        .iter()
                        .any(|(name, value)| name.eq_ignore_ascii_case("mcp-session-id")
                            && value == "test-session")
                );
                socket
                    .send(Message::Binary(
                        bridge_wire::encode_message(&BridgeMessage::McpResponseStart(
                            prompt_ferry::protocol::McpResponseStart {
                                request_id: request.request_id.clone(),
                                status: 202,
                                content_type: Some("application/json".to_string()),
                                headers: Vec::new(),
                            },
                        ))
                        .unwrap()
                        .into(),
                    ))
                    .await
                    .unwrap();
                socket
                    .send(Message::Binary(
                        bridge_wire::encode_message(&BridgeMessage::McpResponseEnd(
                            prompt_ferry::protocol::McpResponseEnd {
                                request_id: request.request_id,
                            },
                        ))
                        .unwrap()
                        .into(),
                    ))
                    .await
                    .unwrap();
                break;
            }
        }
    });

    let initialize = client
        .post(format!("http://{relay_addr}/mcp"))
        .bearer_auth("client-token")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":"1","method":"initialize","params":{}}))
        .send()
        .await
        .unwrap();

    assert_eq!(initialize.status(), StatusCode::OK);
    let session_id = initialize
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap();

    let initialized = client
        .post(format!("http://{relay_addr}/mcp"))
        .bearer_auth("client-token")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header("mcp-session-id", session_id)
        .header("mcp-protocol-version", "2025-11-25")
        .json(&serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .send()
        .await
        .unwrap();

    assert_eq!(initialized.status(), StatusCode::ACCEPTED);
    response_task.await.unwrap();
}

#[tokio::test]
async fn relay_mcp_get_root_streams_sse_through_worker() {
    let (relay_addr, worker_addr, _relay_handle) = spawn_relay().await;
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();

    let response_task = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Binary(bytes) = message else {
                continue;
            };
            let BridgeMessage::McpRequestStart(request) =
                bridge_wire::decode_message(&bytes).unwrap()
            else {
                continue;
            };
            assert_eq!(request.method, "GET");
            assert_eq!(request.path, "/mcp");
            assert!(request.headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("accept") && value.contains("text/event-stream")
            }));
            while let Some(message) = socket.next().await {
                let message = message.unwrap();
                let Message::Binary(bytes) = message else {
                    continue;
                };
                if let BridgeMessage::McpRequestEnd(end) =
                    bridge_wire::decode_message(&bytes).unwrap()
                    && end.request_id == request.request_id
                {
                    break;
                }
            }

            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseStart(
                        prompt_ferry::protocol::McpResponseStart {
                            request_id: request.request_id.clone(),
                            status: 200,
                            content_type: Some("text/event-stream".to_string()),
                            headers: vec![("cache-control".to_string(), "no-cache".to_string())],
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseChunk(
                        prompt_ferry::protocol::McpResponseChunk {
                            request_id: request.request_id.clone(),
                            data: b"data: {\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n\n".to_vec(),
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseEnd(
                        prompt_ferry::protocol::McpResponseEnd {
                            request_id: request.request_id,
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            break;
        }
    });

    let response = reqwest::Client::new()
        .get(format!("http://{relay_addr}/mcp"))
        .bearer_auth("client-token")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", "test-session")
        .header("mcp-protocol-version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let body = response.text().await.unwrap();
    assert!(body.contains(r#"data: {"jsonrpc":"2.0","method":"ping"}"#));
    response_task.await.unwrap();
}

#[tokio::test]
async fn relay_mcp_get_server_streams_sse_and_preserves_external_path() {
    let (relay_addr, worker_addr, _relay_handle) = spawn_relay().await;
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();

    let response_task = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Binary(bytes) = message else {
                continue;
            };
            let BridgeMessage::McpRequestStart(request) =
                bridge_wire::decode_message(&bytes).unwrap()
            else {
                continue;
            };
            assert_eq!(request.method, "GET");
            assert_eq!(request.path, "/mcp/cloudiful");
            assert_eq!(request.server_name.as_deref(), Some("cloudiful"));
            while let Some(message) = socket.next().await {
                let message = message.unwrap();
                let Message::Binary(bytes) = message else {
                    continue;
                };
                if let BridgeMessage::McpRequestEnd(end) =
                    bridge_wire::decode_message(&bytes).unwrap()
                    && end.request_id == request.request_id
                {
                    break;
                }
            }

            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseStart(
                        prompt_ferry::protocol::McpResponseStart {
                            request_id: request.request_id.clone(),
                            status: 200,
                            content_type: Some("text/event-stream".to_string()),
                            headers: Vec::new(),
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseChunk(
                        prompt_ferry::protocol::McpResponseChunk {
                            request_id: request.request_id.clone(),
                            data: b"data: {\"jsonrpc\":\"2.0\",\"method\":\"server/scope\"}\n\n"
                                .to_vec(),
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseEnd(
                        prompt_ferry::protocol::McpResponseEnd {
                            request_id: request.request_id,
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            break;
        }
    });

    let response = reqwest::Client::new()
        .get(format!("http://{relay_addr}/mcp/cloudiful"))
        .bearer_auth("client-token")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", "test-session")
        .header("mcp-protocol-version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains(r#"data: {"jsonrpc":"2.0","method":"server/scope"}"#));
    response_task.await.unwrap();
}

#[tokio::test]
async fn relay_mcp_sse_stream_does_not_inject_json_error_after_start() {
    let (relay_addr, worker_addr, _relay_handle) = spawn_relay().await;
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();

    let response_task = tokio::spawn(async move {
        while let Some(message) = socket.next().await {
            let message = message.unwrap();
            let Message::Binary(bytes) = message else {
                continue;
            };
            let BridgeMessage::McpRequestStart(request) =
                bridge_wire::decode_message(&bytes).unwrap()
            else {
                continue;
            };
            while let Some(message) = socket.next().await {
                let message = message.unwrap();
                let Message::Binary(bytes) = message else {
                    continue;
                };
                if let BridgeMessage::McpRequestEnd(end) =
                    bridge_wire::decode_message(&bytes).unwrap()
                    && end.request_id == request.request_id
                {
                    break;
                }
            }

            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseStart(
                        prompt_ferry::protocol::McpResponseStart {
                            request_id: request.request_id.clone(),
                            status: 200,
                            content_type: Some("text/event-stream".to_string()),
                            headers: Vec::new(),
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::McpResponseChunk(
                        prompt_ferry::protocol::McpResponseChunk {
                            request_id: request.request_id.clone(),
                            data: b"data: {\"jsonrpc\":\"2.0\",\"method\":\"first\"}\n\n".to_vec(),
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Binary(
                    bridge_wire::encode_message(&BridgeMessage::ResponseError(
                        prompt_ferry::protocol::ResponseError {
                            request_id: request.request_id,
                            status: 502,
                            code: "mcp_stream_error".to_string(),
                            message: "broken stream".to_string(),
                        },
                    ))
                    .unwrap()
                    .into(),
                ))
                .await
                .unwrap();
            break;
        }
    });

    let response = reqwest::Client::new()
        .get(format!("http://{relay_addr}/mcp"))
        .bearer_auth("client-token")
        .header(header::ACCEPT, "text/event-stream")
        .header("mcp-session-id", "test-session")
        .header("mcp-protocol-version", "2025-11-25")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains(r#"data: {"jsonrpc":"2.0","method":"first"}"#));
    assert!(!body.contains("mcp_stream_error"));
    assert!(!body.contains(r#""error""#));
    response_task.await.unwrap();
}

#[tokio::test]
async fn encrypted_relay_rejects_unencrypted_worker() {
    let upstream_addr = spawn_upstream().await;
    let (_relay_addr, worker_addr, relay_handle) =
        spawn_relay_with_encryption(BridgeEncryptionMode::Required, BRIDGE_KEY).await;

    let worker_config = worker_config(worker_addr, upstream_addr);
    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    assert_worker_not_registered(&relay_handle, &mut worker_handle).await;
    worker_handle.abort();
}

#[tokio::test]
async fn encrypted_relay_rejects_worker_with_wrong_key() {
    let upstream_addr = spawn_upstream().await;
    let (_relay_addr, worker_addr, relay_handle) =
        spawn_relay_with_encryption(BridgeEncryptionMode::Required, BRIDGE_KEY).await;

    let worker_config = config::WorkerConfig {
        bridge_encryption_mode: BridgeEncryptionMode::Required,
        bridge_encryption_key: OTHER_BRIDGE_KEY.to_string(),
        ..worker_config(worker_addr, upstream_addr)
    };
    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    assert_worker_not_registered(&relay_handle, &mut worker_handle).await;
    worker_handle.abort();
}

async fn spawn_relay() -> (SocketAddr, SocketAddr, RelayHandle) {
    spawn_relay_with_encryption(BridgeEncryptionMode::Off, "").await
}

async fn spawn_relay_with_heartbeat_timeout(
    worker_heartbeat_timeout_seconds: u64,
) -> (SocketAddr, SocketAddr, RelayHandle) {
    let public_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_addr = public_listener.local_addr().unwrap();
    let worker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let worker_addr = worker_listener.local_addr().unwrap();
    let (public_app, worker_app, handle) = relay::apps(config::RelayConfig {
        bind: public_addr.to_string(),
        worker_bind: worker_addr.to_string(),
        client_token: "client-token".to_string(),
        worker_token: "worker-token".to_string(),
        request_timeout_seconds: 5,
        worker_heartbeat_timeout_seconds,
        ..config::RelayConfig::default()
    });
    tokio::spawn(async move {
        axum::serve(
            public_listener,
            public_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await
        .unwrap();
    });
    tokio::spawn(async move {
        axum::serve(
            worker_listener,
            worker_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await
        .unwrap();
    });
    (public_addr, worker_addr, handle)
}

async fn spawn_relay_with_encryption(
    bridge_encryption_mode: BridgeEncryptionMode,
    bridge_encryption_key: &str,
) -> (SocketAddr, SocketAddr, RelayHandle) {
    let public_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_addr = public_listener.local_addr().unwrap();
    let worker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let worker_addr = worker_listener.local_addr().unwrap();
    let (public_app, worker_app, handle) = relay::apps(config::RelayConfig {
        bind: public_addr.to_string(),
        worker_bind: worker_addr.to_string(),
        client_token: "client-token".to_string(),
        worker_token: "worker-token".to_string(),
        request_timeout_seconds: 5,
        worker_heartbeat_timeout_seconds: 90,
        bridge_encryption_mode,
        bridge_encryption_key: bridge_encryption_key.to_string(),
        ..config::RelayConfig::default()
    });
    tokio::spawn(async move {
        axum::serve(
            public_listener,
            public_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await
        .unwrap();
    });
    tokio::spawn(async move {
        axum::serve(
            worker_listener,
            worker_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await
        .unwrap();
    });
    (public_addr, worker_addr, handle)
}

fn worker_config(worker_addr: SocketAddr, upstream_addr: SocketAddr) -> config::WorkerConfig {
    config::WorkerConfig {
        relay_urls: vec![format!("ws://{worker_addr}/ws/worker")],
        worker_token: "worker-token".to_string(),
        upstream_base_url: format!("http://{upstream_addr}"),
        upstream_api_key: "upstream-key".to_string(),
        upstream_native_api: NativeApi::Chat,
        connect_timeout_seconds: 5,
        ..config::WorkerConfig::default()
    }
}

async fn assert_streaming_chat(relay_addr: SocketAddr) {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "fake",
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains("data: {\"delta\":\"hello\"}"));
    assert!(body.contains("data: [DONE]"));
}

async fn assert_large_request_round_trip(relay_addr: SocketAddr) {
    const LARGE_INPUT_BYTES: usize = 17 * 1024 * 1024;

    let client = reqwest::Client::new();
    let body = format!(
        r#"{{"model":"fake","input":"{}"}}"#,
        "a".repeat(LARGE_INPUT_BYTES)
    );
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains("data: {\"delta\":\"hello\"}"), "body={body}");
    assert!(body.contains("data: [DONE]"), "body={body}");
}

fn gzip_bytes(input: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input).unwrap();
    encoder.finish().unwrap()
}

async fn assert_streaming_chat_stops_after_done(relay_addr: SocketAddr) {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "fake",
            "stream": true,
            "messages": [{"role": "user", "content": "stop after done"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains("data: [DONE]"));
    assert!(!body.contains("\"cost\":\"0\""));
}

async fn assert_worker_not_registered(
    handle: &RelayHandle,
    worker_handle: &mut tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    for _ in 0..10 {
        assert_eq!(handle.worker_count().await, 0);
        if worker_handle.is_finished() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(handle.worker_count().await, 0);
}

async fn spawn_upstream() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_stream))
        .route("/v1/responses", post(fake_stream))
        .route("/v1/models", get(fake_models));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn fake_stream(request: Request) -> Response {
    let _ = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap();
    let chunks = vec![
        Ok::<bytes::Bytes, std::io::Error>(bytes::Bytes::from_static(
            b"data: {\"delta\":\"hello\"}\n\n",
        )),
        Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n")),
        Ok(bytes::Bytes::from_static(
            b"data: {\"choices\":[],\"cost\":\"0\"}\n\n",
        )),
    ];
    let stream = futures::stream::iter(chunks);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

#[tokio::test]
async fn relay_stops_streaming_chat_after_done_marker() {
    let upstream_addr = spawn_upstream().await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;

    let worker_config = worker_config(worker_addr, upstream_addr);

    let mut worker_handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        worker::connect_for_test(worker_config, client).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;
    assert_streaming_chat_stops_after_done(relay_addr).await;

    worker_handle.abort();
}

async fn fake_models() -> Response {
    Response::new(Body::from(r#"{"object":"list","data":[]}"#))
}

async fn collect_sse_data_lines(response: reqwest::Response) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = pending.find('\n') {
            let line = pending[..index].trim_end_matches('\r').to_string();
            pending.drain(..=index);
            if let Some(data) = line.strip_prefix("data: ") {
                lines.push(data.to_string());
                if data == "[DONE]" {
                    return lines;
                }
            }
        }
    }
    lines
}

async fn wait_for_worker(
    handle: &RelayHandle,
    worker_handle: &mut tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    for _ in 0..20 {
        if handle.worker_count().await > 0 {
            return;
        }
        tokio::select! {
            result = &mut *worker_handle => panic!("worker exited before connecting: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }
    panic!("worker did not connect");
}

async fn wait_for_worker_count(handle: &RelayHandle, expected: usize) {
    for _ in 0..40 {
        if handle.worker_count().await == expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(handle.worker_count().await, expected);
}

#[tokio::test]
async fn relay_evicts_stale_worker_after_heartbeat_timeout() {
    let (_relay_addr, worker_addr, handle) = spawn_relay_with_heartbeat_timeout(1).await;

    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (first_socket, _) = connect_async(request).await.unwrap();
    wait_for_worker_count(&handle, 1).await;
    wait_for_worker_count(&handle, 0).await;

    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (second_socket, _) = connect_async(request).await.unwrap();
    wait_for_worker_count(&handle, 1).await;

    drop(first_socket);
    drop(second_socket);
}

#[tokio::test]
async fn relay_allows_all_clients_when_whitelist_is_empty() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(worker_addr, &handle, 1, RelayIpPolicy::default()).await;

    let response = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn relay_allows_direct_ip_matching_single_ip_rule() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(
        worker_addr,
        &handle,
        1,
        RelayIpPolicy {
            allowed_cidrs: vec!["127.0.0.1".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;

    let response = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn relay_allows_direct_ip_matching_cidr_rule() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(
        worker_addr,
        &handle,
        1,
        RelayIpPolicy {
            allowed_cidrs: vec!["127.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;

    let response = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn relay_denies_direct_ip_outside_whitelist() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(
        worker_addr,
        &handle,
        1,
        RelayIpPolicy {
            allowed_cidrs: vec!["10.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;

    let response = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn relay_denies_spoofed_forwarded_for_when_peer_is_not_trusted() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(
        worker_addr,
        &handle,
        1,
        RelayIpPolicy {
            allowed_cidrs: vec!["10.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{relay_addr}/healthz"))
        .header("x-forwarded-for", "10.0.0.5")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn relay_honors_forwarded_for_when_peer_is_trusted() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(
        worker_addr,
        &handle,
        1,
        RelayIpPolicy {
            allowed_cidrs: vec!["10.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec!["127.0.0.0/8".to_string()],
        },
    )
    .await;

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{relay_addr}/healthz"))
        .header("x-forwarded-for", "10.1.2.3")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn relay_applies_ip_policy_to_public_routes_but_not_worker_ws() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(
        worker_addr,
        &handle,
        1,
        RelayIpPolicy {
            allowed_cidrs: vec!["10.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;

    let healthz = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(healthz.status(), StatusCode::FORBIDDEN);

    let client = reqwest::Client::new();
    let models = client
        .get(format!("http://{relay_addr}/v1/models"))
        .bearer_auth("client-token")
        .send()
        .await
        .unwrap();
    assert_eq!(models.status(), StatusCode::FORBIDDEN);

    let mcp = client
        .post(format!("http://{relay_addr}/mcp"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":"1","method":"initialize","params":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(mcp.status(), StatusCode::FORBIDDEN);

    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (socket, _) = connect_async(request).await.unwrap();
    assert_eq!(handle.worker_count().await, 1);
    drop(socket);
}

#[tokio::test]
async fn relay_applies_snapshot_updates_without_restart() {
    let (relay_addr, worker_addr, handle) = spawn_relay().await;
    push_snapshot(worker_addr, &handle, 1, RelayIpPolicy::default()).await;

    let allowed = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(allowed.status(), StatusCode::OK);

    push_snapshot(
        worker_addr,
        &handle,
        2,
        RelayIpPolicy {
            allowed_cidrs: vec!["10.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;
    let denied = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    push_snapshot(
        worker_addr,
        &handle,
        3,
        RelayIpPolicy {
            allowed_cidrs: vec!["127.0.0.0/8".to_string()],
            trusted_proxy_cidrs: vec![],
        },
    )
    .await;
    let reallowed = reqwest::get(format!("http://{relay_addr}/healthz"))
        .await
        .unwrap();
    assert_eq!(reallowed.status(), StatusCode::OK);
}

async fn push_snapshot(
    worker_addr: SocketAddr,
    handle: &RelayHandle,
    version: i64,
    relay_ip_policy: RelayIpPolicy,
) {
    let mut request = format!("ws://{worker_addr}/ws/worker")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer worker-token"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();
    socket
        .send(Message::Binary(
            bridge_wire::encode_message(&BridgeMessage::ConfigSnapshot(ConfigSnapshot {
                version,
                keys: vec![],
                relay_ip_policy,
            }))
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    wait_for_config(handle, version).await;
    socket.close(None).await.unwrap();
}

async fn wait_for_config(handle: &RelayHandle, version: i64) {
    for _ in 0..20 {
        if handle.config_version().await == Some(version) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("relay did not apply config snapshot {version}");
}

#[test]
fn validates_worker_tls_mode_url_mismatch() {
    let config = config::WorkerConfig {
        tls_mode: WorkerTlsMode::Server,
        relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
        ..config::WorkerConfig::default()
    };

    let err = prompt_ferry::tls::validate_worker_config(&config).unwrap_err();
    assert!(err.to_string().contains("wss://"));
}

#[test]
fn validates_worker_server_tls_allows_native_roots() {
    let config = config::WorkerConfig {
        tls_mode: WorkerTlsMode::Server,
        relay_urls: vec!["wss://bridge.internal:8788/ws/worker".to_string()],
        relay_ca: String::new(),
        ..config::WorkerConfig::default()
    };

    prompt_ferry::tls::validate_worker_config(&config).unwrap();
}

#[test]
fn validates_worker_mtls_allows_native_roots() {
    let config = config::WorkerConfig {
        tls_mode: WorkerTlsMode::Mtls,
        relay_urls: vec!["wss://bridge.internal:8788/ws/worker".to_string()],
        relay_ca: String::new(),
        client_cert: "worker.crt".to_string(),
        client_key: "worker.key".to_string(),
        ..config::WorkerConfig::default()
    };

    prompt_ferry::tls::validate_worker_config(&config).unwrap();
}

#[test]
fn auto_worker_tls_mode_follows_relay_url_scheme() {
    let wss_config = config::WorkerConfig {
        relay_urls: vec!["wss://bridge.internal:8788/ws/worker".to_string()],
        ..config::WorkerConfig::default()
    };
    assert_eq!(
        prompt_ferry::tls::worker_tls_mode(&wss_config, &wss_config.relay_urls[0]).unwrap(),
        config::TlsMode::Server
    );
    prompt_ferry::tls::validate_worker_config(&wss_config).unwrap();

    let ws_config = config::WorkerConfig {
        relay_urls: vec!["ws://bridge.internal:8788/ws/worker".to_string()],
        ..config::WorkerConfig::default()
    };
    assert_eq!(
        prompt_ferry::tls::worker_tls_mode(&ws_config, &ws_config.relay_urls[0]).unwrap(),
        config::TlsMode::Off
    );
    prompt_ferry::tls::validate_worker_config(&ws_config).unwrap();
}

#[test]
fn explicit_worker_tls_mode_overrides_relay_url_scheme() {
    let config = config::WorkerConfig {
        tls_mode: WorkerTlsMode::Off,
        relay_urls: vec!["wss://bridge.internal:8788/ws/worker".to_string()],
        ..config::WorkerConfig::default()
    };

    let err = prompt_ferry::tls::validate_worker_config(&config).unwrap_err();
    assert!(err.to_string().contains("ws://"));
}

#[test]
fn duplicate_relay_urls_fail_worker_validation() {
    let config = config::WorkerConfig {
        relay_urls: vec![
            "ws://127.0.0.1:8788/ws/worker".to_string(),
            " ws://127.0.0.1:8788/ws/worker/ ".to_string(),
        ],
        upstream_api_key: "upstream-key".to_string(),
        ..config::WorkerConfig::default()
    };

    let err = worker::run_embedded(config);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let err = runtime.block_on(err).unwrap_err();
    assert!(err.to_string().contains("unique"));
}

#[test]
fn validates_relay_mtls_requires_client_ca() {
    let config = config::RelayConfig {
        tls_mode: config::TlsMode::Mtls,
        tls_cert: "relay.crt".to_string(),
        tls_key: "relay.key".to_string(),
        ..config::RelayConfig::default()
    };

    let err = prompt_ferry::tls::validate_relay_config(&config).unwrap_err();
    assert!(err.to_string().contains("tls_client_ca"));
}

#[test]
fn validates_relay_worker_mtls_requires_client_ca() {
    let config = config::RelayConfig {
        worker_tls_mode: config::TlsMode::Mtls,
        worker_tls_cert: "relay-worker.crt".to_string(),
        worker_tls_key: "relay-worker.key".to_string(),
        ..config::RelayConfig::default()
    };

    let err = prompt_ferry::tls::validate_relay_worker_config(&config).unwrap_err();
    assert!(err.to_string().contains("worker_tls_client_ca"));
}
