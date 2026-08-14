use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::Response,
    routing::{get, post},
};
use futures::StreamExt;
use prompt_ferry::{
    config::{self, NativeApi},
    relay::{self, RelayHandle},
    worker,
};
use serde_json::Value;
use std::{
    net::SocketAddr,
    sync::Arc,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

#[derive(Default)]
struct ChatRequestLog {
    bodies: Mutex<Vec<Value>>,
}

#[derive(Default)]
struct ResponsesRequestLog {
    bodies: Mutex<Vec<Value>>,
}

#[tokio::test]
async fn translates_responses_request_for_chat_native_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "instructions": "be terse",
            "input": [{"role":"user","content":[{"type":"input_text","text":"hello"}]}],
            "stream": false,
            "max_output_tokens": 4
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body.get("object").and_then(Value::as_str), Some("response"));
    assert_eq!(
        body.get("output_text").and_then(Value::as_str),
        Some("hello")
    );
    assert_eq!(body["text"]["format"]["type"].as_str(), Some("text"));
    assert_eq!(body["truncation"].as_str(), Some("disabled"));
    assert_eq!(
        body["usage"]["output_tokens_details"]["reasoning_tokens"].as_i64(),
        Some(0)
    );

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["messages"][0]["role"].as_str(), Some("system"));
    assert_eq!(requests[0]["messages"][1]["role"].as_str(), Some("user"));

    worker_handle.abort();
}

#[tokio::test]
async fn translates_streaming_responses_request_for_chat_native_upstream() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains("\"type\":\"response.output_text.delta\""));
    assert!(body.contains("\"type\":\"response.output_text.done\""));
    assert!(body.contains("\"type\":\"response.completed\""));
    assert!(body.contains("\"delta\":\"hel\""));
    assert!(body.contains("\"delta\":\"lo\""));

    worker_handle.abort();
}

#[tokio::test]
async fn preserves_native_responses_sse_framing_for_passthrough_upstream() {
    let upstream_addr =
        spawn_native_responses_upstream(ResponsesUpstreamMode::FramedPassthrough).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Responses);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert_eq!(body, native_responses_passthrough_sse());

    worker_handle.abort();
}

#[tokio::test]
async fn preserves_deepseek_v4_flash_native_responses_reasoning_sse() {
    let (status, requests, body) = fetch_deepseek_native_responses(NativeApi::Responses).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, deepseek_v4_flash_responses_sse());
    assert!(body.contains("event: response.reasoning_summary_text.delta\r\n"));
    assert!(body.contains("\"delta\":\"思考\""));
    assert!(body.contains("event: response.completed\r\n"));

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"].as_str(), Some("deepseek-v4-flash"));
    assert_eq!(
        requests[0]["instructions"].as_str(),
        Some("return a concise answer")
    );
    assert_eq!(requests[0]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[0]["input"][0]["content"].as_str(),
        Some("What is the capital of France?")
    );
    assert_eq!(requests[0]["stream"].as_bool(), Some(true));
    assert_eq!(requests[0]["max_output_tokens"].as_i64(), Some(32));
    assert_eq!(
        requests[0]["metadata"]["request_tag"].as_str(),
        Some("native-responses")
    );
    assert!(requests[0].get("messages").is_none());
}

#[tokio::test]
async fn auto_selects_native_responses_for_deepseek_v4_flash() {
    let (status, requests, body) = fetch_deepseek_native_responses(NativeApi::Auto).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, deepseek_v4_flash_responses_sse());
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"].as_str(), Some("deepseek-v4-flash"));
}

#[tokio::test]
async fn preserves_native_responses_completed_event_without_synthetic_error() {
    let (status, body) = fetch_native_responses(ResponsesUpstreamMode::OfficialCompleted).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("event: response.completed"), "body={body}");
    assert!(!body.contains("event: error"), "body={body}");
}

#[tokio::test]
async fn reports_native_responses_stream_without_terminal_event() {
    let (status, body) = fetch_native_responses(ResponsesUpstreamMode::MissingTerminal).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.matches("event: error").count(), 1, "body={body}");
    assert!(body.contains("\"code\":\"server_error\""), "body={body}");
}

