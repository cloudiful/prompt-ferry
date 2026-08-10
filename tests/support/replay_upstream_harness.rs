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

#[derive(Default)]
pub struct ChatRequestLog {
    pub bodies: Mutex<Vec<Value>>,
    pub fail_next_chat_turns: Mutex<Vec<usize>>,
    pub omit_reasoning: bool,
    pub multi_tool_turns: bool,
}

pub async fn spawn_replay_upstream(log: Arc<ChatRequestLog>) -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_chat_completion))
        .route("/deepseek/v1/chat/completions", post(fake_chat_completion))
        .route("/v1/models", get(fake_models))
        .route("/deepseek/v1/models", get(fake_models))
        .with_state(log);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

async fn fake_chat_completion(State(log): State<Arc<ChatRequestLog>>, body: Bytes) -> Response {
    let value = serde_json::from_slice::<Value>(&body).unwrap();
    let omit_reasoning = log.omit_reasoning;
    let multi_tool_turns = log.multi_tool_turns;
    let mut requests = log.bodies.lock().await;
    requests.push(value.clone());
    let turn = requests.len();
    drop(requests);

    let mut fail_turns = log.fail_next_chat_turns.lock().await;
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
                r#"{"message":"error code: 502","type":"invalid_request_error","param":null,"code":"bad_gateway"}"#,
            ))
            .unwrap();
    }
    drop(fail_turns);

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("gpt-test");
    let tool_turn = if multi_tool_turns {
        turn <= 2
    } else {
        turn == 1
    };
    let body = if tool_turn {
        let call_id = if turn == 1 { "call_1" } else { "call_2" };
        let reasoning = if multi_tool_turns {
            format!("internal steps {turn}")
        } else {
            "internal steps".to_string()
        };
        serde_json::json!({
            "id": format!("chatcmpl_turn{turn}"),
            "created": 123,
            "model": model,
            "choices": [{
                "message": {
                    "content": Value::Null,
                    "tool_calls": [{
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Boston\"}"
                        }
                    }],
                    "reasoning_content": if omit_reasoning {
                        Value::Null
                    } else {
                        Value::String(reasoning)
                    }
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 2,
                "completion_tokens": 3,
                "total_tokens": 5
            }
        })
    } else {
        serde_json::json!({
            "id": format!("chatcmpl_turn{turn}"),
            "created": 124,
            "model": model,
            "choices": [{
                "message": {
                    "content": "done"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 4,
                "completion_tokens": 2,
                "total_tokens": 6
            }
        })
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub async fn fake_models() -> Response {
    Response::new(Body::from(
        r#"{"object":"list","data":[{"id":"deepseek-chat"},{"id":"gpt-test"}]}"#,
    ))
}
