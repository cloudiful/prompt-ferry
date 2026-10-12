use std::{sync::Arc, time::Duration};

use chrono::{Duration as ChronoDuration, Utc};
use tokio::time::timeout;

use crate::db;

use super::support::{
    cleanup_endpoint, cleanup_orphan_fixtures, create_endpoint, endpoint, seed_snapshot, service,
    test_pool,
};
use super::{ChatGptQuotaError, FakeFetcher};

pub(super) async fn failure_body() {
    let Some(pool) = test_pool().await else {
        return;
    };
    cleanup_orphan_fixtures(&pool).await;
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
    let b_arrival = Utc::now();
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
    let state = db::quota_snapshots::get_refresh_state(&pool, endpoint_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.lease_owner, None);
    assert!(
        state
            .last_attempt_at
            .is_some_and(|last_attempt_at| last_attempt_at >= b_arrival),
        "persisted failure attempt precedes B arrival",
    );
    assert_eq!(state.last_error_code.as_deref(), Some("upstream"));
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