#[tokio::test]
async fn preserves_native_responses_failure_terminals_without_synthetic_error() {
    for (mode, event_type) in [
        (ResponsesUpstreamMode::FailedTerminal, "response.failed"),
        (
            ResponsesUpstreamMode::IncompleteTerminal,
            "response.incomplete",
        ),
        (ResponsesUpstreamMode::ErrorTerminal, "error"),
    ] {
        let (status, body) = fetch_native_responses(mode).await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains(&format!("\"type\":\"{event_type}\"")),
            "body={body}"
        );
        if event_type == "error" {
            assert!(body.contains("\"code\":\"provider_error\""), "body={body}");
            assert!(
                !body.contains("\"code\":\"responses_upstream_error\""),
                "body={body}"
            );
        } else {
            assert!(!body.contains("event: error"), "body={body}");
        }
    }
}

#[tokio::test]
async fn enriches_bare_native_responses_failed_terminal_for_retry_classification() {
    let (status, body) = fetch_native_responses(ResponsesUpstreamMode::FailedTerminal).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"type\":\"response.failed\""), "body={body}");
    assert!(!body.contains("event: error"), "body={body}");
    assert!(
        body.contains("\"code\":\"server_error\""),
        "bare response.failed should be enriched with a retryable code, body={body}"
    );
    assert!(
        body.contains("\"message\":\"upstream Responses response failed\""),
        "body={body}"
    );
}

#[tokio::test]
async fn preserves_native_responses_failed_terminal_with_upstream_error_details() {
    let (status, body) =
        fetch_native_responses(ResponsesUpstreamMode::FailedTerminalWithError).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"type\":\"response.failed\""), "body={body}");
    assert!(
        body.contains("\"code\":\"rate_limit_exceeded\""),
        "body={body}"
    );
    assert!(body.contains("\"message\":\"Slow down\""), "body={body}");
    assert!(!body.contains("\"code\":\"server_error\""), "body={body}");
    assert!(!body.contains("event: error"), "body={body}");
}

#[tokio::test]
async fn streams_many_sse_events_from_single_upstream_chunk_without_bridge_backpressure() {
    let upstream_addr = spawn_native_responses_upstream(ResponsesUpstreamMode::ManyEvents).await;
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
            "model": "gpt-test",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert_eq!(
        body.matches("\"type\":\"response.output_text.delta\"")
            .count(),
        MANY_EVENTS_COUNT,
        "body={body}"
    );
    assert!(
        body.contains("\"type\":\"response.completed\""),
        "body={body}"
    );
    assert!(!body.contains("relay_bridge_backpressure"), "body={body}");
    assert!(
        !body.contains("\"code\":\"relay_bridge_error\""),
        "body={body}"
    );

    worker_handle.abort();
}

#[tokio::test]
async fn wraps_native_responses_midstream_failure_as_sse_error_event() {
    let upstream_addr =
        spawn_native_responses_upstream(ResponsesUpstreamMode::MidstreamError).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Responses);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.starts_with(
        "event: response.created\r\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\r\n\r\n"
    ), "body={body}");
    assert!(body.contains("event: error\n"), "body={body}");
    assert!(body.contains("\"type\":\"error\""), "body={body}");
    assert!(body.contains("\"sequence_number\":0"), "body={body}");
    assert!(body.contains("\"code\":\"server_error\""), "body={body}");
    assert!(
        body.contains("failed reading upstream response"),
        "body={body}"
    );
    assert!(body.contains("error decoding response body"), "body={body}");

    worker_handle.abort();
}

#[tokio::test]
async fn retries_when_upstream_closes_connection_before_headers() {
    let (upstream_addr, upstream_count) = spawn_flaky_responses_upstream(1).await;
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
            "model": "gpt-test",
            "input": "hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body.get("object").and_then(Value::as_str), Some("response"));
    assert_eq!(
        body["output"][0]["content"][0]["text"].as_str(),
        Some("hello")
    );
    assert_eq!(
        upstream_count.load(Ordering::SeqCst),
        2,
        "upstream should be requested twice after the first connection closed early"
    );

    worker_handle.abort();
}

