use super::super::{
    RequestExecutionContext, context::RouteExecutionContext,
    request_assembly::BufferedBridgeRequest,
};
use crate::{
    anthropic_compat::responses_request_to_anthropic_messages,
    chat_replay::prepare_responses_replay_request,
    db,
    openai_compat::{
        CompatError, NormalizedResponsesRequest, conversation_key, previous_response_id,
    },
    redact,
    redact_upstream::{UpstreamRedactionSession, decrypt_upstream_session},
    upstream_adapter::{
        PreparedRequestBody, PreparedUpstreamRequest, ResponseAdapter, prepare_upstream_request,
    },
    usage::upstream_body,
    worker_admin::AdminState,
    worker_usage::UsageLog,
};
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

use super::upstream_redaction::redact_ai_request_json_blocking;

pub(super) fn ai_route_usage_log(
    request_ctx: &RequestExecutionContext,
    request: &BufferedBridgeRequest,
    route_ctx: &RouteExecutionContext,
) -> UsageLog {
    request_ctx
        .ai_usage_log(request, Some(route_ctx.route.user_id))
        .with_route(route_ctx.endpoint_id, route_ctx.model_route_rule_id)
        .with_endpoint_key(
            route_ctx.route.endpoint_key_id,
            route_ctx.route.endpoint_key_label.clone(),
        )
        .with_route_selection(route_ctx.route_selection_reason)
        .with_upstream_model(route_ctx.route.upstream_model.clone())
        .with_upstream_redaction(
            request_ctx.request_prompt_log.upstream_redaction_enabled,
            request_ctx
                .request_prompt_log
                .upstream_redacted_request_json
                .clone(),
            request_ctx
                .request_prompt_log
                .upstream_restore_session
                .clone(),
        )
}

pub(super) async fn mark_function_call_outputs_received(
    admin_state: Option<&AdminState>,
    parent_event_id: Option<i64>,
    request_body: &[u8],
) {
    let (Some(state), Some(parent_event_id)) = (admin_state, parent_event_id) else {
        return;
    };
    let Ok(value) = serde_json::from_slice::<Value>(request_body) else {
        return;
    };
    let input = value
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let call_ids = extract_function_call_output_ids(&input);
    if call_ids.is_empty() {
        return;
    }
    let existing = match db::list_request_record_tool_calls(&state.pool, parent_event_id).await {
        Ok(existing) => existing,
        Err(err) => {
            warn!(error = %err, parent_event_id, "failed to load tool call child events");
            return;
        }
    };
    let by_call_id = existing
        .into_iter()
        .map(|tool_call| (tool_call.call_id.clone(), tool_call))
        .collect::<HashMap<_, _>>();
    for call_id in call_ids {
        let Some(existing_call) = by_call_id.get(&call_id) else {
            continue;
        };
        if let Err(err) = db::upsert_request_record_tool_call(
            &state.pool,
            db::RequestRecordToolCallCreate {
                parent_event_id,
                conversation_id: existing_call.conversation_id,
                call_id: existing_call.call_id.clone(),
                tool_name: existing_call.tool_name.clone(),
                arguments_json: existing_call.arguments_json.clone(),
                arguments_preview: existing_call.arguments_preview.clone(),
                status: db::RequestToolCallStatus::OutputReceived,
                sequence_in_turn: existing_call.sequence_in_turn,
                mcp_request_event_id: existing_call.mcp_request_event_id,
            },
        )
        .await
        {
            warn!(
                error = %err,
                parent_event_id,
                call_id = %existing_call.call_id,
                "failed to mark tool call output received"
            );
        }
    }
}

