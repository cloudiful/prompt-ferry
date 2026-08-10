use std::sync::Arc;

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::{StatusCode, header},
    response::Response,
    routing::{get, post},
};
use serde_json::Value;
use tokio::sync::Mutex;

#[path = "replay_upstream_harness.rs"]
mod chat;

pub use chat::{ChatRequestLog, spawn_replay_upstream};

#[derive(Default)]
pub struct ResponsesRequestLog {
    pub bodies: Mutex<Vec<Value>>,
    pub fail_next_response_turns: Mutex<Vec<usize>>,
}

pub async fn spawn_replay_responses_upstream(
    log: Arc<ResponsesRequestLog>,
) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/responses", post(fake_responses_completion))
        .route("/v1/models", get(chat::fake_models))
        .with_state(log);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

pub async fn spawn_replay_responses_upstream_without_conversation(
    log: Arc<ResponsesRequestLog>,
) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/v1/responses",
            post(fake_responses_completion_without_conversation),
        )
        .route("/v1/models", get(chat::fake_models))
        .with_state(log);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn fake_responses_completion(
    State(log): State<Arc<ResponsesRequestLog>>,
    body: Bytes,
) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    let mut requests = log.bodies.lock().await;
    requests.push(value.clone());
    let turn = requests.len();
    drop(requests);

    let mut fail_turns = log.fail_next_response_turns.lock().await;
    if let Some(index) = fail_turns
        .iter()
        .position(|queued_turn| *queued_turn == turn)
    {
        fail_turns.remove(index);
        drop(fail_turns);
        return Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"error":{"message":"upstream failure","type":"server_error","code":"bad_gateway"}}"#,
            ))
            .unwrap();
    }
    drop(fail_turns);

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("gpt-test");
    let body = if turn == 1 {
        serde_json::json!({
            "id": "resp_turn1",
            "conversation": "conv_replay",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": model,
            "output": [{
                "id": "rs_1",
                "type": "reasoning",
                "summary": [],
                "content": [{
                    "type": "reasoning_text",
                    "text": "internal steps"
                }]
            }, {
                "id": "fc_1",
                "type": "function_call",
                "status": "completed",
                "call_id": "call_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"Boston\"}"
            }],
            "output_text": "",
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3,
                "total_tokens": 5,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            },
            "text": { "format": { "type": "text" } },
            "truncation": "disabled",
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "store": false,
            "error": null,
            "incomplete_details": null,
            "metadata": {}
        })
    } else {
        serde_json::json!({
            "id": format!("resp_turn{turn}"),
            "conversation": "conv_replay",
            "object": "response",
            "created_at": 124,
            "status": "completed",
            "model": model,
            "output": [{
                "id": "msg_1",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "done",
                    "annotations": [],
                    "logprobs": []
                }]
            }],
            "output_text": "done",
            "usage": {
                "input_tokens": 4,
                "output_tokens": 2,
                "total_tokens": 6,
                "input_tokens_details": { "cached_tokens": 0 },
                "output_tokens_details": { "reasoning_tokens": 0 }
            },
            "text": { "format": { "type": "text" } },
            "truncation": "disabled",
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "store": false,
            "error": null,
            "incomplete_details": null,
            "metadata": {}
        })
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn fake_responses_completion_without_conversation(
    State(log): State<Arc<ResponsesRequestLog>>,
    body: Bytes,
) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    let mut requests = log.bodies.lock().await;
    requests.push(value.clone());
    let turn = requests.len();
    drop(requests);

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("gpt-test");
    let body = if turn == 1 {
        serde_json::json!({
            "id": "resp_turn1",
            "object": "response",
            "created_at": 123,
            "status": "completed",
            "model": model,
            "output": [{
                "id": "fc_1",
                "type": "function_call",
                "status": "completed",
                "call_id": "call_1",
                "name": "get_weather",
                "arguments": "{\"city\":\"Boston\"}"
            }],
            "output_text": "",
            "usage": {
                "input_tokens": 2,
                "output_tokens": 3,
                "total_tokens": 5
            },
            "text": { "format": { "type": "text" } },
            "truncation": "disabled",
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "store": false,
            "error": null,
            "incomplete_details": null,
            "metadata": {}
        })
    } else {
        serde_json::json!({
            "id": format!("resp_turn{turn}"),
            "object": "response",
            "created_at": 124,
            "status": "completed",
            "model": model,
            "output": [{
                "id": "msg_1",
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": "done",
                    "annotations": [],
                    "logprobs": []
                }]
            }],
            "output_text": "done",
            "usage": {
                "input_tokens": 4,
                "output_tokens": 2,
                "total_tokens": 6
            },
            "text": { "format": { "type": "text" } },
            "truncation": "disabled",
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "store": false,
            "error": null,
            "incomplete_details": null,
            "metadata": {}
        })
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}