#[tokio::test]
async fn exhausts_retries_when_upstream_always_closes_before_headers() {
    let (upstream_addr, upstream_count) = spawn_flaky_responses_upstream(3).await;
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
            "model": "gpt-test",
            "input": "hello",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(
        body["error"]["code"].as_str(),
        Some("upstream_error"),
        "body={body}"
    );
    assert_eq!(
        upstream_count.load(Ordering::SeqCst),
        3,
        "all three attempts should reach the upstream"
    );

    worker_handle.abort();
}

#[tokio::test]
async fn translates_reasoning_stream_for_chat_native_upstream() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "chat-reasoning",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let events = collect_sse_json_events(response).await;
    assert!(
        events
            .iter()
            .any(|event| event["type"].as_str() == Some("response.reasoning_summary_part.added"))
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"].as_str() == Some("response.reasoning_summary_text.delta"))
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"].as_str() == Some("response.reasoning_summary_part.done"))
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"].as_str() == Some("response.reasoning_text.delta"))
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"].as_str() == Some("response.reasoning_text.done"))
    );
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    assert_eq!(
        completed["response"]["output"][0]["type"].as_str(),
        Some("reasoning")
    );
    assert_eq!(
        completed["response"]["output"][0]["summary"][0]["text"].as_str(),
        Some("need tools then answer")
    );
    assert_eq!(completed["response"]["output_text"].as_str(), Some("done"));

    worker_handle.abort();
}

#[tokio::test]
async fn does_not_restore_deepseek_reasoning_for_chat_to_responses() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
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
            "model": "deepseek-v4-flash",
            "input": [
                {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["messages"][0]["role"].as_str(),
        Some("assistant")
    );
    assert!(
        requests[0]["messages"][0]
            .get("reasoning_content")
            .is_none()
    );
    assert_eq!(
        requests[0]["messages"][0]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn sdk_style_consumer_reads_responses_stream_text_and_completion() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let events = collect_sse_json_events(response).await;
    let mut streamed_text = String::new();
    let mut completed_output_text = None;
    for event in &events {
        match event["type"].as_str() {
            Some("response.output_text.delta") => {
                if let Some(delta) = event["delta"].as_str() {
                    streamed_text.push_str(delta);
                }
            }
            Some("response.completed") => {
                completed_output_text = event["response"]["output_text"]
                    .as_str()
                    .map(str::to_string);
            }
            _ => {}
        }
    }

    assert_eq!(streamed_text, "hello");
    assert_eq!(completed_output_text.as_deref(), Some("hello"));

    worker_handle.abort();
}

#[tokio::test]
async fn translates_tool_calls_for_chat_native_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [
                {"role":"user","content":"need weather"},
                {"type":"function_call","call_id":"call_prev","name":"lookup_cache","arguments":"{\"city\":\"Boston\"}"},
                {"type":"function_call_output","call_id":"call_prev","output":"cached 72F"}
            ],
            "tools": [{
                "name": "get_weather",
                "description": "weather lookup",
                "parameters": {"type":"object"}
            }],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.json::<Value>().await.unwrap();
    assert_eq!(body["output"][0]["type"].as_str(), Some("function_call"));
    assert_eq!(body["output"][0]["call_id"].as_str(), Some("call_1"));
    assert_eq!(body["output"][0]["name"].as_str(), Some("get_weather"));
    assert_eq!(
        body["usage"]["input_tokens_details"]["cached_tokens"].as_i64(),
        Some(0)
    );

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["tools"][0]["type"].as_str(), Some("function"));
    assert_eq!(
        requests[0]["tools"][0]["function"]["name"].as_str(),
        Some("get_weather")
    );
    assert_eq!(
        requests[0]["messages"][1]["tool_calls"][0]["id"].as_str(),
        Some("call_prev")
    );
    assert_eq!(requests[0]["messages"][2]["role"].as_str(), Some("tool"));
    assert_eq!(
        requests[0]["messages"][2]["tool_call_id"].as_str(),
        Some("call_prev")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn translates_assistant_reasoning_parts_for_chat_native_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-reasoning",
            "input": [
                {
                    "role":"assistant",
                    "content":[
                        {
                            "type":"reasoning",
                            "content":[{"type":"reasoning_text","text":"need tools first"}]
                        },
                        {"type":"output_text","text":"working"}
                    ]
                },
                {"role":"user","content":"continue"}
            ],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["messages"][0]["role"].as_str(),
        Some("assistant")
    );
    assert_eq!(
        requests[0]["messages"][0]["reasoning_content"].as_str(),
        Some("need tools first")
    );
    assert_eq!(
        requests[0]["messages"][0]["content"][0]["text"].as_str(),
        Some("working")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn rejects_previous_response_id_without_replay_state() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "previous_response_id": "resp_123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("replay_unavailable"));

    worker_handle.abort();
}