pub(super) async fn prepare_upstream_request_with_replay(
    admin_state: Option<&AdminState>,
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    conversation_id: Option<uuid::Uuid>,
    parent_event_id: Option<i64>,
    replay_unavailable: bool,
) -> Result<PreparedUpstreamRequest, CompatError> {
    if replay_unavailable
        && route.responses_continuation_policy == db::ResponsesContinuationPolicy::ForceReplay
        && (request.path == "/v1/responses" || previous_response_id(&request.body).is_some())
    {
        return Err(CompatError::new(
            StatusCode::BAD_REQUEST,
            "replay_unavailable",
            "stored conversation content has expired or is unavailable",
        ));
    }
    let effective_request_body = effective_request_body(route, request.body.as_slice());
    let redaction_enabled =
        redact::redaction_enabled_for_user(request.user_id.filter(|id| *id > 0));
    let prior_session = if redaction_enabled {
        load_prior_session(admin_state, conversation_id).await?
    } else {
        None
    };
    let (plain_request_body, redacted_request) = if redaction_enabled {
        let redacted = redact_ai_request_json_blocking(
            request.path.clone(),
            effective_request_body,
            request.user_id.filter(|id| *id > 0),
            conversation_id,
            prior_session,
        )
        .await?;
        (None, Some(redacted))
    } else {
        (Some(effective_request_body), None)
    };
    let prepared_body = redacted_request
        .as_ref()
        .map(|prepared| prepared.body.as_slice())
        .or(plain_request_body.as_deref())
        .expect("plain or redacted request body");
    let needs_replay = should_replay_request(route, request, parent_event_id);
    let mut prepared = if !needs_replay {
        if requires_local_conversation_state(route, request) && admin_state.is_none() {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "replay_unavailable",
                "conversation continuations require stored replay state",
            ));
        }
        if should_strip_responses_state_fields_without_replay(route, request) {
            let normalized = NormalizedResponsesRequest::from_body(prepared_body)?;
            normalized.validate_for_raw_responses_passthrough()?;
            let translated = normalized.to_responses_request_with_prefix(&[], false, true)?;
            PreparedUpstreamRequest {
                path: crate::config::NativeApi::Responses.path().to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(
                    crate::config::NativeApi::Responses.path(),
                    &translated,
                )),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            }
        } else {
            prepare_upstream_request(
                &request.path,
                prepared_body,
                route.native_api,
                should_passthrough_responses(route),
            )?
        }
    } else {
        let Some(state) = admin_state else {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "replay_unavailable",
                "previous_response_id for chat-native continuations requires stored replay state",
            ));
        };
        if !state.usage_retention.read().await.replay_enabled {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "replay_unavailable",
                "stored replay state is disabled",
            ));
        }
        let translated =
            prepare_responses_replay_request(crate::chat_replay::ResponsesReplayRequest {
                pool: &state.pool,
                replay_cache: &state.replay_cache,
                user_id: request.user_id.filter(|id| *id > 0),
                resolved_parent_event_id: parent_event_id,
                request_body: prepared_body,
                native_api: route.native_api,
                route_base_url: &route.base_url,
                current_request_model: route.upstream_model.as_deref(),
            })
            .await?;
        match route.native_api {
            crate::config::NativeApi::Chat => PreparedUpstreamRequest {
                path: crate::config::NativeApi::Chat.path().to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(
                    crate::config::NativeApi::Chat.path(),
                    &translated,
                )),
                response_adapter: ResponseAdapter::ChatToResponses,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            },
            crate::config::NativeApi::Responses => PreparedUpstreamRequest {
                path: crate::config::NativeApi::Responses.path().to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(
                    crate::config::NativeApi::Responses.path(),
                    &translated,
                )),
                response_adapter: ResponseAdapter::Passthrough,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            },
            crate::config::NativeApi::AnthropicMessages => PreparedUpstreamRequest {
                path: crate::config::NativeApi::AnthropicMessages
                    .path()
                    .to_string(),
                body: PreparedRequestBody::BufferedBytes(upstream_body(
                    crate::config::NativeApi::AnthropicMessages.path(),
                    &responses_request_to_anthropic_messages(&translated)?,
                )),
                response_adapter: ResponseAdapter::AnthropicMessagesToResponses,
                upstream_redacted_request_json: None,
                upstream_restore_session: None,
            },
            crate::config::NativeApi::Realtime => {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_native_api",
                    "Realtime endpoints are not compatible with HTTP request translation",
                ));
            }
            crate::config::NativeApi::Auto => {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_auto_protocol",
                    "automatic endpoints must resolve to Chat or Responses before replay",
                ));
            }
        }
    };
    prepared.upstream_redacted_request_json = redacted_request
        .as_ref()
        .and_then(|value| value.redacted_request_json.clone());
    prepared.upstream_restore_session = redacted_request
        .as_ref()
        .and_then(|value| value.restore_session.clone());
    Ok(prepared)
}

async fn load_prior_session(
    admin_state: Option<&AdminState>,
    conversation_id: Option<uuid::Uuid>,
) -> Result<Option<UpstreamRedactionSession>, CompatError> {
    let Some(state) = admin_state else {
        return Ok(None);
    };
    let Some(conversation_id) = conversation_id else {
        return Ok(None);
    };
    let row = db::get_conversation_redaction_session(&state.pool, conversation_id)
        .await
        .map_err(|err| {
            CompatError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "redaction_session_load_failed",
                format!("failed to load upstream redaction session: {err}"),
            )
        })?;
    let Some(row) = row else {
        return Ok(None);
    };
    let manager = state.relay_secret_manager().map_err(|err| {
        CompatError::new(
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
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "redaction_session_load_failed",
            format!("failed to decrypt upstream redaction session: {err}"),
        )
    })?;
    Ok(Some(session))
}

fn effective_request_body(route: &db::RouteConfig, request_body: &[u8]) -> Vec<u8> {
    route
        .upstream_model
        .as_deref()
        .map(|model| crate::usage::rewrite_model_in_body(request_body, model))
        .unwrap_or_else(|| request_body.to_vec())
}

fn should_passthrough_responses(route: &db::RouteConfig) -> bool {
    route.native_api == crate::config::NativeApi::Responses
        && route.responses_continuation_policy == db::ResponsesContinuationPolicy::ForcePassthrough
}

fn should_replay_request(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    parent_event_id: Option<i64>,
) -> bool {
    if request.path != "/v1/responses" {
        return false;
    }
    match route.responses_continuation_policy {
        db::ResponsesContinuationPolicy::ForcePassthrough => false,
        db::ResponsesContinuationPolicy::ForceReplay => {
            previous_response_id(&request.body).is_some() || parent_event_id.is_some()
        }
    }
}

fn should_strip_responses_state_fields_without_replay(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
) -> bool {
    request.path == "/v1/responses"
        && matches!(
            route.native_api,
            crate::config::NativeApi::Responses | crate::config::NativeApi::AnthropicMessages
        )
        && route.responses_continuation_policy == db::ResponsesContinuationPolicy::ForceReplay
        && !should_passthrough_responses(route)
}

fn requires_local_conversation_state(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
) -> bool {
    request.path == "/v1/responses"
        && route.responses_continuation_policy == db::ResponsesContinuationPolicy::ForceReplay
        && conversation_key(&request.body).is_some()
}

fn extract_function_call_output_ids(input: &[Value]) -> Vec<String> {
    input
        .iter()
        .filter_map(|item| {
            item.as_object()
                .filter(|object| {
                    object.get("type").and_then(Value::as_str) == Some("function_call_output")
                })
                .and_then(|object| object.get("call_id").and_then(Value::as_str))
                .map(str::to_string)
        })
        .collect()
}
