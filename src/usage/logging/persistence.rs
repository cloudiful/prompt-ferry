use crate::{
    db::{self, UsageEventKind},
    redact_upstream::encrypt_upstream_session,
    replay_cache::{ReplaySnapshotUpdate, update_replay_state},
    storage_sanitization::{
        SanitizationStats, sanitize_optional_json_for_storage, sanitize_optional_text_for_storage,
    },
    worker::json_walker::walk_json_strings,
    worker_admin_state::AdminState,
};
use serde_json::Value;
use tracing::warn;

use super::{UsageLog, inference::infer_failure_family};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsageRecordingMode {
    SharedManaged,
    Standalone,
    Noop,
}

pub(crate) fn usage_recording_mode(
    has_admin_state: bool,
    has_standalone_state: bool,
) -> UsageRecordingMode {
    if has_admin_state {
        UsageRecordingMode::SharedManaged
    } else if has_standalone_state {
        UsageRecordingMode::Standalone
    } else {
        UsageRecordingMode::Noop
    }
}

/// Issue #524 Task 4: redact every string inside `request_raw_json` using the
/// user-scoped runtime before the payload lands in `usage_records`. This is
/// the last line of defense for request bodies that were not gated through
/// `worker::runtime::ai::upstream_text_fields` (e.g. raw REST captures that
/// skip the reasoning-field walker).
///
/// Fail-open with `warn!`: any walker error leaves the original payload
/// untouched so the usage pipeline never aborts because of a redaction blip,
/// but operators see a signal that a particular request slipped past
/// redaction.
fn redact_request_raw_json(value: Option<Value>, user_id: Option<i64>) -> Option<Value> {
    let mut value = value?;
    let result = walk_json_strings(&mut value, |_, text| {
        Ok(Some(crate::redact::redact_text_for_user(text, user_id)))
    });
    if let Err(err) = result {
        warn!(
            error = %err,
            user_id = user_id.unwrap_or(0),
            "failed to walk request_raw_json for redaction; persisting original payload"
        );
    }
    Some(value)
}

