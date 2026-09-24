use super::super::{
    RequestExecutionContext, context::RouteExecutionContext,
    request_assembly::BufferedBridgeRequest,
};
use crate::{
    db,
    openai_compat::CompatError,
    redact,
    redact_upstream::{UpstreamRedactionSession, current_policy_version, decrypt_upstream_session},
    upstream_adapter::{PreparedUpstreamRequest, prepare_upstream_request_with_compact},
    worker_admin::AdminState,
    worker_usage::UsageLog,
};
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::HashMap;
use tracing::warn;

use super::upstream_redaction::redact_ai_request_json_blocking;

/// Issue #564 Task 1: the upstream redaction outcome for one prepared request.
/// The prepared body travels in [`PreparedRouteRequest::prepared`]; this side
/// channel tells the request context what to persist, so every record for the
/// turn (admission, success, failure, terminal) can carry the same session
/// instead of dropping it and deleting the conversation row.
#[derive(Debug, Clone, Default)]
pub(super) struct RouteRedactionState {
    /// Whether upstream redaction was enabled for this user/request at all.
    pub(super) enabled: bool,
    /// The session minted or advanced by this turn, when there is one.
    pub(super) session: Option<UpstreamRedactionSession>,
    /// Redacted request JSON, only present when replacements were applied.
    pub(super) redacted_request_json: Option<Value>,
    /// The existing session must be dropped: redaction was explicitly disabled
    /// for this request, or a prior session was dropped by a policy-generation
    /// mismatch. This is the only path that deletes the persisted row.
    pub(super) reset: bool,
}

pub(super) struct PreparedRouteRequest {
    pub(super) prepared: PreparedUpstreamRequest,
    pub(super) redaction: RouteRedactionState,
}

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
        .with_applied_thinking_effort_override(route_ctx.applied_thinking_effort_override.clone())
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
    created_at: chrono::DateTime<chrono::Utc>,
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
                created_at,
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

pub(super) async fn prepare_upstream_request_for_route(
    admin_state: Option<&AdminState>,
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    conversation_id: Option<uuid::Uuid>,
) -> Result<PreparedRouteRequest, CompatError> {
    let effective_request_body = effective_request_body(route, request.body.as_slice());
    let redaction_enabled =
        redact::redaction_enabled_for_user(request.user_id.filter(|id| *id > 0));
    let prior_session = if redaction_enabled {
        load_prior_session(admin_state, conversation_id).await?
    } else {
        None
    };
    let had_prior_session = prior_session.is_some();
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
    // Stateless Responses requests may route to Chat-native targets via
    // `responses_stateless_request_to_chat` (`reasoning.summary="auto"` and
    // `include` are accepted and dropped, `reasoning.effort` including `xhigh`
    // is forwarded as `reasoning_effort`). Stateful fields such as
    // previous_response_id or conversation are rejected with
    // `invalid_responses_continuation` inside `prepare_upstream_request` and are
    // never silently stripped here. Responses -> Anthropic/Auto remains rejected.
    // Issue #392 Phase K: thread the per-target normalize switch; false
    // skips Chat developer->system rewriting (default-off passthrough).
    // Issue #464: thread the per-target thinking effort override; None
    // means inherit (follow the caller).
    // Issue #502 Task 5: thread the per-target compact mode; `passthrough`
    // keeps Task 3 behavior, `self_summarize` enables the ferry-side
    // handoff flow for non-Responses targets, `off` rejects compact.
    let mut prepared = prepare_upstream_request_with_compact(
        &request.path,
        prepared_body,
        route.native_api,
        route.dev_system_normalize,
        route.thinking_effort_override.as_deref(),
        route.compact_mode,
    )?;
    prepared.upstream_redacted_request_json = redacted_request
        .as_ref()
        .and_then(|value| value.redacted_request_json.clone());
    prepared.upstream_restore_session = redacted_request
        .as_ref()
        .and_then(|value| value.restore_session.clone());
    let redaction = route_redaction_state(
        redaction_enabled,
        had_prior_session,
        prepared.upstream_restore_session.clone(),
        prepared.upstream_redacted_request_json.clone(),
    );
    Ok(PreparedRouteRequest {
        prepared,
        redaction,
    })
}

/// Issue #564 Task 2: classify why a prepared request carries no redaction
/// session. A missing session is only a reset signal when the caller proved
/// there was state to drop (explicit disable, or a prior session dropped by a
/// policy-generation mismatch). Everything else is `no_session_available` and
/// must never delete a still-valid row.
fn route_redaction_state(
    redaction_enabled: bool,
    had_prior_session: bool,
    session: Option<UpstreamRedactionSession>,
    redacted_request_json: Option<Value>,
) -> RouteRedactionState {
    if !redaction_enabled {
        return RouteRedactionState {
            enabled: false,
            session: None,
            redacted_request_json: None,
            reset: true,
        };
    }
    RouteRedactionState {
        enabled: true,
        reset: session.is_none() && had_prior_session,
        session,
        redacted_request_json,
    }
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
    let row = db::get_conversation_redaction_session(
        &state.pool,
        conversation_id,
        current_policy_version(),
    )
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

#[cfg(test)]
mod tests {
    use redactor::{FindingKind, InputKind, RedactionPolicy, RedactorBuilder, RestoreState};

    use super::route_redaction_state;
    use crate::redact_upstream::UpstreamRedactionSession;

    fn session(original: &str) -> UpstreamRedactionSession {
        let redactor = RedactorBuilder::new()
            .with_redaction_policy(RedactionPolicy::default().with_kind(FindingKind::Domain, true))
            .build();
        let artifact = redactor
            .redact_artifact_with_input_kind_source_and_prior_session(
                original,
                InputKind::Text,
                None,
                None,
                Some("conversation"),
            )
            .expect("redact");
        UpstreamRedactionSession::current(RestoreState::new(artifact.session).expect("state"))
    }

    #[test]
    fn explicit_disable_requests_a_reset() {
        let state = route_redaction_state(false, false, None, None);
        assert!(!state.enabled);
        assert!(state.reset);
        assert!(state.session.is_none());
    }

    #[test]
    fn missing_session_without_prior_state_is_not_a_reset() {
        // First redaction of a conversation that matched nothing: there is no
        // row to drop, so the persistence layer must keep whatever exists.
        let state = route_redaction_state(true, false, None, None);
        assert!(state.enabled);
        assert!(!state.reset);
    }

    #[test]
    fn refused_prior_session_is_a_reset() {
        // A policy-generation mismatch drops the loaded prior session, so the
        // persisted row must be cleared even though this turn carried state.
        let state = route_redaction_state(true, true, None, None);
        assert!(state.reset);
    }

    #[test]
    fn carried_session_is_never_a_reset() {
        let session = session("a.example.com");
        let state = route_redaction_state(
            true,
            true,
            Some(session),
            Some(serde_json::json!({"instructions": "[[RDX:v2:x]]"})),
        );
        assert!(state.enabled);
        assert!(!state.reset);
        assert!(state.session.is_some());
        assert!(state.redacted_request_json.is_some());
    }
}
