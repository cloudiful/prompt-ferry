use anyhow::anyhow;
use http::Method;
use std::error::Error as _;

use crate::{
    db,
    openai_compat::CompatError,
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
    upstream_adapter::{PreparedRequestBody, PreparedUpstreamRequest},
};

use super::super::{
    RequestExecutionContext,
    context::{RouteExecutionContext, RuntimeServices},
    materialize_route_api_key_selection_with_quota,
    request_assembly::{BufferedBridgeRequest, RequestCancellation},
};
use super::{
    forward::{
        QuotaFailoverSignal, ResponseForwardContext, ResponseLoggingContext,
        ThinkingEchoRetrySignal, forward_upstream_response, respond_upstream_error,
    },
    proxy,
    request_logging::log_prepared_upstream_summary,
    request_support::{ai_route_usage_log, prepare_upstream_request_for_route},
    thinking_downgrade::{self, ThinkingDisposition},
    upstream::{build_upstream_request, upstream_url_for_route},
};

const MAX_UPSTREAM_ATTEMPTS: usize = 3;
const RETRY_BACKOFF_MS: [u64; 2] = [250, 1000];
/// Maximum number of same-endpoint API key rotations for a non-stream
/// upstream quota-exhaustion response before the error is surfaced.
const MAX_QUOTA_FAILOVERS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UpstreamFailurePhase {
    BeforeResponseHeaders,
    BufferedResponseBody,
    CommittedStream,
    LocalProcessing,
    RelayBridge,
}

impl UpstreamFailurePhase {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::BeforeResponseHeaders => "before_response_headers",
            Self::BufferedResponseBody => "buffered_response_body",
            Self::CommittedStream => "committed_stream",
            Self::LocalProcessing => "local_processing",
            Self::RelayBridge => "relay_bridge",
        }
    }

    pub(super) fn is_transient(self, err: &reqwest::Error) -> bool {
        match self {
            Self::BeforeResponseHeaders => is_transient_before_headers(err),
            Self::BufferedResponseBody => is_transient_buffered_body(err),
            Self::CommittedStream | Self::LocalProcessing | Self::RelayBridge => false,
        }
    }
}

#[derive(Debug)]
pub(super) struct UpstreamAttemptFailure {
    pub(super) phase: UpstreamFailurePhase,
    pub(super) error: anyhow::Error,
    pub(super) retryable: bool,
}

impl std::fmt::Display for UpstreamAttemptFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.error)
    }
}

impl std::error::Error for UpstreamAttemptFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}

#[derive(Debug)]
pub(super) enum ForwardOutcome {
    Handled,
    CompatError(CompatError),
    TransportError {
        error: anyhow::Error,
        terminal_recorded: bool,
    },
}

pub(super) struct RouteForwardRequest<'a> {
    pub(super) services: &'a RuntimeServices,
    pub(super) request: &'a BufferedBridgeRequest,
    pub(super) request_ctx: &'a RequestExecutionContext,
    pub(super) route: &'a db::RouteConfig,
    pub(super) method: &'a Method,
    pub(super) redact_content: bool,
    pub(super) content_logging_enabled: bool,
    pub(super) raw_content_logging_enabled: bool,
}