pub async fn record_usage_event(admin_state: Option<&AdminState>, log: UsageLog) -> Option<i64> {
    let state = admin_state?;
    let failure_family = log.failure_family.or_else(|| infer_failure_family(&log));
    let snapshot_prompt_refs_json = log.snapshot_prompt_refs_json.clone();
    let conversation_id = log.conversation_id;
    let conversation_seq = log.conversation_seq;
    let (request_full_json, request_full_json_stats) =
        sanitize_optional_json_for_storage(log.request_full_json);
    let (request_delta_json, request_delta_json_stats) =
        sanitize_optional_json_for_storage(log.request_delta_json);
    let (request_raw_json, request_raw_json_stats) = sanitize_optional_json_for_storage(
        redact_request_raw_json(log.request_raw_json, log.user_id),
    );
    let (response_prompt, response_prompt_stats) =
        sanitize_optional_text_for_storage(log.response_prompt);
    let (response_raw_body, response_raw_body_stats) =
        sanitize_optional_text_for_storage(log.response_raw_body);
    let (upstream_error_body, upstream_error_body_stats) =
        sanitize_optional_text_for_storage(log.upstream_error_body);
    let (error_message, error_message_stats) =
        sanitize_optional_text_for_storage(log.error_message);
    let mut request_storage_stats = SanitizationStats::default();
    request_storage_stats.merge(request_full_json_stats);
    request_storage_stats.merge(request_delta_json_stats);
    request_storage_stats.merge(request_raw_json_stats);
    request_storage_stats.merge(response_prompt_stats);
    request_storage_stats.merge(response_raw_body_stats);
    request_storage_stats.merge(upstream_error_body_stats);
    request_storage_stats.merge(error_message_stats);
    let storage_sanitized = log.storage_sanitized || request_storage_stats.sanitized();
    let storage_sanitized_nul_count = log.storage_sanitized_nul_count
        + i32::try_from(request_storage_stats.nul_count).unwrap_or(i32::MAX);
    let create = match log.request_category {
        db::RequestRecordCategory::Ai => {
            db::RequestRecordCreate::ai_request(log.request_id, log.path.clone())
        }
        db::RequestRecordCategory::Mcp => {
            db::RequestRecordCreate::mcp_request(log.request_id, log.path.clone())
        }
    }
    .with_state(log.event_kind, log.request_state)
    .with_request_actor(
        log.user_id,
        log.client_key_id,
        log.client_key_label,
        log.request_user_agent,
    )
    .with_http_request_compression(
        log.http_request_content_encoding,
        log.http_request_compressed,
        log.http_request_compressed_bytes,
        log.http_request_decompressed_bytes,
        log.http_request_compression_ratio,
    )
    .with_route(log.endpoint_id, log.model_route_rule_id)
    .with_endpoint_key(log.endpoint_key_id, log.endpoint_key_label)
    .with_mcp_context(
        log.mcp_server_id,
        log.mcp_server_name,
        log.mcp_protocol_method,
        log.mcp_operation_name,
    )
    .with_model(log.model)
    .with_billing_models(log.requested_model, log.upstream_model)
    .with_timing(log.status, log.ok, log.duration_ms, log.ttft_ms)
    .with_usage(
        log.usage.input_tokens,
        log.usage.output_tokens,
        log.usage.total_tokens,
        log.usage.cached_tokens,
        log.usage.cache_read_tokens,
        log.usage.cache_write_tokens,
    )
    .with_request_context(db::RequestRecordContextInput {
        conversation_id: log.conversation_id,
        parent_event_id: log.parent_event_id,
        conversation_seq: log.conversation_seq,
        conversation_source: log.conversation_source,
        client_installation_id: log.client_installation_id,
        normalized_item_count: log.normalized_item_count,
        normalized_chain_hash: log.normalized_chain_hash,
        normalized_first_ref_hash: log.normalized_first_ref_hash,
        normalized_last_ref_hash: log.normalized_last_ref_hash,
        base_checkpoint_event_id: log.base_checkpoint_event_id,
    })
    .with_request_storage(db::RequestRecordStorageInput {
        storage_sanitized,
        storage_sanitized_nul_count,
        redaction: db::RequestRecordRedactionSummaryInput {
            applied: log.redaction.applied,
            findings_count: log.redaction.findings_count,
            replacements_count: log.redaction.replacements_count,
            types_json: if log.redaction.types.is_empty() {
                None
            } else {
                Some(serde_json::json!(log.redaction.types))
            },
            fields_json: if log.redaction.fields.is_empty() {
                None
            } else {
                Some(serde_json::json!(log.redaction.fields))
            },
        },
        request_storage_mode: log.request_storage_mode,
        request_full_json,
        request_delta_json,
        request_raw_json: request_raw_json.clone(),
        request_has_previous_response_id: log.request_has_previous_response_id,
        request_previous_response_id: log.request_previous_response_id,
        request_previous_response_parent_found: log.request_previous_response_parent_found,
        request_conversation_key: log.request_conversation_key,
        request_conversation_parent_found: log.request_conversation_parent_found,
        upstream_redaction_enabled: log.upstream_redaction_enabled,
        response_prompt,
        response_raw_body: response_raw_body.clone(),
        response_capture_truncated: log.response_capture_truncated,
    })
    .with_provider_response(log.provider_response_id, log.provider_conversation_key)
    .with_error(upstream_error_body, log.error_code, error_message)
    .with_failure_family(failure_family)
    .with_mcp_token_slot(log.mcp_bearer_token_slot)
    .with_route_selection(log.route_selection_reason)
    .with_worker_lease(
        log.owner_worker_id,
        log.lease_expires_at,
        log.last_heartbeat_at,
    );
    let retention = state.usage_retention.read().await.clone().normalized();
    let raw_store = state.raw_payload_store.read().await.clone();
    match db::record_request_record_with_raw_store(
        &state.pool,
        create,
        raw_store.as_deref(),
        i64::from(retention.raw_retention_days),
    )
    .await
    {
        Ok(event_id) => {
            if let Some(conversation_id) = conversation_id {
                match log.upstream_restore_session.as_ref() {
                    Some(session) => {
                        if let Ok(manager) = state.relay_secret_manager()
                            && let Ok(encrypted) = encrypt_upstream_session(manager, session)
                        {
                            let create = db::ConversationRedactionSessionCreate {
                                conversation_id,
                                session_ciphertext: encrypted.ciphertext,
                                session_nonce: encrypted.nonce,
                                session_key_version: encrypted.key_version,
                                last_event_id: Some(event_id),
                                policy_version: session.policy_version(),
                            };
                            match db::upsert_conversation_redaction_session(&state.pool, create)
                                .await
                            {
                                Ok(0) => warn!(
                                    conversation_id = %conversation_id,
                                    event_id,
                                    "stale conversation redaction session write ignored"
                                ),
                                Ok(_) => {}
                                Err(err) => warn!(
                                    error = %err,
                                    conversation_id = %conversation_id,
                                    event_id,
                                    "failed to persist conversation redaction session"
                                ),
                            }
                        }
                    }
                    None => {
                        // Issue #524 Task 5: `None` means redaction is disabled
                        // for this request (disable deletes the row) or the
                        // session exceeded its budget (Task 2). Both drop the
                        // persisted row so re-enabling starts from a fresh token
                        // counter instead of reviving the old mapping.
                        if let Err(err) =
                            db::delete_conversation_redaction_session(&state.pool, conversation_id)
                                .await
                        {
                            warn!(
                                error = %err,
                                conversation_id = %conversation_id,
                                event_id,
                                "failed to delete conversation redaction session"
                            );
                        }
                    }
                }
            }
            if storage_sanitized_nul_count > 0 {
                warn!(
                    request_id = %log.request_id,
                    event_id,
                    nul_count = storage_sanitized_nul_count,
                    "sanitized NUL bytes from request storage payload before postgres write"
                );
            }
            if log.event_kind == UsageEventKind::Request
                && let (
                    Some(conversation_id),
                    Some(conversation_seq),
                    Some(snapshot_prompt_refs_json),
                ) = (conversation_id, conversation_seq, snapshot_prompt_refs_json)
                && let Ok(prompt_refs) = db::decode_prompt_message_refs(&snapshot_prompt_refs_json)
            {
                update_replay_state(
                    &state.pool,
                    &state.replay_cache,
                    ReplaySnapshotUpdate {
                        event_id,
                        conversation_id,
                        conversation_seq,
                        prompt_refs,
                    },
                )
                .await;
            }
            Some(event_id)
        }
        Err(err) => {
            warn!(error = %err, "failed to record usage event");
            None
        }
    }
}
