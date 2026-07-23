use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::db;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RequestRecordSummaryQuery {
    pub days: Option<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestRecordOverviewRange {
    #[serde(rename = "24h")]
    Last24h,
    #[serde(rename = "7d")]
    Last7d,
    #[serde(rename = "30d")]
    Last30d,
    Custom,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RequestRecordOverviewQuery {
    pub request_category: Option<db::RequestRecordCategory>,
    pub range: Option<RequestRecordOverviewRange>,
    pub start: Option<chrono::DateTime<chrono::Utc>>,
    pub end: Option<chrono::DateTime<chrono::Utc>>,
    pub user: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RequestRecordsQuery {
    pub request_category: Option<db::RequestRecordCategory>,
    pub first: Option<i64>,
    pub rows: Option<i64>,
    pub sort_field: Option<String>,
    pub sort_order: Option<i64>,
    pub search: Option<String>,
    pub date: Option<String>,
    pub user: Option<String>,
    pub model: Option<String>,
    pub endpoint_id: Option<Uuid>,
    pub mcp_server_id: Option<Uuid>,
    pub mcp_bearer_token_slot: Option<i16>,
    pub request_state: Option<db::RequestRecordState>,
    pub redaction_applied: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RequestRecordFacetsQuery {
    pub request_category: Option<db::RequestRecordCategory>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageClearScope {
    CurrentUser,
    AllUsers,
    TargetUser,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct RequestRecordsClearRequest {
    pub scope: Option<UsageClearScope>,
    pub user_id: Option<i64>,
    pub start_at: Option<chrono::DateTime<chrono::Utc>>,
    pub end_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delete_all: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RequestRecordSeriesQuery {
    pub request_category: Option<db::RequestRecordCategory>,
    pub bucket: Option<String>,
    pub limit: Option<i64>,
    pub start: Option<chrono::DateTime<chrono::Utc>>,
    pub end: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestRecordPruneResponse {
    pub deleted: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestRecordsClearResponse {
    pub deleted: u64,
    pub deleted_prompt_blocks: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestRecordFullMessage {
    pub role: String,
    pub block_hash: String,
    pub preview_text: String,
    pub content_json: serde_json::Value,
    pub same_as_turn: Option<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RequestRecordFullResponse {
    pub conversation_id: Option<uuid::Uuid>,
    pub record_id: i64,
    pub conversation_source: String,
    pub client_installation_id: Option<String>,
    pub normalized_item_count: Option<i32>,
    pub request_storage_mode: String,
    pub request_raw_json: Option<serde_json::Value>,
    pub request_has_previous_response_id: bool,
    pub request_previous_response_id: Option<String>,
    pub request_previous_response_parent_found: Option<bool>,
    pub messages: Vec<RequestRecordFullMessage>,
    pub rendered_text: String,
}

pub type UsageContentLoggingMode = super::RequestContentLoggingMode;
pub type UsageContentLoggingResponse = super::RequestContentLoggingResponse;
pub type UsageContentLoggingRequest = super::RequestContentLoggingRequest;
pub type UsageSummaryQuery = RequestRecordSummaryQuery;
pub type UsageEventsQuery = RequestRecordsQuery;
pub type UsageClearRequest = RequestRecordsClearRequest;
pub type UsageSeriesQuery = RequestRecordSeriesQuery;
pub type UsagePruneResponse = RequestRecordPruneResponse;
pub type UsageClearResponse = RequestRecordsClearResponse;
pub type UsageRequestFullMessage = RequestRecordFullMessage;
pub type UsageRequestFullResponse = RequestRecordFullResponse;
