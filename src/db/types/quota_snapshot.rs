use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatgptQuotaSnapshotSource {
    Manual,
    Request,
    Periodic,
}

impl ChatgptQuotaSnapshotSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Request => "request",
            Self::Periodic => "periodic",
        }
    }

    pub(crate) fn from_database(value: &str) -> Option<Self> {
        match value {
            "manual" => Some(Self::Manual),
            "request" => Some(Self::Request),
            "periodic" => Some(Self::Periodic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatgptQuotaSnapshotCreate {
    pub endpoint_id: Uuid,
    pub observed_at: DateTime<Utc>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: Value,
    pub source: ChatgptQuotaSnapshotSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatgptQuotaSnapshot {
    pub snapshot_id: i64,
    pub endpoint_id: Uuid,
    pub observed_at: DateTime<Utc>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: Value,
    pub source: ChatgptQuotaSnapshotSource,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatgptQuotaRefreshState {
    pub endpoint_id: Uuid,
    pub lease_owner: Option<Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatgptQuotaEndpointActivity {
    pub endpoint_id: Uuid,
    pub last_activity_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatgptQuotaHistoryEndpoint {
    pub endpoint_id: Uuid,
    pub provider: String,
}
