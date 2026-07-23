mod redaction;
mod restore_failure;
mod streaming;

use super::super::mcp_support::McpResponseContext;
use super::super::{
    RequestExecutionContext, check_named_request_budget,
    context::{FailurePayload, RuntimeServices},
    record_mcp_request_event, redaction_enabled,
    request_assembly::BufferedMcpRequest,
    resolve_mcp_conversation_log, safe_error,
};
use crate::{
    db, mcp,
    protocol::{BridgeMessage, McpResponseChunk, McpResponseEnd, McpResponseStart},
    redact_upstream::{UpstreamRedactionSession, decrypt_upstream_session},
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
    worker_usage::record_usage_event,
};
use redaction::redact_mcp_request_body_blocking;
use reqwest::StatusCode;
use std::time::Instant;
use streaming::{handle_buffered_transport_response, handle_streaming_transport_response};

use crate::mcp::targeting::extract_mcp_request_metadata;

pub(super) async fn handle_mcp_request(request: BufferedMcpRequest, services: &RuntimeServices) {
    let started = Instant::now();
    let request_content_logging = if let Some(state) = services.admin_state() {
        state.request_content_logging.read().await.clone()
    } else {
        RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        }
    };
    let redact_content = redaction_enabled(services.admin_state());
    let metadata = extract_mcp_request_metadata(
        request.server_name.as_deref(),
        &request.headers,
        &request.body,
    );
    let request_ctx = RequestExecutionContext::for_mcp(
        uuid::Uuid::parse_str(&request.request_id).unwrap_or_else(|_| uuid::Uuid::new_v4()),
        started,
        request.user_id.filter(|id| *id > 0),
        services.runtime_state.worker_instance_id(),
        resolve_mcp_conversation_log(),
    );
    let _request_lease = services
        .runtime_state
        .spawn_request_lease_guard(services.admin_state(), request_ctx.request_id);
    record_usage_event(
        services.admin_state(),
        request_ctx.mcp_usage_log(
            &request,
            &metadata,
            &request_content_logging,
            redact_content,
        ),
    )
    .await;
    let Some(state) = services.admin_state() else {
        let _ = services
            .out_tx
            .send(BridgeMessage::McpResponseStart(McpResponseStart {
                request_id: request.request_id,
                status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                content_type: Some("application/json".to_string()),
                headers: Vec::new(),
            }));
        return;
    };
    let server = if let Some(server_name) = metadata.server_name.as_deref() {
        db::get_visible_mcp_server(&state.pool, request.user_id, server_name)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    if let Some(server) = server.as_ref()
        && let Ok(Some(message)) = check_named_request_budget(
            &state.pool,
            db::RequestRecordCategory::Mcp,
            db::RequestBudgetScope::McpServer(server.server_id),
            "mcp server",
            &server.name,
            server.daily_max_requests,
            server.monthly_max_requests,
        )
        .await
    {
        let body = serde_json::json!({
            "error": {
                "code": "budget_exceeded",
                "message": message,
            }
        })
        .to_string();
        send_mcp_response(
            services,
            &request.request_id,
            StatusCode::TOO_MANY_REQUESTS.as_u16(),
            Some("application/json".to_string()),
            Vec::new(),
            body.clone().into_bytes(),
        );
        record_mcp_request_event(
            &McpResponseContext {
                request: &request,
                request_ctx: &request_ctx,
                metadata: &metadata,
                request_content_logging: &request_content_logging,
                redact_content,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
                selected_token_slot: None,
                server: Some(server),
                services,
            },
            FailurePayload {
                status: StatusCode::TOO_MANY_REQUESTS,
                error_code: "budget_exceeded".to_string(),
                error_message: message,
                upstream_error_body: Some(body),
                response_body: None,
            },
        )
        .await;
        return;
    }
    let conversation_id = request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("x-prompt-ferry-conversation-id"))
        .and_then(|(_, value)| uuid::Uuid::parse_str(value).ok());
    let prior_session = load_prior_session(state, conversation_id).await;
    let (effective_body, upstream_redacted_request_json, upstream_restore_session) =
        if crate::redact::effective_config_for_user(request.user_id.filter(|id| *id > 0)).enabled {
            match redact_mcp_request_body_blocking(
                request.body.clone(),
                request.user_id.filter(|id| *id > 0),
                conversation_id,
                prior_session,
            )
            .await
            {
                Ok(value) => value,
                Err(err) => {
                    let body = serde_json::json!({
                        "error": {"code":"invalid_request","message": err.to_string()}
                    })
                    .to_string();
                    send_mcp_response(
                        services,
                        &request.request_id,
                        StatusCode::BAD_REQUEST.as_u16(),
                        Some("application/json".to_string()),
                        Vec::new(),
                        body.clone().into_bytes(),
                    );
                    record_mcp_request_event(
                        &McpResponseContext {
                            request: &request,
                            request_ctx: &request_ctx,
                            metadata: &metadata,
                            request_content_logging: &request_content_logging,
                            redact_content,
                            upstream_redacted_request_json: None,
                            upstream_restore_session: None,
                            selected_token_slot: None,
                            server: server.as_ref(),
                            services,
                        },
                        FailurePayload {
                            status: StatusCode::BAD_REQUEST,
                            error_code: "invalid_request".to_string(),
                            error_message: err.to_string(),
                            upstream_error_body: Some(body),
                            response_body: None,
                        },
                    )
                    .await;
                    return;
                }
            }
        } else {
            (request.body.clone(), None, None)
        };
    let result = mcp::handle_stream_with_session_store(
        &state.pool,
        &state.mcp_catalog_cache,
        mcp::McpRequestContext {
            user_id: request.user_id,
            server_name: request.server_name.as_deref(),
            method: &request.method,
            path: &request.path,
            headers: &request.headers,
            body: &effective_body,
        },
        state.mcp_session_store.clone(),
    )
    .await;
    let mut response_context = McpResponseContext {
        request: &request,
        request_ctx: &request_ctx,
        metadata: &metadata,
        request_content_logging: &request_content_logging,
        redact_content,
        upstream_redacted_request_json: upstream_redacted_request_json.clone(),
        upstream_restore_session: upstream_restore_session.clone(),
        selected_token_slot: None,
        server: server.as_ref(),
        services,
    };
    match result {
        Ok(mcp::McpTransportResponse::Buffered {
            status,
            content_type,
            headers,
            body,
            selected_token_slot,
        }) => {
            response_context.selected_token_slot = selected_token_slot;
            handle_buffered_transport_response(
                &response_context,
                status,
                content_type,
                headers,
                body,
            )
            .await;
        }
        Ok(mcp::McpTransportResponse::Streaming {
            status,
            content_type,
            headers,
            stream,
            selected_token_slot,
        }) => {
            response_context.selected_token_slot = selected_token_slot;
            handle_streaming_transport_response(
                &response_context,
                status,
                content_type,
                headers,
                stream,
            )
            .await;
        }
        Err(err) => {
            let body = serde_json::json!({
                "error": {
                    "code": "mcp_error",
                    "message": safe_error(&err, redaction_enabled(services.admin_state()), request_ctx.user_id),
                }
            })
            .to_string();
            send_mcp_response(
                services,
                &request.request_id,
                StatusCode::BAD_GATEWAY.as_u16(),
                Some("application/json".to_string()),
                Vec::new(),
                body.clone().into_bytes(),
            );
            record_mcp_request_event(
                &response_context,
                FailurePayload {
                    status: StatusCode::BAD_GATEWAY,
                    error_code: "mcp_error".to_string(),
                    error_message: safe_error(
                        &err,
                        redaction_enabled(services.admin_state()),
                        request_ctx.user_id,
                    ),
                    upstream_error_body: Some(body),
                    response_body: None,
                },
            )
            .await;
        }
    }
}

