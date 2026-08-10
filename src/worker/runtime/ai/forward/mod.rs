mod non_stream;
mod responses_to_chat;

use super::super::{
    ERROR_BODY_SAMPLE_BYTES, RequestExecutionContext,
    context::{RouteExecutionContext, RuntimeServices},
    error_handling::{format_response_raw_body, http_error_message, maybe_redact_text},
    request_assembly::BufferedBridgeRequest,
};
use super::{
    request_support::ai_route_usage_log, streaming::forward_streaming_response,
    upstream::read_response_sample,
};
use crate::{
    chat_replay::{AssistantArtifactCapture, ResponsesArtifactCapture},
    db,
    openai_compat::normalize_response_error,
    upstream_adapter::ResponseAdapter,
    upstream_error::is_quota_exhaustion,
    worker_usage::record_usage_event,
};
use http::header;
use non_stream::{
    forward_non_stream_anthropic_response, forward_non_stream_chat_response,
    forward_non_stream_responses_response, send_json_response,
};
use responses_to_chat::forward_non_stream_responses_to_chat_response;
use tracing::warn;

#[derive(Clone, Copy)]
pub(super) struct ResponseLoggingContext {
    pub(super) redact_content: bool,
    pub(super) content_logging_enabled: bool,
    pub(super) raw_content_logging_enabled: bool,
}

pub(super) struct ResponseForwardContext<'a> {
    pub(super) route_ctx: &'a RouteExecutionContext,
    pub(super) request: &'a BufferedBridgeRequest,
    pub(super) request_ctx: &'a RequestExecutionContext,
    pub(super) upstream_redacted_request_json: Option<serde_json::Value>,
    pub(super) upstream_restore_session: Option<crate::redact_upstream::UpstreamRedactionSession>,
    pub(super) logging: ResponseLoggingContext,
    pub(super) response_adapter: ResponseAdapter,
    pub(super) services: &'a RuntimeServices,
}

impl ResponseForwardContext<'_> {
    pub(super) fn cloned(&self) -> Self {
        Self {
            route_ctx: self.route_ctx,
            request: self.request,
            request_ctx: self.request_ctx,
            upstream_redacted_request_json: self.upstream_redacted_request_json.clone(),
            upstream_restore_session: self.upstream_restore_session.clone(),
            logging: self.logging,
            response_adapter: self.response_adapter,
            services: self.services,
        }
    }
}

pub(super) async fn forward_upstream_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
) -> anyhow::Result<()> {
    let route_ctx = context.route_ctx;
    let request = context.request;
    let request_ctx = context.request_ctx;
    let upstream_redacted_request_json = context.upstream_redacted_request_json.clone();
    let upstream_restore_session = context.upstream_restore_session.clone();
    let redact_content = context.logging.redact_content;
    let response_adapter = context.response_adapter;
    let services = context.services;
    let route = &route_ctx.route;
    let status = response.status();
    let upstream_content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let is_sse = upstream_content_type
        .as_deref()
        .is_some_and(|value| value.contains("text/event-stream"));
    let mut assistant_capture = (route.native_api == crate::config::NativeApi::Chat)
        .then(|| AssistantArtifactCapture::new(is_sse));
    let mut responses_capture = (matches!(
        response_adapter,
        ResponseAdapter::Passthrough
            | ResponseAdapter::ResponsesToChat
            | ResponseAdapter::AnthropicMessagesToResponses
    ) && matches!(
        route.native_api,
        crate::config::NativeApi::Responses | crate::config::NativeApi::AnthropicMessages
    ))
    .then(|| {
        ResponsesArtifactCapture::new(
            is_sse || response_adapter == ResponseAdapter::AnthropicMessagesToResponses,
        )
    });

    if !status.is_success() {
        let body = read_response_sample(response, ERROR_BODY_SAMPLE_BYTES).await;
        let body_text = String::from_utf8_lossy(&body).to_string();
        let client_status = client_status_for_upstream_error(status, &body_text);
        let error_body = (!body_text.trim().is_empty())
            .then(|| maybe_redact_text(&body_text, redact_content, request_ctx.user_id));
        let normalized_error = normalize_response_error(&body_text);
        let normalized_bytes =
            serde_json::to_vec(&normalized_error).unwrap_or_else(|_| body.to_vec());
        send_json_response(
            services,
            &request.request_id,
            client_status.as_u16(),
            normalized_bytes,
        )
        .await?;
        record_usage_event(
            services.admin_state(),
            ai_route_usage_log(request_ctx, request, route_ctx)
                .with_upstream_redaction(
                    upstream_restore_session.is_some(),
                    upstream_redacted_request_json.clone(),
                    upstream_restore_session.clone(),
                )
                .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
                .with_status(
                    Some(client_status.as_u16() as i32),
                    Some(false),
                    Some(request_ctx.elapsed_ms()),
                    None,
                )
                .with_error(
                    Some("http_error".to_string()),
                    Some(http_error_message(status.as_u16(), error_body.as_deref())),
                    error_body.clone(),
                ),
        )
        .await;
        warn!(
            endpoint_id = %route.route_id,
            base_url = %route.base_url,
            native_api = %route.native_api.as_str(),
            path = %request.path,
            status = status.as_u16(),
            "upstream returned non-success status"
        );
        return Ok(());
    }

    if response_adapter == ResponseAdapter::ChatToResponses && !is_sse {
        return forward_non_stream_chat_response(
            response,
            context.cloned(),
            assistant_capture.as_mut(),
        )
        .await;
    }

    if let Some(capture) = responses_capture.as_mut()
        && response_adapter == ResponseAdapter::AnthropicMessagesToResponses
        && !is_sse
    {
        return forward_non_stream_anthropic_response(response, context.cloned(), capture).await;
    }

    if let Some(capture) = responses_capture.as_mut()
        && response_adapter == ResponseAdapter::ResponsesToChat
        && !is_sse
    {
        return forward_non_stream_responses_to_chat_response(response, context.cloned(), capture)
            .await;
    }

    if let Some(capture) = responses_capture.as_mut()
        && !is_sse
    {
        return forward_non_stream_responses_response(response, context.cloned(), capture).await;
    }

    Box::pin(forward_streaming_response(
        response,
        context.cloned(),
        assistant_capture.as_mut(),
        responses_capture.as_mut(),
        upstream_content_type,
        is_sse,
    ))
    .await
}

