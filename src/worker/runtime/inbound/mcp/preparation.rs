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
    /// Body sent upstream. Taken by the transport stage so later stages stop
    /// retaining the heap allocation once the upstream call consumed it.
    pub(super) effective_body: Option<Vec<u8>>,
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
}

/// Send an HTTP error response and record the MCP failure event. The body
/// message may differ from the recorded error message (e.g. quota
/// exhaustion explains the server scope to the caller).
///
/// Narrowed rejection helper: takes only the response context plus the ids
/// needed, so rejection paths pin this small future instead of keeping the
/// whole `McpExecution` holder alive across the send/record awaits.
/// Call-site pattern: build the narrowed response context first, then
/// `Box::pin(send_failure(...)).await`.
async fn send_failure(
    services: &RuntimeServices,
    context: &McpResponseContext<'_>,
    request_id: &str,
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
        request_id,
        status.as_u16(),
        Some("application/json".to_string()),
        Vec::new(),
        body.clone().into_bytes(),
    )
    .await;
    record_mcp_request_event(
        context,
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

/// Narrowed stage 1a: snapshot the request content logging config without
/// holding the buffered request across the lock await.
async fn load_request_content_logging(services: &RuntimeServices) -> RequestContentLoggingResponse {
    if let Some(state) = services.mcp_state() {
        state.request_content_logging.read().await.clone()
    } else {
        RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        }
    }
}

/// Narrowed stage 1b: record the initial usage event without holding the
/// buffered request and context inside the recording future.
async fn record_mcp_admission_event(
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    request: &BufferedMcpRequest,
    metadata: &crate::mcp::targeting::McpRequestMetadata,
    request_content_logging: &RequestContentLoggingResponse,
    redact_content: bool,
) {
    services
        .record_usage_event(request_ctx.mcp_usage_log(
            request,
            metadata,
            request_content_logging,
            redact_content,
        ))
        .await;
}

/// Stage 1: build the request context and record the initial usage event
/// before any admission checks. Each sub-stage runs behind `Box::pin` so the
/// buffered request and context are not inlined into the recording future.
pub(super) async fn build_request_context(
    request: BufferedMcpRequest,
    services: &RuntimeServices,
) -> Option<McpExecution> {
    let started = Instant::now();
    let request_content_logging = Box::pin(load_request_content_logging(services)).await;
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
    let request_lease = services.runtime_state.spawn_request_lease_guard(
        services.admin_state(),
        services.standalone_state(),
        request_ctx.request_id,
    );
    Box::pin(record_mcp_admission_event(
        services,
        &request_ctx,
        &request,
        &metadata,
        &request_content_logging,
        redact_content,
    ))
    .await;
    if services.mcp_state().is_none() {
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
        effective_body: None,
        upstream_redacted_request_json: None,
        upstream_restore_session: None,
        _request_lease: request_lease,
    })
}

/// Rejection computed by the server-budget stage; the caller sends it and
/// ends the request.
struct BudgetRejection {
    status: StatusCode,
    code: &'static str,
    error_message: String,
    body_message: String,
}

/// Narrowed stage 2a: fetch the visible named server without holding the
/// execution holder across the repository await.
async fn fetch_visible_server(
    state: &mcp::McpRuntimeState,
    user_id: Option<i64>,
    server_name: Option<&str>,
) -> Option<db::McpServer> {
    let server_name = server_name?;
    state
        .storage
        .repository()
        .get_visible_mcp_server(user_id, server_name)
        .await
        .ok()
        .flatten()
}

/// Narrowed stage 2b: enforce the named server request budget.
async fn check_server_request_budget(
    state: &mcp::McpRuntimeState,
    server: &db::McpServer,
) -> Option<BudgetRejection> {
    if state.storage.repository().is_sqlite() {
        if server.daily_max_requests.is_some() || server.monthly_max_requests.is_some() {
            let message = crate::db::Capability::McpQuota.description().to_string();
            return Some(BudgetRejection {
                status: StatusCode::NOT_IMPLEMENTED,
                code: crate::db::Capability::McpQuota.as_code(),
                error_message: message.clone(),
                body_message: message,
            });
        }
        return None;
    }
    let message = check_named_request_budget(
        state.storage.postgres_pool().expect("PostgreSQL MCP state"),
        db::RequestRecordCategory::Mcp,
        db::RequestBudgetScope::McpServer(server.server_id),
        "mcp server",
        &server.name,
        server.daily_max_requests,
        server.monthly_max_requests,
    )
    .await
    .ok()
    .flatten()?;
    Some(BudgetRejection {
        status: StatusCode::TOO_MANY_REQUESTS,
        code: "budget_exceeded",
        error_message: message.clone(),
        body_message: message,
    })
}

