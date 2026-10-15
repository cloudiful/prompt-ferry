use std::{panic::AssertUnwindSafe, sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use futures::{FutureExt, future::BoxFuture};
use sqlx::PgPool;
use tokio::time::timeout;
use uuid::Uuid;

use crate::{
    db::{self, ConfigRepository},
    worker_admin::chatgpt_quota_service::{ChatGptQuotaService, policy::ACTIVITY_LOOKBACK},
};

use super::support::{
    cleanup_endpoint, cleanup_orphan_fixtures, create_endpoint, endpoint, insert_endpoint_row,
    quota, seed_snapshot, service, test_pool, try_cleanup_endpoint, with_owned_fixtures,
};
use super::{FakeFetcher, fake_fetcher};

#[allow(dead_code)]
mod db_harness {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/db_harness.rs"
    ));
}

#[tokio::test]
async fn selected_request_wakes_are_nonblocking_and_throttled_per_endpoint() {
    // Wakes are asserted without a query, so this never-connects pool is enough
    // and keeps the test away from any real database.
    let pool = db_harness::test_db_url::lazy_test_pool();
    let service = ChatGptQuotaService::with_fetcher(
        pool.clone(),
        ConfigRepository::postgres(&pool),
        Arc::new(fake_fetcher(quota(Some(10.0)))),
    );
    let mut receiver = service
        .take_request_wake_receiver()
        .expect("request wake receiver starts once");
    let first = uuid::Uuid::new_v4();
    let second = uuid::Uuid::new_v4();

    service.notify_selected_request(first);
    service.notify_selected_request(first);
    service.notify_selected_request(second);

    assert_eq!(receiver.try_recv().unwrap(), first);
    assert_eq!(receiver.try_recv().unwrap(), second);
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn separate_services_share_one_database_leased_manual_refresh() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let (fetcher, started, release) = FakeFetcher::gated(quota(Some(20.0)));
    let fetcher = Arc::new(fetcher);
    let service_a = Arc::new(service(&pool, fetcher.clone()));
    let service_b = Arc::new(service(&pool, fetcher.clone()));
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let a = {
        let service = service_a.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };
    let b = {
        let service = service_b.clone();
        tokio::spawn(async move { service.read(&provider_endpoint, true).await })
    };

    timeout(Duration::from_secs(3), started.notified())
        .await
        .expect("fake provider started");
    tokio::time::sleep(Duration::from_millis(100)).await;
    release.open();
    let (a, b) = timeout(Duration::from_secs(5), async { tokio::join!(a, b) })
        .await
        .expect("manual callers share the leased result");
    let a = a.expect("first service task").expect("first quota read");
    let b = b.expect("second service task").expect("second quota read");
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    for response in [&a, &b] {
        assert_eq!(
            response.keys[0].model_remains[0]
                .observation
                .as_ref()
                .unwrap()
                .source
                .as_deref(),
            Some("manual")
        );
    }
    assert_eq!(
        db::quota_snapshots::list_snapshots(&pool, endpoint_id, None, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn reviewer_expired_replica_lease_is_recovered_without_relocking_self() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let now = Utc::now();
    assert!(
        db::quota_snapshots::acquire_refresh_lease(
            &pool,
            endpoint_id,
            uuid::Uuid::new_v4(),
            now,
            now + ChronoDuration::seconds(1),
        )
        .await
        .unwrap()
    );
    let fetcher = Arc::new(fake_fetcher(quota(Some(20.0))));
    let service = service(&pool, fetcher.clone());
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let result = timeout(
        Duration::from_secs(4),
        service.read(&provider_endpoint, true),
    )
    .await;
    cleanup_endpoint(&pool, endpoint_id).await;
    assert!(
        result.is_ok(),
        "expired replica lease recovery deadlocked on the caller's endpoint mutex"
    );
    assert!(result.unwrap().is_ok());
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn reviewer_manual_waiting_for_capacity_joins_completed_replica_refresh() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let fetcher = Arc::new(fake_fetcher(quota(Some(20.0))));
    let service_a = service(&pool, fetcher.clone());
    let service_b = Arc::new(service(&pool, fetcher.clone()));
    let permits = service_b
        .inner
        .quota_slots
        .clone()
        .acquire_many_owned(4)
        .await
        .unwrap();
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let b = {
        let service = service_b.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };
    timeout(Duration::from_secs(3), async {
        loop {
            if service_b
                .inner
                .endpoint_locks
                .lock()
                .await
                .contains_key(&endpoint_id)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!b.is_finished());
    service_a.read(&provider_endpoint, true).await.unwrap();
    drop(permits);
    timeout(Duration::from_secs(3), b)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let calls = fetcher.calls.load(std::sync::atomic::Ordering::SeqCst);
    cleanup_endpoint(&pool, endpoint_id).await;
    assert_eq!(
        calls, 1,
        "a concurrent manual arrival must join a replica's completed refresh after waiting for capacity"
    );
}

#[tokio::test]
async fn manual_waiting_for_capacity_joins_completed_failure_with_last_good() {
    super::manual_waiting_failure::failure_body().await;
}

#[tokio::test]
async fn passive_cold_read_fetches_once_and_warm_read_uses_the_snapshot() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let fetcher = Arc::new(fake_fetcher(quota(None)));
    let service = service(&pool, fetcher.clone());
    let provider_endpoint = endpoint(&pool, endpoint_id).await;

    let cold = service.read(&provider_endpoint, false).await.unwrap();
    let warm = service.read(&provider_endpoint, false).await.unwrap();
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let first_window = &cold.keys[0].model_remains[0].windows.as_ref().unwrap()[0];
    assert_eq!(first_window.used_percent, None);
    assert_eq!(first_window.remaining_percent, None);
    assert_eq!(
        first_window.availability,
        crate::worker_admin_types::SubscriptionWindowAvailability::Unknown
    );
    let observation = warm.keys[0].model_remains[0].observation.as_ref().unwrap();
    assert_eq!(observation.source.as_deref(), Some("request"));
    assert!(!observation.stale);
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn selected_request_wake_refreshes_only_observations_older_than_five_minutes() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    seed_snapshot(
        &pool,
        endpoint_id,
        Utc::now() - ChronoDuration::minutes(6),
        Some(75.0),
    )
    .await;
    let fetcher = Arc::new(fake_fetcher(quota(Some(25.0))));
    let service = service(&pool, fetcher.clone());
    service.refresh_selected_request(endpoint_id).await.unwrap();
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let latest = db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.source, db::ChatgptQuotaSnapshotSource::Request);
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn periodic_collection_uses_recent_request_activity_for_five_minute_cadence() {
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let pool = fixtures.pool().clone();
        cleanup_orphan_fixtures(&pool).await;
        let active_endpoint = fixtures.endpoint().await;
        let idle_endpoint = fixtures.endpoint().await;
        let recent = Utc::now() - ChronoDuration::minutes(6);
        seed_snapshot(&pool, active_endpoint, recent, Some(75.0)).await;
        seed_snapshot(&pool, idle_endpoint, recent, Some(75.0)).await;
        fixtures
            .request_activity(
                active_endpoint,
                Utc::now() - ACTIVITY_LOOKBACK + ChronoDuration::minutes(1),
            )
            .await;

        let fetcher = Arc::new(fake_fetcher(quota(Some(10.0))));
        let service = service(&pool, fetcher.clone());
        service.run_periodic_pass().await.unwrap();

        // The real global pass still runs over every eligible endpoint, so
        // historical orphans may be fetched too; cadence is asserted per
        // owned endpoint instead of the shared global counter.
        assert_eq!(
            fetcher.calls_for(active_endpoint),
            1,
            "the active owned fixture must be refreshed exactly once"
        );
        assert_eq!(
            fetcher.calls_for(idle_endpoint),
            0,
            "the idle owned fixture must not be refreshed"
        );
        eprintln!(
            "periodic pass fetched {} foreign call(s) outside the owned fixtures; \
             reported, not failed",
            fetcher.foreign_calls_excluding(&[active_endpoint, idle_endpoint])
        );
        assert_eq!(
            db::quota_snapshots::latest_snapshot(&pool, active_endpoint)
                .await
                .unwrap()
                .unwrap()
                .source,
            db::ChatgptQuotaSnapshotSource::Periodic
        );
        assert_eq!(
            db::quota_snapshots::latest_snapshot(&pool, idle_endpoint)
                .await
                .unwrap()
                .unwrap()
                .source,
            db::ChatgptQuotaSnapshotSource::Manual
        );
        Ok::<(), anyhow::Error>(())
    })
    .await
    else {
        return;
    };
    outcome.unwrap();
}

/// Every row an owned fixture wrote is gone: the endpoint, its OAuth
/// presence, and its request activity. Snapshots follow the pre-existing
/// regime (the service's ordinary global path also writes them for foreign
/// fixtures), so ownership is proven on endpoints and requests.
async fn assert_fixture_gone(pool: &PgPool, endpoint_id: Uuid) {
    let name: Option<String> =
        sqlx::query_scalar("SELECT name FROM provider_endpoints WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .fetch_optional(pool)
            .await
            .expect("verify owned endpoint removed");
    assert!(
        name.is_none(),
        "owned endpoint {endpoint_id} was not removed"
    );
    let requests: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_records WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .fetch_one(pool)
            .await
            .expect("verify owned request activity removed");
    assert_eq!(
        requests, 0,
        "owned request activity for {endpoint_id} was not removed"
    );
    let oauth: Option<Uuid> =
        sqlx::query_scalar("SELECT endpoint_id FROM endpoint_oauth_tokens WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .fetch_optional(pool)
            .await
            .expect("verify owned OAuth presence removed");
    assert!(
        oauth.is_none(),
        "owned OAuth presence for {endpoint_id} was not removed"
    );
}

async fn assert_pool_fixture_gone(endpoint_id: Uuid) {
    let Some(pool) = test_pool().await else {
        return;
    };
    assert_fixture_gone(&pool, endpoint_id).await;
    pool.close().await;
}

#[tokio::test]
async fn owned_fixtures_are_removed_after_a_successful_body() {
    let stashed = Arc::new(std::sync::Mutex::new(None));
    let stashed_in = stashed.clone();
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let pool = fixtures.pool().clone();
        let endpoint_id = fixtures.endpoint().await;
        *stashed_in.lock().expect("stash stays lockable") = Some(endpoint_id);
        fixtures
            .request_activity(endpoint_id, Utc::now() - ChronoDuration::minutes(1))
            .await;
        // Both ids are tracked before their writes land.
        assert_eq!(fixtures.endpoint_ids(), vec![endpoint_id]);
        assert_eq!(fixtures.request_ids().len(), 1);
        // Join-tracked background work finishes before database cleanup even
        // though the body never joins it.
        let service = service(&pool, Arc::new(fake_fetcher(quota(Some(10.0)))));
        let provider_endpoint = endpoint(&pool, endpoint_id).await;
        fixtures.spawn(async move {
            let _ = service.read(&provider_endpoint, true).await;
        });
        Ok::<(), anyhow::Error>(())
    })
    .await
    else {
        return;
    };
    outcome.unwrap();
    assert_pool_fixture_gone(stashed.lock().expect("stash stays lockable").unwrap()).await;
}

#[tokio::test]
async fn owned_fixtures_are_removed_when_the_body_fails() {
    let stashed = Arc::new(std::sync::Mutex::new(None));
    let stashed_in = stashed.clone();
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let endpoint_id = fixtures.endpoint().await;
        *stashed_in.lock().expect("stash stays lockable") = Some(endpoint_id);
        fixtures
            .request_activity(endpoint_id, Utc::now() - ChronoDuration::minutes(1))
            .await;
        Err::<(), anyhow::Error>(anyhow::anyhow!("simulated body failure"))
    })
    .await
    else {
        return;
    };
    let error = outcome.expect_err("the body error must propagate after cleanup");
    assert!(
        error.to_string().contains("simulated body failure"),
        "the original failure must survive cleanup: {error:#}"
    );
    assert_pool_fixture_gone(stashed.lock().expect("stash stays lockable").unwrap()).await;
}

