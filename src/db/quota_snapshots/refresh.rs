use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{ChatgptQuotaRefreshState, ChatgptQuotaSnapshotCreate};

use super::{storage::SnapshotDbRow, validate_windows};

pub async fn acquire_refresh_lease(
    pool: &PgPool,
    endpoint_id: Uuid,
    owner: Uuid,
    attempted_at: DateTime<Utc>,
    requested_expires_at: DateTime<Utc>,
) -> Result<bool> {
    Ok(sqlx::query_file_scalar!(
        "src/sql/quota_snapshots/acquire_refresh_lease.sql",
        endpoint_id,
        owner,
        attempted_at,
        requested_expires_at,
    )
    .fetch_optional(pool)
    .await?
    .is_some())
}

pub async fn complete_refresh_success(
    pool: &PgPool,
    owner: Uuid,
    snapshot: ChatgptQuotaSnapshotCreate,
) -> Result<bool> {
    validate_windows(&snapshot.windows)?;
    let mut transaction = pool.begin().await?;
    let _snapshot = sqlx::query_file_as!(
        SnapshotDbRow,
        "src/sql/quota_snapshots/insert_snapshot.sql",
        snapshot.endpoint_id,
        snapshot.observed_at,
        snapshot.plan_type.as_deref(),
        snapshot.limit_reached,
        &snapshot.windows,
        snapshot.source.as_str(),
    )
    .fetch_one(&mut *transaction)
    .await?;
    let completed_endpoint = sqlx::query_file_scalar!(
        "src/sql/quota_snapshots/complete_refresh_success.sql",
        snapshot.endpoint_id,
        owner,
        snapshot.observed_at,
    )
    .fetch_optional(&mut *transaction)
    .await?;
    if completed_endpoint.is_none() {
        transaction.rollback().await?;
        return Ok(false);
    }
    transaction.commit().await?;
    Ok(true)
}

pub async fn complete_refresh_failure(
    pool: &PgPool,
    endpoint_id: Uuid,
    owner: Uuid,
    attempted_at: DateTime<Utc>,
    next_retry_at: Option<DateTime<Utc>>,
    error_code: Option<&str>,
) -> Result<bool> {
    validate_error_code(error_code)?;
    Ok(sqlx::query_file_scalar!(
        "src/sql/quota_snapshots/complete_refresh_failure.sql",
        endpoint_id,
        owner,
        attempted_at,
        next_retry_at,
        error_code,
    )
    .fetch_optional(pool)
    .await?
    .is_some())
}

pub async fn get_refresh_state(
    pool: &PgPool,
    endpoint_id: Uuid,
) -> Result<Option<ChatgptQuotaRefreshState>> {
    Ok(sqlx::query_file_as!(
        ChatgptQuotaRefreshState,
        "src/sql/quota_snapshots/get_refresh_state.sql",
        endpoint_id,
    )
    .fetch_optional(pool)
    .await?)
}

fn validate_error_code(error_code: Option<&str>) -> Result<()> {
    let Some(error_code) = error_code else {
        return Ok(());
    };
    if error_code.is_empty()
        || error_code.len() > 64
        || !error_code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b":_-".contains(&byte)
        })
    {
        bail!("ChatGPT quota refresh error code must be a bounded stable code");
    }
    Ok(())
}

#[cfg(test)]
#[path = "lease_tests.rs"]
mod lease_tests;
