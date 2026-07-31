use super::super::{
    context::{BridgeSender, RouteExecutionContext},
    error_handling::{PassthroughSseFilter, ResponsesSseTerminal, safe_error},
};
use super::{
    artifact::{persist_assistant_artifact, resolve_assistant_artifact},
    errors::respond_with_client_error,
    request_support::ai_route_usage_log,
};
use crate::{
    chat_replay::{AssistantArtifactCapture, ResponsesArtifactCapture},
    db,
    openai_compat::{
        AnthropicResponseStreamAdapter, ChatResponseStreamAdapter, CompatError,
        ResponsesChatResponseStreamAdapter,
    },
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
    upstream_adapter::ResponseAdapter,
    usage::UsageCapture,
    worker::stream_delta_batcher::StreamDeltaBatcher,
    worker_usage::record_usage_event,
};
use anyhow::anyhow;
use futures::StreamExt;
use tokio::time::{self, MissedTickBehavior};
use tracing::{debug, error, info, warn};

use super::forward::ResponseForwardContext;
use super::stream_restore::SseRestoreFilter;
use super::streaming_terminal::{failure_details, finish_failure};
use super::streaming_usage::observe_usage_chunk;
use super::upstream_restore::restore_ai_response_json_blocking;

struct UpstreamStreamDiag {
    request_id: String,
    path: String,
    endpoint_id: uuid::Uuid,
    base_url: String,
    native_api: &'static str,
    response_adapter: &'static str,
    upstream_content_type: String,
    upstream_chunks: usize,
    upstream_bytes: usize,
    emitted_chunks: usize,
    emitted_bytes: usize,
    terminal_reason: Option<&'static str>,
    terminal_error: Option<String>,
    finished: bool,
}

impl UpstreamStreamDiag {
    fn new(
        request_id: String,
        path: String,
        route_ctx: &RouteExecutionContext,
        response_adapter: ResponseAdapter,
        upstream_content_type: Option<&str>,
    ) -> Self {
        Self {
            request_id,
            path,
            endpoint_id: route_ctx.endpoint_id.unwrap_or(uuid::Uuid::nil()),
            base_url: route_ctx.route.base_url.clone(),
            native_api: route_ctx.route.native_api.as_str(),
            response_adapter: response_adapter_name(response_adapter),
            upstream_content_type: upstream_content_type.unwrap_or("").to_string(),
            upstream_chunks: 0,
            upstream_bytes: 0,
            emitted_chunks: 0,
            emitted_bytes: 0,
            terminal_reason: None,
            terminal_error: None,
            finished: false,
        }
    }

    fn record_upstream_chunk(&mut self, len: usize) {
        self.upstream_chunks += 1;
        self.upstream_bytes += len;
    }

    fn record_emitted_chunk(&mut self, len: usize) {
        self.emitted_chunks += 1;
        self.emitted_bytes += len;
    }

    fn mark_terminal(&mut self, reason: &'static str, error: Option<String>) {
        self.terminal_reason = Some(reason);
        self.terminal_error = error;
    }

    fn finish(&mut self) {
        if self.finished {
            return;
        }
        let reason = self.terminal_reason.unwrap_or("completed");
        if reason == "completed" {
            info!(
                category = "stream_diag",
                request_id = %self.request_id,
                path = %self.path,
                endpoint_id = %self.endpoint_id,
                base_url = %self.base_url,
                native_api = self.native_api,
                response_adapter = self.response_adapter,
                upstream_content_type = %self.upstream_content_type,
                upstream_chunks = self.upstream_chunks,
                upstream_bytes = self.upstream_bytes,
                emitted_chunks = self.emitted_chunks,
                emitted_bytes = self.emitted_bytes,
                terminal_reason = reason,
                terminal_error = self.terminal_error.as_deref().unwrap_or(""),
                "worker upstream stream finished"
            );
        } else {
            warn!(
                category = "stream_diag",
                request_id = %self.request_id,
                path = %self.path,
                endpoint_id = %self.endpoint_id,
                base_url = %self.base_url,
                native_api = self.native_api,
                response_adapter = self.response_adapter,
                upstream_content_type = %self.upstream_content_type,
                upstream_chunks = self.upstream_chunks,
                upstream_bytes = self.upstream_bytes,
                emitted_chunks = self.emitted_chunks,
                emitted_bytes = self.emitted_bytes,
                terminal_reason = reason,
                terminal_error = self.terminal_error.as_deref().unwrap_or(""),
                "worker upstream stream finished"
            );
        }
        self.finished = true;
    }
}

