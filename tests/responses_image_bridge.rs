use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::Response,
    routing::{get, post},
};
use prompt_ferry::{
    config::{self, NativeApi},
    relay::{self, RelayHandle},
    worker,
};
use serde_json::Value;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct UpstreamState {
    bodies: Arc<Mutex<Vec<Value>>>,
    reject_images: bool,
}

#[tokio::test]
async fn forwards_responses_images_to_standard_chat_image_url_parts() {
    let state = UpstreamState::default();
    let upstream_addr = spawn_upstream(state.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "vision-test",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "describe this chart"},
                    {"type": "input_image", "image_url": {
                        "url": "data:image/png;base64,AA==",
                        "detail": "high"
                    }}
                ]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["output_text"].as_str(), Some("image accepted"));

    let requests = state.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    let content = requests[0]["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"].as_str(), Some("text"));
    assert_eq!(content[1]["type"].as_str(), Some("image_url"));
    assert_eq!(
        content[1]["image_url"]["url"].as_str(),
        Some("data:image/png;base64,AA==")
    );
    assert_eq!(content[1]["image_url"]["detail"].as_str(), Some("high"));

    worker_handle.abort();
}

#[tokio::test]
async fn preserves_text_only_provider_error_for_image_requests() {
    let state = UpstreamState {
        reject_images: true,
        ..Default::default()
    };
    let upstream_addr = spawn_upstream(state).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "text-only-test",
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "https://example.com/chart.png"
                }]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("unknown variant `image_url`"), "body={body}");
    assert!(body.contains("expected `text`"), "body={body}");

    worker_handle.abort();
}

#[tokio::test]
async fn preserves_input_image_parts_for_native_responses_upstream() {
    let state = UpstreamState::default();
    let upstream_addr = spawn_upstream(state.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Responses);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "vision-test",
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_image",
                    "image_url": "https://example.com/chart.png"
                }]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let requests = state.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["input"][0]["content"][0]["type"].as_str(),
        Some("input_image")
    );
    assert_eq!(
        requests[0]["input"][0]["content"][0]["image_url"].as_str(),
        Some("https://example.com/chart.png")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn auto_endpoint_routes_chat_images_to_chat_upstream() {
    let state = UpstreamState::default();
    let upstream_addr = spawn_upstream(state.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Auto);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;
    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "vision-test",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AA=="}}
            ]}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        state.bodies.lock().await[0]["messages"][0]["content"][1]["type"],
        "image_url"
    );
    worker_handle.abort();
}

#[tokio::test]
async fn auto_endpoint_routes_responses_images_to_responses_upstream() {
    let state = UpstreamState::default();
    let upstream_addr = spawn_upstream(state.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Auto);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;
    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "vision-test",
            "input": [{"role": "user", "content": [
                {"type": "input_image", "image_url": "https://example.com/chart.png"}
            ]}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        state.bodies.lock().await[0]["input"][0]["content"][0]["type"],
        "input_image"
    );
    worker_handle.abort();
}

#[tokio::test]
async fn responses_endpoint_returns_chat_shape_for_chat_image_requests() {
    let state = UpstreamState::default();
    let upstream_addr = spawn_upstream(state.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Responses);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;
    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "vision-test",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "describe"},
                {"type": "image_url", "image_url": "https://example.com/chart.png"}
            ]}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["object"], "chat.completion");
    assert_eq!(body["choices"][0]["message"]["content"], "image accepted");
    assert_eq!(
        state.bodies.lock().await[0]["input"][0]["content"][1]["type"],
        "input_image"
    );
    worker_handle.abort();
}

async fn spawn_upstream(state: UpstreamState) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_chat_completion))
        .route("/v1/responses", post(fake_responses_completion))
        .route("/v1/models", get(fake_models))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn fake_chat_completion(State(state): State<UpstreamState>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    state.bodies.lock().await.push(value.clone());
    if state.reject_images && contains_image_url(&value) {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"message":"Failed to deserialize the JSON body into the target type: messages[0]: unknown variant `image_url`, expected `text`"}"#,
            ))
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"id":"chatcmpl_image","created":123,"model":"vision-test","choices":[{"message":{"content":"image accepted"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#,
        ))
        .unwrap()
}

async fn fake_responses_completion(State(state): State<UpstreamState>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    state.bodies.lock().await.push(value);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"id":"resp_image","object":"response","created_at":123,"status":"completed","model":"vision-test","output":[],"output_text":"image accepted","usage":null,"error":null,"incomplete_details":null}"#,
        ))
        .unwrap()
}

async fn fake_models() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"object":"list","data":[{"id":"vision-test"}]}"#,
        ))
        .unwrap()
}

fn contains_image_url(value: &Value) -> bool {
    value["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| message["content"].as_array())
        .flatten()
        .any(|part| part["type"].as_str() == Some("image_url"))
}

async fn spawn_relay() -> (SocketAddr, SocketAddr, RelayHandle) {
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

fn worker_config(
    worker_addr: SocketAddr,
    upstream_addr: SocketAddr,
    upstream_native_api: NativeApi,
) -> config::WorkerConfig {
    config::WorkerConfig {
        relay_urls: vec![format!("ws://{worker_addr}/ws/worker")],
        worker_token: "worker-token".to_string(),
        upstream_base_url: format!("http://{upstream_addr}"),
        upstream_api_key: "upstream-key".to_string(),
        upstream_native_api,
        connect_timeout_seconds: 5,
        ..config::WorkerConfig::default()
    }
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