pub(super) async fn forward_route_request(
    input: RouteForwardRequest<'_>,
) -> anyhow::Result<ForwardOutcome> {
    let RouteForwardRequest {
        services,
        request,
        request_ctx,
        route,
        method,
        redact_content,
        content_logging_enabled,
        raw_content_logging_enabled,
    } = input;
    let mut route = route.clone();

    let prepared = match prepare_upstream_request_for_route(
        services.admin_state(),
        &route,
        request,
        request_ctx.request_prompt_log.conversation_id,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(err) => return Ok(ForwardOutcome::CompatError(err)),
    };
    let upstream_url = upstream_url_for_route(&route, &prepared.path);
    if let Some(state) = services.admin_state() {
        let _ = db::record_request_state(
            &state.pool,
            db::RequestRecordStateInput {
                request_id: request_ctx.request_id,
                request_state: db::RequestRecordState::UpstreamProcessing,
                endpoint_id: Some(route.route_id).filter(|id| !id.is_nil()),
                model_route_rule_id: route.model_route_rule_id,
                model: request_ctx.request_model.as_deref(),
                endpoint_key_id: route.endpoint_key_id,
                endpoint_key_label: route.endpoint_key_label.as_deref(),
            },
        )
        .await;
    }
    log_prepared_upstream_summary(&route, &prepared);
    // Issue #502 Task 4: ferry-side self-summarize compact. Non-Responses
    // targets with `compact_mode=self_summarize` never call upstream
    // compact; prune + one same-route summarize call produce the plaintext
    // `response.compaction` body instead.
    if prepared.response_adapter == crate::upstream_adapter::ResponseAdapter::SelfSummarizeLocal {
        return Box::pin(handle_self_summarize_compact(
            services,
            request,
            request_ctx,
            &route,
            &prepared,
            ResponseLoggingContext {
                redact_content,
                content_logging_enabled,
                raw_content_logging_enabled,
            },
        ))
        .await;
    }
    let cancellation = services
        .runtime_state
        .request_cancellation(request.request_id.as_str())
        .await;
    // Issue #556: pre-flight thinking downgrade. The decision reads the final
    // outbound body (translation and the `teo` effort override already
    // applied) plus the parent artifact chain, and only rewrites a second body
    // copy: the first attempt keeps its requested thinking unless the decision
    // proves this turn has no reasoning to pass back.
    let thinking_downgrade = thinking_downgrade::resolve_thinking_disposition(
        services.admin_state(),
        request_ctx.request_prompt_log.parent_event_id,
        &route,
        prepared_body_bytes(&prepared.body),
    )
    .await;
    if thinking_downgrade.is_downgraded() {
        tracing::info!(
            event = "thinking_downgrade",
            request_id = %request_ctx.request_id,
            conversation_id = %conversation_id_for_log(request_ctx),
            endpoint_id = %route.route_id,
            provider = route.provider.as_str(),
            native_api = route.native_api.as_str(),
            disposition = thinking_downgrade.as_str(),
            "parent turn carries no reasoning to pass back; disabling thinking for this turn"
        );
    }
    // The escape hatch keeps both the pre-flight rewrite and the fingerprint
    // retry off, so a bypassed deployment forwards byte-identical requests.
    // Issue #566: the per-target switch gates both paths the same way, so a
    // target that never opted in keeps the pre-#556 single attempt.
    let thinking_downgrade_bypassed = thinking_downgrade::thinking_downgrade_bypassed();
    let thinking_downgrade_enabled =
        !thinking_downgrade_bypassed && thinking_downgrade::thinking_downgrade_enabled_for(&route);
    // The downgraded copy is only materialized when a downgrade is actually
    // going to be sent: the pre-flight decision here, or a fingerprint
    // rejection later.
    let mut downgraded_body = if thinking_downgrade_enabled && thinking_downgrade.is_downgraded() {
        downgrade_prepared_body(&route, &prepared.body)
    } else {
        None
    };
    let mut thinking_off = downgraded_body.is_some();
    let mut thinking_retried = false;
    let mut pending_thinking_echo: Option<ThinkingEchoRetrySignal> = None;
    let mut retried = false;
    let mut last_retried_phase = None;
    let mut attempt = 0usize;
    let mut quota_failovers = 0usize;
    loop {
        let attempt_number = attempt + 1;
        let route_ctx = RouteExecutionContext::new(&route);
        let response_ctx = ResponseForwardContext {
            route_ctx: &route_ctx,
            request,
            request_ctx,
            upstream_redacted_request_json: prepared.upstream_redacted_request_json.clone(),
            upstream_restore_session: prepared.upstream_restore_session.clone(),
            logging: ResponseLoggingContext {
                redact_content,
                content_logging_enabled,
                raw_content_logging_enabled,
            },
            response_adapter: prepared.response_adapter,
            services,
            quota_failover_enabled: !route.api_keys.is_empty(),
        };
        // Issue #368 Phase D: per-route proxy client. Direct routes clone
        // the shared client unchanged; proxy routes use the pooled client.
        // Invalid proxy fails closed without echoing userinfo.
        let proxy_client = match proxy::client_for_route(&services.client, &route) {
            Ok(client) => client,
            Err(message) => {
                tracing::warn!(
                    event = "invalid_proxy_url",
                    request_id = %request_ctx.request_id,
                    endpoint_id = %route.route_id,
                    proxy = %crate::db::redact_proxy_url_for_log(
                        route.proxy_url.as_deref().unwrap_or_default()
                    ),
                    "rejecting upstream request with invalid proxy configuration"
                );
                return Ok(ForwardOutcome::TransportError {
                    error: anyhow!("{message}"),
                    terminal_recorded: false,
                });
            }
        };
        if !proxy::redacted_proxy_for_log(&route).is_empty() {
            tracing::debug!(
                event = "upstream_via_proxy",
                request_id = %request_ctx.request_id,
                endpoint_id = %route.route_id,
                proxy = %proxy::redacted_proxy_for_log(&route),
                "sending upstream request via configured proxy"
            );
        }
        let send_result = build_upstream_request(
            &proxy_client,
            method,
            &upstream_url,
            &route,
            attempt_body(&prepared, downgraded_body.as_ref(), thinking_off),
            &request.headers,
            request_ctx.request_prompt_log.conversation_id,
        )
        .send()
        .await;
        match send_result {
            Err(err) => {
                let retryable = UpstreamFailurePhase::BeforeResponseHeaders.is_transient(&err);
                let failure = UpstreamAttemptFailure {
                    phase: UpstreamFailurePhase::BeforeResponseHeaders,
                    error: anyhow!(err).context("upstream request failed"),
                    retryable,
                };
                if failure.retryable && attempt_number < MAX_UPSTREAM_ATTEMPTS {
                    if retry_after_backoff(request_ctx, &route, attempt, &failure, &cancellation)
                        .await
                    {
                        retried = true;
                        last_retried_phase = Some(failure.phase);
                        attempt += 1;
                        continue;
                    }
                    return Ok(ForwardOutcome::TransportError {
                        error: failure.error,
                        terminal_recorded: false,
                    });
                }
                if failure.retryable {
                    log_retry_exhausted(request_ctx, &route, attempt_number, &failure);
                }
                return Ok(ForwardOutcome::TransportError {
                    error: failure.error,
                    terminal_recorded: false,
                });
            }
            Ok(response) => {
                let outcome =
                    Box::pin(handle_attempt_response(response, response_ctx.cloned())).await?;
                match outcome {
                    AttemptOutcome::Handled => {
                        if retried {
                            log_retry_succeeded(
                                request_ctx,
                                &route,
                                attempt_number,
                                last_retried_phase,
                            );
                        }
                        if thinking_retried {
                            tracing::info!(
                                event = "thinking_echo_retry_sent",
                                request_id = %request_ctx.request_id,
                                conversation_id = %conversation_id_for_log(request_ctx),
                                endpoint_id = %route.route_id,
                                attempt = attempt_number,
                                disposition = ThinkingDisposition::DowngradedNoReasoning.as_str(),
                                "thinking-off resend was no longer rejected by the reasoning-echo fingerprint"
                            );
                        }
                        return Ok(ForwardOutcome::Handled);
                    }
                    AttemptOutcome::ThinkingEchoRetry(signal) => {
                        if !thinking_retried && !thinking_off && thinking_downgrade_enabled {
                            if downgraded_body.is_none() {
                                downgraded_body = downgrade_prepared_body(&route, &prepared.body);
                            }
                            if downgraded_body.is_some() {
                                thinking_retried = true;
                                thinking_off = true;
                                tracing::warn!(
                                    event = "thinking_echo_retry",
                                    request_id = %request_ctx.request_id,
                                    conversation_id = %conversation_id_for_log(request_ctx),
                                    endpoint_id = %route.route_id,
                                    provider = route.provider.as_str(),
                                    native_api = route.native_api.as_str(),
                                    attempt = attempt_number,
                                    status = signal.status.as_u16(),
                                    disposition =
                                        ThinkingDisposition::DowngradedNoReasoning.as_str(),
                                    "upstream rejected the turn for a missing reasoning echo; resending once with thinking off"
                                );
                                pending_thinking_echo = Some(signal);
                                attempt += 1;
                                continue;
                            }
                        }
                        tracing::warn!(
                            event = "thinking_echo_retry_rejected",
                            request_id = %request_ctx.request_id,
                            conversation_id = %conversation_id_for_log(request_ctx),
                            endpoint_id = %route.route_id,
                            attempt = attempt_number,
                            status = signal.status.as_u16(),
                            thinking_retried,
                            "thinking-off resend was not available or was rejected again; returning the original upstream error"
                        );
                        let signal = pending_thinking_echo.take().unwrap_or(signal);
                        Box::pin(respond_upstream_error(
                            &response_ctx,
                            signal.status,
                            signal.body,
                            signal.response_headers,
                        ))
                        .await?;
                        return Ok(ForwardOutcome::Handled);
                    }
                    AttemptOutcome::QuotaFailover(signal) => {
                        if let Some(failover_route) = next_quota_failover_route(
                            &route,
                            request,
                            request_ctx,
                            services,
                            quota_failovers,
                        ) {
                            log_quota_failover_retry(request_ctx, &route, &failover_route);
                            quota_failovers += 1;
                            route = failover_route;
                            attempt += 1;
                            continue;
                        }
                        respond_upstream_error(
                            &response_ctx,
                            signal.status,
                            signal.body,
                            signal.response_headers,
                        )
                        .await?;
                        return Ok(ForwardOutcome::Handled);
                    }
                    AttemptOutcome::Failure(failure) => {
                        if failure.retryable && attempt_number < MAX_UPSTREAM_ATTEMPTS {
                            if retry_after_backoff(
                                request_ctx,
                                &route,
                                attempt,
                                &failure,
                                &cancellation,
                            )
                            .await
                            {
                                retried = true;
                                last_retried_phase = Some(failure.phase);
                                attempt += 1;
                                continue;
                            }
                            return Ok(ForwardOutcome::TransportError {
                                error: failure.error,
                                terminal_recorded: false,
                            });
                        }
                        if failure.retryable {
                            log_retry_exhausted(request_ctx, &route, attempt_number, &failure);
                        }
                        return Ok(ForwardOutcome::TransportError {
                            error: failure.error,
                            terminal_recorded: failure.phase
                                == UpstreamFailurePhase::CommittedStream,
                        });
                    }
                }
            }
        }
    }
}

