use super::*;
use crate::db::quota_snapshots::REFRESH_LEASE_MAX_SECONDS;
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
            replacement_at + Duration::seconds(REFRESH_LEASE_MAX_SECONDS + 1),
        )
        .await
        .unwrap()
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn lease_expiry_uses_database_clock_with_app_ahead() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true, true).await;
    let database_time_before = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    let attempted_at = database_time_before + Duration::seconds(1);
    let requested_expires_at = attempted_at + Duration::seconds(REFRESH_LEASE_MAX_SECONDS);

    assert!(
        requested_expires_at > database_time_before + Duration::seconds(REFRESH_LEASE_MAX_SECONDS)
    );
    assert!(
        acquire_refresh_lease(
            &pool,
            endpoint_id,
            owner,
            attempted_at,
            requested_expires_at,
        )
        .await
        .unwrap()
    );
    let state = get_refresh_state(&pool, endpoint_id)
        .await
        .unwrap()
        .unwrap();
    let stored_expiry = state.lease_expires_at.unwrap();
    let database_time_after = sqlx::query_scalar::<_, DateTime<Utc>>("SELECT clock_timestamp()")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert!(stored_expiry > database_time_after);
    assert!(stored_expiry <= database_time_after + Duration::seconds(REFRESH_LEASE_MAX_SECONDS));
    assert_eq!(
        state.last_attempt_at.unwrap().timestamp_micros(),
        attempted_at.timestamp_micros()
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn invalid_requested_lease_deadlines_are_rejected() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool, "openai", true, true).await;
    let now = Utc::now();
    let cases = [
        (now, now),
        (now, now - Duration::seconds(1)),
        (now - Duration::seconds(10), now - Duration::seconds(9)),
        (now, now + Duration::seconds(REFRESH_LEASE_MAX_SECONDS + 1)),
    ];

    for (attempted_at, requested_expires_at) in cases {
        assert!(
            !acquire_refresh_lease(
                &pool,
                endpoint_id,
                Uuid::new_v4(),
                attempted_at,
                requested_expires_at,
            )
            .await
            .unwrap(),
            "invalid requested lease deadline should not acquire a lease"
        );
    }
    cleanup_endpoint(&pool, endpoint_id).await;
}