/// Narrowed stage 2c: reserve credential quota for the resolved server.
async fn acquire_budget_grant(
    state: &mcp::McpRuntimeState,
    server: &db::McpServer,
    request_id: uuid::Uuid,
) -> mcp::QuotaDecision {
    mcp::prepare_quota(
        state.storage.postgres_pool().expect("PostgreSQL MCP state"),
        server.server_id,
        request_id,
        chrono::Utc::now(),
    )
    .await
}

/// Stage 2: resolve the named server, enforce its request budget, and reserve
/// credential quota. Every rejection path sends the same HTTP error and
/// records the failure event before returning `None`. Each sub-stage runs
/// behind `Box::pin` so the `McpExecution` holder is not inlined into the
/// repository/quota futures.
pub(super) async fn resolve_server_and_quota(
    mut execution: McpExecution,
    services: &RuntimeServices,
) -> Option<McpExecution> {
    let state = services.mcp_state()?;
    execution.server = Box::pin(fetch_visible_server(
        state,
        execution.request.user_id,
        execution.metadata.server_name.as_deref(),
    ))
    .await;
    let rejection = match execution.server.as_ref() {
        Some(server) => Box::pin(check_server_request_budget(state, server)).await,
        None => None,
    };
    if let Some(rejection) = rejection {
        let context = execution.response_context(services);
        Box::pin(send_failure(
            services,
            &context,
            &execution.request.request_id,
            rejection.status,
            rejection.code,
            rejection.error_message,
            rejection.body_message,
        ))
        .await;
        return None;
    }
    let conversation_id = execution
        .request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("x-prompt-ferry-conversation-id"))
        .and_then(|(_, value)| uuid::Uuid::parse_str(value).ok());
    let decision = match execution.server.as_ref() {
        Some(server) if !state.storage.repository().is_sqlite() => Some(
            Box::pin(acquire_budget_grant(
                state,
                server,
                execution.request_ctx.request_id,
            ))
            .await,
        ),
        _ => None,
    };
    let budget_grant = match decision {
        None => None,
        Some(mcp::QuotaDecision::Granted { grant }) => Some(grant),
        Some(mcp::QuotaDecision::Unconstrained) => None,
        Some(mcp::QuotaDecision::Exhausted) => {
            let server_name = execution
                .server
                .as_ref()
                .map(|server| server.name.clone())
                .unwrap_or_default();
            let context = execution.response_context(services);
            Box::pin(send_failure(
                services,
                &context,
                &execution.request.request_id,
                StatusCode::TOO_MANY_REQUESTS,
                "budget_exceeded",
                "no credential with remaining budget".to_string(),
                format!("mcp server {server_name} has no credentials with remaining budget"),
            ))
            .await;
            return None;
        }
        Some(mcp::QuotaDecision::Unavailable { reason }) => {
            let context = execution.response_context(services);
            Box::pin(send_failure(
                services,
                &context,
                &execution.request.request_id,
                StatusCode::SERVICE_UNAVAILABLE,
                "quota_unavailable",
                reason.clone(),
                format!("quota ledger unavailable: {reason}"),
            ))
            .await;
            return None;
        }
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
    let upstream_redaction_enabled =
        crate::redact::redaction_enabled_for_user(execution.request.user_id.filter(|id| *id > 0));
    let prior_session = if upstream_redaction_enabled {
        let Some(state) = services.admin_state() else {
            let context = execution.response_context(services);
            Box::pin(send_failure(
                services,
                &context,
                &execution.request.request_id,
                StatusCode::NOT_IMPLEMENTED,
                "sqlite_raw_payload_retention_unavailable",
                "SQLite MCP upstream redaction session persistence is unavailable".to_string(),
                "SQLite MCP upstream redaction session persistence is unavailable".to_string(),
            ))
            .await;
            return None;
        };
        match load_prior_session(state, execution.conversation_id).await {
            Ok(session) => session,
            Err(err) => {
                let context = execution.response_context(services);
                Box::pin(send_failure(
                    services,
                    &context,
                    &execution.request.request_id,
                    err.status,
                    &err.code,
                    err.message.clone(),
                    err.message,
                ))
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
                    let context = execution.response_context(services);
                    Box::pin(send_failure(
                        services,
                        &context,
                        &execution.request.request_id,
                        StatusCode::BAD_REQUEST,
                        "redaction_failed",
                        err.to_string(),
                        err.to_string(),
                    ))
                    .await;
                    return None;
                }
            }
        } else {
            (execution.request.body.clone(), None, None)
        };
    execution.effective_body = Some(effective_body);
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