#[tokio::test]
async fn owned_fixtures_are_removed_when_the_body_panics() {
    let stashed = Arc::new(std::sync::Mutex::new(None));
    let stashed_in = stashed.clone();
    let caught = AssertUnwindSafe(async {
        with_owned_fixtures(|fixtures| async move {
            let endpoint_id = fixtures.endpoint().await;
            *stashed_in.lock().expect("stash stays lockable") = Some(endpoint_id);
            panic!("simulated body panic");
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        })
        .await
    })
    .catch_unwind()
    .await;
    assert!(
        caught.is_err(),
        "the body panic must resume after cleanup instead of being swallowed"
    );
    assert_pool_fixture_gone(stashed.lock().expect("stash stays lockable").unwrap()).await;
}

#[tokio::test]
async fn partially_created_fixture_still_cleans_up_safely() {
    let stashed = Arc::new(std::sync::Mutex::new(None));
    let stashed_in = stashed.clone();
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let pool = fixtures.pool().clone();
        let endpoint_id = fixtures.endpoint().await;
        *stashed_in.lock().expect("stash stays lockable") = Some(endpoint_id);
        // Simulate the missing second write: the OAuth row never lands, and
        // no activity is recorded before the body fails.
        sqlx::query("DELETE FROM endpoint_oauth_tokens WHERE endpoint_id = $1")
            .bind(endpoint_id)
            .execute(&pool)
            .await
            .expect("simulate the missing OAuth write");
        Err::<(), anyhow::Error>(anyhow::anyhow!("simulated partial-creation failure"))
    })
    .await
    else {
        return;
    };
    outcome.expect_err("the partial-creation failure must propagate after cleanup");
    assert_pool_fixture_gone(stashed.lock().expect("stash stays lockable").unwrap()).await;
}

