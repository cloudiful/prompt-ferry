use reqwest::StatusCode;

use super::preparation::{self, McpExecution};
use super::streaming::{handle_buffered_transport_response, handle_streaming_transport_response};
use super::{send_mcp_response, settle_quota};
use crate::worker::runtime::context::{FailurePayload, RuntimeServices};
use crate::worker::runtime::mcp_support::McpResponseContext;
use crate::worker::runtime::{
    record_mcp_request_event, redaction_enabled, request_assembly::BufferedMcpRequest, safe_error,
};
use crate::{db, mcp};

/// Stage orchestration for one buffered MCP request. Early stages may already
/// have responded (admission failures), so `None` means the request ended.
pub(super) async fn execute_mcp_request(request: BufferedMcpRequest, services: &RuntimeServices) {
    let Some(execution) = Box::pin(preparation::build_request_context(request, services)).await
    else {
        return;
    };
    let Some(execution) =
        Box::pin(preparation::resolve_server_and_quota(execution, services)).await
    else {
        return;
    };
    let Some(execution) = Box::pin(preparation::prepare_upstream(execution, services)).await else {
        return;
    };
    Box::pin(run_transport(execution, services)).await;
}

/// Stage 4: execute the MCP transport with narrowed inputs so the large
/// execution holder is not kept alive inside the transport future.
async fn invoke_mcp_transport(
    state: &mcp::McpRuntimeState,
    request: &BufferedMcpRequest,
    effective_body: &[u8],
    selected_credential: Option<&db::McpCredential>,
) -> anyhow::Result<mcp::McpTransportResponse> {
    mcp::handle_stream_with_storage(
        &state.storage,
        &state.catalog_cache,
        mcp::McpRequestContext {
            user_id: request.user_id,
            server_name: request.server_name.as_deref(),
            method: &request.method,
            path: &request.path,
            headers: &request.headers,
            body: effective_body,
            selected_credential: selected_credential.cloned(),
        },
        state.session_store.clone(),
        &state.allowed_origins,
    )
    .await
}

/// Stage 5: send the final response with usage recording on every outcome.
/// Takes the response context by value plus narrowed references so each
/// branch future stays small; the large transport handlers run behind
/// `Box::pin`.
async fn settle_and_respond(
    services: &RuntimeServices,
    mut context: McpResponseContext<'_>,
    selected_credential: Option<&db::McpCredential>,
    request_id: uuid::Uuid,
    response: anyhow::Result<mcp::McpTransportResponse>,
) {
    match response {
        Ok(mcp::McpTransportResponse::Buffered {
            status,
            content_type,
            headers,
            body,
            selected_token_slot,
        }) => {
            settle_quota(services, selected_credential, request_id, status).await;
            context.selected_token_slot = selected_token_slot;
            Box::pin(handle_buffered_transport_response(
                &context,
                status,
                content_type,
                headers,
                body,
            ))
            .await;
        }
        Ok(mcp::McpTransportResponse::Streaming {
            status,
            content_type,
            headers,
            stream,
            selected_token_slot,
        }) => {
            settle_quota(services, selected_credential, request_id, status).await;
            context.selected_token_slot = selected_token_slot;
            Box::pin(handle_streaming_transport_response(
                &context,
                status,
                content_type,
                headers,
                stream,
            ))
            .await;
        }
        Err(err) => {
            settle_quota(services, selected_credential, request_id, 502).await;
            let body = serde_json::json!({
                "error": {
                    "code": "mcp_error",
                    "message": safe_error(
                        &err,
                        redaction_enabled(services.admin_state()),
                        context.request_ctx.user_id,
                    ),
                }
            })
            .to_string();
            send_mcp_response(
                services,
                &context.request.request_id,
                StatusCode::BAD_GATEWAY.as_u16(),
                Some("application/json".to_string()),
                Vec::new(),
                body.clone().into_bytes(),
            )
            .await;
            record_mcp_request_event(
                &context,
                FailurePayload {
                    status: StatusCode::BAD_GATEWAY,
                    error_code: "mcp_error".to_string(),
                    error_message: safe_error(
                        &err,
                        redaction_enabled(services.admin_state()),
                        context.request_ctx.user_id,
                    ),
                    upstream_error_body: Some(body),
                    response_body: None,
                },
            )
            .await;
        }
    }
}

/// Stage 4 + 5: execute the MCP transport, then send the final response
/// with usage recording on every outcome. Quota groups were removed, so the
/// transport always uses legacy token balancing (`None` credential).
async fn run_transport(mut execution: McpExecution, services: &RuntimeServices) {
    let Some(state) = services.mcp_state() else {
        return;
    };
    let effective_body = execution.effective_body.take().unwrap_or_default();
    let transport = Box::pin(invoke_mcp_transport(
        state,
        &execution.request,
        &effective_body,
        execution.selected_credential.as_ref(),
    ))
    .await;
    drop(effective_body);
    let request_id = execution.request_ctx.request_id;
    let context = McpResponseContext {
        request: &execution.request,
        request_ctx: &execution.request_ctx,
        metadata: &execution.metadata,
        request_content_logging: &execution.request_content_logging,
        redact_content: execution.redact_content,
        upstream_redacted_request_json: execution.upstream_redacted_request_json.take(),
        upstream_restore_session: execution.upstream_restore_session.take(),
        selected_token_slot: None,
        server: execution.server.as_ref(),
        services,
    };
    Box::pin(settle_and_respond(
        services,
        context,
        execution.selected_credential.as_ref(),
        request_id,
        transport,
    ))
    .await;
}
