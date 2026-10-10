use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::{db, worker_admin::chatgpt_quota_service::ChatGptQuotaError};

use super::FakeFetcher;
use super::support::{
    cleanup_endpoint, create_endpoint, endpoint, seed_snapshot, service, test_pool,
};

#[tokio::test]
async fn failures_keep_last_good_and_back_off_automatic_request_wakes() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let observed_at = Utc::now() - Duration::minutes(8);
    seed_snapshot(&pool, endpoint_id, observed_at, Some(30.0)).await;
    let fetcher = Arc::new(FakeFetcher::failure(ChatGptQuotaError::Upstream));
    let service = service(&pool, fetcher.clone());
    let provider_endpoint = endpoint(&pool, endpoint_id).await;

    let response = service
        .read(&provider_endpoint, true)
        .await
        .expect("failed refresh still serves the persisted last-good observation");
    let observation = response.keys[0].model_remains[0]
        .observation
        .as_ref()
        .unwrap();
    assert_eq!(
        observation
            .observed_at
            .map(|timestamp| timestamp.timestamp_micros()),
        Some(observed_at.timestamp_micros())
    );
    assert_eq!(observation.last_error_code.as_deref(), Some("upstream"));
    assert!(observation.stale);
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        db::quota_snapshots::list_snapshots(&pool, endpoint_id, None, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        db::quota_snapshots::get_refresh_state(&pool, endpoint_id)
            .await
            .unwrap()
            .unwrap()
            .consecutive_failures,
        1
    );

    service.refresh_selected_request(endpoint_id).await.unwrap();
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 1);

    let forced = service
        .read(&provider_endpoint, true)
        .await
        .expect("manual refresh bypasses the automatic retry delay");
    assert_eq!(fetcher.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(
        forced.keys[0].model_remains[0]
            .observation
            .as_ref()
            .unwrap()
            .observed_at
            .map(|timestamp| timestamp.timestamp_micros()),
        Some(observed_at.timestamp_micros())
    );
    assert_eq!(
        db::quota_snapshots::list_snapshots(&pool, endpoint_id, None, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}