enum AttemptOutcome {
    Handled,
    Failure(UpstreamAttemptFailure),
    QuotaFailover(QuotaFailoverSignal),
    ThinkingEchoRetry(ThinkingEchoRetrySignal),
}

/// Issue #502 Task 4: ferry-side self-summarize compact for non-Responses
/// targets with `compact_mode=self_summarize`. Prunes the compact input
/// (drops all `encrypted_content`, trims tool history), runs one same-route
/// upstream summarize call, and returns the plaintext `response.compaction`
/// body. The summarize call targets the native path (never compact), so
/// compact never recurses.
async fn handle_self_summarize_compact(
    services: &RuntimeServices,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    route: &db::RouteConfig,
    prepared: &PreparedUpstreamRequest,
    logging: ResponseLoggingContext,
) -> anyhow::Result<ForwardOutcome> {
    let route_ctx = RouteExecutionContext::new(route);
    let fail = |code: &'static str, message: String| {
        ForwardOutcome::CompatError(CompatError::new(
            reqwest::StatusCode::BAD_GATEWAY,
            code,
            message,
        ))
    };
    let request_bytes = match &prepared.body {
        PreparedRequestBody::PassthroughStream(bytes)
        | PreparedRequestBody::BufferedBytes(bytes) => bytes.clone(),
    };
    let summarize_path = route.native_api.path().to_string();
    let summarize_url = upstream_url_for_route(route, &summarize_path);
    let proxy_client = match proxy::client_for_route(&services.client, route) {
        Ok(client) => client,
        Err(message) => return Ok(fail("invalid_proxy_url", message)),
    };
    let model_fallback = route
        .upstream_model
        .as_deref()
        .or(request_ctx.request_model.as_deref());
    let (body, summary) = match super::super::compaction::run_self_summarize_compact(
        &proxy_client,
        &summarize_url,
        &route.api_key,
        route.native_api,
        &request_bytes,
        model_fallback,
    )
    .await
    {
        Ok(ok) => ok,
        Err(err) => {
            return Ok(fail(
                "compact_summarize_failed",
                format!("self-summarize compact failed: {err}"),
            ));
        }
    };
    if let Err(err) = send_compact_json(services, &request.request_id, body.clone()).await {
        return Ok(ForwardOutcome::TransportError {
            error: err,
            terminal_recorded: false,
        });
    }
    let response_prompt = logging
        .content_logging_enabled
        .then(|| {
            super::super::error_handling::maybe_redact_text(
                &summary,
                logging.redact_content,
                request_ctx.user_id,
            )
        })
        .filter(|text| !text.is_empty());
    let response_raw_body = logging
        .raw_content_logging_enabled
        .then(|| String::from_utf8_lossy(&body).to_string())
        .filter(|text| !text.trim().is_empty());
    services
        .record_usage_event(
            ai_route_usage_log(request_ctx, request, &route_ctx)
                .with_upstream_redaction(
                    prepared.upstream_restore_session.is_some(),
                    prepared.upstream_redacted_request_json.clone(),
                    prepared.upstream_restore_session.clone(),
                )
                .with_state(
                    db::UsageEventKind::Request,
                    db::RequestRecordState::Completed,
                )
                .with_model(request_ctx.request_model.clone())
                .with_status(Some(200), Some(true), Some(request_ctx.elapsed_ms()), None)
                .with_response(
                    None,
                    request_ctx
                        .request_prompt_log
                        .request_conversation_key
                        .clone(),
                    response_prompt,
                    response_raw_body,
                ),
        )
        .await;
    Ok(ForwardOutcome::Handled)
}

