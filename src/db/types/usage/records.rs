use super::{RequestRecordCategory, RequestRecordState, RequestRecordToolCall};
use crate::db::{RequestFailureFamily, RouteSelectionReason};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordRedactionSummary {
    pub applied: bool,
    pub findings_count: i32,
    pub replacements_count: i32,
    pub types: Vec<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordListRow {
    pub record_id: i64,
    pub request_id: Uuid,
    pub request_category: RequestRecordCategory,
    pub user_id: Option<i64>,
    pub user_login_name: Option<String>,
    pub client_key_label: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    pub mcp_server_id: Option<Uuid>,
    pub mcp_server_name: Option<String>,
    pub mcp_protocol_method: Option<String>,
    pub mcp_operation_name: Option<String>,
    pub path: String,
    pub model: Option<String>,
    pub request_state: RequestRecordState,
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
    pub cache_rate: Option<f64>,
    pub conversation_id: Option<Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: String,
    pub storage_sanitized: bool,
    pub storage_sanitized_nul_count: i32,
    pub redaction: RequestRecordRedactionSummary,
    pub has_full_request: bool,
    pub has_parent: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub failure_family: Option<RequestFailureFamily>,
    pub mcp_bearer_token_slot: Option<i16>,
    pub route_selection_reason: RouteSelectionReason,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordDetail {
    pub record_id: i64,
    pub request_id: Uuid,
    pub request_category: RequestRecordCategory,
    pub user_id: Option<i64>,
    pub user_login_name: Option<String>,
    pub client_key_label: Option<String>,
    pub request_user_agent: Option<String>,
    pub http_request_content_encoding: Option<String>,
    pub http_request_compressed: bool,
    pub http_request_compressed_bytes: Option<i64>,
    pub http_request_decompressed_bytes: Option<i64>,
    pub http_request_compression_ratio: Option<f64>,
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    pub endpoint_key_id: Option<Uuid>,
    pub endpoint_key_label: Option<String>,
    pub mcp_server_id: Option<Uuid>,
    pub mcp_server_name: Option<String>,
    pub mcp_protocol_method: Option<String>,
    pub mcp_operation_name: Option<String>,
    pub path: String,
    pub model: Option<String>,
    pub request_state: RequestRecordState,
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
    pub cache_rate: Option<f64>,
    pub conversation_id: Option<Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: String,
    pub storage_sanitized: bool,
    pub storage_sanitized_nul_count: i32,
    pub redaction: RequestRecordRedactionSummary,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub request_storage_mode: String,
    pub request_raw_json: Option<Value>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub provider_response_id: Option<String>,
    pub has_full_request: bool,
    pub has_parent: bool,
    pub response_prompt: Option<String>,
    pub response_raw_body: Option<String>,
    pub assistant_message_json: Option<Value>,
    pub assistant_output_items_json: Option<Value>,
    pub has_reasoning_content: Option<bool>,
    pub upstream_error_body: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub failure_family: Option<RequestFailureFamily>,
    pub mcp_bearer_token_slot: Option<i16>,
    pub route_selection_reason: RouteSelectionReason,
    pub response_capture_truncated: bool,
    #[serde(default)]
    pub tool_call_events: Vec<RequestRecordToolCall>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct RequestRecordSummary {
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cached_tokens: i64,
    pub cache_rate: Option<f64>,
    pub avg_duration_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct RequestRecordBucket {
    pub bucket_at: DateTime<Utc>,
    pub request_count: i64,
    pub error_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cached_tokens: i64,
    pub cache_rate: Option<f64>,
    pub error_rate: Option<f64>,
    pub avg_duration_ms: Option<f64>,
    pub avg_first_chunk_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordPage {
    pub total: i64,
    pub records: Vec<RequestRecordListRow>,
}

#[derive(Debug, Clone, Serialize, Default, ToSchema)]
pub struct RequestRecordFacets {
    pub users: Vec<String>,
    pub models: Vec<String>,
    pub dates: Vec<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct UsageFacet {
    pub facet: String,
    pub value: String,
}
