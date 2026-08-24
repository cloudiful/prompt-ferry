//! Tests for the standalone request-lease guard and stale-row
//! reconciler. These exercise the runtime spawn boundaries; the
//! underlying store contract is covered in `store_tests.rs`.

use sqlx::Row;
use uuid::Uuid;

use crate::worker::runtime::lifecycle::RuntimeControl;
use crate::worker::runtime::lifecycle_standalone::{
    StandaloneLeaseInputs, spawn_standalone_request_lease_guard,
    spawn_standalone_stale_lease_reconciler,
};

use super::support::open_standalone_state;

#[tokio::test]
async fn standalone_guard_acquires_and_keeps_owner_checked_row() {
    let (state, path) = open_standalone_state().await;
    let control = RuntimeControl::new();
    let inputs =
        StandaloneLeaseInputs::from_standalone_state(&state, control.worker_instance_id(), 60, 30);
    let request_id = Uuid::new_v4();
    let guard =
        spawn_standalone_request_lease_guard(Some(inputs.clone()), control.clone(), request_id)
            .expect("guard");

    let pool = state.store_pool();
    // Poll for the guard's acquire to land. Yielding alone is not
    // enough under heavy parallel test load; we also include a few
    // short sleeps so the spawned task gets scheduler time without
    // depending on a wall-clock interval in production code paths.
    let mut owner: Option<String> = None;
    for _ in 0..500 {
        let active = sqlx::query(
            "SELECT owner_worker_id FROM standalone_request_leases WHERE request_id = ?",
        )
        .bind(request_id.to_string())
        .fetch_optional(&pool)
        .await
        .expect("row");
        if let Some(row) = active {
            owner = Some(row.try_get("owner_worker_id").expect("owner"));
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let owner = owner.expect("lease row should be persisted by the guard");
    assert_eq!(owner, control.worker_instance_id().to_string());

    // The guard must release the row when dropped. The release is
    // dispatched on a fire-and-forget task, so poll the count until
    // the row disappears or the bounded budget is exhausted.
    drop(guard);

    let mut count: i64 = 1;
    for _ in 0..500 {
        count = sqlx::query("SELECT COUNT(*) AS c FROM standalone_request_leases")
            .fetch_one(&pool)
            .await
            .expect("count")
            .try_get("c")
            .expect("count value");
        if count == 0 {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(count, 0, "release should delete the row");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn standalone_guard_does_not_heartbeat_when_blocked_by_other_owner() {
    let (state, path) = open_standalone_state().await;
    let control = RuntimeControl::new();
    let other_owner = Uuid::new_v4();

    // Pre-seed a live lease owned by a different worker so the guard
    // acquires Blocked and exits without heartbeating.
    let pool = state.store_pool();
    let request_id = Uuid::new_v4();
    let far_future = i64::MAX / 2;
    sqlx::query(
        "INSERT INTO standalone_request_leases(request_id, owner_worker_id, lease_expires_at, last_heartbeat_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(request_id.to_string())
    .bind(other_owner.to_string())
    .bind(far_future)
    .bind(far_future)
    .bind(far_future)
    .execute(&pool)
    .await
    .expect("seed lease");

    let inputs =
        StandaloneLeaseInputs::from_standalone_state(&state, control.worker_instance_id(), 60, 1);
    let guard = spawn_standalone_request_lease_guard(Some(inputs), control.clone(), request_id)
        .expect("guard");

    // Yield enough times for the spawned task to attempt acquire and
    // exit on the Blocked branch.
    for _ in 0..40 {
        tokio::task::yield_now().await;
    }

    let owner: String =
        sqlx::query("SELECT owner_worker_id FROM standalone_request_leases WHERE request_id = ?")
            .bind(request_id.to_string())
            .fetch_one(&pool)
            .await
            .expect("row")
            .try_get("owner_worker_id")
            .expect("owner");
    assert_eq!(
        owner,
        other_owner.to_string(),
        "blocked attempt must not take over"
    );

    drop(guard);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn standalone_reconciler_deletes_only_expired_rows() {
    use std::time::Duration;

    let (state, path) = open_standalone_state().await;
    let control = RuntimeControl::new();

    let pool = state.store_pool();
    let live_id = Uuid::new_v4();
    let expired_id = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let far_future = i64::MAX / 2;

    sqlx::query(
        "INSERT INTO standalone_request_leases(request_id, owner_worker_id, lease_expires_at, last_heartbeat_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(live_id.to_string())
    .bind(owner.to_string())
    .bind(far_future)
    .bind(far_future)
    .bind(far_future)
    .execute(&pool)
    .await
    .expect("live seed");

    sqlx::query(
        "INSERT INTO standalone_request_leases(request_id, owner_worker_id, lease_expires_at, last_heartbeat_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(expired_id.to_string())
    .bind(owner.to_string())
    .bind(1_i64)
    .bind(1_i64)
    .bind(1_i64)
    .execute(&pool)
    .await
    .expect("expired seed");

    // Use a short sweep interval so the test does not depend on
    // wall-clock sleeps. The minimum interval enforced by the
    // reconciler is one second, so the test runs within seconds.
    let handle = spawn_standalone_stale_lease_reconciler(
        Some(state.clone()),
        control.clone(),
        Duration::from_millis(50),
    )
    .expect("reconciler");

    // Wait for the reconciler to do its work by sleeping in small
    // increments so the spawned task gets a chance to advance its
    // timer and run the sweep. Yielding alone does not advance time.
    let mut count: i64 = 2;
    for _ in 0..200 {
        count = sqlx::query("SELECT COUNT(*) AS c FROM standalone_request_leases")
            .fetch_one(&pool)
            .await
            .expect("count")
            .try_get("c")
            .expect("c");
        if count == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(count, 1, "only the expired row should be pruned");

    control.begin_shutdown();
    let _ = handle.await;
    let _ = std::fs::remove_file(path);
}