/// Issue #556: bytes of the prepared outbound body, independent of the
/// passthrough/buffered classification.
fn prepared_body_bytes(body: &PreparedRequestBody) -> &[u8] {
    match body {
        PreparedRequestBody::PassthroughStream(bytes)
        | PreparedRequestBody::BufferedBytes(bytes) => bytes.as_slice(),
    }
}

/// Issue #556: a copy of the outbound body with thinking turned off, or
/// `None` when the route's protocol (or the body itself) cannot be rewritten
/// — the retry is only useful when the resend actually differs.
fn downgrade_prepared_body(
    route: &db::RouteConfig,
    body: &PreparedRequestBody,
) -> Option<PreparedRequestBody> {
    match body {
        PreparedRequestBody::PassthroughStream(bytes) => {
            let rewritten = thinking_downgrade::apply_thinking_off(route.native_api, bytes.clone());
            (rewritten != *bytes).then_some(PreparedRequestBody::PassthroughStream(rewritten))
        }
        PreparedRequestBody::BufferedBytes(bytes) => {
            let rewritten = thinking_downgrade::apply_thinking_off(route.native_api, bytes.clone());
            (rewritten != *bytes).then_some(PreparedRequestBody::BufferedBytes(rewritten))
        }
    }
}

fn attempt_body<'a>(
    prepared: &'a PreparedUpstreamRequest,
    downgraded: Option<&'a PreparedRequestBody>,
    thinking_off: bool,
) -> &'a PreparedRequestBody {
    if thinking_off {
        downgraded.unwrap_or(&prepared.body)
    } else {
        &prepared.body
    }
}