fn client_status_for_upstream_error(
    upstream_status: http::StatusCode,
    body: &str,
) -> http::StatusCode {
    if !upstream_status.is_success() && is_quota_exhaustion(body) {
        http::StatusCode::TOO_MANY_REQUESTS
    } else {
        upstream_status
    }
}

pub(super) fn response_logging_payload(
    response_text: &str,
    body: &[u8],
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    redact_content: bool,
    user_id: Option<i64>,
) -> (Option<String>, Option<String>) {
    (
        logged_response_prompt(
            response_text,
            content_logging_enabled,
            raw_content_logging_enabled,
            redact_content,
            user_id,
        ),
        logged_response_raw_body(body, raw_content_logging_enabled),
    )
}

fn logged_response_prompt(
    response_text: &str,
    content_logging_enabled: bool,
    raw_content_logging_enabled: bool,
    redact_content: bool,
    user_id: Option<i64>,
) -> Option<String> {
    if !content_logging_enabled || raw_content_logging_enabled {
        return None;
    }

    let text = maybe_redact_text(response_text, redact_content, user_id);
    (!text.is_empty()).then_some(text)
}

pub(super) fn logged_response_raw_body(
    body: &[u8],
    raw_content_logging_enabled: bool,
) -> Option<String> {
    raw_content_logging_enabled
        .then(|| format_response_raw_body(body))
        .flatten()
        .filter(|text| !text.trim().is_empty())
}

#[cfg(test)]
mod status_tests {
    use super::client_status_for_upstream_error;
    use http::StatusCode;

    #[test]
    fn maps_explicit_quota_exhaustion_to_too_many_requests() {
        let status = client_status_for_upstream_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"Token Plan usage limit exhausted"}}"#,
        );

        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn preserves_ordinary_upstream_server_errors() {
        let status = client_status_for_upstream_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":{"message":"temporary provider failure"}}"#,
        );

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}

#[cfg(test)]
mod tests {
    use super::response_logging_payload;
    use crate::redact_test_support::secret_redaction;

    #[test]
    fn normalized_and_raw_logs_only_plain_raw_response() {
        let _guard = secret_redaction();

        let (response_prompt, response_raw_body) = response_logging_payload(
            "API_TOKEN=sk_live_1234567890ABCDEFghij",
            br#"{"secret":"sk_live_1234567890ABCDEFghij"}"#,
            true,
            true,
            true,
            None,
        );

        assert!(response_prompt.is_none());
        let response_raw_body = response_raw_body.expect("raw response body");
        assert!(response_raw_body.contains("sk_live_1234567890ABCDEFghij"));
        assert!(!response_raw_body.contains("[[RDX:"));
    }

    #[test]
    fn normalized_only_keeps_redacted_response_prompt() {
        let _guard = secret_redaction();

        let (response_prompt, response_raw_body) = response_logging_payload(
            "API_TOKEN=sk_live_1234567890ABCDEFghij",
            br#"{"secret":"sk_live_1234567890ABCDEFghij"}"#,
            true,
            false,
            true,
            None,
        );

        let response_prompt = response_prompt.expect("normalized response prompt");
        assert!(!response_prompt.contains("sk_live_1234567890ABCDEFghij"));
        assert!(response_prompt.contains("[[RDX:"));
        assert!(response_raw_body.is_none());
    }

    #[test]
    fn logging_off_skips_both_response_fields() {
        let (response_prompt, response_raw_body) =
            response_logging_payload("done", br#"{"ok":true}"#, false, false, false, None);

        assert!(response_prompt.is_none());
        assert!(response_raw_body.is_none());
    }
}
