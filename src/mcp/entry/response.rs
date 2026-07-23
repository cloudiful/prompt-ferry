use anyhow::{Context, anyhow};
use axum::body::{Body, to_bytes};
use futures::StreamExt;
use serde_json::{Value, json};

use super::{MAX_MCP_BODY_BYTES, McpTransportResponse};

pub(super) async fn normalize_response<B>(
    response: http::Response<B>,
) -> anyhow::Result<McpTransportResponse>
where
    B: axum::body::HttpBody<Data = bytes::Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError> + std::fmt::Display,
{
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let headers = response_headers(response.headers());
    if content_type.starts_with("text/event-stream") {
        let stream = Body::new(response.into_body())
            .into_data_stream()
            .map(|chunk| {
                chunk.map_err(|err| anyhow!("failed to read rmcp response stream: {err}"))
            });
        return Ok(McpTransportResponse::Streaming {
            status,
            content_type,
            headers,
            stream: Box::pin(stream),
            selected_token_slot: None,
        });
    }

    let body = to_bytes(Body::new(response.into_body()), MAX_MCP_BODY_BYTES)
        .await
        .context("failed to read rmcp response body")?;
    let (content_type, body) = normalize_buffered_body(status, &content_type, &body)?;
    Ok(McpTransportResponse::Buffered {
        status,
        content_type,
        headers,
        body,
        selected_token_slot: None,
    })
}

fn response_headers(headers: &http::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| {
            *name != http::header::CONTENT_TYPE
                && *name != http::header::CONTENT_LENGTH
                && *name != http::header::TRANSFER_ENCODING
        })
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn normalize_buffered_body(
    status: u16,
    content_type: &str,
    body: &[u8],
) -> anyhow::Result<(String, Vec<u8>)> {
    if body.is_empty() {
        return Ok((content_type.to_string(), Vec::new()));
    }

    if let Ok(mut value) = serde_json::from_slice::<Value>(body) {
        if is_session_not_found(status, &value) {
            normalize_error_code(&mut value, "session_not_found");
            return Ok((content_type.to_string(), serde_json::to_vec(&value)?));
        }
        return Ok((content_type.to_string(), body.to_vec()));
    }

    let message = String::from_utf8_lossy(body).trim().to_string();
    let code = if message.contains("Session not found") {
        "session_not_found"
    } else {
        "mcp_error"
    };
    Ok((
        "application/json".to_string(),
        serde_json::to_vec(&json!({
            "error": {
                "code": code,
                "message": if message.is_empty() { "unknown MCP error" } else { &message },
            }
        }))?,
    ))
}

fn is_session_not_found(status: u16, value: &Value) -> bool {
    status == 404
        || value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("Session not found"))
}

fn normalize_error_code(value: &mut Value, code: &str) {
    if let Some(error) = value.get_mut("error")
        && let Some(object) = error.as_object_mut()
    {
        object.insert("code".to_string(), Value::String(code.to_string()));
    }
}

pub(super) fn json_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}