#[tokio::test]
async fn rejects_conversation_without_replay_state() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "conversation": "conv_missing"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("replay_unavailable"));

    worker_handle.abort();
}

#[tokio::test]
async fn rejects_input_file_for_chat_native_upstream() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [{
                "role":"user",
                "content":[{"type":"input_file","file_id":"file_123"}]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("unsupported_feature"));
    assert!(body.contains("input_file"));

    worker_handle.abort();
}

#[tokio::test]
async fn rejects_input_image_file_id_for_chat_native_upstream() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [{
                "role":"user",
                "content":[{"type":"input_image","image_url":{"file_id":"file_123"}}]
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("unsupported_feature"));
    assert!(body.contains("file_id"));

    worker_handle.abort();
}

#[tokio::test]
async fn rejects_non_text_function_call_output_for_chat_native_upstream() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [
                {"role":"user","content":"check weather"},
                {"type":"function_call","call_id":"call_1","name":"get_weather","arguments":"{\"city\":\"Boston\"}"},
                {"type":"function_call_output","call_id":"call_1","output":[{"type":"input_image","image_url":"https://example.com/image.png"}]}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("unsupported_feature"));
    assert!(body.contains("function_call_output"));

    worker_handle.abort();
}

#[tokio::test]
async fn forwards_json_schema_text_format_to_chat_native_response_format() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "answer_schema",
                    "description": "answer payload",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "answer": { "type": "string" }
                        },
                        "required": ["answer"]
                    },
                    "strict": true
                }
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["response_format"]["type"].as_str(),
        Some("json_schema")
    );
    assert_eq!(
        requests[0]["response_format"]["json_schema"]["name"].as_str(),
        Some("answer_schema")
    );
    assert_eq!(
        requests[0]["response_format"]["json_schema"]["strict"].as_bool(),
        Some(true)
    );

    worker_handle.abort();
}

#[tokio::test]
async fn forwards_json_object_text_format_to_chat_native_response_format() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "text": {
                "format": {
                    "type": "json_object"
                }
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["response_format"]["type"].as_str(),
        Some("json_object")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn accepts_include_and_forwards_prompt_cache_key_for_chat_native_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "include": [
                "file_search_call.results",
                "reasoning.encrypted_content"
            ],
            "prompt_cache_key": "thread-123"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert!(requests[0].get("include").is_none());
    assert_eq!(requests[0]["prompt_cache_key"].as_str(), Some("thread-123"));
    assert_eq!(
        requests[0]["messages"][0]["content"].as_str(),
        Some("hello")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn forwards_reasoning_effort_to_chat_native_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "hello",
            "reasoning": {
                "effort": "low"
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["reasoning_effort"].as_str(), Some("low"));
    assert_eq!(
        requests[0]["messages"][0]["content"].as_str(),
        Some("hello")
    );

    worker_handle.abort();
}

#[tokio::test]
async fn normalizes_developer_role_and_forwards_max_reasoning_effort_to_chat_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role":"developer","content":"be concise"},
                {"role":"user","content":"hello"}
            ],
            "reasoning_effort": "max",
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["messages"][0]["role"].as_str(), Some("system"));
    assert_eq!(
        requests[0]["messages"][0]["content"].as_str(),
        Some("be concise")
    );
    assert_eq!(requests[0]["reasoning_effort"].as_str(), Some("max"));

    worker_handle.abort();
}

