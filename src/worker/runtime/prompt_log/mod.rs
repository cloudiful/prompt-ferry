mod conversation;
mod reconstruct;

use crate::{
    db,
    openai_compat::{conversation_key, previous_response_id},
    redact_upstream::UpstreamRedactionSession,
    usage_prompt::{
        PromptMessageRef, REQUEST_CHAIN_DEPTH_LIMIT, REQUEST_FULL_BYTES_LIMIT, append_delta,
        normalize_prompt_request, prompt_block_hash, prompt_message_refs,
    },
    worker_admin::AdminState,
    worker_admin_types::RequestContentLoggingResponse,
};
use anyhow::Result;
use serde_json::Value;

use super::request_assembly::BufferedBridgeRequest;

use conversation::resolve_prompt_conversation;
use reconstruct::{reconstruct_prompt_chain, redact_prompt_item};

#[derive(Debug, Clone)]
pub(super) struct RequestPromptLog {
    pub(super) conversation_id: Option<uuid::Uuid>,
    pub(super) parent_event_id: Option<i64>,
    pub(super) replay_unavailable: bool,
    pub(super) conversation_seq: Option<i32>,
    pub(super) conversation_source: String,
    pub(super) storage_sanitized: bool,
    pub(super) storage_sanitized_nul_count: i32,
    pub(super) redaction: crate::usage::logging::UsageRedactionSummary,
    pub(super) preferred_endpoint_id: Option<uuid::Uuid>,
    pub(super) conversation_override_endpoint_id: Option<uuid::Uuid>,
    pub(super) conversation_override_endpoint_key_id: Option<uuid::Uuid>,
    pub(super) client_installation_id: Option<String>,
    pub(super) session_header_id: Option<String>,
    pub(super) normalized_item_count: Option<i32>,
    pub(super) normalized_chain_hash: Option<String>,
    pub(super) normalized_first_ref_hash: Option<String>,
    pub(super) normalized_last_ref_hash: Option<String>,
    pub(super) request_storage_mode: String,
    pub(super) request_full_json: Option<Value>,
    pub(super) request_delta_json: Option<Value>,
    pub(super) snapshot_prompt_refs_json: Option<Value>,
    pub(super) request_raw_json: Option<Value>,
    pub(super) request_has_previous_response_id: bool,
    pub(super) request_previous_response_id: Option<String>,
    pub(super) request_previous_response_parent_found: Option<bool>,
    pub(super) request_conversation_key: Option<String>,
    pub(super) request_conversation_parent_found: Option<bool>,
    pub(super) base_checkpoint_event_id: Option<i64>,
    pub(super) upstream_redaction_enabled: bool,
    pub(super) upstream_redacted_request_json: Option<Value>,
    pub(super) upstream_restore_session: Option<UpstreamRedactionSession>,
}

