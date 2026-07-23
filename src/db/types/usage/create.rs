use super::{RequestRecordCategory, RequestRecordState, UsageEventKind};
use crate::db::{RequestFailureFamily, RouteSelectionReason};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct EncryptedPayloadInput {
    pub ciphertext: Option<Vec<u8>>,
    pub nonce: Option<Vec<u8>>,
    pub key_version: Option<i16>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestRecordRedactionSummaryInput {
    pub applied: bool,
    pub findings_count: i32,
    pub replacements_count: i32,
    pub types_json: Option<Value>,
    pub fields_json: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct RequestRecordCreate {
    pub event_kind: UsageEventKind,
    pub request_category: RequestRecordCategory,
    pub request_state: RequestRecordState,
    pub request_id: Uuid,
    pub user_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub request_user_agent: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_key_id: Option<Uuid>,
    pub endpoint_key_label: Option<String>,
    pub model_route_rule_id: Option<Uuid>,
    pub mcp_server_id: Option<Uuid>,
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
    pub status: Option<i32>,
    pub ok: Option<bool>,
    pub duration_ms: Option<i64>,
    pub first_chunk_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub conversation_id: Option<Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: String,
    pub storage_sanitized: bool,
    pub storage_sanitized_nul_count: i32,
    pub redaction_applied: bool,
    pub redaction_findings_count: i32,
    pub redaction_replacements_count: i32,
    pub redaction_types_json: Option<Value>,
    pub redaction_fields_json: Option<Value>,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub normalized_chain_hash: Option<String>,
    pub normalized_first_ref_hash: Option<String>,
    pub normalized_last_ref_hash: Option<String>,
    pub request_storage_mode: String,
    pub request_full_json: Option<Value>,
    pub request_delta_json: Option<Value>,
    pub request_raw_json: Option<Value>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub upstream_redaction_enabled: bool,
    pub upstream_redacted_request_json: Option<Value>,
    pub restore_session: EncryptedPayloadInput,
    pub provider_response_id: Option<String>,
    pub provider_conversation_key: Option<String>,
    pub base_checkpoint_event_id: Option<i64>,
    pub response_prompt: Option<String>,
    pub response_raw_body: Option<String>,
    pub upstream_error_body: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub failure_family: Option<RequestFailureFamily>,
    pub mcp_bearer_token_slot: Option<i16>,
    pub route_selection_reason: RouteSelectionReason,
    pub owner_worker_id: Option<Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct RequestRecordContextInput {
    pub conversation_id: Option<Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: String,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub normalized_chain_hash: Option<String>,
    pub normalized_first_ref_hash: Option<String>,
    pub normalized_last_ref_hash: Option<String>,
    pub base_checkpoint_event_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RequestRecordStorageInput {
    pub storage_sanitized: bool,
    pub storage_sanitized_nul_count: i32,
    pub redaction: RequestRecordRedactionSummaryInput,
    pub request_storage_mode: String,
    pub request_full_json: Option<Value>,
    pub request_delta_json: Option<Value>,
    pub request_raw_json: Option<Value>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub upstream_redaction_enabled: bool,
    pub upstream_redacted_request_json: Option<Value>,
    pub restore_session: EncryptedPayloadInput,
    pub response_prompt: Option<String>,
    pub response_raw_body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RequestRecordAssistantArtifactCreate {
    pub event_id: i64,
    pub message_json: Value,
    pub has_reasoning_content: bool,
    pub has_tool_calls: bool,
}

impl RequestRecordCreate {
    pub fn ai_request(request_id: Uuid, path: impl Into<String>) -> Self {
        Self::base(
            UsageEventKind::Request,
            RequestRecordCategory::Ai,
            request_id,
            path.into(),
        )
    }

    pub fn mcp_request(request_id: Uuid, path: impl Into<String>) -> Self {
        Self::base(
            UsageEventKind::Request,
            RequestRecordCategory::Mcp,
            request_id,
            path.into(),
        )
    }

    fn base(
        event_kind: UsageEventKind,
        request_category: RequestRecordCategory,
        request_id: Uuid,
        path: String,
    ) -> Self {
        Self {
            event_kind,
            request_category,
            request_state: RequestRecordState::Received,
            request_id,
            user_id: None,
            client_key_label: None,
            request_user_agent: None,
            endpoint_id: None,
            endpoint_key_id: None,
            endpoint_key_label: None,
            model_route_rule_id: None,
            mcp_server_id: None,
            mcp_server_name: None,
            mcp_protocol_method: None,
            mcp_operation_name: None,
            path,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
            http_request_decompressed_bytes: None,
            http_request_compression_ratio: None,
            model: None,
            status: None,
            ok: None,
            duration_ms: None,
            first_chunk_ms: None,
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cached_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            conversation_id: None,
            parent_event_id: None,
            conversation_seq: None,
            conversation_source: "none".to_string(),
            storage_sanitized: false,
            storage_sanitized_nul_count: 0,
            redaction_applied: false,
            redaction_findings_count: 0,
            redaction_replacements_count: 0,
            redaction_types_json: None,
            redaction_fields_json: None,
            client_installation_id: None,
            normalized_item_count: None,
            normalized_chain_hash: None,
            normalized_first_ref_hash: None,
            normalized_last_ref_hash: None,
            request_storage_mode: "full".to_string(),
            request_full_json: None,
            request_delta_json: None,
            request_raw_json: None,
            request_has_previous_response_id: false,
            request_previous_response_id: None,
            request_previous_response_parent_found: None,
            request_conversation_key: None,
            request_conversation_parent_found: None,
            upstream_redaction_enabled: false,
            upstream_redacted_request_json: None,
            restore_session: EncryptedPayloadInput::default(),
            provider_response_id: None,
            provider_conversation_key: None,
            base_checkpoint_event_id: None,
            response_prompt: None,
            response_raw_body: None,
            upstream_error_body: None,
            error_code: None,
            error_message: None,
            failure_family: None,
            mcp_bearer_token_slot: None,
            route_selection_reason: RouteSelectionReason::Default,
            owner_worker_id: None,
            lease_expires_at: None,
            last_heartbeat_at: None,
        }
    }

    pub fn with_state(
        mut self,
        event_kind: UsageEventKind,
        request_state: RequestRecordState,
    ) -> Self {
        self.event_kind = event_kind;
        self.request_state = request_state;
        self
    }

    pub fn with_request_actor(
        mut self,
        user_id: Option<i64>,
        client_key_label: Option<String>,
        request_user_agent: Option<String>,
    ) -> Self {
        self.user_id = user_id;
        self.client_key_label = client_key_label;
        self.request_user_agent = request_user_agent;
        self
    }

    pub fn with_http_request_compression(
        mut self,
        http_request_content_encoding: Option<String>,
        http_request_compressed: bool,
        http_request_compressed_bytes: Option<i64>,
        http_request_decompressed_bytes: Option<i64>,
        http_request_compression_ratio: Option<f64>,
    ) -> Self {
        self.http_request_content_encoding = http_request_content_encoding;
        self.http_request_compressed = http_request_compressed;
        self.http_request_compressed_bytes = http_request_compressed_bytes;
        self.http_request_decompressed_bytes = http_request_decompressed_bytes;
        self.http_request_compression_ratio = http_request_compression_ratio;
        self
    }

    pub fn with_route(
        mut self,
        endpoint_id: Option<Uuid>,
        model_route_rule_id: Option<Uuid>,
    ) -> Self {
        self.endpoint_id = endpoint_id;
        self.model_route_rule_id = model_route_rule_id;
        self
    }

    pub fn with_endpoint_key(
        mut self,
        endpoint_key_id: Option<Uuid>,
        endpoint_key_label: Option<String>,
    ) -> Self {
        self.endpoint_key_id = endpoint_key_id;
        self.endpoint_key_label = endpoint_key_label;
        self
    }

    pub fn with_mcp_context(
        mut self,
        mcp_server_id: Option<Uuid>,
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
        self.model = model;
        self
    }

    pub fn with_timing(
        mut self,
        status: Option<i32>,
        ok: Option<bool>,
        duration_ms: Option<i64>,
        first_chunk_ms: Option<i64>,
    ) -> Self {
        self.status = status;
        self.ok = ok;
        self.duration_ms = duration_ms;
        self.first_chunk_ms = first_chunk_ms;
        self
    }

    pub fn with_usage(
        mut self,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        total_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        cache_read_tokens: Option<i64>,
        cache_write_tokens: Option<i64>,
    ) -> Self {
        self.input_tokens = input_tokens;
        self.output_tokens = output_tokens;
        self.total_tokens = total_tokens;
        self.cached_tokens = cached_tokens;
        self.cache_read_tokens = cache_read_tokens;
        self.cache_write_tokens = cache_write_tokens;
        self
    }

    pub fn with_request_context(mut self, context: RequestRecordContextInput) -> Self {
        self.conversation_id = context.conversation_id;
        self.parent_event_id = context.parent_event_id;
        self.conversation_seq = context.conversation_seq;
        self.conversation_source = context.conversation_source;
        self.client_installation_id = context.client_installation_id;
        self.normalized_item_count = context.normalized_item_count;
        self.normalized_chain_hash = context.normalized_chain_hash;
        self.normalized_first_ref_hash = context.normalized_first_ref_hash;
        self.normalized_last_ref_hash = context.normalized_last_ref_hash;
        self.base_checkpoint_event_id = context.base_checkpoint_event_id;
        self
    }

    pub fn with_request_storage(mut self, storage: RequestRecordStorageInput) -> Self {
        self.storage_sanitized = storage.storage_sanitized;
        self.storage_sanitized_nul_count = storage.storage_sanitized_nul_count;
        self.redaction_applied = storage.redaction.applied;
        self.redaction_findings_count = storage.redaction.findings_count;
        self.redaction_replacements_count = storage.redaction.replacements_count;
        self.redaction_types_json = storage.redaction.types_json;
        self.redaction_fields_json = storage.redaction.fields_json;
        self.request_storage_mode = storage.request_storage_mode;
        self.request_full_json = storage.request_full_json;
        self.request_delta_json = storage.request_delta_json;
        self.request_raw_json = storage.request_raw_json;
        self.request_has_previous_response_id = storage.request_has_previous_response_id;
        self.request_previous_response_id = storage.request_previous_response_id;
        self.request_previous_response_parent_found =
            storage.request_previous_response_parent_found;
        self.request_conversation_key = storage.request_conversation_key;
        self.request_conversation_parent_found = storage.request_conversation_parent_found;
        self.upstream_redaction_enabled = storage.upstream_redaction_enabled;
        self.upstream_redacted_request_json = storage.upstream_redacted_request_json;
        self.restore_session = storage.restore_session;
        self.response_prompt = storage.response_prompt;
        self.response_raw_body = storage.response_raw_body;
        self
    }

    pub fn with_provider_response(
        mut self,
        provider_response_id: Option<String>,
        provider_conversation_key: Option<String>,
    ) -> Self {
        self.provider_response_id = provider_response_id;
        self.provider_conversation_key = provider_conversation_key;
        self
    }

    pub fn with_error(
        mut self,
        upstream_error_body: Option<String>,
        error_code: Option<String>,
        error_message: Option<String>,
    ) -> Self {
        self.upstream_error_body = upstream_error_body;
        self.error_code = error_code;
        self.error_message = error_message;
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
        owner_worker_id: Option<Uuid>,
        lease_expires_at: Option<DateTime<Utc>>,
        last_heartbeat_at: Option<DateTime<Utc>>,
    ) -> Self {
        self.owner_worker_id = owner_worker_id;
        self.lease_expires_at = lease_expires_at;
        self.last_heartbeat_at = last_heartbeat_at;
        self
    }
}
