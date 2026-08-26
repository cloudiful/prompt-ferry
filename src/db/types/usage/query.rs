use super::{RequestRecordCategory, RequestRecordState};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct RequestRecordQuery {
    pub visible_user_id: Option<i64>,
    pub request_category: RequestRecordCategory,
    pub first: i64,
    pub rows: i64,
    pub sort_field: String,
    pub sort_order: i64,
    pub search: Option<String>,
    pub date_start: Option<DateTime<Utc>>,
    pub date_end: Option<DateTime<Utc>>,
    pub client_key_id: Option<i64>,
    pub user: Option<String>,
    pub model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub mcp_server_id: Option<Uuid>,
    pub mcp_bearer_token_slot: Option<i16>,
    pub request_state: Option<RequestRecordState>,
    pub redaction_applied: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageClearScope {
    CurrentUser,
    AllUsers,
    TargetUser,
}

#[derive(Debug, Clone)]
pub struct RequestRecordClearQuery {
    pub scope: UsageClearScope,
    pub visible_user_id: Option<i64>,
    pub target_user_id: Option<i64>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
}
