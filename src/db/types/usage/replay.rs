use super::RequestToolCallStatus;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct RequestRecordPromptBlock {
    pub block_hash: String,
    pub role: String,
    pub content_json: Value,
    pub preview_text: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RequestRecordChainEntry {
    pub event_id: i64,
    pub request_id: Uuid,
    pub user_id: Option<i64>,
    pub endpoint_id: Option<Uuid>,
    pub path: Option<String>,
    pub model: Option<String>,
    pub conversation_id: Option<Uuid>,
    pub parent_event_id: Option<i64>,
    pub conversation_seq: Option<i32>,
    pub conversation_source: Option<String>,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub normalized_chain_hash: Option<String>,
    pub normalized_first_ref_hash: Option<String>,
    pub normalized_last_ref_hash: Option<String>,
    pub request_storage_mode: Option<String>,
    pub request_full_json: Option<Value>,
    pub request_delta_json: Option<Value>,
    pub request_raw_json: Option<Value>,
    pub request_has_previous_response_id: Option<bool>,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub request_conversation_key: Option<String>,
    pub request_conversation_parent_found: Option<bool>,
    pub provider_response_id: Option<String>,
    pub base_checkpoint_event_id: Option<i64>,
    pub response_prompt: Option<String>,
    pub response_raw_body: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RequestRecordConversationLocator {
    pub event_id: i64,
    pub endpoint_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub conversation_seq: Option<i32>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RequestRecordAssistantArtifact {
    pub event_id: i64,
    pub message_json: Value,
    pub has_reasoning_content: bool,
    pub has_tool_calls: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ReplaySnapshotCreate {
    pub event_id: i64,
    pub conversation_id: Uuid,
    pub conversation_seq: i32,
    pub base_event_id: i64,
    pub prompt_refs_json: Value,
    pub ref_count: i32,
    pub byte_size: i32,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct RequestRecordToolCall {
    pub tool_call_event_id: i64,
    pub parent_event_id: i64,
    pub conversation_id: Option<Uuid>,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_json: Option<Value>,
    pub arguments_preview: Option<String>,
    pub status: RequestToolCallStatus,
    pub sequence_in_turn: Option<i32>,
    pub mcp_request_event_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RequestRecordToolCallReplayCandidate {
    pub tool_call: RequestRecordToolCall,
    pub has_assistant_artifact: bool,
}

#[derive(Debug, Clone)]
pub struct RequestRecordToolCallCreate {
    pub parent_event_id: i64,
    pub conversation_id: Option<Uuid>,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_json: Option<Value>,
    pub arguments_preview: Option<String>,
    pub status: RequestToolCallStatus,
    pub sequence_in_turn: Option<i32>,
    pub mcp_request_event_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ReplaySnapshotRow {
    pub event_id: i64,
    pub conversation_id: Uuid,
    pub conversation_seq: i32,
    pub base_event_id: i64,
    pub prompt_refs_json: Value,
    pub ref_count: i32,
    pub byte_size: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ConversationRedactionSessionRow {
    pub conversation_id: Uuid,
    pub session_ciphertext: Vec<u8>,
    pub session_nonce: Vec<u8>,
    pub session_key_version: i16,
    pub last_event_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ConversationRedactionSessionCreate {
    pub conversation_id: Uuid,
    pub session_ciphertext: Vec<u8>,
    pub session_nonce: Vec<u8>,
    pub session_key_version: i16,
    pub last_event_id: Option<i64>,
}