impl Drop for UpstreamStreamDiag {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        warn!(
            category = "stream_diag",
            request_id = %self.request_id,
            path = %self.path,
            endpoint_id = %self.endpoint_id,
            base_url = %self.base_url,
            native_api = self.native_api,
            response_adapter = self.response_adapter,
            upstream_content_type = %self.upstream_content_type,
            upstream_chunks = self.upstream_chunks,
            upstream_bytes = self.upstream_bytes,
            emitted_chunks = self.emitted_chunks,
            emitted_bytes = self.emitted_bytes,
            terminal_reason = self.terminal_reason.unwrap_or("dropped"),
            terminal_error = self.terminal_error.as_deref().unwrap_or(""),
            "worker upstream stream dropped before finish logging"
        );
    }
}

fn response_adapter_name(adapter: ResponseAdapter) -> &'static str {
    match adapter {
        ResponseAdapter::Passthrough => "passthrough",
        ResponseAdapter::ChatToResponses => "chat_to_responses",
        ResponseAdapter::ResponsesToChat => "responses_to_chat",
        ResponseAdapter::AnthropicMessagesToResponses => "anthropic_messages_to_responses",
    }
}

fn send_stream_message(
    out_tx: &BridgeSender,
    message: BridgeMessage,
    stream_diag: &mut UpstreamStreamDiag,
) -> anyhow::Result<()> {
    out_tx.send(message).map_err(|err| {
        let reason = err.diagnostic_reason();
        let error_message = err.to_string();
        stream_diag.mark_terminal(reason, Some(error_message));
        stream_diag.finish();
        anyhow::Error::new(err).context("failed sending response to relay")
    })
}