impl Default for RequestPromptLog {
    fn default() -> Self {
        Self {
            conversation_id: None,
            parent_event_id: None,
            replay_unavailable: false,
            conversation_seq: None,
            conversation_source: "none".to_string(),
            storage_sanitized: false,
            storage_sanitized_nul_count: 0,
            redaction: crate::usage::logging::UsageRedactionSummary::default(),
            preferred_endpoint_id: None,
            conversation_override_endpoint_id: None,
            conversation_override_endpoint_key_id: None,
            client_installation_id: None,
            session_header_id: None,
            normalized_item_count: None,
            normalized_chain_hash: None,
            normalized_first_ref_hash: None,
            normalized_last_ref_hash: None,
            request_storage_mode: "full".to_string(),
            request_full_json: None,
            request_delta_json: None,
            snapshot_prompt_refs_json: None,
            request_raw_json: None,
            request_has_previous_response_id: false,
            request_previous_response_id: None,
            request_previous_response_parent_found: None,
            request_conversation_key: None,
            request_conversation_parent_found: None,
            base_checkpoint_event_id: None,
            upstream_redaction_enabled: false,
            upstream_redacted_request_json: None,
            upstream_restore_session: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PromptConversationResolution {
    pub(super) conversation_id: uuid::Uuid,
    pub(super) parent_event_id: Option<i64>,
    pub(super) replay_unavailable: bool,
    pub(super) endpoint_id: Option<uuid::Uuid>,
    pub(super) conversation_seq: i32,
    pub(super) source: &'static str,
}

#[derive(Debug, Clone)]
pub(super) struct RawRequestObservability {
    pub(super) request_raw_json: Option<Value>,
    pub(super) request_has_previous_response_id: bool,
    pub(super) request_previous_response_id: Option<String>,
    pub(super) request_previous_response_parent_found: Option<bool>,
    pub(super) request_conversation_key: Option<String>,
    pub(super) request_conversation_parent_found: Option<bool>,
}

#[derive(Debug, Clone)]
pub(super) struct CodexRequestMetadata {
    pub(super) client_installation_id: Option<String>,
    pub(super) prompt_cache_key: Option<String>,
    pub(super) window_thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ReconstructedPromptChain {
    pub(super) refs: Vec<PromptMessageRef>,
    pub(super) depth: usize,
}

pub(super) async fn prepare_request_prompt_log(
    state: &AdminState,
    request: &BufferedBridgeRequest,
    user_id: Option<i64>,
    _request_model: Option<&str>,
    request_content_logging: &RequestContentLoggingResponse,
    redact_content: bool,
) -> Result<RequestPromptLog> {
    let raw_observability =
        build_raw_request_observability(state, request, user_id, request_content_logging).await?;
    let codex_metadata = codex_request_metadata(&request.body);
    let Some(mut normalized) = normalize_prompt_request(&request.path, &request.body) else {
        return Ok(RequestPromptLog {
            client_installation_id: codex_metadata.client_installation_id,
            session_header_id: session_header_id(request.headers.as_slice()),
            request_raw_json: raw_observability.request_raw_json,
            request_has_previous_response_id: raw_observability.request_has_previous_response_id,
            request_previous_response_id: raw_observability.request_previous_response_id,
            request_previous_response_parent_found: raw_observability
                .request_previous_response_parent_found,
            request_conversation_key: raw_observability.request_conversation_key,
            request_conversation_parent_found: raw_observability.request_conversation_parent_found,
            ..RequestPromptLog::default()
        });
    };
    let mut prompt_redaction = crate::usage::logging::UsageRedactionSummary::default();
    if redact_content {
        let summary = crate::redact::summarize_text_for_user(
            &serde_json::to_string(&normalized.items)?,
            redactor::InputKind::Text,
            user_id,
            &["request_prompt"],
        );
        prompt_redaction = crate::usage::logging::UsageRedactionSummary {
            applied: summary.applied,
            findings_count: summary.findings_count,
            replacements_count: summary.replacements_count,
            types: summary.types,
            fields: summary.fields,
        };
        normalized.items = normalized
            .items
            .into_iter()
            .map(|item| redact_prompt_item(item, user_id))
            .collect();
    }

    let current_refs = prompt_message_refs(&normalized.items);
    let fingerprint = normalized.fingerprint.clone();
    let mut log = RequestPromptLog {
        storage_sanitized: false,
        storage_sanitized_nul_count: 0,
        redaction: prompt_redaction,
        client_installation_id: codex_metadata.client_installation_id.clone(),
        session_header_id: session_header_id(request.headers.as_slice()),
        normalized_item_count: Some(fingerprint.normalized_item_count),
        normalized_chain_hash: Some(fingerprint.normalized_chain_hash.clone()),
        normalized_first_ref_hash: fingerprint.normalized_first_ref_hash.clone(),
        normalized_last_ref_hash: fingerprint.normalized_last_ref_hash.clone(),
        request_full_json: None,
        snapshot_prompt_refs_json: None,
        request_raw_json: raw_observability.request_raw_json,
        request_has_previous_response_id: raw_observability.request_has_previous_response_id,
        request_previous_response_id: raw_observability.request_previous_response_id,
        request_previous_response_parent_found: raw_observability
            .request_previous_response_parent_found,
        request_conversation_key: raw_observability.request_conversation_key,
        request_conversation_parent_found: raw_observability.request_conversation_parent_found,
        replay_unavailable: raw_observability.request_has_previous_response_id
            && raw_observability.request_previous_response_parent_found == Some(false),
        ..RequestPromptLog::default()
    };

    let Some(resolution) = resolve_prompt_conversation(
        state,
        request,
        user_id,
        normalized.previous_response_id.as_deref(),
        normalized.conversation.as_deref(),
        log.session_header_id.as_deref(),
        codex_thread_key(&codex_metadata),
    )
    .await?
    else {
        return Ok(log);
    };

    log.replay_unavailable |= resolution.replay_unavailable;
    if !state.usage_retention.read().await.replay_enabled {
        return Ok(log);
    }

    let mut prompt_block_nul_count = 0_i32;
    for item in &normalized.items {
        let stats = db::upsert_usage_prompt_block(
            &state.pool,
            &prompt_block_hash(&item.role, &item.content_json),
            &item.role,
            &item.content_json,
            &item.preview_text,
        )
        .await?;
        prompt_block_nul_count = prompt_block_nul_count
            .saturating_add(i32::try_from(stats.nul_count).unwrap_or(i32::MAX));
    }
    log.storage_sanitized = prompt_block_nul_count > 0;
    log.storage_sanitized_nul_count = prompt_block_nul_count;
    log.request_full_json = Some(serde_json::to_value(&current_refs)?);
    log.snapshot_prompt_refs_json = Some(serde_json::to_value(&current_refs)?);

    log.conversation_id = Some(resolution.conversation_id);
    log.parent_event_id = resolution.parent_event_id;
    log.conversation_seq = Some(resolution.conversation_seq);
    log.conversation_source = resolution.source.to_string();
    log.preferred_endpoint_id = resolution.endpoint_id;
    if let Some(override_entry) =
        db::get_conversation_endpoint_override(&state.pool, resolution.conversation_id).await?
    {
        log.conversation_override_endpoint_id = Some(override_entry.endpoint_id);
        log.conversation_override_endpoint_key_id = override_entry.endpoint_key_id;
    }

    let force_full = normalized.normalized_bytes_len > REQUEST_FULL_BYTES_LIMIT
        || resolution.conversation_seq == 1
        || resolution.conversation_seq % REQUEST_CHAIN_DEPTH_LIMIT as i32 == 0;
    let parent_entry = if let Some(parent_event_id) = resolution.parent_event_id {
        db::get_usage_event_chain_entry(&state.pool, parent_event_id).await?
    } else {
        None
    };
    let reconstructed_parent = if let Some(parent) = parent_entry.as_ref() {
        reconstruct_prompt_chain(&state.pool, parent).await?
    } else {
        None
    };
    let parent_depth = reconstructed_parent
        .as_ref()
        .map(|chain| chain.depth)
        .unwrap_or(0);
    if !force_full
        && parent_depth < REQUEST_CHAIN_DEPTH_LIMIT
        && let Some(parent_chain) = reconstructed_parent
        && let Some(delta_refs) = append_delta(&parent_chain.refs, &current_refs)
    {
        log.request_storage_mode = "append_delta".to_string();
        log.request_full_json = None;
        log.request_delta_json = Some(serde_json::to_value(&delta_refs)?);
        log.snapshot_prompt_refs_json = Some(serde_json::to_value(&current_refs)?);
        log.base_checkpoint_event_id = parent_entry
            .as_ref()
            .and_then(|parent| parent.base_checkpoint_event_id.or(Some(parent.event_id)));
        return Ok(log);
    }

    Ok(log)
}

async fn build_raw_request_observability(
    state: &AdminState,
    request: &BufferedBridgeRequest,
    user_id: Option<i64>,
    request_content_logging: &RequestContentLoggingResponse,
) -> Result<RawRequestObservability> {
    let request_raw_json = if request_content_logging.mode.captures_raw() {
        serde_json::from_slice::<Value>(&request.body).ok()
    } else {
        None
    };
    let request_previous_response_id = previous_response_id(&request.body);
    let request_has_previous_response_id = request_previous_response_id.is_some();
    let request_previous_response_parent_found =
        if let Some(previous_response_id) = request_previous_response_id.as_deref() {
            Some(
                db::get_usage_event_locator_by_provider_response_id(
                    &state.pool,
                    user_id,
                    previous_response_id,
                )
                .await?
                .is_some(),
            )
        } else {
            None
        };
    let request_conversation_key = conversation_key(&request.body);
    let request_conversation_parent_found =
        if let Some(conversation_key) = request_conversation_key.as_deref() {
            Some(
                db::latest_usage_event_locator_by_provider_conversation_key(
                    &state.pool,
                    user_id,
                    conversation_key,
                )
                .await?
                .is_some(),
            )
        } else {
            None
        };

    Ok(RawRequestObservability {
        request_raw_json,
        request_has_previous_response_id,
        request_previous_response_id,
        request_previous_response_parent_found,
        request_conversation_key,
        request_conversation_parent_found,
    })
}

fn codex_request_metadata(body: &[u8]) -> CodexRequestMetadata {
    let value = serde_json::from_slice::<Value>(body).ok();
    let prompt_cache_key = value
        .as_ref()
        .and_then(|json| json.get("prompt_cache_key"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let window_thread_id = value
        .as_ref()
        .and_then(|json| json.get("client_metadata"))
        .and_then(|json| json.get("x-codex-window-id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            value
                .rsplit_once(':')
                .map(|(thread_id, _)| thread_id)
                .or(Some(value))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    CodexRequestMetadata {
        client_installation_id: value
            .as_ref()
            .and_then(|json| json.get("client_metadata"))
            .and_then(|json| json.get("x-codex-installation-id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        prompt_cache_key,
        window_thread_id,
    }
}

fn codex_thread_key(metadata: &CodexRequestMetadata) -> Option<&str> {
    metadata
        .window_thread_id
        .as_deref()
        .or(metadata.prompt_cache_key.as_deref())
}

fn session_header_id(headers: &[(String, String)]) -> Option<String> {
    [
        "x-session-id",
        "x-session-affinity",
        "x-opencode-session",
        "session-id",
    ]
    .into_iter()
    .find_map(|expected_name| {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(expected_name))
            .map(|(_, value)| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

pub(super) fn resolve_mcp_conversation_log() -> RequestPromptLog {
    RequestPromptLog::default()
}

#[cfg(test)]
mod tests {
    use super::{codex_request_metadata, codex_thread_key, session_header_id};

    #[test]
    fn codex_thread_identity_precedes_prompt_cache_key() {
        let metadata = codex_request_metadata(
            br#"{
                "prompt_cache_key": "guardian:parent-session",
                "client_metadata": {
                    "x-codex-window-id": "guardian-child:0"
                }
            }"#,
        );

        assert_eq!(codex_thread_key(&metadata), Some("guardian-child"));
    }

    #[test]
    fn prompt_cache_key_is_used_without_codex_thread_identity() {
        let metadata = codex_request_metadata(
            br#"{
                "prompt_cache_key": "guardian:parent-session",
                "input": "hello"
            }"#,
        );

        assert_eq!(codex_thread_key(&metadata), Some("guardian:parent-session"));
    }

    #[test]
    fn accepts_opencode_session_affinity_headers() {
        let headers = vec![
            ("x-session-affinity".to_string(), "ses_123".to_string()),
            ("user-agent".to_string(), "opencode".to_string()),
        ];

        assert_eq!(session_header_id(&headers).as_deref(), Some("ses_123"));
    }

    #[test]
    fn prefers_explicit_session_id_over_affinity_fallback() {
        let headers = vec![
            ("x-session-affinity".to_string(), "affinity".to_string()),
            ("X-Session-Id".to_string(), "session".to_string()),
        ];

        assert_eq!(session_header_id(&headers).as_deref(), Some("session"));
    }
}
