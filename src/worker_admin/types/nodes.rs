use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::WorkerRecord;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeSummary {
    pub node_id: String,
    pub version: String,
    pub incarnation: Uuid,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub status: String,
    pub config_applied_revision: Option<i64>,
    pub config_restart_required: bool,
}

impl From<WorkerRecord> for NodeSummary {
    fn from(record: WorkerRecord) -> Self {
        Self {
            node_id: record.worker_id,
            version: record.version,
            incarnation: record.incarnation,
            started_at: record.started_at,
            last_heartbeat_at: record.last_heartbeat_at,
            stopped_at: record.stopped_at,
            status: record.status.as_str().to_string(),
            config_applied_revision: record.config_applied_revision,
            config_restart_required: record.config_restart_required,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeCoordinationState {
    pub mode: String,
    pub shared_valkey_configured: bool,
    pub degraded: bool,
    pub degraded_reason: Option<String>,
    pub live_node_count: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeListResponse {
    pub nodes: Vec<NodeSummary>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
    pub online_count: i64,
    pub coordination: NodeCoordinationState,
}
