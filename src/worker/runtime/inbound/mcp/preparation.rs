use std::time::Instant;

use reqwest::StatusCode;

use super::redaction::redact_mcp_request_body_blocking;
use super::send_mcp_response;
use crate::mcp::targeting::extract_mcp_request_metadata;
use crate::worker::runtime::context::{FailurePayload, RuntimeServices};
use crate::worker::runtime::lifecycle::RequestLeaseGuard;
use crate::worker::runtime::{
    RequestExecutionContext, check_named_request_budget, mcp_support::McpResponseContext,
    record_mcp_request_event, redaction_enabled, request_assembly::BufferedMcpRequest,
    resolve_mcp_conversation_log,
};
use crate::{
    db, mcp,
    protocol::{BridgeMessage, McpResponseStart},
    redact_upstream::{UpstreamRedactionSession, decrypt_upstream_session},
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
    worker_usage::record_usage_event,
};

/// Owned state shared across the MCP request stages: request context and
/// usage recording, server resolution and quota reservation, upstream
/// redaction, transport execution, and final quota settle and response.
///
/// Holds the request lease so it covers the entire MCP request lifecycle.
pub(super) struct McpExecution {
    pub(super) request: BufferedMcpRequest,
    pub(super) request_ctx: RequestExecutionContext,
    pub(super) metadata: crate::mcp::targeting::McpRequestMetadata,
    pub(super) request_content_logging: RequestContentLoggingResponse,
    pub(super) redact_content: bool,
    pub(super) server: Option<db::McpServer>,
    pub(super) budget_grant: Option<Box<db::QuotaGrant>>,
    pub(super) conversation_id: Option<uuid::Uuid>,
    pub(super) effective_body: Vec<u8>,
    pub(super) upstream_redacted_request_json: Option<serde_json::Value>,
    pub(super) upstream_restore_session: Option<UpstreamRedactionSession>,
    _request_lease: Option<RequestLeaseGuard>,
}

impl McpExecution {
    fn response_context<'a>(&'a self, services: &'a RuntimeServices) -> McpResponseContext<'a> {
        McpResponseContext {
            request: &self.request,
            request_ctx: &self.request_ctx,
            metadata: &self.metadata,
            request_content_logging: &self.request_content_logging,
            redact_content: self.redact_content,
            upstream_redacted_request_json: self.upstream_redacted_request_json.clone(),
            upstream_restore_session: self.upstream_restore_session.clone(),
            selected_token_slot: None,
            server: self.server.as_ref(),
            services,
        }
    }

    /// Send an HTTP error response and record the MCP failure event. The body
    /// message may differ from the recorded error message (e.g. quota
    /// exhaustion explains the server scope to the caller).
    pub(super) async fn send_failure(
        &self,
        services: &RuntimeServices,
        status: StatusCode,
        error_code: &str,
        error_message: String,
        body_message: String,
    ) {
        let body = serde_json::json!({
            "error": {
                "code": error_code,
                "message": body_message,
            }
        })
        .to_string();
        send_mcp_response(
            services,
            &self.request.request_id,
            status.as_u16(),
            Some("application/json".to_string()),
            Vec::new(),
            body.clone().into_bytes(),
        )
        .await;
        record_mcp_request_event(
            &self.response_context(services),
            FailurePayload {
                status,
                error_code: error_code.to_string(),
                error_message,
                upstream_error_body: Some(body),
                response_body: None,
            },
        )
        .await;
    }
}

/// Stage 1: build the request context and record the initial usage event
/// before any admission checks.
pub(super) async fn build_request_context(
    request: BufferedMcpRequest,
    services: &RuntimeServices,
) -> Option<McpExecution> {
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
    let request_lease = services
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
    if services.admin_state().is_none() {
        let _ = services
            .out_tx
            .send(BridgeMessage::McpResponseStart(McpResponseStart {
                request_id: request.request_id,
                status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                content_type: Some("application/json".to_string()),
                headers: Vec::new(),
            }))
            .await;
        return None;
    }
    Some(McpExecution {
        request,
        request_ctx,
        metadata,
        request_content_logging,
        redact_content,
        server: None,
        budget_grant: None,
        conversation_id: None,
        effective_body: Vec::new(),
        upstream_redacted_request_json: None,
        upstream_restore_session: None,
        _request_lease: request_lease,
    })
}

