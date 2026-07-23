use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct ConversationEndpointOverride {
    pub conversation_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_key_id: Option<Uuid>,
    pub endpoint_key_label: Option<String>,
    pub endpoint_name: Option<String>,
    pub created_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionRouteOption {
    pub endpoint_id: Uuid,
    pub endpoint_name: String,
    pub keys: Vec<SessionRouteKeyOption>,
    pub is_override: bool,
    pub is_preferred: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionRouteKeyOption {
    pub key_id: Uuid,
    pub key_label: String,
}
