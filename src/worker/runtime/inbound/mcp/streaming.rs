use super::super::super::{
    MCP_ERROR_BODY_CAPTURE_BYTES, MCP_RESPONSE_BODY_CAPTURE_BYTES, context::FailurePayload,
    extract_mcp_error, format_mcp_response_body, record_mcp_request_event, redaction_enabled,
    safe_error,
};
use crate::protocol::{
    BridgeMessage, McpResponseChunk, McpResponseEnd, McpResponseStart, ResponseError,
};
use futures::Stream;
use reqwest::StatusCode;
use tracing::warn;

use crate::worker::runtime::ai::upstream_restore::restore_mcp_body_json_blocking;

use super::restore_failure::handle_restore_failure;
use super::send_mcp_response;
use crate::worker::runtime::mcp_support::McpResponseContext;

pub(super) async fn handle_streaming_transport_response<S>(
    context: &McpResponseContext<'_>,
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    mut stream: S,
) where
    S: Stream<Item = anyhow::Result<bytes::Bytes>> + Unpin,
{
    let request = context.request;
    let request_ctx = context.request_ctx;
    let request_content_logging = context.request_content_logging;
    let upstream_restore_session = context.upstream_restore_session.clone();
    let services = context.services;
    if let Some(session) = upstream_restore_session.clone() {
        let mut body = Vec::new();
        while let Some(chunk) = futures::StreamExt::next(&mut stream).await {
            match chunk {
                Ok(chunk) => body.extend_from_slice(&chunk),
                Err(err) => {
                    let error_message = safe_error(
                        &err,
                        redaction_enabled(services.admin_state()),
                        request_ctx.user_id,
                    );
                    record_mcp_request_event(
                        context,
                        FailurePayload {
                            status: StatusCode::BAD_GATEWAY,
                            error_code: "mcp_stream_error".to_string(),
                            error_message,
                            upstream_error_body: None,
                            response_body: None,
                        },
                    )
                    .await;
                    return;
                }
            }
        }
        let restored_body =
            match restore_mcp_body_json_blocking(body.clone(), session.clone()).await {
                Ok(restored_body) => restored_body,
                Err(err) => {
                    return handle_restore_failure(context, err, body).await;
                }
            };
        send_mcp_response(
            services,
            &request.request_id,
            status,
            Some(content_type),
            headers,
            restored_body.clone(),
        )
        .await;
        let ok = (200..300).contains(&status);
        let response_body = (ok && request_content_logging.mode.captures_normalized())
            .then(|| format_mcp_response_body(&restored_body))
            .flatten();
        let upstream_error_body = (!ok)
            .then(|| String::from_utf8_lossy(&restored_body).to_string())
            .filter(|value| !value.is_empty());
        let (error_code, error_message) = if ok {
            (String::new(), String::new())
        } else {
            extract_mcp_error(status, &restored_body)
        };
        record_mcp_request_event(
            context,
            FailurePayload {
                status: StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
                error_code,
                error_message,
                upstream_error_body,
                response_body,
            },
        )
        .await;
        return;
    }
    let base_status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
    let ok = base_status.is_success();
    let content_logging_enabled = request_content_logging.mode.captures_normalized();
    let mut captured_error_body = Vec::new();
    let mut captured_success_body = Vec::new();
    let mut success_body_truncated = false;
    let mut streamed_chunks = 0usize;
    let mut streamed_bytes = 0usize;
    let mut failure = match services
        .out_tx
        .send(BridgeMessage::McpResponseStart(McpResponseStart {
            request_id: request.request_id.clone(),
            status,
            content_type: Some(content_type.clone()),
            headers,
        }))
        .await
    {
        Ok(()) => None,
        Err(err) => {
            warn!(
                category = "mcp_bridge_diag",
                phase = "response_start",
                request_id = %request.request_id,
                status,
                error = %err,
                "failed to send MCP response start from worker to relay"
            );
            Some(FailurePayload {
                status: StatusCode::BAD_GATEWAY,
                error_code: "mcp_stream_disconnected".to_string(),
                error_message: format!("failed to send MCP response start to relay: {err}"),
                upstream_error_body: None,
                response_body: None,
            })
        }
    };
    while failure.is_none() {
        let Some(chunk) = futures::StreamExt::next(&mut stream).await else {
            break;
        };
        match chunk {
            Ok(chunk) => {
                let chunk_bytes = chunk.len();
                if !ok {
                    capture_body_prefix(
                        &mut captured_error_body,
                        &chunk,
                        MCP_ERROR_BODY_CAPTURE_BYTES,
                    );
                } else if content_logging_enabled {
                    success_body_truncated |= capture_body_prefix(
                        &mut captured_success_body,
                        &chunk,
                        MCP_RESPONSE_BODY_CAPTURE_BYTES,
                    );
                }
                match services
                    .out_tx
                    .send(BridgeMessage::McpResponseChunk(McpResponseChunk {
                        request_id: request.request_id.clone(),
                        data: chunk.to_vec(),
                    }))
                    .await
                {
                    Ok(()) => {
                        streamed_chunks += 1;
                        streamed_bytes += chunk_bytes;
                    }
                    Err(err) => {
                        warn!(
                            category = "mcp_bridge_diag",
                            phase = "response_chunk",
                            request_id = %request.request_id,
                            status,
                            streamed_chunks,
                            streamed_bytes,
                            chunk_bytes,
                            error = %err,
                            "failed to send MCP response chunk from worker to relay"
                        );
                        failure = Some(FailurePayload {
                            status: StatusCode::BAD_GATEWAY,
                            error_code: "mcp_stream_disconnected".to_string(),
                            error_message: format!(
                                "failed to send MCP response chunk to relay: {err}"
                            ),
                            upstream_error_body: None,
                            response_body: None,
                        });
                        break;
                    }
                }
            }
            Err(err) => {
                let error_message = safe_error(
                    &err,
                    redaction_enabled(services.admin_state()),
                    request_ctx.user_id,
                );
                if let Err(send_err) = services
                    .out_tx
                    .send(BridgeMessage::ResponseError(ResponseError {
                        request_id: request.request_id.clone(),
                        status: StatusCode::BAD_GATEWAY.as_u16(),
                        code: "mcp_stream_error".to_string(),
                        message: error_message.clone(),
                    }))
                    .await
                {
                    warn!(
                        category = "mcp_bridge_diag",
                        phase = "response_error",
                        request_id = %request.request_id,
                        status,
                        error = %send_err,
                        "failed to send MCP stream error from worker to relay"
                    );
                }
                failure = Some(FailurePayload {
                    status: StatusCode::BAD_GATEWAY,
                    error_code: "mcp_stream_error".to_string(),
                    error_message,
                    upstream_error_body: (!captured_error_body.is_empty())
                        .then(|| String::from_utf8_lossy(&captured_error_body).to_string()),
                    response_body: None,
                });
                break;
            }
        }
    }
    if failure.is_none() {
        if let Err(err) = services
            .out_tx
            .send(BridgeMessage::McpResponseEnd(McpResponseEnd {
                request_id: request.request_id.clone(),
            }))
            .await
        {
            warn!(
                category = "mcp_bridge_diag",
                phase = "response_end",
                request_id = %request.request_id,
                status,
                streamed_chunks,
                streamed_bytes,
                error = %err,
                "failed to send MCP response end from worker to relay"
            );
            failure = Some(FailurePayload {
                status: StatusCode::BAD_GATEWAY,
                error_code: "mcp_stream_disconnected".to_string(),
                error_message: format!("failed to send MCP response end to relay: {err}"),
                upstream_error_body: None,
                response_body: None,
            });
        }
    }
    let failure = failure.unwrap_or_else(|| {
        if ok {
            FailurePayload {
                status: base_status,
                error_code: String::new(),
                error_message: String::new(),
                upstream_error_body: None,
                response_body: format_captured_stream_body(
                    &captured_success_body,
                    success_body_truncated,
                ),
            }
        } else {
            let (error_code, error_message) = extract_mcp_error(status, &captured_error_body);
            FailurePayload {
                status: base_status,
                error_code,
                error_message,
                upstream_error_body: (!captured_error_body.is_empty())
                    .then(|| String::from_utf8_lossy(&captured_error_body).to_string()),
                response_body: None,
            }
        }
    });
    record_mcp_request_event(context, failure).await;
}