pub(super) async fn forward_streaming_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
    mut assistant_capture: Option<&mut AssistantArtifactCapture>,
    mut responses_capture: Option<&mut ResponsesArtifactCapture>,
    upstream_content_type: Option<String>,
    is_sse: bool,
) -> anyhow::Result<()> {
    let route_ctx = context.route_ctx;
    let request = context.request;
    let request_ctx = context.request_ctx;
    let upstream_redacted_request_json = context.upstream_redacted_request_json.clone();
    let upstream_restore_session = context.upstream_restore_session.clone();
    let redact_content = context.logging.redact_content;
    let content_logging_enabled = context.logging.content_logging_enabled;
    let raw_content_logging_enabled = context.logging.raw_content_logging_enabled;
    let response_adapter = context.response_adapter;
    let services = context.services;
    let emits_sse = response_adapter != ResponseAdapter::Passthrough || is_sse;
    if !emits_sse && upstream_restore_session.is_some() {
        debug!(
            request_id = %request.request_id,
            path = %request.path,
            response_adapter = response_adapter_name(response_adapter),
            is_sse,
            "using buffered non-SSE restore path"
        );
        return forward_buffered_non_sse_response(
            response,
            context.cloned(),
            assistant_capture,
            responses_capture,
            upstream_content_type,
        )
        .await;
    }
    let status = response.status();
    let capture_is_sse = matches!(
        response_adapter,
        ResponseAdapter::ChatToResponses
            | ResponseAdapter::ResponsesToChat
            | ResponseAdapter::AnthropicMessagesToResponses
    ) || is_sse;
    let mut capture = UsageCapture::new(capture_is_sse, request_ctx.request_model.clone());
    capture
        .set_response_text_capture_limit(services.response_limits.max_response_text_capture_bytes);
    let response_content_type = if matches!(
        response_adapter,
        ResponseAdapter::ChatToResponses
            | ResponseAdapter::ResponsesToChat
            | ResponseAdapter::AnthropicMessagesToResponses
    ) {
        Some("text/event-stream".to_string())
    } else {
        upstream_content_type
    };
    let mut stream_diag = UpstreamStreamDiag::new(
        request.request_id.clone(),
        request.path.clone(),
        route_ctx,
        response_adapter,
        response_content_type.as_deref(),
    );
    send_stream_message(
        &services.out_tx,
        BridgeMessage::ResponseStart(ResponseStart {
            request_id: request.request_id.clone(),
            status: status.as_u16(),
            content_type: response_content_type,
            headers: Vec::new(),
        }),
        &mut stream_diag,
    )?;
    let log_stream_adapter_error =
        |provider_response_id: Option<&str>, provider_model: Option<&str>, err: &CompatError| {
            error!(
                request_id = %request.request_id,
                model = request_ctx
                    .request_model
                    .as_deref()
                    .or(provider_model)
                    .unwrap_or("unknown"),
                tool_error = %err.message,
                provider_response_id = provider_response_id.unwrap_or(""),
                streaming = true,
                "failed adapting invalid upstream tool call arguments"
            );
        };

    let mut stream = response.bytes_stream();
    let mut raw_response_body = Vec::new();
    let mut upstream_response_bytes = 0usize;
    let mut raw_response_capture_truncated = false;
    let mut ttft_ms = None;
    let mut chat_stream_adapter =
        (response_adapter == ResponseAdapter::ChatToResponses).then(ChatResponseStreamAdapter::new);
    let mut responses_to_chat_stream_adapter = (response_adapter
        == ResponseAdapter::ResponsesToChat)
        .then(ResponsesChatResponseStreamAdapter::new);
    let mut anthropic_stream_adapter = (response_adapter
        == ResponseAdapter::AnthropicMessagesToResponses)
        .then(AnthropicResponseStreamAdapter::new);
    let responses_passthrough = request.path == "/v1/responses"
        && response_adapter == ResponseAdapter::Passthrough
        && is_sse;
    let mut passthrough_sse_filter = (response_adapter == ResponseAdapter::Passthrough && is_sse)
        .then(|| {
            if responses_passthrough {
                PassthroughSseFilter::new_responses()
            } else {
                PassthroughSseFilter::new()
            }
        });
    let mut sse_restore_filter = upstream_restore_session.as_ref().map(|session| {
        if responses_passthrough {
            SseRestoreFilter::new_responses(session)
        } else {
            SseRestoreFilter::new(session)
        }
    });
    let stream_delta_batching = services
        .admin_state()
        .map(|state| {
            state
                .stream_delta_batching
                .try_read()
                .map(|value| value.clone())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let mut stream_delta_batcher = StreamDeltaBatcher::new(stream_delta_batching);
    let mut flush_interval = stream_delta_batcher.is_enabled().then(|| {
        time::interval(std::time::Duration::from_millis(
            stream_delta_batcher.flush_window_ms(),
        ))
    });
    if let Some(interval) = flush_interval.as_mut() {
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    }
    let emit_output_chunks = |output_chunks: Vec<Vec<u8>>,
                              out_tx: &BridgeSender,
                              stream_diag: &mut UpstreamStreamDiag|
     -> anyhow::Result<()> {
        for output_chunk in output_chunks {
            stream_diag.record_emitted_chunk(output_chunk.len());
            send_stream_message(
                out_tx,
                BridgeMessage::ResponseChunk(ResponseChunk {
                    request_id: request.request_id.clone(),
                    data: output_chunk,
                }),
                stream_diag,
            )?;
        }
        Ok(())
    };

    loop {
        tokio::select! {
            _ = async {
                if let Some(interval) = flush_interval.as_mut() {
                    interval.tick().await;
                }
            }, if flush_interval.is_some() => {
                emit_output_chunks(
                    stream_delta_batcher.flush_due()?,
                    &services.out_tx,
                    &mut stream_diag,
                )?;
            }
            maybe_chunk = stream.next() => {
                let Some(chunk) = maybe_chunk else {
                    break;
                };
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        let err = anyhow!(err).context("failed reading upstream response");
                        let safe_err = safe_error(&err, redact_content, request_ctx.user_id);
                        let (response_prompt, response_raw_body) =
                            super::forward::response_logging_payload(
                                &capture.response_text,
                                &raw_response_body,
                                content_logging_enabled,
                                raw_content_logging_enabled,
                                redact_content,
                                request_ctx.user_id,
                            );
                        record_usage_event(
                            services.admin_state(),
                            ai_route_usage_log(request_ctx, request, route_ctx)
                                .with_upstream_redaction(
                                    upstream_restore_session.is_some(),
                                    upstream_redacted_request_json.clone(),
                                    upstream_restore_session.clone(),
                                )
                                .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
                                .with_model(capture.model.clone())
                                .with_status(
                                    Some(status.as_u16() as i32),
                                    Some(false),
                                    Some(request_ctx.elapsed_ms()),
                                    ttft_ms,
                                )
                                .with_usage(capture.usage.clone())
                                .with_response(
                                    capture.response_id.clone(),
                                    capture
                                        .provider_conversation_key
                                        .clone()
                                        .or_else(|| {
                                            request_ctx
                                                .request_prompt_log
                                                .request_conversation_key
                                                .clone()
                                        }),
                                    response_prompt,
                                    response_raw_body,
                                )
                                .with_response_capture_truncated(
                                    capture.response_text_truncated || raw_response_capture_truncated,
                                )
                                .with_error(
                                    Some("upstream_stream_error".to_string()),
                                    Some(safe_err.clone()),
                                    None,
                                ),
                        )
                        .await;
                        stream_diag.mark_terminal("upstream_read_error", Some(safe_err));
                        stream_diag.finish();
                        return Err(err);
                    }
                };
                upstream_response_bytes = upstream_response_bytes
                    .checked_add(chunk.len())
                    .ok_or_else(|| anyhow!("upstream_response_too_large"))?;
                if upstream_response_bytes > services.response_limits.max_upstream_response_bytes {
                    return Err(anyhow!("upstream_response_too_large"));
                }
                stream_diag.record_upstream_chunk(chunk.len());
                observe_usage_chunk(
                    &mut capture,
                    &mut ttft_ms,
                    &chunk,
                    request_ctx.elapsed_ms(),
                );
                if raw_content_logging_enabled {
                    append_limited_capture(
                        &mut raw_response_body,
                        &chunk,
                        services.response_limits.max_raw_response_capture_bytes,
                        &mut raw_response_capture_truncated,
                    );
                }
                if let Some(capture) = assistant_capture.as_mut() {
                    capture.observe_chunk(&chunk);
                }
                if let Some(capture) = responses_capture.as_mut() {
                    capture.observe_chunk(&chunk);
                }
                let output_chunks: Vec<Vec<u8>> = if let Some(adapter) = responses_to_chat_stream_adapter.as_mut() {
                    match adapter.push_chunk(&chunk) {
                        Ok(output_chunks) => output_chunks,
                        Err(err) => {
                            log_stream_adapter_error(
                                adapter.provider_response_id(),
                                adapter.model_name(),
                                &err,
                            );
                            stream_diag
                                .mark_terminal("stream_adapter_error", Some(err.message.clone()));
                            stream_diag.finish();
                            return respond_with_client_error(
                                services,
                                request,
                                request_ctx,
                                route_ctx,
                                err,
                            )
                            .await;
                        }
                    }
                } else if let Some(adapter) = chat_stream_adapter.as_mut() {
                    match adapter.push_chunk(&chunk) {
                        Ok(output_chunks) => output_chunks,
                        Err(err) => {
                            log_stream_adapter_error(
                                adapter.provider_response_id(),
                                adapter.model_name(),
                                &err,
                            );
                            stream_diag
                                .mark_terminal("stream_adapter_error", Some(err.message.clone()));
                            stream_diag.finish();
                            return respond_with_client_error(
                                services,
                                request,
                                request_ctx,
                                route_ctx,
                                err,
                            )
                            .await;
                        }
                    }
                } else if let Some(adapter) = anthropic_stream_adapter.as_mut() {
                    match adapter.push_chunk(&chunk) {
                        Ok(output_chunks) => output_chunks,
                        Err(err) => {
                            log_stream_adapter_error(
                                adapter.provider_response_id(),
                                adapter.model_name(),
                                &err,
                            );
                            stream_diag
                                .mark_terminal("stream_adapter_error", Some(err.message.clone()));
                            stream_diag.finish();
                            return respond_with_client_error(
                                services,
                                request,
                                request_ctx,
                                route_ctx,
                                err,
                            )
                            .await;
                        }
                    }
                } else if let Some(filter) = passthrough_sse_filter.as_mut() {
                    match filter.push_chunk(&chunk) {
                        Ok(output_chunks) => output_chunks,
                        Err(err) => match err {},
                    }
                } else {
                    vec![chunk.to_vec()]
                };
                let output_chunks = match sse_restore_filter.as_mut() {
                    Some(filter) => filter.push_chunks(output_chunks)?,
                    None => output_chunks,
                };
                for output_chunk in output_chunks {
                    emit_output_chunks(
                        stream_delta_batcher.push_chunk(output_chunk)?,
                        &services.out_tx,
                        &mut stream_diag,
                    )?;
                }
                if passthrough_sse_filter
                    .as_ref()
                    .is_some_and(PassthroughSseFilter::is_done)
                    || sse_restore_filter
                        .as_ref()
                        .is_some_and(SseRestoreFilter::is_done)
                {
                    break;
                }
            }
        }
    }

    if let Some(adapter) = responses_to_chat_stream_adapter.as_mut() {
        let output_chunks = match adapter.finish() {
            Ok(output_chunks) => output_chunks,
            Err(err) => {
                log_stream_adapter_error(
                    adapter.provider_response_id(),
                    adapter.model_name(),
                    &err,
                );
                return respond_with_client_error(services, request, request_ctx, route_ctx, err)
                    .await;
            }
        };
        let output_chunks = match sse_restore_filter.as_mut() {
            Some(filter) => filter.push_chunks(output_chunks)?,
            None => output_chunks,
        };
        for output_chunk in output_chunks {
            emit_output_chunks(
                stream_delta_batcher.push_chunk(output_chunk)?,
                &services.out_tx,
                &mut stream_diag,
            )?;
        }
    }

    if let Some(adapter) = chat_stream_adapter.as_mut() {
        let output_chunks = match adapter.finish() {
            Ok(output_chunks) => output_chunks,
            Err(err) => {
                log_stream_adapter_error(
                    adapter.provider_response_id(),
                    adapter.model_name(),
                    &err,
                );
                return respond_with_client_error(services, request, request_ctx, route_ctx, err)
                    .await;
            }
        };
        let output_chunks = match sse_restore_filter.as_mut() {
            Some(filter) => filter.push_chunks(output_chunks)?,
            None => output_chunks,
        };
        for output_chunk in output_chunks {
            emit_output_chunks(
                stream_delta_batcher.push_chunk(output_chunk)?,
                &services.out_tx,
                &mut stream_diag,
            )?;
        }
    }

    if let Some(adapter) = anthropic_stream_adapter.as_mut() {
        let output_chunks = match adapter.finish() {
            Ok(output_chunks) => output_chunks,
            Err(err) => {
                log_stream_adapter_error(
                    adapter.provider_response_id(),
                    adapter.model_name(),
                    &err,
                );
                return respond_with_client_error(services, request, request_ctx, route_ctx, err)
                    .await;
            }
        };
        let output_chunks = match sse_restore_filter.as_mut() {
            Some(filter) => filter.push_chunks(output_chunks)?,
            None => output_chunks,
        };
        for output_chunk in output_chunks {
            emit_output_chunks(
                stream_delta_batcher.push_chunk(output_chunk)?,
                &services.out_tx,
                &mut stream_diag,
            )?;
        }
    } else if let Some(filter) = passthrough_sse_filter.as_mut() {
        let output_chunks = match filter.finish() {
            Ok(output_chunks) => output_chunks,
            Err(err) => match err {},
        };
        let output_chunks = match sse_restore_filter.as_mut() {
            Some(filter) => filter.push_chunks(output_chunks)?,
            None => output_chunks,
        };
        for output_chunk in output_chunks {
            emit_output_chunks(
                stream_delta_batcher.push_chunk(output_chunk)?,
                &services.out_tx,
                &mut stream_diag,
            )?;
        }
    }
    let (responses_stream_terminal, responses_error_body) =
        if let Some(filter) = sse_restore_filter.as_mut() {
            for output_chunk in filter.finish()? {
                emit_output_chunks(
                    stream_delta_batcher.push_chunk(output_chunk)?,
                    &services.out_tx,
                    &mut stream_diag,
                )?;
            }
            (
                filter.responses_terminal(),
                filter.responses_error_body().map(str::to_owned),
            )
        } else {
            (
                passthrough_sse_filter
                    .as_ref()
                    .and_then(PassthroughSseFilter::responses_terminal),
                passthrough_sse_filter
                    .as_ref()
                    .and_then(PassthroughSseFilter::responses_error_body)
                    .map(str::to_owned),
            )
        };
    let buffered_output = stream_delta_batcher.finish()?;
    if responses_passthrough
        && !matches!(
            responses_stream_terminal,
            Some(ResponsesSseTerminal::Completed)
        )
    {
        emit_output_chunks(buffered_output, &services.out_tx, &mut stream_diag)?;
        if capture.finish() && ttft_ms.is_none() {
            ttft_ms = Some(request_ctx.elapsed_ms());
        }
        let (code, message) = failure_details(responses_stream_terminal);
        finish_failure(
            responses_stream_terminal,
            &context,
            status.as_u16(),
            &mut capture,
            &raw_response_body,
            responses_error_body.as_deref(),
            ttft_ms,
        )
        .await?;
        stream_diag.mark_terminal(code, Some(message.to_string()));
        stream_diag.finish();
        return Ok(());
    }
    emit_output_chunks(buffered_output, &services.out_tx, &mut stream_diag)?;
    if capture.finish() && ttft_ms.is_none() {
        ttft_ms = Some(request_ctx.elapsed_ms());
    }
    if let Some(assistant_capture) = assistant_capture.as_mut() {
        assistant_capture.finish();
    }
    if let Some(responses_capture) = responses_capture.as_mut() {
        responses_capture.finish();
    }

    send_stream_message(
        &services.out_tx,
        BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }),
        &mut stream_diag,
    )?;
    stream_diag.mark_terminal("completed", None);
    stream_diag.finish();
    let captured_artifact = assistant_capture
        .as_ref()
        .and_then(|capture| capture.artifact())
        .or_else(|| {
            responses_capture
                .as_ref()
                .and_then(|capture| capture.artifact())
        });
    let (response_prompt, response_raw_body) = super::forward::response_logging_payload(
        &capture.response_text,
        &raw_response_body,
        content_logging_enabled,
        raw_content_logging_enabled,
        redact_content,
        request_ctx.user_id,
    );
    let usage_event_id = record_usage_event(
        services.admin_state(),
        ai_route_usage_log(request_ctx, request, route_ctx)
            .with_upstream_redaction(
                upstream_restore_session.is_some(),
                upstream_redacted_request_json,
                upstream_restore_session,
            )
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_model(capture.model.clone())
            .with_status(
                Some(status.as_u16() as i32),
                Some(true),
                Some(request_ctx.elapsed_ms()),
                ttft_ms,
            )
            .with_usage(capture.usage.clone())
            .with_response(
                capture.response_id.clone(),
                capture.provider_conversation_key.clone().or_else(|| {
                    request_ctx
                        .request_prompt_log
                        .request_conversation_key
                        .clone()
                }),
                response_prompt.clone(),
                response_raw_body,
            )
            .with_response_capture_truncated(
                capture.response_text_truncated || raw_response_capture_truncated,
            ),
    )
    .await;
    persist_assistant_artifact(
        services.admin_state(),
        usage_event_id,
        resolve_assistant_artifact(
            captured_artifact,
            Some(&capture.response_text),
            response_prompt.as_deref(),
        ),
        request_ctx.request_prompt_log.conversation_id,
        request,
        &route_ctx.route,
        capture.response_id.as_deref(),
    )
    .await;
    Ok(())
}