fn conversation_id_for_log(request_ctx: &RequestExecutionContext) -> String {
    request_ctx
        .request_prompt_log
        .conversation_id
        .map(|id| id.to_string())
        .unwrap_or_default()
}

async fn send_compact_json(
    services: &RuntimeServices,
    request_id: &str,
    body: Vec<u8>,
) -> anyhow::Result<()> {
    use anyhow::Context as _;
    services
        .out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request_id.to_string(),
            status: 200,
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request_id.to_string(),
            data: body,
        }))
        .await
        .context("relay response channel closed")?;
    services
        .out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request_id.to_string(),
        }))
        .await
        .context("relay response channel closed")?;
    Ok(())
}

async fn handle_attempt_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
) -> anyhow::Result<AttemptOutcome> {
    match Box::pin(forward_upstream_response(response, context)).await {
        Ok(()) => Ok(AttemptOutcome::Handled),
        Err(err) => match err.downcast::<UpstreamAttemptFailure>() {
            Ok(failure) => Ok(AttemptOutcome::Failure(failure)),
            Err(err) => match err.downcast::<QuotaFailoverSignal>() {
                Ok(signal) => Ok(AttemptOutcome::QuotaFailover(signal)),
                Err(err) => match err.downcast::<ThinkingEchoRetrySignal>() {
                    Ok(signal) => Ok(AttemptOutcome::ThinkingEchoRetry(signal)),
                    Err(err) => {
                        let phase = if super::super::context::is_bridge_send_error(&err) {
                            UpstreamFailurePhase::RelayBridge
                        } else {
                            UpstreamFailurePhase::LocalProcessing
                        };
                        Ok(AttemptOutcome::Failure(UpstreamAttemptFailure {
                            phase,
                            error: err,
                            retryable: false,
                        }))
                    }
                },
            },
        },
    }
}

