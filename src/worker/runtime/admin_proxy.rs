use super::{
    context::{BridgeSender, RuntimeServices},
    request_assembly::BufferedBridgeRequest,
};
use crate::protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseError, ResponseStart};
use anyhow::Context;
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode, header},
};
use futures::StreamExt;
use tower::ServiceExt;

pub(super) async fn process_admin_request(
    request: BufferedBridgeRequest,
    services: &RuntimeServices,
) -> anyhow::Result<()> {
    let Some(state) = services.admin_state().cloned() else {
        return send_static_response(
            &services.out_tx,
            &request.request_id,
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"admin UI unavailable".to_vec(),
        );
    };

    let mut builder = Request::builder()
        .method(request.method.as_str())
        .uri(request.path.as_str());
    if let Some(headers) = builder.headers_mut() {
        apply_request_headers(headers, &request.headers);
    }
    let admin_request = builder
        .body(Body::from(request.body))
        .context("failed to build admin proxy request")?;

    let response = crate::worker_admin::router(state)
        .oneshot(admin_request)
        .await
        .context("admin router failed")?;

    stream_admin_response(&services.out_tx, &request.request_id, response).await
}

fn apply_request_headers(target: &mut HeaderMap, headers: &[(String, String)]) {
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(name.as_str()),
            header::HeaderValue::from_str(value),
        ) {
            target.append(name, value);
        }
    }
}

async fn stream_admin_response(
    out_tx: &BridgeSender,
    request_id: &str,
    response: axum::response::Response,
) -> anyhow::Result<()> {
    let (parts, body) = response.into_parts();
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request_id.to_string(),
            status: parts.status.as_u16(),
            content_type,
            headers: bridge_response_headers(&parts.headers),
        }))
        .context("relay response channel closed")?;

    let mut stream = body.into_data_stream();
    while let Some(next) = stream.next().await {
        match next {
            Ok(chunk) => {
                out_tx
                    .send(BridgeMessage::ResponseChunk(ResponseChunk {
                        request_id: request_id.to_string(),
                        data: chunk.to_vec(),
                    }))
                    .context("relay response channel closed")?;
            }
            Err(err) => {
                out_tx
                    .send(BridgeMessage::ResponseError(ResponseError {
                        request_id: request_id.to_string(),
                        status: StatusCode::BAD_GATEWAY.as_u16(),
                        code: "admin_response_read_failed".to_string(),
                        message: format!("failed to read admin response body: {err}"),
                    }))
                    .context("relay response channel closed")?;
                return Ok(());
            }
        }
    }

    out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request_id.to_string(),
        }))
        .context("relay response channel closed")?;
    Ok(())
}

fn bridge_response_headers(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            (!is_hop_by_hop_response_header(name)
                && *name != header::CONTENT_TYPE
                && *name != header::CONTENT_LENGTH)
                .then_some((name, value))
        })
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn is_hop_by_hop_response_header(name: &header::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn send_static_response(
    out_tx: &BridgeSender,
    request_id: &str,
    status: StatusCode,
    content_type: &str,
    body: Vec<u8>,
) -> anyhow::Result<()> {
    out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request_id.to_string(),
            status: status.as_u16(),
            content_type: Some(content_type.to_string()),
            headers: Vec::new(),
        }))
        .context("relay response channel closed")?;
    out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request_id.to_string(),
            data: body,
        }))
        .context("relay response channel closed")?;
    out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request_id.to_string(),
        }))
        .context("relay response channel closed")?;
    Ok(())
}