#[tokio::test]
async fn foreign_sample_is_excluded_from_owned_counts_and_left_alive() {
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let pool = fixtures.pool().clone();
        let active_endpoint = fixtures.endpoint().await;
        let recent = Utc::now() - ChronoDuration::minutes(6);
        seed_snapshot(&pool, active_endpoint, recent, Some(75.0)).await;
        fixtures
            .request_activity(
                active_endpoint,
                Utc::now() - ACTIVITY_LOOKBACK + ChronoDuration::minutes(1),
            )
            .await;
        // Live-orphan sample: registered for failure-safe cleanup, but
        // excluded from the asserted counter subset, with recent activity so
        // the real pass treats it like a live orphan.
        let foreign_endpoint = fixtures.endpoint().await;
        seed_snapshot(&pool, foreign_endpoint, recent, Some(75.0)).await;
        fixtures
            .request_activity(
                foreign_endpoint,
                Utc::now() - ACTIVITY_LOOKBACK + ChronoDuration::minutes(1),
            )
            .await;

        let fetcher = Arc::new(fake_fetcher(quota(Some(10.0))));
        let service = service(&pool, fetcher.clone());
        service.run_periodic_pass().await.unwrap();

        assert_eq!(
            fetcher.calls_for(active_endpoint),
            1,
            "the active owned fixture must be refreshed exactly once"
        );
        // The sample is registered, so cleanup covers it; only the asserted
        // subset excludes it.
        assert!(
            fixtures.endpoint_ids().contains(&foreign_endpoint),
            "the live-orphan sample stays registered for end-of-test cleanup"
        );
        eprintln!(
            "foreign sample fetched {} time(s); excluded from owned counts, never deleted mid-pass",
            fetcher.calls_for(foreign_endpoint)
        );
        // Still alive: nothing in the pass deletes foreign fixtures, and the
        // context only cleans up after the body finishes.
        assert!(
            endpoint(&pool, foreign_endpoint).await.scope == "admin",
            "the foreign sample must survive the periodic pass"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await
    else {
        return;
    };
    outcome.unwrap();
}

#[tokio::test]
async fn owned_worker_panic_still_joins_all_workers_and_cleans_up() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let stashed = Arc::new(std::sync::Mutex::new(None));
    let stashed_in = stashed.clone();
    let second_finished = Arc::new(AtomicBool::new(false));
    let second_finished_in = second_finished.clone();
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let endpoint_id = fixtures.endpoint().await;
        *stashed_in.lock().expect("stash stays lockable") = Some(endpoint_id);
        fixtures.spawn(async move {
            panic!("simulated worker panic");
        });
        fixtures.spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            second_finished_in.store(true, Ordering::SeqCst);
        });
        Ok::<(), anyhow::Error>(())
    })
    .await
    else {
        return;
    };
    let error = outcome.expect_err("a panicking owned worker must fail the cleanup");
    assert!(
        error.to_string().contains("owned worker did not finish"),
        "the worker failure must be reported: {error:#}"
    );
    assert!(
        second_finished.load(Ordering::SeqCst),
        "the surviving worker must be joined, not detached"
    );
    assert_pool_fixture_gone(stashed.lock().expect("stash stays lockable").unwrap()).await;
}

