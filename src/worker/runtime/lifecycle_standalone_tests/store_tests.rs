//! Direct tests of `StandaloneRequestLeaseStore`. They exercise the
//! acquire / refresh / release / list-active / abort-stale SQL contract
//! without driving the heartbeat task; the lifecycle tests in
//! `lifecycle_tests.rs` cover the guard and reconciler paths.

use uuid::Uuid;

use crate::standalone_config::{RequestLeaseAcquireOutcome, StandaloneRequestLeaseStore};

use super::support::{cleanup, open_store};

#[tokio::test]
async fn acquire_inserts_live_owner_row() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let request_id = Uuid::new_v4();
    let owner = Uuid::new_v4();

    let outcome = leases
        .acquire(request_id, owner, 60)
        .await
        .expect("acquire");
    assert_eq!(outcome, RequestLeaseAcquireOutcome::Acquired);

    let active = leases.list_active().await.expect("list active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].request_id, request_id);
    assert_eq!(active[0].owner_worker_id, owner);
    assert!(active[0].lease_expires_at > 0);

    cleanup(store, path).await;
}

#[tokio::test]
async fn acquire_blocks_live_other_owner() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let request_id = Uuid::new_v4();
    let first_owner = Uuid::new_v4();
    let second_owner = Uuid::new_v4();

    assert_eq!(
        leases
            .acquire(request_id, first_owner, 60)
            .await
            .expect("first"),
        RequestLeaseAcquireOutcome::Acquired
    );
    assert_eq!(
        leases
            .acquire(request_id, second_owner, 60)
            .await
            .expect("second"),
        RequestLeaseAcquireOutcome::Blocked
    );

    // The blocked attempt must not have mutated the row.
    let active = leases.list_active().await.expect("list active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].owner_worker_id, first_owner);

    cleanup(store, path).await;
}

#[tokio::test]
async fn acquire_takes_over_expired_row() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let request_id = Uuid::new_v4();
    let original_owner = Uuid::new_v4();
    let new_owner = Uuid::new_v4();

    assert_eq!(
        leases
            .acquire(request_id, original_owner, 1)
            .await
            .expect("original"),
        RequestLeaseAcquireOutcome::Acquired
    );

    // Force expiry by reaching into the table directly so the test does
    // not rely on `time::sleep`.
    let expired_at: i64 = 1;
    sqlx::query(
        "UPDATE standalone_request_leases SET lease_expires_at = ?, last_heartbeat_at = ?, updated_at = ? WHERE request_id = ?",
    )
    .bind(expired_at)
    .bind(expired_at)
    .bind(expired_at)
    .bind(request_id.to_string())
    .execute(store.pool())
    .await
    .expect("force expiry");

    assert_eq!(
        leases
            .acquire(request_id, new_owner, 60)
            .await
            .expect("takeover"),
        RequestLeaseAcquireOutcome::Acquired
    );

    let active = leases.list_active().await.expect("list active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].owner_worker_id, new_owner);

    cleanup(store, path).await;
}

#[tokio::test]
async fn refresh_owner_check_blocks_other_owner() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let request_id = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let other_owner = Uuid::new_v4();

    leases
        .acquire(request_id, owner, 60)
        .await
        .expect("acquire");

    // Owner refresh succeeds.
    assert!(
        leases
            .refresh(request_id, owner, 90)
            .await
            .expect("refresh")
    );
    // Other-owner refresh is rejected without mutating the row.
    assert!(
        !leases
            .refresh(request_id, other_owner, 90)
            .await
            .expect("other refresh")
    );

    let active = leases.list_active().await.expect("list");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].owner_worker_id, owner);

    cleanup(store, path).await;
}

#[tokio::test]
async fn release_owner_check_blocks_other_owner() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let request_id = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let other_owner = Uuid::new_v4();

    leases
        .acquire(request_id, owner, 60)
        .await
        .expect("acquire");

    assert!(
        !leases
            .release(request_id, other_owner)
            .await
            .expect("other release")
    );
    // The owner's row must still exist after the rejected attempt.
    assert_eq!(leases.list_active().await.expect("list").len(), 1);
    assert!(
        leases
            .release(request_id, owner)
            .await
            .expect("owner release")
    );
    assert!(leases.list_active().await.expect("list after").is_empty());

    cleanup(store, path).await;
}

#[tokio::test]
async fn abort_stale_deletes_only_expired_rows() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let live_id = Uuid::new_v4();
    let expired_id = Uuid::new_v4();
    let owner = Uuid::new_v4();

    leases
        .acquire(live_id, owner, 3600)
        .await
        .expect("live acquire");
    leases
        .acquire(expired_id, owner, 1)
        .await
        .expect("expired acquire");

    // Force the second row into an expired state without sleeping.
    let expired_at: i64 = 1;
    sqlx::query(
        "UPDATE standalone_request_leases SET lease_expires_at = ?, last_heartbeat_at = ?, updated_at = ? WHERE request_id = ?",
    )
    .bind(expired_at)
    .bind(expired_at)
    .bind(expired_at)
    .bind(expired_id.to_string())
    .execute(store.pool())
    .await
    .expect("expire row");

    let removed = leases.abort_stale().await.expect("abort stale");
    assert_eq!(removed, 1);

    let active = leases.list_active().await.expect("list");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].request_id, live_id);

    cleanup(store, path).await;
}

#[tokio::test]
async fn list_active_skips_expired_rows() {
    let (store, path) = open_store().await;
    let leases = StandaloneRequestLeaseStore::new(store.pool().clone());
    let live_id = Uuid::new_v4();
    let expired_id = Uuid::new_v4();
    let owner = Uuid::new_v4();

    leases.acquire(live_id, owner, 3600).await.expect("live");
    leases
        .acquire(expired_id, owner, 1)
        .await
        .expect("soon expired");

    let expired_at: i64 = 1;
    sqlx::query(
        "UPDATE standalone_request_leases SET lease_expires_at = ?, last_heartbeat_at = ?, updated_at = ? WHERE request_id = ?",
    )
    .bind(expired_at)
    .bind(expired_at)
    .bind(expired_at)
    .bind(expired_id.to_string())
    .execute(store.pool())
    .await
    .expect("expire");

    let active = leases.list_active().await.expect("list");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].request_id, live_id);

    cleanup(store, path).await;
}
