use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{
    db::{self, RequestFailureFamily, RouteSelectionReason, UsageEventKind},
    redact_upstream::UpstreamRedactionSession,
    usage::TokenUsage,
};

use super::inference::infer_failure_family;

const STANDALONE_SUMMARY_TEXT_LIMIT: usize = 256;
const STANDALONE_SUMMARY_LIST_LIMIT: usize = 16;
const STANDALONE_SUMMARY_LIST_TEXT_LIMIT: usize = 64;

#[derive(Debug, Clone, Default)]
pub struct UsageRedactionSummary {
    pub applied: bool,
    pub findings_count: i32,
    pub replacements_count: i32,
    pub types: Vec<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UsageRequestMetadata {
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub request_user_agent: Option<String>,
    pub path: String,
    pub http_request_content_encoding: Option<String>,
    pub http_request_compressed: bool,
    pub http_request_compressed_bytes: Option<i64>,
    pub http_request_decompressed_bytes: Option<i64>,
    pub http_request_compression_ratio: Option<f64>,
    pub conversation_id: Option<uuid::Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: String,
    pub storage_sanitized: bool,
    pub storage_sanitized_nul_count: i32,
    pub redaction: UsageRedactionSummary,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub normalized_chain_hash: Option<String>,
    pub normalized_first_ref_hash: Option<String>,
    pub normalized_last_ref_hash: Option<String>,
    pub request_storage_mode: String,
    pub request_full_json: Option<Value>,
    pub request_delta_json: Option<Value>,
    pub snapshot_prompt_refs_json: Option<Value>,
    pub request_raw_json: Option<Value>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub base_checkpoint_event_id: Option<i64>,
    pub upstream_redaction_enabled: bool,
    pub upstream_redacted_request_json: Option<Value>,
    pub upstream_restore_session: Option<UpstreamRedactionSession>,
    pub owner_worker_id: Option<uuid::Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
}

impl Default for UsageRequestMetadata {
    fn default() -> Self {
        Self {
            user_id: None,
            client_key_id: None,
            client_key_label: None,
            request_user_agent: None,
            path: String::new(),
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
            http_request_decompressed_bytes: None,
            http_request_compression_ratio: None,
            conversation_id: None,
            parent_event_id: None,
            conversation_seq: None,
            conversation_source: "none".to_string(),
            storage_sanitized: false,
            storage_sanitized_nul_count: 0,
            redaction: UsageRedactionSummary::default(),
            client_installation_id: None,
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
            owner_worker_id: None,
            lease_expires_at: None,
            last_heartbeat_at: None,
        }
    }
}

pub struct UsageLog {
    pub event_kind: UsageEventKind,
    pub request_category: db::RequestRecordCategory,
    pub request_state: db::RequestRecordState,
    pub request_id: uuid::Uuid,
    pub user_id: Option<i64>,
    pub client_key_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub request_user_agent: Option<String>,
    pub endpoint_id: Option<uuid::Uuid>,
    pub endpoint_key_id: Option<uuid::Uuid>,
    pub endpoint_key_label: Option<String>,
    pub model_route_rule_id: Option<uuid::Uuid>,
    pub mcp_server_id: Option<uuid::Uuid>,
    pub mcp_server_name: Option<String>,
    pub mcp_protocol_method: Option<String>,
    pub mcp_operation_name: Option<String>,
    pub path: String,
    pub http_request_content_encoding: Option<String>,
    pub http_request_compressed: bool,
    pub http_request_compressed_bytes: Option<i64>,
    pub http_request_decompressed_bytes: Option<i64>,
    pub http_request_compression_ratio: Option<f64>,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub upstream_model: Option<String>,
    pub status: Option<i32>,
    pub ok: Option<bool>,
    pub duration_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub usage: TokenUsage,
    pub conversation_id: Option<uuid::Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: String,
    pub storage_sanitized: bool,
    pub storage_sanitized_nul_count: i32,
    pub redaction: UsageRedactionSummary,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub normalized_chain_hash: Option<String>,
    pub normalized_first_ref_hash: Option<String>,
    pub normalized_last_ref_hash: Option<String>,
    pub request_storage_mode: String,
    pub request_full_json: Option<Value>,
    pub request_delta_json: Option<Value>,
    pub snapshot_prompt_refs_json: Option<Value>,
    pub request_raw_json: Option<Value>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub provider_response_id: Option<String>,
    pub provider_conversation_key: Option<String>,
    pub base_checkpoint_event_id: Option<i64>,
    pub upstream_redaction_enabled: bool,
    pub upstream_redacted_request_json: Option<Value>,
    pub upstream_restore_session: Option<UpstreamRedactionSession>,
    pub response_prompt: Option<String>,
    pub response_raw_body: Option<String>,
    pub response_capture_truncated: bool,
    pub upstream_error_body: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub failure_family: Option<RequestFailureFamily>,
    pub mcp_bearer_token_slot: Option<i16>,
    pub route_selection_reason: RouteSelectionReason,
    pub owner_worker_id: Option<uuid::Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
}

impl UsageLog {
    pub(crate) fn into_standalone_summary(self) -> StandaloneUsageSummary {
        let failure_family = self
            .failure_family
            .or_else(|| infer_failure_family(&self))
            .map(|family| family.as_str().to_string());
        let conversation_source = bounded_text(
            Some(self.conversation_source),
            STANDALONE_SUMMARY_TEXT_LIMIT,
        )
        .unwrap_or_else(|| "none".to_string());
        let request_storage_mode = bounded_text(
            Some(self.request_storage_mode),
            STANDALONE_SUMMARY_TEXT_LIMIT,
        )
        .unwrap_or_else(|| "full".to_string());
        let snapshot_prompt_refs_json = self.snapshot_prompt_refs_json;
        let replay_conversation_id = self.conversation_id;
        let replay_conversation_seq = self.conversation_seq;
        StandaloneUsageSummary {
            request_id: self.request_id,
            event_kind: self.event_kind.as_str().to_string(),
            category: self.request_category.as_str().to_string(),
            state: self.request_state.as_str().to_string(),
            path: bounded_text(Some(self.path), STANDALONE_SUMMARY_TEXT_LIMIT).unwrap_or_default(),
            recorded_at: chrono::Utc::now(),
            status: self.status,
            ok: self.ok,
            duration_ms: self.duration_ms,
            ttft_ms: self.ttft_ms,
            model: bounded_text(self.model, STANDALONE_SUMMARY_TEXT_LIMIT),
            requested_model: bounded_text(self.requested_model, STANDALONE_SUMMARY_TEXT_LIMIT),
            upstream_model: bounded_text(self.upstream_model, STANDALONE_SUMMARY_TEXT_LIMIT),
            endpoint_id: self.endpoint_id,
            endpoint_key_id: self.endpoint_key_id,
            model_route_rule_id: self.model_route_rule_id,
            mcp_server_id: self.mcp_server_id,
            input_tokens: self.usage.input_tokens,
            output_tokens: self.usage.output_tokens,
            total_tokens: self.usage.total_tokens,
            cached_tokens: self.usage.cached_tokens,
            cache_read_tokens: self.usage.cache_read_tokens,
            cache_write_tokens: self.usage.cache_write_tokens,
            error_code: bounded_text(self.error_code, STANDALONE_SUMMARY_TEXT_LIMIT),
            failure_family,
            redaction: StandaloneUsageRedactionSummary {
                applied: self.redaction.applied,
                findings_count: self.redaction.findings_count,
                replacements_count: self.redaction.replacements_count,
                types: bounded_list(self.redaction.types),
                fields: bounded_list(self.redaction.fields),
            },
            route_selection_reason: self.route_selection_reason.as_str().to_string(),
            user_id: self.user_id,
            client_key_id: self.client_key_id,
            client_key_label: bounded_text(self.client_key_label, STANDALONE_SUMMARY_TEXT_LIMIT),
            request_user_agent: bounded_text(
                self.request_user_agent,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            endpoint_key_label: bounded_text(
                self.endpoint_key_label,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            mcp_server_name: bounded_text(self.mcp_server_name, STANDALONE_SUMMARY_TEXT_LIMIT),
            mcp_protocol_method: bounded_text(
                self.mcp_protocol_method,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            mcp_operation_name: bounded_text(
                self.mcp_operation_name,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            http_request_content_encoding: bounded_text(
                self.http_request_content_encoding,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            http_request_compressed: self.http_request_compressed,
            http_request_compressed_bytes: self.http_request_compressed_bytes,
            http_request_decompressed_bytes: self.http_request_decompressed_bytes,
            http_request_compression_ratio: self.http_request_compression_ratio,
            conversation_source,
            client_installation_id: bounded_text(
                self.client_installation_id,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            provider_response_id: bounded_text(
                self.provider_response_id,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            provider_conversation_key: bounded_text(
                self.provider_conversation_key,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            request_storage_mode,
            error_message: bounded_text(self.error_message, STANDALONE_SUMMARY_TEXT_LIMIT),
            request_has_previous_response_id: self.request_has_previous_response_id,
            request_previous_response_id: bounded_text(
                self.request_previous_response_id,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            request_previous_response_parent_found: self.request_previous_response_parent_found,
            request_conversation_key: bounded_text(
                self.request_conversation_key,
                STANDALONE_SUMMARY_TEXT_LIMIT,
            ),
            request_conversation_parent_found: self.request_conversation_parent_found,
            upstream_redaction_enabled: self.upstream_redaction_enabled,
            response_capture_truncated: self.response_capture_truncated,
            replay_conversation_id,
            replay_conversation_seq,
            snapshot_prompt_refs_json: snapshot_prompt_refs_json_to_string(
                snapshot_prompt_refs_json,
            ),
        }
    }

    pub fn ai_request(
        request_id: uuid::Uuid,
        metadata: UsageRequestMetadata,
        model: Option<String>,
    ) -> Self {
        Self::base(
            db::UsageEventKind::Request,
            db::RequestRecordCategory::Ai,
            request_id,
            metadata,
            model,
        )
    }

    pub fn mcp_request(
        request_id: uuid::Uuid,
        metadata: UsageRequestMetadata,
        server_name: Option<String>,
        protocol_method: Option<String>,
        operation_name: Option<String>,
    ) -> Self {
        Self::base(
            db::UsageEventKind::Request,
            db::RequestRecordCategory::Mcp,
            request_id,
            metadata,
            None,
        )
        .with_mcp_context(None, server_name, protocol_method, operation_name)
    }

    fn base(
        event_kind: db::UsageEventKind,
        request_category: db::RequestRecordCategory,
        request_id: uuid::Uuid,
        metadata: UsageRequestMetadata,
        model: Option<String>,
    ) -> Self {
        Self {
            event_kind,
            request_category,
            request_state: db::RequestRecordState::Received,
            request_id,
            user_id: metadata.user_id,
            client_key_id: metadata.client_key_id,
            client_key_label: metadata.client_key_label,
            request_user_agent: metadata.request_user_agent,
            endpoint_id: None,
            endpoint_key_id: None,
            endpoint_key_label: None,
            model_route_rule_id: None,
            mcp_server_id: None,
            mcp_server_name: None,
            mcp_protocol_method: None,
            mcp_operation_name: None,
            path: metadata.path,
            http_request_content_encoding: metadata.http_request_content_encoding,
            http_request_compressed: metadata.http_request_compressed,
            http_request_compressed_bytes: metadata.http_request_compressed_bytes,
            http_request_decompressed_bytes: metadata.http_request_decompressed_bytes,
            http_request_compression_ratio: metadata.http_request_compression_ratio,
            model: model.clone(),
            requested_model: model,
            upstream_model: None,
            status: None,
            ok: None,
            duration_ms: None,
            ttft_ms: None,
            usage: TokenUsage::default(),
            conversation_id: metadata.conversation_id,
            parent_event_id: metadata.parent_event_id,
            conversation_seq: metadata.conversation_seq,
            conversation_source: metadata.conversation_source,
            storage_sanitized: metadata.storage_sanitized,
            storage_sanitized_nul_count: metadata.storage_sanitized_nul_count,
            redaction: metadata.redaction,
            client_installation_id: metadata.client_installation_id,
            normalized_item_count: metadata.normalized_item_count,
            normalized_chain_hash: metadata.normalized_chain_hash,
            normalized_first_ref_hash: metadata.normalized_first_ref_hash,
            normalized_last_ref_hash: metadata.normalized_last_ref_hash,
            request_storage_mode: metadata.request_storage_mode,
            request_full_json: metadata.request_full_json,
            request_delta_json: metadata.request_delta_json,
            snapshot_prompt_refs_json: metadata.snapshot_prompt_refs_json,
            request_raw_json: metadata.request_raw_json,
            request_has_previous_response_id: metadata.request_has_previous_response_id,
            request_previous_response_id: metadata.request_previous_response_id,
            request_previous_response_parent_found: metadata.request_previous_response_parent_found,
            request_conversation_key: metadata.request_conversation_key,
            request_conversation_parent_found: metadata.request_conversation_parent_found,
            provider_response_id: None,
            provider_conversation_key: None,
            base_checkpoint_event_id: metadata.base_checkpoint_event_id,
            upstream_redaction_enabled: metadata.upstream_redaction_enabled,
            upstream_redacted_request_json: metadata.upstream_redacted_request_json,
            upstream_restore_session: metadata.upstream_restore_session,
            response_prompt: None,
            response_raw_body: None,
            response_capture_truncated: false,
            upstream_error_body: None,
            error_code: None,
            error_message: None,
            failure_family: None,
            mcp_bearer_token_slot: None,
            route_selection_reason: RouteSelectionReason::Default,
            owner_worker_id: metadata.owner_worker_id,
            lease_expires_at: metadata.lease_expires_at,
            last_heartbeat_at: metadata.last_heartbeat_at,
        }
    }

    pub fn with_state(
        mut self,
        event_kind: db::UsageEventKind,
        request_state: db::RequestRecordState,
    ) -> Self {
        self.event_kind = event_kind;
        self.request_state = request_state;
        self
    }

    pub fn with_route(
        mut self,
        endpoint_id: Option<uuid::Uuid>,
        model_route_rule_id: Option<uuid::Uuid>,
    ) -> Self {
        self.endpoint_id = endpoint_id;
        self.model_route_rule_id = model_route_rule_id;
        self
    }

    pub fn with_endpoint_key(
        mut self,
        endpoint_key_id: Option<uuid::Uuid>,
        endpoint_key_label: Option<String>,
    ) -> Self {
        self.endpoint_key_id = endpoint_key_id;
        self.endpoint_key_label = endpoint_key_label;
        self
    }

    pub fn with_mcp_context(
        mut self,
        mcp_server_id: Option<uuid::Uuid>,
        mcp_server_name: Option<String>,
        mcp_protocol_method: Option<String>,
        mcp_operation_name: Option<String>,
    ) -> Self {
        self.mcp_server_id = mcp_server_id;
        self.mcp_server_name = mcp_server_name;
        self.mcp_protocol_method = mcp_protocol_method;
        self.mcp_operation_name = mcp_operation_name;
        self
    }

    pub fn with_model(mut self, model: Option<String>) -> Self {
        if model.is_some() {
            self.model = model.clone();
            self.upstream_model = model;
        }
        self
    }

    pub fn with_upstream_model(mut self, model: Option<String>) -> Self {
        if model.is_some() {
            self.upstream_model = model;
        }
        self
    }

    pub fn with_status(
        mut self,
        status: Option<i32>,
        ok: Option<bool>,
        duration_ms: Option<i64>,
        ttft_ms: Option<i64>,
    ) -> Self {
        self.status = status;
        self.ok = ok;
        self.duration_ms = duration_ms;
        self.ttft_ms = ttft_ms;
        self
    }

    pub fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = usage;
        self
    }

    pub fn with_response(
        mut self,
        provider_response_id: Option<String>,
        provider_conversation_key: Option<String>,
        response_prompt: Option<String>,
        response_raw_body: Option<String>,
    ) -> Self {
        self.provider_response_id = provider_response_id;
        self.provider_conversation_key = provider_conversation_key;
        self.response_prompt = response_prompt;
        self.response_raw_body = response_raw_body;
        self
    }

    pub fn with_response_capture_truncated(mut self, truncated: bool) -> Self {
        self.response_capture_truncated = truncated;
        self
    }

    pub fn with_request_raw_json(mut self, request_raw_json: Option<Value>) -> Self {
        self.request_raw_json = request_raw_json;
        self
    }

    pub fn with_upstream_redaction(
        mut self,
        upstream_redaction_enabled: bool,
        upstream_redacted_request_json: Option<Value>,
        upstream_restore_session: Option<UpstreamRedactionSession>,
    ) -> Self {
        self.upstream_redaction_enabled = upstream_redaction_enabled;
        self.upstream_redacted_request_json = upstream_redacted_request_json;
        self.upstream_restore_session = upstream_restore_session;
        self
    }

    pub fn with_error(
        mut self,
        error_code: Option<String>,
        error_message: Option<String>,
        upstream_error_body: Option<String>,
    ) -> Self {
        self.error_code = error_code;
        self.error_message = error_message;
        self.upstream_error_body = upstream_error_body;
        self
    }

    pub fn with_failure_family(mut self, failure_family: Option<RequestFailureFamily>) -> Self {
        self.failure_family = failure_family;
        self
    }

    pub fn with_mcp_token_slot(mut self, mcp_bearer_token_slot: Option<i16>) -> Self {
        self.mcp_bearer_token_slot = mcp_bearer_token_slot;
        self
    }

    pub fn with_route_selection(mut self, route_selection_reason: RouteSelectionReason) -> Self {
        self.route_selection_reason = route_selection_reason;
        self
    }

    pub fn with_worker_lease(
        mut self,
        owner_worker_id: Option<uuid::Uuid>,
        lease_expires_at: Option<DateTime<Utc>>,
        last_heartbeat_at: Option<DateTime<Utc>>,
    ) -> Self {
        self.owner_worker_id = owner_worker_id;
        self.lease_expires_at = lease_expires_at;
        self.last_heartbeat_at = last_heartbeat_at;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StandaloneUsageRedactionSummary {
    pub(crate) applied: bool,
    pub(crate) findings_count: i32,
    pub(crate) replacements_count: i32,
    pub(crate) types: Vec<String>,
    pub(crate) fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StandaloneUsageSummary {
    pub(crate) request_id: uuid::Uuid,
    pub(crate) event_kind: String,
    pub(crate) category: String,
    pub(crate) state: String,
    pub(crate) path: String,
    pub(crate) recorded_at: chrono::DateTime<chrono::Utc>,
    pub(crate) status: Option<i32>,
    pub(crate) ok: Option<bool>,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) ttft_ms: Option<i64>,
    pub(crate) model: Option<String>,
    pub(crate) requested_model: Option<String>,
    pub(crate) upstream_model: Option<String>,
    pub(crate) endpoint_id: Option<uuid::Uuid>,
    pub(crate) endpoint_key_id: Option<uuid::Uuid>,
    pub(crate) model_route_rule_id: Option<uuid::Uuid>,
    pub(crate) mcp_server_id: Option<uuid::Uuid>,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) cached_tokens: Option<i64>,
    pub(crate) cache_read_tokens: Option<i64>,
    pub(crate) cache_write_tokens: Option<i64>,
    pub(crate) error_code: Option<String>,
    pub(crate) failure_family: Option<String>,
    pub(crate) redaction: StandaloneUsageRedactionSummary,
    pub(crate) route_selection_reason: String,
    pub(crate) user_id: Option<i64>,
    pub(crate) client_key_id: Option<i64>,
    pub(crate) client_key_label: Option<String>,
    pub(crate) request_user_agent: Option<String>,
    pub(crate) endpoint_key_label: Option<String>,
    pub(crate) mcp_server_name: Option<String>,
    pub(crate) mcp_protocol_method: Option<String>,
    pub(crate) mcp_operation_name: Option<String>,
    pub(crate) http_request_content_encoding: Option<String>,
    pub(crate) http_request_compressed: bool,
    pub(crate) http_request_compressed_bytes: Option<i64>,
    pub(crate) http_request_decompressed_bytes: Option<i64>,
    pub(crate) http_request_compression_ratio: Option<f64>,
    pub(crate) conversation_source: String,
    pub(crate) client_installation_id: Option<String>,
    pub(crate) provider_response_id: Option<String>,
    pub(crate) provider_conversation_key: Option<String>,
    pub(crate) request_storage_mode: String,
    pub(crate) error_message: Option<String>,
    pub(crate) request_has_previous_response_id: bool,
    pub(crate) request_previous_response_id: Option<String>,
    pub(crate) request_previous_response_parent_found: Option<bool>,
    pub(crate) request_conversation_key: Option<String>,
    pub(crate) request_conversation_parent_found: Option<bool>,
    pub(crate) upstream_redaction_enabled: bool,
    pub(crate) response_capture_truncated: bool,
    // Phase 1C-b transient replay metadata. The standalone persistence
    // layer decodes `snapshot_prompt_refs_json` and upserts the SQLite
    // replay snapshot row after the usage-summary write completes; raw
    // request or response bodies are intentionally absent. `None`
    // values mean the request was not part of a replay-capable
    // conversation and the snapshot path is skipped.
    pub(crate) replay_conversation_id: Option<uuid::Uuid>,
    pub(crate) replay_conversation_seq: Option<i32>,
    pub(crate) snapshot_prompt_refs_json: Option<String>,
}

fn bounded_text(value: Option<String>, limit: usize) -> Option<String> {
    value.map(|value| value.chars().take(limit).collect())
}

fn bounded_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .take(STANDALONE_SUMMARY_LIST_LIMIT)
        .map(|value| {
            value
                .chars()
                .take(STANDALONE_SUMMARY_LIST_TEXT_LIMIT)
                .collect()
        })
        .collect()
}

/// Serialize the transient `snapshot_prompt_refs_json` payload into the
/// storage representation carried by `StandaloneUsageSummary`. The
/// persistence layer hands the string back to
/// `db::decode_prompt_message_refs` for typed validation, so any
/// `serde_json` failure here would otherwise silently lose replay data;
/// we therefore fall back to the `Display` form so a malformed value
/// is still visible (and round-trippable) for diagnostics rather than
/// vanishing.
fn snapshot_prompt_refs_json_to_string(value: Option<Value>) -> Option<String> {
    let value = value?;
    match serde_json::to_string(&value) {
        Ok(serialized) => Some(serialized),
        Err(error) => Some(format!("/* invalid snapshot_prompt_refs_json: {error} */")),
    }
}

impl From<&StandaloneUsageSummary> for crate::standalone_config::StandaloneUsageSummaryRecord {
    fn from(summary: &StandaloneUsageSummary) -> Self {
        Self {
            request_id: summary.request_id,
            event_kind: summary.event_kind.clone(),
            category: summary.category.clone(),
            state: summary.state.clone(),
            path: summary.path.clone(),
            recorded_at: summary.recorded_at,
            status: summary.status,
            ok: summary.ok,
            duration_ms: summary.duration_ms,
            ttft_ms: summary.ttft_ms,
            model: summary.model.clone(),
            requested_model: summary.requested_model.clone(),
            upstream_model: summary.upstream_model.clone(),
            endpoint_id: summary.endpoint_id,
            endpoint_key_id: summary.endpoint_key_id,
            model_route_rule_id: summary.model_route_rule_id,
            mcp_server_id: summary.mcp_server_id,
            input_tokens: summary.input_tokens,
            output_tokens: summary.output_tokens,
            total_tokens: summary.total_tokens,
            cached_tokens: summary.cached_tokens,
            cache_read_tokens: summary.cache_read_tokens,
            cache_write_tokens: summary.cache_write_tokens,
            error_code: summary.error_code.clone(),
            failure_family: summary.failure_family.clone(),
            redaction_applied: summary.redaction.applied,
            redaction_findings_count: summary.redaction.findings_count,
            redaction_replacements_count: summary.redaction.replacements_count,
            redaction_types: summary.redaction.types.clone(),
            redaction_fields: summary.redaction.fields.clone(),
            route_selection_reason: summary.route_selection_reason.clone(),
            user_id: summary.user_id,
            client_key_id: summary.client_key_id,
            client_key_label: summary.client_key_label.clone(),
            request_user_agent: summary.request_user_agent.clone(),
            endpoint_key_label: summary.endpoint_key_label.clone(),
            mcp_server_name: summary.mcp_server_name.clone(),
            mcp_protocol_method: summary.mcp_protocol_method.clone(),
            mcp_operation_name: summary.mcp_operation_name.clone(),
            http_request_content_encoding: summary.http_request_content_encoding.clone(),
            http_request_compressed: summary.http_request_compressed,
            http_request_compressed_bytes: summary.http_request_compressed_bytes,
            http_request_decompressed_bytes: summary.http_request_decompressed_bytes,
            http_request_compression_ratio: summary.http_request_compression_ratio,
            conversation_source: summary.conversation_source.clone(),
            client_installation_id: summary.client_installation_id.clone(),
            provider_response_id: summary.provider_response_id.clone(),
            provider_conversation_key: summary.provider_conversation_key.clone(),
            request_storage_mode: summary.request_storage_mode.clone(),
            error_message: summary.error_message.clone(),
            request_has_previous_response_id: summary.request_has_previous_response_id,
            request_previous_response_id: summary.request_previous_response_id.clone(),
            request_previous_response_parent_found: summary.request_previous_response_parent_found,
            request_conversation_key: summary.request_conversation_key.clone(),
            request_conversation_parent_found: summary.request_conversation_parent_found,
            upstream_redaction_enabled: summary.upstream_redaction_enabled,
            response_capture_truncated: summary.response_capture_truncated,
        }
    }
}

impl From<&crate::standalone_config::StandaloneUsageSummaryRecord> for StandaloneUsageSummary {
    fn from(record: &crate::standalone_config::StandaloneUsageSummaryRecord) -> Self {
        Self {
            request_id: record.request_id,
            event_kind: record.event_kind.clone(),
            category: record.category.clone(),
            state: record.state.clone(),
            path: record.path.clone(),
            recorded_at: record.recorded_at,
            status: record.status,
            ok: record.ok,
            duration_ms: record.duration_ms,
            ttft_ms: record.ttft_ms,
            model: record.model.clone(),
            requested_model: record.requested_model.clone(),
            upstream_model: record.upstream_model.clone(),
            endpoint_id: record.endpoint_id,
            endpoint_key_id: record.endpoint_key_id,
            model_route_rule_id: record.model_route_rule_id,
            mcp_server_id: record.mcp_server_id,
            input_tokens: record.input_tokens,
            output_tokens: record.output_tokens,
            total_tokens: record.total_tokens,
            cached_tokens: record.cached_tokens,
            cache_read_tokens: record.cache_read_tokens,
            cache_write_tokens: record.cache_write_tokens,
            error_code: record.error_code.clone(),
            failure_family: record.failure_family.clone(),
            redaction: StandaloneUsageRedactionSummary {
                applied: record.redaction_applied,
                findings_count: record.redaction_findings_count,
                replacements_count: record.redaction_replacements_count,
                types: record.redaction_types.clone(),
                fields: record.redaction_fields.clone(),
            },
            route_selection_reason: record.route_selection_reason.clone(),
            user_id: record.user_id,
            client_key_id: record.client_key_id,
            client_key_label: record.client_key_label.clone(),
            request_user_agent: record.request_user_agent.clone(),
            endpoint_key_label: record.endpoint_key_label.clone(),
            mcp_server_name: record.mcp_server_name.clone(),
            mcp_protocol_method: record.mcp_protocol_method.clone(),
            mcp_operation_name: record.mcp_operation_name.clone(),
            http_request_content_encoding: record.http_request_content_encoding.clone(),
            http_request_compressed: record.http_request_compressed,
            http_request_compressed_bytes: record.http_request_compressed_bytes,
            http_request_decompressed_bytes: record.http_request_decompressed_bytes,
            http_request_compression_ratio: record.http_request_compression_ratio,
            conversation_source: record.conversation_source.clone(),
            client_installation_id: record.client_installation_id.clone(),
            provider_response_id: record.provider_response_id.clone(),
            provider_conversation_key: record.provider_conversation_key.clone(),
            request_storage_mode: record.request_storage_mode.clone(),
            error_message: record.error_message.clone(),
            request_has_previous_response_id: record.request_has_previous_response_id,
            request_previous_response_id: record.request_previous_response_id.clone(),
            request_previous_response_parent_found: record.request_previous_response_parent_found,
            request_conversation_key: record.request_conversation_key.clone(),
            request_conversation_parent_found: record.request_conversation_parent_found,
            upstream_redaction_enabled: record.upstream_redaction_enabled,
            response_capture_truncated: record.response_capture_truncated,
            // Replay metadata is transient: the durable ledger does not
            // store it, so the round-trip from the record starts empty
            // and the runtime's `persist` path re-decodes the JSON
            // snapshot only when an incoming usage event provides it.
            replay_conversation_id: None,
            replay_conversation_seq: None,
            snapshot_prompt_refs_json: None,
        }
    }
}
