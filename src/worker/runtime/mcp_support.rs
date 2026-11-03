use crate::{
    db, mcp::targeting::McpRequestMetadata, redact_upstream::UpstreamRedactionSession,
    worker_admin_types::RequestContentLoggingResponse,
};

use super::{
    RequestExecutionContext,
    context::{FailurePayload, RuntimeServices},
    error_handling::maybe_redact_text,
    request_assembly::BufferedMcpRequest,
};

pub(super) struct McpResponseContext<'a> {
    pub(super) request: &'a BufferedMcpRequest,
    pub(super) request_ctx: &'a RequestExecutionContext,
    pub(super) metadata: &'a McpRequestMetadata,
    pub(super) request_content_logging: &'a RequestContentLoggingResponse,
    pub(super) redact_content: bool,
    pub(super) upstream_redacted_request_json: Option<serde_json::Value>,
    pub(super) upstream_restore_session: Option<UpstreamRedactionSession>,
    pub(super) selected_token_slot: Option<i16>,
    /// The tracker of this request (issue #1179 P2). The settlement reads its
    /// snapshot only after the response stream has been consumed, so a tool
    /// outcome that happens after the response headers still reaches the
    /// record. It carries the endpoint key that served the call and the actual
    /// tool outcome, which the HTTP status alone cannot express.
    pub(super) tracker: Option<&'a crate::mcp::McpCallTracker>,
    pub(super) server: Option<&'a db::McpServer>,
    pub(super) services: &'a RuntimeServices,
}

pub(super) async fn record_mcp_request_event(
    context: &McpResponseContext<'_>,
    failure: FailurePayload,
) {
    let log = mcp_request_log(context, failure);
    Box::pin(context.services.record_usage_event(log)).await;
}