/// Re-select an API key on the same endpoint after a live quota-exhaustion
/// response, excluding the key that just failed. Returns `None` when there
/// is no other key to rotate to or the failover budget is exhausted.
fn next_quota_failover_route(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_ctx: &RequestExecutionContext,
    services: &RuntimeServices,
    quota_failovers: usize,
) -> Option<db::RouteConfig> {
    if quota_failovers >= MAX_QUOTA_FAILOVERS {
        return None;
    }
    let quota_cache = services.admin_state().map(|state| &state.token_plan_quota);
    let failed_key_id = route.endpoint_key_id;
    let failed_secret = route.api_key.as_str();
    let mut candidate = route.clone();
    candidate.api_keys.retain(|key| {
        key.endpoint_id != route.route_id
            || (Some(key.key_id) != failed_key_id && key.api_key.as_str() != failed_secret)
    });
    if candidate.api_keys.is_empty() {
        return None;
    }
    let selection = materialize_route_api_key_selection_with_quota(
        &candidate,
        request,
        &request_ctx.request_prompt_log,
        quota_cache,
    );
    if selection.selection.secret == route.api_key {
        return None;
    }
    candidate.api_key = selection.selection.secret;
    candidate.endpoint_key_id = selection.selection.key_id;
    candidate.endpoint_key_label = selection.selection.key_label;
    candidate.route_selection_reason = db::RouteSelectionReason::QuotaFailover;
    Some(candidate)
}

