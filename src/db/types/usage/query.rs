use super::{RequestRecordCategory, RequestRecordState};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Which MCP records a query covers.
///
/// The split is by record identity, never by path or display label: a record
/// belongs to a *service* when it carries an MCP server id or a stored server
/// name, and to the *gateway* when it carries neither. A name-only failed
/// attempt and a historical id-only record for a deleted server therefore stay
/// on the service side, and the gateway keeps its own history.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestRecordMcpScope {
    /// Records that reached an MCP service.
    Service,
    /// Gateway chatter: records without any MCP service identity.
    Gateway,
    /// Every MCP record, gateway chatter included. The default keeps the
    /// historical behaviour of an omitted scope.
    #[default]
    All,
}

impl RequestRecordMcpScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Gateway => "gateway",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RequestRecordQuery {
    pub visible_user_id: Option<i64>,
    pub request_category: RequestRecordCategory,
    pub mcp_scope: RequestRecordMcpScope,
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