#[tokio::test]
async fn translates_bare_reasoning_into_chat_tool_call_history() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
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
            "model": "deepseek-v4-pro",
            "input": [
                {"type":"reasoning","content":[{"type":"reasoning_text","text":"check first"}]},
                {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"},
                {"type":"function_call_output","call_id":"call_1","output":"ok"}
            ],
            "tools": [{"name":"lookup","description":"lookup","parameters":{"type":"object"}}],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["messages"][0]["reasoning_content"].as_str(),
        Some("check first")
    );
    assert_eq!(
        requests[0]["messages"][0]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[0]["messages"][1]["role"].as_str(), Some("tool"));

    worker_handle.abort();
}

#[tokio::test]
async fn rejects_unpaired_function_call_output_for_chat_native_upstream() {
    let upstream_addr = spawn_chat_only_upstream(Arc::new(ChatRequestLog::default())).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [
                {"type":"function_call_output","call_id":"call_1","output":"72F"}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    assert!(body.contains("invalid_responses_continuation"));

    worker_handle.abort();
}

#[tokio::test]
async fn folds_non_leading_instruction_messages_for_chat_native_upstream() {
    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_chat_only_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, NativeApi::Chat);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [
                {"role":"user","content":"hello"},
                {"role":"developer","content":"use tools carefully"},
                {"role":"assistant","content":"ok"},
                {"role":"system","content":"stay terse"}
            ],
            "stream": false
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["messages"][0]["role"].as_str(), Some("system"));
    assert_eq!(
        requests[0]["messages"][0]["content"].as_str(),
        Some("use tools carefully\n\nstay terse")
    );
    assert_eq!(requests[0]["messages"][1]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[0]["messages"][2]["role"].as_str(),
        Some("assistant")
    );

    worker_handle.abort();
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

async fn spawn_chat_only_upstream(log: Arc<ChatRequestLog>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_chat_completion))
        .route("/v1/models", get(fake_models))
        .with_state(log);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn spawn_flaky_responses_upstream(fail_attempts: usize) -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
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
                if n < fail_attempts {
                    let _ = sock.shutdown().await;
                    return;
                }
                let body = r#"{"id":"resp_1","object":"response","status":"completed","output":[{"type":"message","id":"msg_1","role":"assistant","content":[{"type":"output_text","text":"hello"}]}],"usage":{"total_tokens":5}}"#;
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

#[derive(Clone, Copy)]
enum ResponsesUpstreamMode {
    FramedPassthrough,
    OfficialCompleted,
    MissingTerminal,
    FailedTerminal,
    FailedTerminalWithError,
    IncompleteTerminal,
    ErrorTerminal,
    MidstreamError,
    ManyEvents,
}

const MANY_EVENTS_COUNT: usize = 300;

async fn fetch_native_responses(mode: ResponsesUpstreamMode) -> (StatusCode, String) {
    let upstream_addr = spawn_native_responses_upstream(mode).await;
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
            "model": "gpt-test",
            "input": "hello",
            "stream": true
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();

    worker_handle.abort();
    (status, body)
}

