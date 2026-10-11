use super::tests::opencode_go_usage;
use super::*;

#[tokio::test]
async fn cloned_cache_shares_feedback_and_lock_miss_is_unknown() {
    use crate::worker_admin::quota_feedback::{QuotaFeedbackEligibility, QuotaFeedbackScope};
    let cache = TokenPlanQuotaCache::default();
    let clone = cache.clone();
    let id = Uuid::new_v4();
    let scope = QuotaFeedbackScope::new(id, None);
    let now = Instant::now();
    cache.feedback.record_429(scope, None, now, now);
    assert_eq!(
        clone.feedback.eligibility(scope, now),
        QuotaFeedbackEligibility::Cooling
    );
    cache
        .store_for_test(id, opencode_go_usage(Uuid::nil(), 0.0))
        .await;
    assert_eq!(
        clone.key_remaining_percent_now(id, Uuid::nil(), None),
        Some(0.0)
    );
    let _writer = cache.entries.write().await;
    assert_eq!(clone.key_remaining_percent_now(id, Uuid::nil(), None), None);
}

#[tokio::test]
async fn arrivals_during_long_completed_refresh_do_not_fetch_again() {
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let now = Instant::now();
    let lock = Arc::new(Mutex::new(refresh::RefreshState::default()));
    cache.refresh_locks.lock().await.insert(id, lock.clone());
    {
        let mut state = lock.lock().await;
        state.attempted_at = Some(now);
        state.completed_at = Some(now + Duration::from_secs(30));
    }
    assert!(
        cache
            .refresh_with(id, true, now + Duration::from_secs(20), || async {
                panic!("arrival was served by in-flight completion")
            })
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn force_and_failure_share_minimum_attempt_gate() {
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let now = Instant::now();
    cache
        .refresh_with(id, false, now, || async {
            Ok(Some(opencode_go_usage(key, 80.0)))
        })
        .await
        .unwrap();
    cache
        .refresh_with(id, true, now + Duration::from_secs(9), || async {
            panic!("force must coalesce")
        })
        .await
        .unwrap();
    assert!(
        cache
            .refresh_with(id, true, now + Duration::from_secs(10), || async {
                anyhow::bail!("fixture outage")
            })
            .await
            .is_err()
    );
    assert!(cache.fresh_observed_key_now(id, key).is_none());
    assert_eq!(
        cache.raw_snapshot(id).await.unwrap().keys[0]
            .opencodego_rolling
            .as_ref()
            .unwrap()
            .percent,
        Some(20.0)
    );
    cache
        .refresh_with(id, true, now + Duration::from_secs(19), || async {
            panic!("failure must throttle")
        })
        .await
        .unwrap();
    cache
        .refresh_with(id, true, now + Duration::from_secs(20), || async {
            Ok(Some(opencode_go_usage(key, 0.0)))
        })
        .await
        .unwrap();
    assert_eq!(cache.key_weight_percent_now(id, key, None), Some(0.0));
}

#[tokio::test]
async fn failed_key_retains_detail_but_not_effective_capacity() {
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let now = Instant::now();
    let mut initial = opencode_go_usage(a, 80.0);
    initial.keys.extend(opencode_go_usage(b, 50.0).keys);
    cache.store_observation(id, initial.clone(), now).await;
    let mut failed = initial;
    failed.keys[0].ok = false;
    failed.keys[0].opencodego_rolling = None;
    cache.store_observation(id, failed, now).await;
    assert!(cache.fresh_observed_key_now(id, a).is_none());
    assert!(cache.fresh_observed_key_now(id, b).is_some());
    assert!(
        cache.raw_snapshot(id).await.unwrap().keys[0]
            .opencodego_rolling
            .is_some()
    );
    assert!(cache.observed_key_at(id, b, now + REFRESH_AFTER).is_none());
}

#[tokio::test]
async fn reset_passed_zero_is_unknown_not_full_or_permanent_zero() {
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let mut usage = opencode_go_usage(key, 0.0);
    usage.keys[0].opencodego_rolling.as_mut().unwrap().resets_at =
        Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    cache.store_for_test(id, usage).await;
    assert_eq!(cache.key_remaining_percent_now(id, key, None), None);
    assert_eq!(cache.key_weight_percent_now(id, key, None), None);
}

#[tokio::test]
async fn independent_fresh_zero_still_blocks_when_another_window_reset() {
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let mut usage = opencode_go_usage(key, 0.0);
    usage.keys[0].opencodego_rolling.as_mut().unwrap().resets_at =
        Some(chrono::Utc::now() - chrono::Duration::seconds(1));
    usage.keys[0].opencodego_weekly = Some(crate::worker_admin_types::OpencodeGoWindowUsage {
        status: None,
        percent: Some(100.0),
        resets_at: None,
    });
    cache.store_for_test(id, usage).await;
    assert_eq!(cache.key_remaining_percent_now(id, key, None), Some(0.0));
    assert_eq!(cache.key_weight_percent_now(id, key, None), Some(0.0));
}

#[tokio::test]
async fn relative_reset_ages_effective_input_without_mutating_raw_detail() {
    use crate::worker_admin_types::{TokenPlanModelUsage, TokenPlanWindowUsage};
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let now = Instant::now();
    let mut usage = opencode_go_usage(key, 100.0);
    usage.keys[0].opencodego_rolling = None;
    usage.keys[0].model_remains = vec![TokenPlanModelUsage {
        model_name: "general".into(),
        interval: Some(TokenPlanWindowUsage {
            status: None,
            remaining_percent: Some(0.0),
            total_count: None,
            usage_count: None,
            boost_permille: None,
            start_at: None,
            end_at: None,
            remains_time_ms: Some(1000),
        }),
        weekly: None,
        windows: None,
        observation: None,
    }];
    cache.store_observation(id, usage, now).await;
    let effective = cache
        .observed_key_at(id, key, now + Duration::from_secs(2))
        .unwrap();
    assert!(observation::expired_zero(
        &effective,
        Some("general"),
        chrono::Utc::now()
    ));
    assert!(
        cache
            .observed_key_at(id, key, now + REFRESH_AFTER)
            .is_none()
    );
    let raw = cache.raw_snapshot(id).await.unwrap();
    assert_eq!(
        raw.keys[0].model_remains[0]
            .interval
            .as_ref()
            .unwrap()
            .remains_time_ms,
        Some(1000)
    );
}

#[tokio::test]
async fn concurrent_forced_refreshes_fetch_once() {
    let cache = TokenPlanQuotaCache::default();
    let id = Uuid::new_v4();
    let key = Uuid::new_v4();
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let other = cache.clone();
    let entered_fetch = entered.clone();
    let release_fetch = release.clone();
    let first = tokio::spawn(async move {
        other
            .refresh_with(id, true, Instant::now(), || async {
                entered_fetch.notify_one();
                release_fetch.notified().await;
                Ok(Some(opencode_go_usage(key, 10.0)))
            })
            .await
            .unwrap()
    });
    entered.notified().await;
    let queued = cache.refresh_with(id, true, Instant::now(), || async {
        panic!("queued refresh duplicated GET")
    });
    release.notify_one();
    assert!(queued.await.unwrap().is_some());
    assert!(first.await.unwrap().is_some());
}
