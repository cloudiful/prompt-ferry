use std::{sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use sqlx::postgres::PgPoolOptions;
use tokio::time::timeout;

use crate::{
    db::{self, ConfigRepository},
    worker_admin::chatgpt_quota_service::{ChatGptQuotaService, policy::ACTIVITY_LOOKBACK},
};

use super::support::{
    cleanup_endpoint, cleanup_orphan_fixtures, create_endpoint, endpoint, quota, seed_snapshot,
    service, test_pool,
};
use super::{ChatGptQuotaError, FakeFetcher, fake_fetcher};

#[tokio::test]
async fn selected_request_wakes_are_nonblocking_and_throttled_per_endpoint() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgresql://quota_test@127.0.0.1:55479/quota_759_test")
        .expect("lazy isolated test pool");
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
    let snapshot_id = db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
        .await
        .unwrap()
        .unwrap()
        .snapshot_id;
    let fetcher = Arc::new(FakeFetcher::failure(ChatGptQuotaError::Upstream));
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
    let first = service_a
        .read(&provider_endpoint, true)
        .await
        .expect("failed refresh keeps the last good observation");
    assert_eq!(
        first.keys[0].model_remains[0]
            .observation
            .as_ref()
            .unwrap()
            .last_error_code
            .as_deref(),
        Some("upstream")
    );
    drop(permits);
    let joined = timeout(Duration::from_secs(3), b)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        joined.keys[0].model_remains[0]
            .observation
            .as_ref()
            .unwrap()
            .last_error_code
            .as_deref(),
        Some("upstream")
    );
    let latest = db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
        .await
        .unwrap()
        .unwrap();
    let calls = fetcher.calls.load(std::sync::atomic::Ordering::SeqCst);
    cleanup_endpoint(&pool, endpoint_id).await;
    assert_eq!(latest.snapshot_id, snapshot_id);
    assert_eq!(calls, 1);
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
    let Some(pool) = test_pool().await else {
        return;
    };
    cleanup_orphan_fixtures(&pool).await;
    let active_endpoint = create_endpoint(&pool).await;
    let idle_endpoint = create_endpoint(&pool).await;
    let recent = Utc::now() - ChronoDuration::minutes(6);
    seed_snapshot(&pool, active_endpoint, recent, Some(75.0)).await;
    seed_snapshot(&pool, idle_endpoint, recent, Some(75.0)).await;
    sqlx::query(
        "INSERT INTO request_records (request_id, endpoint_id, path, created_at) \
         VALUES ($1, $2, '/v1/responses', $3)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(active_endpoint)
    .bind(Utc::now() - ACTIVITY_LOOKBACK + ChronoDuration::minutes(1))
    .execute(&pool)
    .await
    .expect("insert recent selected-request activity");

    let fetcher = Arc::new(fake_fetcher(quota(Some(10.0))));
    let service = service(&pool, fetcher.clone());
    service.run_periodic_pass().await.unwrap();

    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
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
    cleanup_endpoint(&pool, active_endpoint).await;
    cleanup_endpoint(&pool, idle_endpoint).await;
}