fn log_quota_failover_retry(
    request_ctx: &RequestExecutionContext,
    route: &db::RouteConfig,
    failover_route: &db::RouteConfig,
) {
    tracing::warn!(
        event = "quota_failover",
        request_id = %request_ctx.request_id,
        endpoint_id = %route.route_id,
        from_endpoint_key_id = route
            .endpoint_key_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        to_endpoint_key_id = failover_route
            .endpoint_key_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        "rotating endpoint API key after live quota-exhaustion response"
    );
}

async fn retry_after_backoff(
    request_ctx: &RequestExecutionContext,
    route: &db::RouteConfig,
    attempt: usize,
    failure: &UpstreamAttemptFailure,
    cancellation: &Option<RequestCancellation>,
) -> bool {
    if cancellation
        .as_ref()
        .is_some_and(RequestCancellation::is_cancelled)
    {
        return false;
    }
    let backoff_ms = RETRY_BACKOFF_MS[attempt.min(RETRY_BACKOFF_MS.len() - 1)];
    tracing::warn!(
        event = "upstream_retry_scheduled",
        request_id = %request_ctx.request_id,
        endpoint_id = %route.route_id,
        base_url = %route.base_url,
        attempt = attempt + 1,
        max_attempts = MAX_UPSTREAM_ATTEMPTS,
        failure_phase = failure.phase.as_str(),
        error = %failure.error,
        backoff_ms,
        "scheduling upstream retry after transient transport failure"
    );
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)) => {}
        _ = async {
            if let Some(cancellation) = cancellation {
                cancellation.cancelled().await;
            }
        }, if cancellation.is_some() => {
            return false;
        }
    }
    !cancellation
        .as_ref()
        .is_some_and(RequestCancellation::is_cancelled)
}

fn log_retry_succeeded(
    request_ctx: &RequestExecutionContext,
    route: &db::RouteConfig,
    attempt: usize,
    retried_phase: Option<UpstreamFailurePhase>,
) {
    tracing::info!(
        event = "upstream_retry_succeeded",
        request_id = %request_ctx.request_id,
        endpoint_id = %route.route_id,
        base_url = %route.base_url,
        attempt,
        max_attempts = MAX_UPSTREAM_ATTEMPTS,
        failure_phase = retried_phase.map(UpstreamFailurePhase::as_str).unwrap_or(""),
        "upstream retry attempt succeeded"
    );
}

fn log_retry_exhausted(
    request_ctx: &RequestExecutionContext,
    route: &db::RouteConfig,
    attempt: usize,
    failure: &UpstreamAttemptFailure,
) {
    tracing::warn!(
        event = "upstream_retry_exhausted",
        request_id = %request_ctx.request_id,
        endpoint_id = %route.route_id,
        base_url = %route.base_url,
        attempt,
        max_attempts = MAX_UPSTREAM_ATTEMPTS,
        failure_phase = failure.phase.as_str(),
        error = %failure.error,
        "upstream retry attempts exhausted"
    );
}

fn is_transient_before_headers(err: &reqwest::Error) -> bool {
    if err.is_connect() || err.is_timeout() {
        return true;
    }
    error_chain_contains(err, |cause| {
        if let Some(hyper_err) = cause.downcast_ref::<hyper::Error>()
            && hyper_err.is_incomplete_message()
        {
            return true;
        }
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            return matches!(
                io_err.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::UnexpectedEof
            );
        }
        if let Some(h2_err) = cause.downcast_ref::<h2::Error>() {
            return h2_err.is_io() || h2_err.is_go_away() || h2_err.is_reset();
        }
        false
    })
}

fn is_transient_buffered_body(err: &reqwest::Error) -> bool {
    err.is_body() || err.is_decode() || err.is_connect() || err.is_timeout()
}

fn error_chain_contains(
    err: &reqwest::Error,
    mut predicate: impl FnMut(&(dyn std::error::Error + 'static)) -> bool,
) -> bool {
    let mut source = err.source();
    while let Some(cause) = source {
        if predicate(cause) {
            return true;
        }
        source = cause.source();
    }
    false
}

#[cfg(test)]
mod tests;