#[tokio::test]
async fn failure_between_endpoint_and_oauth_writes_cleans_up_safely() {
    let stashed = Arc::new(std::sync::Mutex::new(None));
    let stashed_in = stashed.clone();
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        let pool = fixtures.pool().clone();
        // Controlled between-write failure: the id is registered first, the
        // endpoint row lands, and the OAuth write never happens.
        let endpoint_id = Uuid::new_v4();
        fixtures.register_endpoint(endpoint_id);
        *stashed_in.lock().expect("stash stays lockable") = Some(endpoint_id);
        insert_endpoint_row(&pool, endpoint_id).await;
        Err::<(), anyhow::Error>(anyhow::anyhow!("simulated between-write failure"))
    })
    .await
    else {
        return;
    };
    let error = outcome.expect_err("the between-write failure must propagate after cleanup");
    assert!(
        error
            .to_string()
            .contains("simulated between-write failure"),
        "the original failure must survive cleanup: {error:#}"
    );
    assert_pool_fixture_gone(stashed.lock().expect("stash stays lockable").unwrap()).await;
}

/// Controlled remover error: no DB schema or permission is tampered with; the
/// owned remover simply reports a DELETE failure so the aggregation path is
/// exercised deterministically.
fn failing_owned_deleter<'a>(
    _pool: &'a PgPool,
    _endpoint_id: Uuid,
) -> BoxFuture<'a, anyhow::Result<()>> {
    Box::pin(async move { Err(anyhow::anyhow!("injected DELETE failure")) })
}