async fn fetch_deepseek_native_responses(
    native_api: NativeApi,
) -> (StatusCode, Vec<Value>, String) {
    let request_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_deepseek_responses_upstream(request_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_config = worker_config(worker_addr, upstream_addr, native_api);
    let mut worker_handle = tokio::spawn(async move {
        worker::connect_for_test(worker_config, reqwest::Client::new()).await
    });

    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let response = reqwest::Client::new()
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-v4-flash",
            "instructions": "return a concise answer",
            "input": [{"role":"user","content":"What is the capital of France?"}],
            "stream": true,
            "max_output_tokens": 32,
            "metadata": {"request_tag":"native-responses"}
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    let requests = request_log.bodies.lock().await.clone();

    worker_handle.abort();
    (status, requests, body)
}

async fn spawn_native_responses_upstream(mode: ResponsesUpstreamMode) -> SocketAddr {
    if matches!(mode, ResponsesUpstreamMode::MidstreamError) {
        return spawn_broken_native_responses_upstream().await;
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/responses", post(fake_native_responses_stream))
        .route("/v1/models", get(fake_models))
        .with_state(mode);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn spawn_deepseek_responses_upstream(log: Arc<ResponsesRequestLog>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/responses", post(fake_deepseek_responses_stream))
        .route("/v1/models", get(fake_models))
        .with_state(log);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn fake_native_responses_stream(
    State(mode): State<ResponsesUpstreamMode>,
    _body: Bytes,
) -> Response {
    let chunks = match mode {
        ResponsesUpstreamMode::FramedPassthrough => {
            let body = native_responses_passthrough_sse().into_bytes();
            let utf8_split = body
                .windows("你".len())
                .position(|window| window == "你".as_bytes())
                .unwrap()
                + 1;
            let crlf_split = body
                .windows(": keep-alive\r\n\r\n".len())
                .position(|window| window == b": keep-alive\r\n\r\n")
                .unwrap()
                + ": keep-alive\r".len();
            vec![
                Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&body[..utf8_split])),
                Ok(Bytes::copy_from_slice(&body[utf8_split..crlf_split])),
                Ok(Bytes::copy_from_slice(&body[crlf_split..])),
            ]
        }
        ResponsesUpstreamMode::OfficialCompleted => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_completed_sse(),
            ))]
        }
        ResponsesUpstreamMode::MissingTerminal => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_missing_terminal_sse(),
            ))]
        }
        ResponsesUpstreamMode::FailedTerminal => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_terminal_sse("response.failed"),
            ))]
        }
        ResponsesUpstreamMode::FailedTerminalWithError => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_failed_with_error_sse(),
            ))]
        }
        ResponsesUpstreamMode::IncompleteTerminal => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_terminal_sse("response.incomplete"),
            ))]
        }
        ResponsesUpstreamMode::ErrorTerminal => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_terminal_sse("error"),
            ))]
        }
        ResponsesUpstreamMode::MidstreamError => unreachable!("handled by raw tcp server"),
        ResponsesUpstreamMode::ManyEvents => {
            vec![Ok::<Bytes, std::io::Error>(Bytes::from(
                native_responses_many_events_sse(MANY_EVENTS_COUNT),
            ))]
        }
    };
    let stream = futures::stream::iter(chunks);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

async fn fake_deepseek_responses_stream(
    State(log): State<Arc<ResponsesRequestLog>>,
    body: Bytes,
) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    log.bodies.lock().await.push(value);

    let body = deepseek_v4_flash_responses_sse().into_bytes();
    let marker = "\"delta\":\"思考\"".as_bytes();
    let split = body
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len()
        - 1;
    let chunks = vec![
        Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&body[..split])),
        Ok(Bytes::copy_from_slice(&body[split..])),
    ];
    let mut response = Response::new(Body::from_stream(futures::stream::iter(chunks)));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
    response
}

async fn spawn_broken_native_responses_upstream() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
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
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        stream
            .write_all(b"40\r\ndata: {\"type\":\"response.completed\"")
            .await
            .unwrap();
        stream.flush().await.unwrap();
    });
    addr
}

async fn write_chunk(stream: &mut tokio::net::TcpStream, body: &[u8]) {
    stream
        .write_all(format!("{:X}\r\n", body.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
    stream.write_all(b"\r\n").await.unwrap();
}

fn native_responses_many_events_sse(event_count: usize) -> String {
    let mut body = String::from(
        "event: response.created\n\
         data: {\"type\":\"response.created\"}\n\n",
    );
    for index in 0..event_count {
        body.push_str("event: response.output_text.delta\n");
        body.push_str(&format!(
            "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"evt_{index}\"}}\n\n"
        ));
    }
    body.push_str(
        "event: response.completed\n\
         data: {\"type\":\"response.completed\"}\n\n",
    );
    body
}

fn native_responses_passthrough_sse() -> String {
    concat!(
        "event: response.created\r\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\r\n",
        "\r\n",
        "event: response.output_text.delta\r\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你\"}\r\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"好\"}\r\n",
        "\r\n",
        ": keep-alive\r\n",
        "\r\n",
        "data: [DONE]\r\n",
        "\r\n"
    )
    .to_string()
}

fn native_responses_completed_sse() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\"}\n\n"
    )
    .to_string()
}

fn native_responses_missing_terminal_sse() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n"
    )
    .to_string()
}

fn native_responses_terminal_sse(event_type: &str) -> String {
    let data = if event_type == "error" {
        r#"{"type":"error","code":"provider_error","message":"provider failed"}"#
    } else {
        return format!(
            "event: response.created\n\
             data: {{\"type\":\"response.created\"}}\n\n\
             event: {event_type}\n\
             data: {{\"type\":\"{event_type}\"}}\n\n"
        );
    };
    format!(
        "event: response.created\n\
         data: {{\"type\":\"response.created\"}}\n\n\
         event: {event_type}\n\
         data: {data}\n\n"
    )
}

