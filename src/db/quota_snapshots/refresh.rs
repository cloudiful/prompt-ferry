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
    lease_expires_at: DateTime<Utc>,
) -> Result<bool> {
    Ok(sqlx::query_file_scalar!(
        "src/sql/quota_snapshots/acquire_refresh_lease.sql",
        endpoint_id,
        owner,
        attempted_at,
        lease_expires_at,
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
mod tests {
    use super::*;
    use crate::db::{
        ChatgptQuotaSnapshotSource,
        quota_snapshots::tests::test_support::{
            cleanup_endpoint, create_endpoint, known_window, test_pool,
        },
    };
    use chrono::Duration;

    #[tokio::test]
    async fn leases_serialize_owners_and_failures_preserve_last_success() {
        let Some(pool) = test_pool().await else {
            return;
        };
        let endpoint_id = create_endpoint(&pool, "openai", true, true).await;
        let now = Utc::now();
        let owner_a = Uuid::new_v4();
        let owner_b = Uuid::new_v4();
        let expires_at = now + Duration::seconds(45);
        let (a, b) = tokio::join!(
            acquire_refresh_lease(&pool, endpoint_id, owner_a, now, expires_at),
            acquire_refresh_lease(&pool, endpoint_id, owner_b, now, expires_at),
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_ne!(a, b);
        let owner = if a { owner_a } else { owner_b };
        let loser = if a { owner_b } else { owner_a };
        assert_eq!(
            get_refresh_state(&pool, endpoint_id)
                .await
                .unwrap()
                .unwrap()
                .lease_owner,
            Some(owner)
        );
        assert!(
            !complete_refresh_failure(&pool, endpoint_id, loser, now, None, Some("wrong_owner"))
                .await
                .unwrap()
        );

        assert!(
            complete_refresh_success(
                &pool,
                owner,
                ChatgptQuotaSnapshotCreate {
                    endpoint_id,
                    observed_at: now,
                    plan_type: Some("plus".to_string()),
                    limit_reached: Some(false),
                    windows: known_window("primary", 0.0),
                    source: ChatgptQuotaSnapshotSource::Manual,
                },
            )
            .await
            .unwrap()
        );
        let last_good = crate::db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
            .await
            .unwrap()
            .unwrap();

        let failure_owner = Uuid::new_v4();
        let attempt_at = Utc::now();
        assert!(
            acquire_refresh_lease(
                &pool,
                endpoint_id,
                failure_owner,
                attempt_at,
                attempt_at + Duration::seconds(45),
            )
            .await
            .unwrap()
        );
        let retry_at = attempt_at + Duration::minutes(2);
        assert!(
            complete_refresh_failure(
                &pool,
                endpoint_id,
                failure_owner,
                attempt_at,
                Some(retry_at),
                Some("upstream_timeout"),
            )
            .await
            .unwrap()
        );
        let state = get_refresh_state(&pool, endpoint_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.consecutive_failures, 1);
        assert_eq!(state.last_error_code.as_deref(), Some("upstream_timeout"));
        assert_eq!(
            state.next_retry_at.map(|time| time.timestamp_micros()),
            Some(retry_at.timestamp_micros())
        );
        assert_eq!(
            state.last_success_at.map(|time| time.timestamp_micros()),
            Some(last_good.observed_at.timestamp_micros())
        );
        assert_eq!(
            crate::db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
                .await
                .unwrap()
                .unwrap(),
            last_good
        );

        let stale_owner = Uuid::new_v4();
        let next_owner = Uuid::new_v4();
        let attempt_at = Utc::now();
        assert!(
            acquire_refresh_lease(
                &pool,
                endpoint_id,
                stale_owner,
                attempt_at,
                attempt_at + Duration::seconds(45),
            )
            .await
            .unwrap()
        );
        sqlx::query(
            "UPDATE chatgpt_quota_refresh_state \
             SET lease_expires_at = clock_timestamp() - INTERVAL '1 second' \
             WHERE endpoint_id = $1",
        )
        .bind(endpoint_id)
        .execute(&pool)
        .await
        .unwrap();
        let replacement_at = Utc::now();
        assert!(
            acquire_refresh_lease(
                &pool,
                endpoint_id,
                next_owner,
                replacement_at,
                replacement_at + Duration::seconds(45),
            )
            .await
            .unwrap()
        );
        assert!(
            !complete_refresh_failure(
                &pool,
                endpoint_id,
                stale_owner,
                replacement_at,
                None,
                Some("stale_owner"),
            )
            .await
            .unwrap()
        );
        assert_eq!(
            get_refresh_state(&pool, endpoint_id)
                .await
                .unwrap()
                .unwrap()
                .lease_owner,
            Some(next_owner)
        );
        assert!(
            !acquire_refresh_lease(
                &pool,
                Uuid::new_v4(),
                Uuid::new_v4(),
                replacement_at,
                replacement_at + Duration::seconds(super::super::REFRESH_LEASE_MAX_SECONDS + 1),
            )
            .await
            .unwrap()
        );
        cleanup_endpoint(&pool, endpoint_id).await;
    }
}
