use anyhow::anyhow;
use http::Method;
use std::error::Error as _;

use crate::{db, openai_compat::CompatError};

use super::super::{
    RequestExecutionContext, check_named_request_budget,
    context::{RouteExecutionContext, RuntimeServices},
    request_assembly::{BufferedBridgeRequest, RequestCancellation},
    upstream_url,
};
use super::{
    forward::{ResponseForwardContext, ResponseLoggingContext, forward_upstream_response},
    request_logging::log_prepared_upstream_summary,
    request_support::prepare_upstream_request_with_replay,
    upstream::build_upstream_request,
};

const MAX_UPSTREAM_ATTEMPTS: usize = 3;
const RETRY_BACKOFF_MS: [u64; 2] = [250, 1000];

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
    BudgetError {
        endpoint_id: Option<uuid::Uuid>,
        message: String,
        model_route_rule_id: Option<uuid::Uuid>,
    },
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
    let route_ctx = RouteExecutionContext::new(route);
    if let Some(state) = services.admin_state()
        && !route.route_id.is_nil()
        && let Some(endpoint) = db::get_endpoint(&state.pool, route.route_id).await?
        && let Some(message) = check_named_request_budget(
            &state.pool,
            db::RequestRecordCategory::Ai,
            db::RequestBudgetScope::Endpoint(endpoint.endpoint_id),
            "endpoint",
            &endpoint.name,
            endpoint.daily_max_requests,
            endpoint.monthly_max_requests,
        )
        .await?
    {
        return Ok(ForwardOutcome::BudgetError {
            endpoint_id: Some(endpoint.endpoint_id),
            message,
            model_route_rule_id: route.model_route_rule_id,
        });
    }

    let prepared = match prepare_upstream_request_with_replay(
        services.admin_state(),
        route,
        request,
        request_ctx.request_prompt_log.conversation_id,
        request_ctx.request_prompt_log.parent_event_id,
        request_ctx.request_prompt_log.replay_unavailable,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(err) => return Ok(ForwardOutcome::CompatError(err)),
    };
    let upstream_url = upstream_url(&route.base_url, &prepared.path);
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
    log_prepared_upstream_summary(route, &prepared);
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
    };
    let cancellation = services
        .runtime_state
        .request_cancellation(request.request_id.as_str())
        .await;
    let mut retried = false;
    let mut last_retried_phase = None;
    let mut attempt = 0usize;
    loop {
        let attempt_number = attempt + 1;
        let send_result = build_upstream_request(
            &services.client,
            method,
            &upstream_url,
            route,
            &prepared.body,
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
                    if retry_after_backoff(request_ctx, route, attempt, &failure, &cancellation)
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
                    log_retry_exhausted(request_ctx, route, attempt_number, &failure);
                }
                return Ok(ForwardOutcome::TransportError {
                    error: failure.error,
                    terminal_recorded: false,
                });
            }
            Ok(response) => {
                let outcome = handle_attempt_response(response, response_ctx.cloned()).await?;
                match outcome {
                    AttemptOutcome::Handled => {
                        if retried {
                            log_retry_succeeded(
                                request_ctx,
                                route,
                                attempt_number,
                                last_retried_phase,
                            );
                        }
                        return Ok(ForwardOutcome::Handled);
                    }
                    AttemptOutcome::Failure(failure) => {
                        if failure.retryable && attempt_number < MAX_UPSTREAM_ATTEMPTS {
                            if retry_after_backoff(
                                request_ctx,
                                route,
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
                            log_retry_exhausted(request_ctx, route, attempt_number, &failure);
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
}

async fn handle_attempt_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
) -> anyhow::Result<AttemptOutcome> {
    match forward_upstream_response(response, context).await {
        Ok(()) => Ok(AttemptOutcome::Handled),
        Err(err) => match err.downcast::<UpstreamAttemptFailure>() {
            Ok(failure) => Ok(AttemptOutcome::Failure(failure)),
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
    }
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
