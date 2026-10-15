use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{
    QuotaObservation, QuotaObservationCreate, QuotaObservationProvider, QuotaObservationSource,
};
use crate::worker_admin_types::CanonicalQuotaWindow;

use super::{OBSERVATION_PAGE_MAX, OBSERVATION_PRUNE_BATCH_SIZE, OBSERVATION_RETENTION_DAYS};

#[derive(Debug)]
pub(super) struct ObservationDbRow {
    pub(super) observation_id: i64,
    pub(super) endpoint_id: Uuid,
    pub(super) key_id: Uuid,
    pub(super) provider: String,
    pub(super) key_label: String,
    pub(super) source: String,
    pub(super) observed_at: DateTime<Utc>,
    pub(super) ok: bool,
    pub(super) status: Option<String>,
    pub(super) error_code: Option<String>,
    pub(super) plan_type: Option<String>,
    pub(super) limit_reached: Option<bool>,
    pub(super) windows: serde_json::Value,
    pub(super) created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub(super) struct ObservationEndpointRow {
    pub(super) endpoint_id: Uuid,
    pub(super) key_id: Uuid,
    pub(super) provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationEndpoint {
    pub endpoint_id: Uuid,
    pub key_id: Uuid,
    pub provider: QuotaObservationProvider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibleObservationEndpoint {
    pub endpoint_id: Uuid,
    pub key_id: Uuid,
}

pub(super) fn map_observation(row: ObservationDbRow) -> Result<QuotaObservation> {
    let provider = QuotaObservationProvider::from_database(&row.provider)
        .context("database contains an unknown quota observation provider")?;
    let source = QuotaObservationSource::from_database(&row.source)
        .context("database contains an unknown quota observation source")?;
    let windows: Vec<CanonicalQuotaWindow> = serde_json::from_value(row.windows)
        .context("database contains quota observation windows outside the canonical shape")?;
    Ok(QuotaObservation {
        observation_id: row.observation_id,
        endpoint_id: row.endpoint_id,
        key_id: row.key_id,
        provider,
        key_label: row.key_label,
        source,
        observed_at: row.observed_at,
        ok: row.ok,
        status: row.status,
        error_code: row.error_code,
        plan_type: row.plan_type,
        limit_reached: row.limit_reached,
        windows,
        created_at: row.created_at,
    })
}

pub(super) fn map_observation_endpoint(row: ObservationEndpointRow) -> Result<ObservationEndpoint> {
    let provider = QuotaObservationProvider::from_database(&row.provider)
        .context("database contains an unknown quota observation provider")?;
    Ok(ObservationEndpoint {
        endpoint_id: row.endpoint_id,
        key_id: row.key_id,
        provider,
    })
}

pub async fn insert_observation(
    pool: &PgPool,
    create: &QuotaObservationCreate,
) -> Result<QuotaObservation> {
    create.validate()?;
    let windows = serde_json::to_value(&create.windows)
        .context("quota observation windows must serialize")?;
    sqlx::query_file_as!(
        ObservationDbRow,
        "src/sql/quota_key_observations/insert_observation.sql",
        create.endpoint_id,
        create.key_id,
        create.provider.as_str(),
        create.key_label,
        create.source.as_str(),
        create.observed_at,
        create.ok,
        create.status,
        create.error_code,
        create.plan_type,
        create.limit_reached,
        windows,
    )
    .fetch_one(pool)
    .await
    .context("persist quota observation")
    .and_then(map_observation)
}

pub async fn latest_observation(
    pool: &PgPool,
    endpoint_id: Uuid,
    key_id: Uuid,
) -> Result<Option<QuotaObservation>> {
    sqlx::query_file_as!(
        ObservationDbRow,
        "src/sql/quota_key_observations/latest_observation.sql",
        endpoint_id,
        key_id,
    )
    .fetch_optional(pool)
    .await?
    .map(map_observation)
    .transpose()
}

pub async fn list_observations(
    pool: &PgPool,
    endpoint_id: Uuid,
    key_id: Uuid,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<QuotaObservation>> {
    list_endpoint_observations(pool, endpoint_id, Some(key_id), before_id, limit).await
}

/// Endpoint-wide history with an optional key bound, all in one query:
/// `None` returns every historical key of the endpoint (removed keys stay
/// queryable since `key_id` carries no foreign key), while any `Some` UUID
/// scopes rows to this endpoint only. The caller passes `limit + 1` for the
/// canonical probe row; the clamp keeps the bounded probe.
pub async fn list_endpoint_observations(
    pool: &PgPool,
    endpoint_id: Uuid,
    key_id: Option<Uuid>,
    before_id: Option<i64>,
    limit: i64,
) -> Result<Vec<QuotaObservation>> {
    let rows = sqlx::query_file_as!(
        ObservationDbRow,
        "src/sql/quota_key_observations/list_observations.sql",
        endpoint_id,
        key_id,
        before_id,
        limit.clamp(1, OBSERVATION_PAGE_MAX + 1),
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(map_observation).collect()
}

pub async fn observation_endpoint(
    pool: &PgPool,
    endpoint_id: Uuid,
    key_id: Uuid,
) -> Result<Option<ObservationEndpoint>> {
    sqlx::query_file_as!(
        ObservationEndpointRow,
        "src/sql/quota_key_observations/observation_endpoint.sql",
        endpoint_id,
        key_id,
    )
    .fetch_optional(pool)
    .await?
    .map(map_observation_endpoint)
    .transpose()
}

pub async fn eligible_endpoints(pool: &PgPool) -> Result<Vec<EligibleObservationEndpoint>> {
    Ok(sqlx::query_file_as!(
        EligibleObservationEndpoint,
        "src/sql/quota_key_observations/eligible_endpoints.sql",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn delete_expired_observations(
    pool: &PgPool,
    now: DateTime<Utc>,
    batch_size: i64,
) -> Result<u64> {
    let cutoff = now - Duration::days(OBSERVATION_RETENTION_DAYS);
    let result = sqlx::query_file!(
        "src/sql/quota_key_observations/delete_expired_observations.sql",
        cutoff,
        batch_size.clamp(1, OBSERVATION_PRUNE_BATCH_SIZE),
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