#[tokio::test]
async fn body_error_with_deleter_failure_keeps_both_and_closes_pool() {
    let stashed_id = Arc::new(std::sync::Mutex::new(None));
    let stashed_pool = Arc::new(std::sync::Mutex::new(None));
    let stashed_id_in = stashed_id.clone();
    let stashed_pool_in = stashed_pool.clone();
    let Some(outcome) = with_owned_fixtures(|fixtures| async move {
        *stashed_pool_in.lock().expect("stash stays lockable") = Some(fixtures.pool().clone());
        let endpoint_id = fixtures.endpoint().await;
        *stashed_id_in.lock().expect("stash stays lockable") = Some(endpoint_id);
        fixtures.override_deleter(failing_owned_deleter);
        Err::<(), anyhow::Error>(anyhow::anyhow!("simulated body failure"))
    })
    .await
    else {
        return;
    };
    let error = outcome.expect_err("body and cleanup failures must surface together");
    let message = error.to_string();
    assert!(
        message.contains("simulated body failure"),
        "the original body outcome is kept: {message}"
    );
    assert!(
        message.contains("injected DELETE failure"),
        "the cleanup failure is aggregated: {message}"
    );
    let pool = stashed_pool
        .lock()
        .expect("stash stays lockable")
        .clone()
        .expect("pool is stashed");
    assert!(
        pool.is_closed(),
        "the owned pool must close even when cleanup fails"
    );
    let endpoint_id = stashed_id.lock().expect("stash stays lockable").unwrap();
    // Own-fault recovery with the real remover: nothing test-owned is left.
    let Some(recovery) = test_pool().await else {
        return;
    };
    try_cleanup_endpoint(&recovery, endpoint_id)
        .await
        .expect("own-fault recovery removes the fixture");
    recovery.close().await;
    assert_pool_fixture_gone(endpoint_id).await;
}