fn native_responses_failed_with_error_sse() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\"}\n\n",
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_failed_1\",\"error\":{\"code\":\"rate_limit_exceeded\",\"message\":\"Slow down\"}}}\n\n",
    )
    .to_string()
}

fn deepseek_v4_flash_responses_sse() -> String {
    concat!(
        "event: response.created\r\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_ds_1\",\"object\":\"response\",\"status\":\"in_progress\",\"model\":\"deepseek-v4-flash\"}}\r\n",
        "\r\n",
        "event: response.output_item.added\r\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"reasoning_1\",\"type\":\"reasoning\",\"summary\":[]}}\r\n",
        "\r\n",
        "event: response.reasoning_summary_text.delta\r\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"reasoning_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"思考\"}\r\n",
        "\r\n",
        "event: response.reasoning_summary_text.done\r\n",
        "data: {\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"reasoning_1\",\"output_index\":0,\"summary_index\":0,\"text\":\"思考\"}\r\n",
        "\r\n",
        "event: response.output_item.done\r\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"reasoning_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"思考\"}]}}\r\n",
        "\r\n",
        "event: response.completed\r\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ds_1\",\"status\":\"completed\",\"model\":\"deepseek-v4-flash\"}}\r\n",
        "\r\n"
    )
    .to_string()
}

async fn fake_chat_completion(State(log): State<Arc<ChatRequestLog>>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    log.bodies.lock().await.push(value.clone());

    if value.get("model").and_then(Value::as_str) == Some("chat-reasoning")
        && value.get("stream").and_then(Value::as_bool) == Some(true)
    {
        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(
                br#"data: {"id":"chatcmpl_123","created":123,"model":"chat-reasoning","choices":[{"delta":{"reasoning_content":"need tools "}}]}

"#,
            )),
            Ok(Bytes::from_static(
                br#"data: {"choices":[{"delta":{"reasoning_content":"then answer","content":"done"}}]}
data: [DONE]

"#,
            )),
        ];
        let stream = futures::stream::iter(chunks);
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = StatusCode::OK;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
        return response;
    }
    if value.get("stream").and_then(Value::as_bool) == Some(true)
        && value.get("tools").and_then(Value::as_array).is_some()
    {
        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(
                br#"data: {"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Bos"}}]}}]}

"#,
            )),
            Ok(Bytes::from_static(
                br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"ton\"}"}}]}}]}
data: [DONE]

"#,
            )),
        ];
        let stream = futures::stream::iter(chunks);
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = StatusCode::OK;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
        return response;
    }
    if value.get("stream").and_then(Value::as_bool) == Some(true) {
        let chunks = vec![
            Ok::<Bytes, std::io::Error>(Bytes::from_static(
                br#"data: {"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"delta":{"content":"hel"}}]}

"#,
            )),
            Ok(Bytes::from_static(
                br#"data: {"choices":[{"delta":{"content":"lo"}}]}
data: {"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}
data: [DONE]

"#,
            )),
        ];
        let stream = futures::stream::iter(chunks);
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = StatusCode::OK;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, "text/event-stream".parse().unwrap());
        return response;
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(if value.get("tools").and_then(Value::as_array).is_some() {
            r#"{"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"message":{"content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Boston\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}"#
        } else {
            r#"{"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"message":{"content":"hello"}}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}"#
        }))
        .unwrap()
}

async fn fake_models() -> Response {
    Response::new(Body::from(
        r#"{"object":"list","data":[{"id":"gpt-test"}]}"#,
    ))
}

async fn collect_sse_json_events(response: reqwest::Response) -> Vec<Value> {
    let mut events = Vec::new();
    let mut pending = String::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap();
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = pending.find('\n') {
            let line = pending[..index].trim_end_matches('\r').to_string();
            pending.drain(..=index);
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data == "[DONE]" {
                return events;
            }
            events.push(serde_json::from_str(data).unwrap());
        }
    }
    events
}
