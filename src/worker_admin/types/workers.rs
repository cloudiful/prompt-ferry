use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::db::WorkerRecord;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkerSummary {
    pub worker_id: String,
    pub role: String,
    pub version: String,
    pub incarnation: Uuid,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub status: String,
    pub config_applied_revision: Option<i64>,
    pub config_restart_required: bool,
}

impl From<WorkerRecord> for WorkerSummary {
    fn from(record: WorkerRecord) -> Self {
        Self {
            worker_id: record.worker_id,
            role: record.role,
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
pub struct WorkerCoordinationState {
    /// The coordinator the workers use to share state across processes.
    pub mode: String,
    /// Whether the shared Valkey coordination is configured.
    pub shared_valkey_configured: bool,
    /// Whether the deployment runs in the degraded single-worker mode.
    pub degraded: bool,
    /// Why the deployment is degraded, when it is.
    pub degraded_reason: Option<String>,
    /// The number of live workers in the registry.
    pub live_worker_count: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkerListResponse {
    pub workers: Vec<WorkerSummary>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
    pub online_count: i64,
    pub coordination: WorkerCoordinationState,
}
