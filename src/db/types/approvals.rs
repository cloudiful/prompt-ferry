use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct ApprovalRequest {
    pub approval_id: uuid::Uuid,
    pub request_id: uuid::Uuid,
    pub user_id: Option<i64>,
    pub user_login_name: Option<String>,
    pub client_key_label: Option<String>,
    pub path: String,
    pub model: Option<String>,
    pub review_decision: String,
    pub approval_status: String,
    pub review_reason: String,
    pub review_categories: Vec<String>,
    pub request_preview: String,
    pub request_payload_json: Option<Value>,
    pub request_deadline_unix_ms: i64,
    pub wait_deadline_unix_ms: i64,
    pub decided_by_user_id: Option<i64>,
    pub decided_by_login_name: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApprovalRequestCreate {
    pub approval_id: uuid::Uuid,
    pub request_id: uuid::Uuid,
    pub user_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub path: String,
    pub model: Option<String>,
    pub review_decision: String,
    pub approval_status: String,
    pub review_reason: String,
    pub review_categories: Vec<String>,
    pub request_preview: String,
    pub request_payload_json: Option<Value>,
    pub request_deadline_unix_ms: i64,
    pub wait_deadline_unix_ms: i64,
}

#[derive(Debug, Clone)]
pub struct FlaggedApprovalRequestInput {
    pub request_id: uuid::Uuid,
    pub user_id: Option<i64>,
    pub client_key_label: Option<String>,
    pub path: String,
    pub model: Option<String>,
    pub review_reason: String,
    pub review_categories: Vec<String>,
    pub request_preview: String,
    pub request_payload_json: Value,
    pub request_deadline_unix_ms: i64,
    pub wait_deadline_unix_ms: i64,
}

impl ApprovalRequestCreate {
    pub fn flagged(input: FlaggedApprovalRequestInput) -> Self {
        Self {
            approval_id: uuid::Uuid::new_v4(),
            request_id: input.request_id,
            user_id: input.user_id,
            client_key_label: input.client_key_label,
            path: input.path,
            model: input.model,
            review_decision: "flag".to_string(),
            approval_status: "pending".to_string(),
            review_reason: input.review_reason,
            review_categories: input.review_categories,
            request_preview: input.request_preview,
            request_payload_json: Some(input.request_payload_json),
            request_deadline_unix_ms: input.request_deadline_unix_ms,
            wait_deadline_unix_ms: input.wait_deadline_unix_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApprovalRequestPage {
    pub total: i64,
    pub approvals: Vec<ApprovalRequest>,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatusFilter {
    #[default]
    Pending,
    Resolved,
}