async fn forward_buffered_non_sse_response(
    response: reqwest::Response,
    context: ResponseForwardContext<'_>,
    mut assistant_capture: Option<&mut AssistantArtifactCapture>,
    mut responses_capture: Option<&mut ResponsesArtifactCapture>,
    upstream_content_type: Option<String>,
) -> anyhow::Result<()> {
    let route_ctx = context.route_ctx;
    let request = context.request;
    let request_ctx = context.request_ctx;
    let upstream_redacted_request_json = context.upstream_redacted_request_json.clone();
    let upstream_restore_session = context
        .upstream_restore_session
        .clone()
        .ok_or_else(|| anyhow!("missing upstream restore session"))?;
    let redact_content = context.logging.redact_content;
    let content_logging_enabled = context.logging.content_logging_enabled;
    let raw_content_logging_enabled = context.logging.raw_content_logging_enabled;
    let response_adapter = context.response_adapter;
    let services = context.services;
    let status = response.status();
    let response_content_type = upstream_content_type;
    let mut stream_diag = UpstreamStreamDiag::new(
        request.request_id.clone(),
        request.path.clone(),
        route_ctx,
        response_adapter,
        response_content_type.as_deref(),
    );
    let mut stream = response.bytes_stream();
    let mut raw_response_body = Vec::new();
    let mut output = Vec::new();
    let mut upstream_response_bytes = 0usize;
    let mut raw_response_capture_truncated = false;

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                let err = anyhow!(err).context("failed reading upstream response");
                stream_diag.mark_terminal("upstream_read_error", Some(err.to_string()));
                stream_diag.finish();
                return Err(err);
            }
        };
        upstream_response_bytes = upstream_response_bytes
            .checked_add(chunk.len())
            .ok_or_else(|| anyhow!("upstream_response_too_large"))?;
        if upstream_response_bytes > services.response_limits.max_upstream_response_bytes {
            return Err(anyhow!("upstream_response_too_large"));
        }
        stream_diag.record_upstream_chunk(chunk.len());
        output.reserve(chunk.len());
        if raw_content_logging_enabled {
            append_limited_capture(
                &mut raw_response_body,
                &chunk,
                services.response_limits.max_raw_response_capture_bytes,
                &mut raw_response_capture_truncated,
            );
        }
        if let Some(capture) = assistant_capture.as_mut() {
            capture.observe_chunk(&chunk);
        }
        if let Some(capture) = responses_capture.as_mut() {
            capture.observe_chunk(&chunk);
        }
        output.extend_from_slice(&chunk);
    }

    if let Some(capture) = assistant_capture.as_mut() {
        capture.finish();
    }
    if let Some(capture) = responses_capture.as_mut() {
        capture.finish();
    }

    let buffered_bytes = output.len();
    let restored_output = match restore_ai_response_json_blocking(
        request.path.clone(),
        output,
        upstream_restore_session.clone(),
    )
    .await
    {
        Ok(restored_output) => restored_output,
        Err(err) => {
            stream_diag.mark_terminal("restore_validation_error", Some(err.to_string()));
            stream_diag.finish();
            return Err(err);
        }
    };
    debug!(
        request_id = %request.request_id,
        path = %request.path,
        response_adapter = response_adapter_name(response_adapter),
        buffered_bytes,
        restored_bytes = restored_output.len(),
        raw_logging = raw_content_logging_enabled,
        "completed buffered non-SSE restore path"
    );

    let mut capture = UsageCapture::new(
        response_content_type
            .as_deref()
            .is_some_and(|value| value.contains("text/event-stream")),
        request_ctx.request_model.clone(),
    );
    capture
        .set_response_text_capture_limit(services.response_limits.max_response_text_capture_bytes);
    let _ = capture.observe_chunk(&restored_output);
    capture.finish();

    send_stream_message(
        &services.out_tx,
        BridgeMessage::ResponseStart(ResponseStart {
            request_id: request.request_id.clone(),
            status: status.as_u16(),
            content_type: response_content_type,
            headers: Vec::new(),
        }),
        &mut stream_diag,
    )?;
    stream_diag.record_emitted_chunk(restored_output.len());
    send_stream_message(
        &services.out_tx,
        BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request.request_id.clone(),
            data: restored_output,
        }),
        &mut stream_diag,
    )?;
    send_stream_message(
        &services.out_tx,
        BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }),
        &mut stream_diag,
    )?;
    stream_diag.mark_terminal("completed", None);
    stream_diag.finish();

    let captured_artifact = assistant_capture
        .as_ref()
        .and_then(|capture| capture.artifact())
        .or_else(|| {
            responses_capture
                .as_ref()
                .and_then(|capture| capture.artifact())
        });
    let (response_prompt, response_raw_body) = super::forward::response_logging_payload(
        &capture.response_text,
        &raw_response_body,
        content_logging_enabled,
        raw_content_logging_enabled,
        redact_content,
        request_ctx.user_id,
    );
    let usage_event_id = record_usage_event(
        services.admin_state(),
        ai_route_usage_log(request_ctx, request, route_ctx)
            .with_upstream_redaction(
                true,
                upstream_redacted_request_json,
                Some(upstream_restore_session),
            )
            .with_state(
                db::UsageEventKind::Request,
                db::RequestRecordState::Completed,
            )
            .with_model(capture.model.clone())
            .with_status(
                Some(status.as_u16() as i32),
                Some(true),
                Some(request_ctx.elapsed_ms()),
                None,
            )
            .with_usage(capture.usage.clone())
            .with_response(
                capture.response_id.clone(),
                capture.provider_conversation_key.clone().or_else(|| {
                    request_ctx
                        .request_prompt_log
                        .request_conversation_key
                        .clone()
                }),
                response_prompt.clone(),
                response_raw_body,
            )
            .with_response_capture_truncated(
                capture.response_text_truncated || raw_response_capture_truncated,
            ),
    )
    .await;
    persist_assistant_artifact(
        services.admin_state(),
        usage_event_id,
        resolve_assistant_artifact(
            captured_artifact,
            Some(&capture.response_text),
            response_prompt.as_deref(),
        ),
        request_ctx.request_prompt_log.conversation_id,
        request,
        &route_ctx.route,
        capture.response_id.as_deref(),
    )
    .await;
    Ok(())
}

fn append_limited_capture(output: &mut Vec<u8>, chunk: &[u8], limit: usize, truncated: &mut bool) {
    let remaining = limit.saturating_sub(output.len());
    let take = remaining.min(chunk.len());
    output.extend_from_slice(&chunk[..take]);
    if take < chunk.len() {
        *truncated = true;
    }
}