/// Stage 2: resolve the named server, enforce its request budget, and reserve
/// credential quota. Every rejection path sends the same HTTP error and
/// records the failure event before returning `None`.
pub(super) async fn resolve_server_and_quota(
    mut execution: McpExecution,
    services: &RuntimeServices,
) -> Option<McpExecution> {
    let state = services.admin_state()?;
    let server = if let Some(server_name) = execution.metadata.server_name.as_deref() {
        db::get_visible_mcp_server(&state.pool, execution.request.user_id, server_name)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    execution.server = server;
    if let Some(server) = execution.server.as_ref()
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
        execution
            .send_failure(
                services,
                StatusCode::TOO_MANY_REQUESTS,
                "budget_exceeded",
                message.clone(),
                message,
            )
            .await;
        return None;
    }
    let conversation_id = execution
        .request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("x-prompt-ferry-conversation-id"))
        .and_then(|(_, value)| uuid::Uuid::parse_str(value).ok());
    let budget_grant = match execution.server.as_ref() {
        Some(server) => {
            match mcp::prepare_quota(
                &state.pool,
                server.server_id,
                execution.request_ctx.request_id,
                chrono::Utc::now(),
            )
            .await
            {
                mcp::QuotaDecision::Granted { grant } => Some(grant),
                mcp::QuotaDecision::Unconstrained => None,
                mcp::QuotaDecision::Exhausted => {
                    execution
                        .send_failure(
                            services,
                            StatusCode::TOO_MANY_REQUESTS,
                            "budget_exceeded",
                            "no credential with remaining budget".to_string(),
                            format!(
                                "mcp server {} has no credentials with remaining budget",
                                server.name
                            ),
                        )
                        .await;
                    return None;
                }
                mcp::QuotaDecision::Unavailable { reason } => {
                    execution
                        .send_failure(
                            services,
                            StatusCode::SERVICE_UNAVAILABLE,
                            "quota_unavailable",
                            reason.clone(),
                            format!("quota ledger unavailable: {reason}"),
                        )
                        .await;
                    return None;
                }
            }
        }
        None => None,
    };
    execution.conversation_id = conversation_id;
    execution.budget_grant = budget_grant;
    Some(execution)
}

/// Stage 3: restore and redact the upstream session and request body.
pub(super) async fn prepare_upstream(
    mut execution: McpExecution,
    services: &RuntimeServices,
) -> Option<McpExecution> {
    let state = services.admin_state()?;
    let upstream_redaction_enabled =
        crate::redact::redaction_enabled_for_user(execution.request.user_id.filter(|id| *id > 0));
    let prior_session = if upstream_redaction_enabled {
        match load_prior_session(state, execution.conversation_id).await {
            Ok(session) => session,
            Err(err) => {
                execution
                    .send_failure(
                        services,
                        err.status,
                        &err.code,
                        err.message.clone(),
                        err.message,
                    )
                    .await;
                return None;
            }
        }
    } else {
        None
    };
    let (effective_body, upstream_redacted_request_json, upstream_restore_session) =
        if upstream_redaction_enabled {
            match redact_mcp_request_body_blocking(
                execution.request.body.clone(),
                execution.request.user_id.filter(|id| *id > 0),
                execution.conversation_id,
                prior_session,
            )
            .await
            {
                Ok(prepared) => (
                    prepared.body,
                    prepared.redacted_request_json,
                    prepared.restore_session,
                ),
                Err(err) => {
                    execution
                        .send_failure(
                            services,
                            StatusCode::BAD_REQUEST,
                            "redaction_failed",
                            err.to_string(),
                            err.to_string(),
                        )
                        .await;
                    return None;
                }
            }
        } else {
            (execution.request.body.clone(), None, None)
        };
    execution.effective_body = effective_body;
    execution.upstream_redacted_request_json = upstream_redacted_request_json;
    execution.upstream_restore_session = upstream_restore_session;
    Some(execution)
}

async fn load_prior_session(
    state: &crate::worker_admin::AdminState,
    conversation_id: Option<uuid::Uuid>,
) -> Result<Option<UpstreamRedactionSession>, crate::openai_compat::CompatError> {
    let Some(conversation_id) = conversation_id else {
        return Ok(None);
    };
    let row = db::get_conversation_redaction_session(&state.pool, conversation_id)
        .await
        .map_err(|err| {
            crate::openai_compat::CompatError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "redaction_session_load_failed",
                format!("failed to load upstream redaction session: {err}"),
            )
        })?;
    let Some(row) = row else {
        return Ok(None);
    };
    let manager = state.relay_secret_manager().map_err(|err| {
        crate::openai_compat::CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "redaction_session_load_failed",
            format!("failed to initialize upstream redaction session secrets: {err}"),
        )
    })?;
    let session = decrypt_upstream_session(
        manager,
        &crate::relay_secrets::EncryptedSecretEnvelope {
            ciphertext: row.session_ciphertext,
            nonce: row.session_nonce,
            key_version: row.session_key_version,
        },
    )
    .map_err(|err| {
        crate::openai_compat::CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "redaction_session_load_failed",
            format!("failed to decrypt upstream redaction session: {err}"),
        )
    })?;
    Ok(Some(session))
}
