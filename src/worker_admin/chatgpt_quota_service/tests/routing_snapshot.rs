use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::{
    db,
    worker_admin::chatgpt_quota_service::{ChatGptQuotaError, RequestWakeReason, RoutingSnapshot},
};

use super::support::{cleanup_endpoint, create_endpoint, quota, seed_snapshot, service, test_pool};
use super::{FakeFetcher, fake_fetcher};

#[tokio::test]
async fn routing_snapshot_reads_persisted_oauth_quota_without_fetching() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    seed_snapshot(
        &pool,
        endpoint_id,
        Utc::now() - ChronoDuration::seconds(5),
        Some(100.0),
    )
    .await;
    let fetcher = Arc::new(fake_fetcher(quota(Some(5.0))));
    let service = service(&pool, fetcher.clone());

    let RoutingSnapshot::Subscription(Some(snapshot)) = service
        .routing_snapshot(endpoint_id)
        .await
        .expect("local subscription snapshot")
    else {
        panic!("OAuth endpoint should carry a subscription snapshot");
    };
    assert!(!snapshot.stale);
    assert_eq!(snapshot.limit_reached, Some(false));
    assert_eq!(snapshot.windows[0].remaining_percent, Some(0.0));
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    sqlx::query("DELETE FROM endpoint_oauth_tokens WHERE endpoint_id = $1")
        .bind(endpoint_id)
        .execute(&pool)
        .await
        .expect("remove fixture OAuth token");
    assert!(matches!(
        service.routing_snapshot(endpoint_id).await.unwrap(),
        RoutingSnapshot::NoSubscription
    ));
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn quota_error_wake_forces_a_fresh_snapshot_only_for_subscription_endpoints() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    seed_snapshot(
        &pool,
        endpoint_id,
        Utc::now() - ChronoDuration::seconds(5),
        Some(20.0),
    )
    .await;
    let fetcher = Arc::new(fake_fetcher(quota(Some(10.0))));
    let service = service(&pool, fetcher.clone());

    service
        .refresh_after_quota_error(endpoint_id)
        .await
        .expect("quota error forces a background refresh");
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    let api_key_endpoint = create_endpoint(&pool).await;
    sqlx::query("DELETE FROM endpoint_oauth_tokens WHERE endpoint_id = $1")
        .bind(api_key_endpoint)
        .execute(&pool)
        .await
        .expect("remove API-key fixture OAuth token");
    service
        .refresh_after_quota_error(api_key_endpoint)
        .await
        .expect("API-key endpoint skips OAuth quota refresh");
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    cleanup_endpoint(&pool, endpoint_id).await;
    cleanup_endpoint(&pool, api_key_endpoint).await;
}

#[tokio::test]
async fn failed_error_wake_preserves_raw_snapshot_but_marks_routing_unknown() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    seed_snapshot(
        &pool,
        endpoint_id,
        Utc::now() - ChronoDuration::seconds(5),
        Some(20.0),
    )
    .await;
    let previous = db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
        .await
        .expect("read prior snapshot")
        .expect("seeded quota snapshot");
    let fetcher = Arc::new(FakeFetcher::failure(ChatGptQuotaError::Upstream));
    let service = service(&pool, fetcher.clone());

    service
        .refresh_after_quota_error(endpoint_id)
        .await
        .expect("failed refresh retains its last good snapshot");
    let RoutingSnapshot::Subscription(Some(snapshot)) = service
        .routing_snapshot(endpoint_id)
        .await
        .expect("local subscription observation after failure")
    else {
        panic!("failed refresh should preserve the last good snapshot");
    };
    let persisted = db::quota_snapshots::latest_snapshot(&pool, endpoint_id)
        .await
        .expect("read retained snapshot")
        .expect("last good quota snapshot remains");
    assert_eq!(persisted.snapshot_id, previous.snapshot_id);
    assert!(snapshot.stale);
    assert_eq!(snapshot.windows[0].remaining_percent, Some(80.0));
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn quota_error_wakes_are_reason_scoped_and_throttled_separately() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgresql://quota_test@127.0.0.1:55479/quota_759_test")
        .expect("lazy isolated test pool");
    let fetcher = Arc::new(FakeFetcher::success(quota(Some(20.0))));
    let service = service(&pool, fetcher);
    let mut receiver = service
        .take_request_wake_receiver()
        .expect("request wake receiver starts once");
    let endpoint_id = Uuid::new_v4();

    service.notify_quota_error(endpoint_id);
    service.notify_quota_error(endpoint_id);
    service.notify_selected_request(endpoint_id);
    service.notify_selected_request(endpoint_id);

    assert_eq!(
        receiver.try_recv_event().unwrap().reason,
        RequestWakeReason::QuotaError
    );
    assert_eq!(
        receiver.try_recv_event().unwrap().reason,
        RequestWakeReason::SelectedRequest
    );
    assert!(receiver.try_recv_event().is_err());
}
