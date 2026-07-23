use axum::body::Body;
use http::header::HeaderName;
use serde_json::{Value, json};

use super::super::{protocol::DEFAULT_PROTOCOL_VERSION_STR, server::RequestScope};

pub(super) fn has_mcp_session_id(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("mcp-session-id") && !value.trim().is_empty()
    })
}

pub(super) fn build_rmcp_request(
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
    scope: RequestScope,
) -> anyhow::Result<http::Request<Body>> {
    let uri = format!("http://prompt-ferry.internal{path}");
    let normalized_body = if method.eq_ignore_ascii_case("POST") {
        normalize_request_body(body)
    } else {
        body.to_vec()
    };
    let mut request = http::Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from(normalized_body))?;
    let mut saw_accept = false;
    let mut saw_content_type = false;

    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_str()),
            http::HeaderValue::from_str(value),
        ) {
            if name == http::header::ACCEPT {
                saw_accept = true;
            }
            if name == http::header::CONTENT_TYPE {
                saw_content_type = true;
            }
            request.headers_mut().append(name, value);
        }
    }

    if !saw_accept {
        let accept = if method.eq_ignore_ascii_case("GET") {
            "text/event-stream"
        } else {
            "application/json, text/event-stream"
        };
        request
            .headers_mut()
            .insert(http::header::ACCEPT, http::HeaderValue::from_static(accept));
    }
    if method.eq_ignore_ascii_case("POST") && !saw_content_type {
        request.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
    }
    request.extensions_mut().insert(scope);

    Ok(request)
}

fn normalize_request_body(body: &[u8]) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    if value.get("method").and_then(Value::as_str) != Some("initialize") {
        return body.to_vec();
    }

    if !value.get("params").is_some_and(Value::is_object) {
        value["params"] = json!({});
    }
    let params = value
        .get_mut("params")
        .and_then(Value::as_object_mut)
        .expect("params object just initialized");
    params
        .entry("protocolVersion".to_string())
        .or_insert_with(|| Value::String(DEFAULT_PROTOCOL_VERSION_STR.to_string()));
    params
        .entry("capabilities".to_string())
        .or_insert_with(|| json!({}));
    params.entry("clientInfo".to_string()).or_insert_with(|| {
        json!({
            "name": "prompt-ferry",
            "version": env!("CARGO_PKG_VERSION"),
        })
    });

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}
