use reqwest::StatusCode;

use super::preparation::{self, McpExecution};
use super::streaming::{handle_buffered_transport_response, handle_streaming_transport_response};
use super::{send_mcp_response, settle_quota};
use crate::mcp;
use crate::worker::runtime::context::{FailurePayload, RuntimeServices};
use crate::worker::runtime::mcp_support::McpResponseContext;
use crate::worker::runtime::{
    record_mcp_request_event, redaction_enabled, request_assembly::BufferedMcpRequest, safe_error,
};

/// Stage orchestration for one buffered MCP request. Early stages may already
/// have responded (admission failures), so `None` means the request ended.
pub(super) async fn execute_mcp_request(request: BufferedMcpRequest, services: &RuntimeServices) {
    let Some(execution) = preparation::build_request_context(request, services).await else {
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

/// Stage 4 + 5: execute the MCP transport, then settle quota and send the
/// final response with usage recording on every outcome.
async fn run_transport(execution: McpExecution, services: &RuntimeServices) {
    let Some(state) = services.admin_state() else {
        return;
    };
    let result = mcp::handle_stream_with_session_store(
        &state.pool,
        &state.mcp_catalog_cache,
        mcp::McpRequestContext {
            user_id: execution.request.user_id,
            server_name: execution.request.server_name.as_deref(),
            method: &execution.request.method,
            path: &execution.request.path,
            headers: &execution.request.headers,
            body: &execution.effective_body,
            selected_credential: execution
                .budget_grant
                .as_ref()
                .map(|grant| grant.credential.clone()),
        },
        state.mcp_session_store.clone(),
        &state.mcp_allowed_origins,
    )
    .await;
    let mut response_context = McpResponseContext {
        request: &execution.request,
        request_ctx: &execution.request_ctx,
        metadata: &execution.metadata,
        request_content_logging: &execution.request_content_logging,
        redact_content: execution.redact_content,
        upstream_redacted_request_json: execution.upstream_redacted_request_json.clone(),
        upstream_restore_session: execution.upstream_restore_session.clone(),
        selected_token_slot: None,
        server: execution.server.as_ref(),
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
            settle_quota(
                services,
                execution.budget_grant.as_deref(),
                execution.request_ctx.request_id,
                status,
            )
            .await;
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
            settle_quota(
                services,
                execution.budget_grant.as_deref(),
                execution.request_ctx.request_id,
                status,
            )
            .await;
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
            settle_quota(
                services,
                execution.budget_grant.as_deref(),
                execution.request_ctx.request_id,
                502,
            )
            .await;
            let body = serde_json::json!({
                "error": {
                    "code": "mcp_error",
                    "message": safe_error(
                        &err,
                        redaction_enabled(services.admin_state()),
                        execution.request_ctx.user_id,
                    ),
                }
            })
            .to_string();
            send_mcp_response(
                services,
                &execution.request.request_id,
                StatusCode::BAD_GATEWAY.as_u16(),
                Some("application/json".to_string()),
                Vec::new(),
                body.clone().into_bytes(),
            )
            .await;
            record_mcp_request_event(
                &response_context,
                FailurePayload {
                    status: StatusCode::BAD_GATEWAY,
                    error_code: "mcp_error".to_string(),
                    error_message: safe_error(
                        &err,
                        redaction_enabled(services.admin_state()),
                        execution.request_ctx.user_id,
                    ),
                    upstream_error_body: Some(body),
                    response_body: None,
                },
            )
            .await;
        }
    }
}