pub(super) async fn handle_buffered_transport_response(
    context: &McpResponseContext<'_>,
    status: u16,
    content_type: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    let request = context.request;
    let request_content_logging = context.request_content_logging;
    let upstream_restore_session = context.upstream_restore_session.clone();
    let services = context.services;
    let body = if let Some(session) = upstream_restore_session.clone() {
        match restore_mcp_body_json_blocking(body.clone(), session.clone()).await {
            Ok(restored_body) => restored_body,
            Err(err) => {
                handle_restore_failure(context, err, body).await;
                return;
            }
        }
    } else {
        body
    };
    let ok = (200..300).contains(&status);
    let response_body = (ok && request_content_logging.mode.captures_normalized())
        .then(|| format_mcp_response_body(&body))
        .flatten();
    let upstream_error_body = (!ok)
        .then(|| String::from_utf8_lossy(&body).to_string())
        .filter(|value| !value.is_empty());
    let (error_code, error_message) = if ok {
        (String::new(), String::new())
    } else {
        extract_mcp_error(status, &body)
    };
    send_mcp_response(
        services,
        &request.request_id,
        status,
        Some(content_type),
        headers,
        body,
    )
    .await;
    record_mcp_request_event(
        context,
        FailurePayload {
            status: StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            error_code,
            error_message,
            upstream_error_body,
            response_body,
        },
    )
    .await;
}

fn capture_body_prefix(buffer: &mut Vec<u8>, chunk: &[u8], limit: usize) -> bool {
    if buffer.len() >= limit {
        return true;
    }
    let remaining = limit - buffer.len();
    let take = remaining.min(chunk.len());
    buffer.extend_from_slice(&chunk[..take]);
    take < chunk.len()
}

fn format_captured_stream_body(body: &[u8], truncated: bool) -> Option<String> {
    let formatted = format_mcp_response_body(body)?;
    if truncated {
        Some(format!("{formatted}\n\n[truncated]"))
    } else {
        Some(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::{capture_body_prefix, format_captured_stream_body};

    #[test]
    fn capture_body_prefix_marks_truncation_after_limit() {
        let mut buffer = Vec::new();
        assert!(!capture_body_prefix(&mut buffer, b"abcd", 8));
        assert_eq!(buffer, b"abcd");
        assert!(capture_body_prefix(&mut buffer, b"efghij", 8));
        assert_eq!(buffer, b"abcdefgh");
    }

    #[test]
    fn format_captured_stream_body_marks_truncated_preview() {
        let preview = format_captured_stream_body(br#"{"ok":true}"#, true).expect("preview");
        assert!(preview.contains("\"ok\": true"));
        assert!(preview.ends_with("[truncated]"));
    }
}