async fn load_prior_session(
    state: &crate::worker_admin::AdminState,
    conversation_id: Option<uuid::Uuid>,
) -> Option<UpstreamRedactionSession> {
    let conversation_id = conversation_id?;
    let row = db::get_conversation_redaction_session(&state.pool, conversation_id)
        .await
        .ok()
        .flatten()?;
    let session = decrypt_upstream_session(
        state.relay_secret_manager().ok()?,
        &crate::relay_secrets::EncryptedSecretEnvelope {
            ciphertext: row.session_ciphertext,
            nonce: row.session_nonce,
            key_version: row.session_key_version,
        },
    )
    .ok()?;
    Some(session)
}

fn send_mcp_response(
    services: &RuntimeServices,
    request_id: &str,
    status: u16,
    content_type: Option<String>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) {
    let _ = services
        .out_tx
        .send(BridgeMessage::McpResponseStart(McpResponseStart {
            request_id: request_id.to_string(),
            status,
            content_type,
            headers,
        }));
    if !body.is_empty() {
        let _ = services
            .out_tx
            .send(BridgeMessage::McpResponseChunk(McpResponseChunk {
                request_id: request_id.to_string(),
                data: body,
            }));
    }
    let _ = services
        .out_tx
        .send(BridgeMessage::McpResponseEnd(McpResponseEnd {
            request_id: request_id.to_string(),
        }));
}