#[tokio::test]
async fn body_panic_with_deleter_failure_resumes_original_panic_and_closes_pool() {
    let stashed_id = Arc::new(std::sync::Mutex::new(None));
    let stashed_pool = Arc::new(std::sync::Mutex::new(None));
    let stashed_id_in = stashed_id.clone();
    let stashed_pool_in = stashed_pool.clone();
    let caught = AssertUnwindSafe(async {
        with_owned_fixtures(|fixtures| async move {
            *stashed_pool_in.lock().expect("stash stays lockable") = Some(fixtures.pool().clone());
            let endpoint_id = fixtures.endpoint().await;
            *stashed_id_in.lock().expect("stash stays lockable") = Some(endpoint_id);
            fixtures.override_deleter(failing_owned_deleter);
            panic!("simulated body panic");
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        })
        .await
    })
    .catch_unwind()
    .await;
    let payload = caught.expect_err("the original body panic must resume after cleanup");
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic>");
    assert!(
        message.contains("simulated body panic"),
        "the original panic is preserved, not the cleanup error: {message}"
    );
    let pool = stashed_pool
        .lock()
        .expect("stash stays lockable")
        .clone()
        .expect("pool is stashed");
    assert!(
        pool.is_closed(),
        "the owned pool must close even when cleanup fails"
    );
    let endpoint_id = stashed_id.lock().expect("stash stays lockable").unwrap();
    // Own-fault recovery with the real remover: nothing test-owned is left.
    let Some(recovery) = test_pool().await else {
        return;
    };
    try_cleanup_endpoint(&recovery, endpoint_id)
        .await
        .expect("own-fault recovery removes the fixture");
    recovery.close().await;
    assert_pool_fixture_gone(endpoint_id).await;
}

#[tokio::test]
async fn concurrent_manual_refreshes_fetch_once_and_mirror_once() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let (fetcher, started, release) = FakeFetcher::gated(quota(Some(20.0)));
    let fetcher = Arc::new(fetcher);
    let service = Arc::new(service(&pool, fetcher.clone()));
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let a = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };
    let b = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };

    timeout(Duration::from_secs(3), started.notified())
        .await
        .expect("fake provider started");
    tokio::time::sleep(Duration::from_millis(100)).await;
    release.open();
    let (a, b) = timeout(Duration::from_secs(5), async { tokio::join!(a, b) })
        .await
        .expect("manual callers share the leased result");
    let a = a.expect("first task").expect("first quota read");
    let b = b.expect("second task").expect("second quota read");
    assert_eq!(fetcher.calls_for(endpoint_id), 1);
    assert_eq!(
        db::quota_snapshots::list_snapshots(&pool, endpoint_id, None, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    // The single shared fetch mirrors exactly one observation row.
    assert_eq!(a.keys[0].key_id, b.keys[0].key_id);
    assert_eq!(
        db::quota_key_observations::list_observations(
            &pool,
            endpoint_id,
            a.keys[0].key_id,
            None,
            200
        )
        .await
        .unwrap()
        .len(),
        1
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}