/// The request record of one MCP request.
///
/// The tracked tool call decides the outcome: a tool call that failed inside a
/// successful HTTP response is a failed request, and the record carries the
/// endpoint key that served it.
pub(super) fn mcp_request_log(
    context: &McpResponseContext<'_>,
    failure: FailurePayload,
) -> crate::usage::logging::UsageLog {
    let FailurePayload {
        status,
        error_code,
        error_message,
        upstream_error_body,
        response_body,
    } = failure;
    // Issue #1179 P2: a tool call that failed inside a successful HTTP
    // response is a failed request. The snapshot is taken here, at the actual
    // settlement, so it observes a tool outcome that happened while the
    // response stream was being consumed. It never carries a payload.
    let call_record = context.tracker.map(|tracker| tracker.snapshot());
    let tracked_failure = call_record
        .as_ref()
        .and_then(|record| record.failure())
        .map(|(code, message)| (code.to_string(), message.to_string()));
    let ok = status.is_success() && tracked_failure.is_none();
    let (error_code, error_message) = if !error_code.is_empty() {
        (error_code, error_message)
    } else if let Some((code, message)) = tracked_failure {
        (
            code,
            maybe_redact_text(
                &message,
                context.redact_content,
                context.request_ctx.user_id,
            ),
        )
    } else {
        (String::new(), String::new())
    };
    let response_body = context
        .request_content_logging
        .mode
        .captures_normalized()
        .then(|| {
            response_body
                .as_deref()
                .map(|text| {
                    maybe_redact_text(text, context.redact_content, context.request_ctx.user_id)
                })
                .filter(|text| !text.trim().is_empty())
        })
        .flatten();
    context
        .request_ctx
        .mcp_usage_log(
            context.request,
            context.metadata,
            context.request_content_logging,
            context.redact_content,
        )
        .with_upstream_redaction(
            context.upstream_restore_session.is_some(),
            context.upstream_redacted_request_json.clone(),
            context.upstream_restore_session.clone(),
        )
        .with_state(
            db::UsageEventKind::Request,
            if ok {
                db::RequestRecordState::Completed
            } else {
                db::RequestRecordState::Failed
            },
        )
        .with_mcp_context(
            context.server.map(|value| value.server_id),
            context
                .server
                .map(|value| value.name.clone())
                .or_else(|| context.metadata.server_name.clone()),
            context.metadata.protocol_method.clone(),
            context.metadata.operation_name.clone(),
        )
        .with_endpoint_key(
            call_record
                .as_ref()
                .and_then(|record| record.endpoint_key_id),
            call_record
                .as_ref()
                .and_then(|record| record.endpoint_key_label.clone()),
        )
        .with_mcp_token_slot(match call_record.as_ref() {
            // Issue #1179 P2 repair: a live tracker is authoritative. The slot
            // of the attempt that settled this call wins over the slot captured
            // when the response object was created, so a failover to another key
            // is recorded as the key that actually served it. A tracked call
            // that selected no key — every request that is not a tool call —
            // records no slot instead of inheriting one.
            Some(record) => record.mcp_bearer_token_slot,
            None => context
                .selected_token_slot
                .or(context.metadata.selected_token_slot),
        })
        .with_status(
            Some(status.as_u16() as i32),
            Some(ok),
            Some(context.request_ctx.elapsed_ms()),
            None,
        )
        .with_response(None, None, response_body, None)
        .with_error(
            (!error_code.is_empty()).then_some(error_code),
            (!error_message.is_empty()).then_some(error_message),
            upstream_error_body,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::runtime::context::{BridgeSender, ResponseLimits, RuntimeServices};
    use crate::worker::runtime::{WorkerRuntimeState, request_assembly::BufferedMcpRequest};
    use reqwest::StatusCode;

    fn request() -> BufferedMcpRequest {
        BufferedMcpRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            server_name: Some("minimax-mcp".to_string()),
            method: "POST".to_string(),
            path: "/mcp".to_string(),
            headers: Vec::new(),
            body: br#"{"method":"tools/call"}"#.to_vec(),
            user_id: Some(7),
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
            http_request_decompressed_bytes: None,
            http_request_compression_ratio: None,
        }
    }

    fn services() -> RuntimeServices {
        let (out_tx, _control_rx, _data_rx) = BridgeSender::channel();
        RuntimeServices::new(
            None,
            out_tx,
            reqwest::Client::new(),
            WorkerRuntimeState::default(),
            ResponseLimits::default(),
        )
    }

    fn failure(status: StatusCode) -> FailurePayload {
        FailurePayload {
            status,
            error_code: String::new(),
            error_message: String::new(),
            upstream_error_body: None,
            response_body: None,
        }
    }

    /// The request record of one MCP request at settlement.
    fn request_log(
        tracker: Option<&crate::mcp::McpCallTracker>,
        status: StatusCode,
    ) -> crate::usage::logging::UsageLog {
        request_log_with_slots(tracker, None, None, status)
    }

    /// The same record with the response-time and metadata token slots set.
    fn request_log_with_slots(
        tracker: Option<&crate::mcp::McpCallTracker>,
        selected_token_slot: Option<i16>,
        metadata_token_slot: Option<i16>,
        status: StatusCode,
    ) -> crate::usage::logging::UsageLog {
        let services = services();
        let request = request();
        let request_ctx = crate::worker::runtime::RequestExecutionContext::for_mcp(
            uuid::Uuid::new_v4(),
            std::time::Instant::now(),
            Some(7),
            uuid::Uuid::new_v4(),
            crate::worker::runtime::prompt_log::RequestPromptLog::default(),
        );
        let metadata = crate::mcp::targeting::McpRequestMetadata {
            server_name: Some("minimax-mcp".to_string()),
            protocol_method: Some("tools/call".to_string()),
            operation_name: Some("web_search".to_string()),
            selected_token_slot: metadata_token_slot,
            request_raw_json: None,
        };
        let request_content_logging = crate::worker_admin_types::RequestContentLoggingResponse {
            mode: crate::worker_admin_types::RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        };
        let context = McpResponseContext {
            request: &request,
            request_ctx: &request_ctx,
            metadata: &metadata,
            request_content_logging: &request_content_logging,
            redact_content: false,
            upstream_redacted_request_json: None,
            upstream_restore_session: None,
            selected_token_slot,
            tracker,
            server: None,
            services: &services,
        };
        mcp_request_log(&context, failure(status))
    }

    #[test]
    fn a_tracked_tool_failure_settles_a_successful_transport_as_a_failed_request() {
        let tracker = crate::mcp::McpCallTracker::new();
        let key_id = uuid::Uuid::new_v4();
        tracker.record_endpoint_key(Some(key_id), Some("primary".to_string()));
        tracker.record_token_slot(Some(2));
        tracker.record_failure(crate::mcp::call_record::McpCallFailure::ToolError);

        let log = request_log(Some(&tracker), StatusCode::OK);

        assert_eq!(log.request_state, db::RequestRecordState::Failed);
        assert_eq!(log.ok, Some(false));
        assert_eq!(log.status, Some(200));
        assert_eq!(log.error_code.as_deref(), Some("mcp_tool_error"));
        assert_eq!(
            log.error_message.as_deref(),
            Some("the tool reported an error result")
        );
        assert_eq!(log.endpoint_key_id, Some(key_id));
        assert_eq!(log.endpoint_key_label.as_deref(), Some("primary"));
        assert_eq!(log.mcp_bearer_token_slot, Some(2));
    }

    #[test]
    fn the_snapshot_is_taken_at_settlement_not_at_response_start() {
        // Issue #1179 P2 repair: the stream delivers the tool outcome after
        // the response headers, so the settlement must read the tracker late.
        let tracker = crate::mcp::McpCallTracker::new();
        let key_id = uuid::Uuid::new_v4();

        // The tool runs after the headers: only now does it select the key and
        // report the spent quota.
        tracker.record_endpoint_key(Some(key_id), Some("final".to_string()));
        tracker.record_token_slot(Some(2));
        tracker.record_failure(crate::mcp::call_record::McpCallFailure::QuotaExhausted {
            provider_code: 2056,
        });

        let log = request_log(Some(&tracker), StatusCode::OK);

        assert_eq!(log.request_state, db::RequestRecordState::Failed);
        assert_eq!(log.ok, Some(false));
        assert_eq!(log.error_code.as_deref(), Some("mcp_quota_exhausted"));
        assert_eq!(
            log.error_message.as_deref(),
            Some("the provider reported an exhausted plan quota (2056)")
        );
        assert_eq!(log.endpoint_key_id, Some(key_id));
        assert_eq!(log.endpoint_key_label.as_deref(), Some("final"));
        assert_eq!(log.mcp_bearer_token_slot, Some(2));
    }

    #[test]
    fn a_provider_echo_never_reaches_the_request_record() {
        // A generic JSON-RPC error whose message repeats the bearer token and
        // the request input.
        let tracker = crate::mcp::McpCallTracker::new();
        crate::mcp::call_record::record_response_outcome(
            &tracker,
            &serde_json::json!({
                "error": {
                    "code": -32001,
                    "message": "request with sk-secret for query \"rust\" failed"
                }
            }),
        );

        let log = request_log(Some(&tracker), StatusCode::OK);

        let message = log.error_message.clone().unwrap_or_default();
        assert_eq!(log.request_state, db::RequestRecordState::Failed);
        assert_eq!(log.error_code.as_deref(), Some("mcp_upstream_error"));
        assert!(!message.contains("sk-secret"), "{message}");
        assert!(!message.contains("rust"), "{message}");
    }

    #[test]
    fn a_tracked_success_keeps_the_request_completed() {
        let tracker = crate::mcp::McpCallTracker::new();
        tracker.record_token_slot(Some(1));
        tracker.record_attempt_failure();
        tracker.record_success();

        let log = request_log(Some(&tracker), StatusCode::OK);

        assert_eq!(log.request_state, db::RequestRecordState::Completed);
        assert_eq!(log.ok, Some(true));
        assert!(log.error_code.is_none());
        assert_eq!(log.mcp_bearer_token_slot, Some(1));
    }

    #[test]
    fn the_live_tracker_slot_outranks_the_captured_response_slot() {
        // The response object captured slot 1, then the call failed over to the
        // second key and settled on slot 2. The record must name the key that
        // actually served it.
        let tracker = crate::mcp::McpCallTracker::new();
        tracker.record_endpoint_key(Some(uuid::Uuid::new_v4()), Some("second".to_string()));
        tracker.record_token_slot(Some(2));
        tracker.record_attempt_failure();
        tracker.record_success();

        let log = request_log_with_slots(Some(&tracker), Some(1), Some(3), StatusCode::OK);

        assert_eq!(log.mcp_bearer_token_slot, Some(2));
    }

    #[test]
    fn a_tracked_call_without_a_key_records_no_inherited_slot() {
        // `tools/list` is tracked but never selects a key: the response-time
        // and metadata slots describe another call and must not be inherited.
        let tracker = crate::mcp::McpCallTracker::new();

        let log = request_log_with_slots(Some(&tracker), Some(1), Some(3), StatusCode::OK);

        assert_eq!(log.mcp_bearer_token_slot, None);
    }

    #[test]
    fn an_untracked_request_keeps_the_captured_slot() {
        let log = request_log_with_slots(None, Some(1), Some(3), StatusCode::OK);

        assert_eq!(log.mcp_bearer_token_slot, Some(1));
    }

    #[test]
    fn a_request_without_a_tracked_call_keeps_the_transport_outcome() {
        let log = request_log(None, StatusCode::OK);

        assert_eq!(log.request_state, db::RequestRecordState::Completed);
        assert_eq!(log.ok, Some(true));
        assert!(log.endpoint_key_id.is_none());
    }
}
