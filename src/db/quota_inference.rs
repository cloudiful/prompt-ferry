use anyhow::{Result, ensure};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use super::ChatgptQuotaRefreshState;

pub const MAX_SNAPSHOTS: usize = 30_000;
pub const MAX_INTERVALS: usize = 65;

#[derive(Debug)]
pub struct QuotaInferenceSnapshot {
    pub observed_at: DateTime<Utc>,
    pub plan_type: Option<String>,
    pub limit_reached: Option<bool>,
    pub windows: serde_json::Value,
}

#[derive(Debug)]
pub struct SnapshotEvidence {
    pub snapshots: Vec<QuotaInferenceSnapshot>,
    pub overflow: bool,
    pub observation_provenance: &'static str,
    pub account_history_assumed_stable: bool,
    pub refresh_state: Option<ChatgptQuotaRefreshState>,
}

pub async fn snapshots(
    pool: &PgPool,
    endpoint_id: Uuid,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<SnapshotEvidence> {
    ensure!(
        start <= end && end - start <= Duration::days(8),
        "invalid snapshot range"
    );
    let mut snapshots = sqlx::query_file_as!(
        QuotaInferenceSnapshot,
        "src/sql/usage/quota_inference_snapshots.sql",
        endpoint_id,
        start,
        end,
    )
    .fetch_all(pool)
    .await?;
    let overflow = snapshots.len() > MAX_SNAPSHOTS;
    snapshots.truncate(MAX_SNAPSHOTS);
    let refresh_state = super::quota_snapshots::get_refresh_state(pool, endpoint_id).await?;
    Ok(SnapshotEvidence {
        snapshots,
        overflow,
        observation_provenance: "success_only",
        account_history_assumed_stable: true,
        refresh_state,
    })
}

#[derive(Debug)]
pub struct QuotaInferenceAggregate {
    pub interval_index: i64,
    pub bucket: String,
    pub model: Option<String>,
    pub request_count: i64,
    pub invalid_duration_count: i64,
    pub error_count: i64,
    pub missing_usage_count: i64,
    pub fresh_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub distinct_models: i64,
}

pub async fn aggregate(
    pool: &PgPool,
    endpoint_id: Uuid,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
    starts: &[DateTime<Utc>],
    ends: &[DateTime<Utc>],
) -> Result<Vec<QuotaInferenceAggregate>> {
    ensure!(
        range_start <= range_end
            && range_end - range_start <= Duration::days(8) + Duration::minutes(2),
        "invalid aggregate range"
    );
    ensure!(
        !starts.is_empty() && starts.len() == ends.len() && starts.len() <= MAX_INTERVALS,
        "invalid interval count"
    );
    ensure!(
        starts
            .iter()
            .zip(ends)
            .all(|(start, end)| *start >= range_start && *end <= range_end && start < end),
        "invalid aggregate interval"
    );
    Ok(sqlx::query_file_as!(
        QuotaInferenceAggregate,
        "src/sql/usage/quota_inference_aggregate.sql",
        endpoint_id,
        range_start,
        range_end,
        starts,
        ends,
    )
    .fetch_all(pool)
    .await?)
}
