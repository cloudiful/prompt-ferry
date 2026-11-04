use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::time::timeout;

use crate::worker_admin::quota_feedback::{
    QuotaFeedbackCause, QuotaFeedbackEligibility, QuotaFeedbackScope, QuotaFeedbackStore,
};

use super::support::{
    cleanup_endpoint, create_endpoint, endpoint, quota, service_with_feedback, test_pool,
};
use super::{ChatGptQuotaError, FakeFetcher, fake_fetcher};

fn exhausted_feedback(feedback: &QuotaFeedbackStore, scope: QuotaFeedbackScope) -> Instant {
    let failed_at = Instant::now();
    feedback.record_failure(
        scope,
        QuotaFeedbackCause::Exhausted,
        None,
        failed_at,
        failed_at,
    );
    failed_at
}

#[tokio::test]
async fn manual_waiter_reusing_an_automatic_success_keeps_feedback() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let (fetcher, started, release) = FakeFetcher::gated(quota(Some(20.0)));
    let fetcher = Arc::new(fetcher);
    let feedback = QuotaFeedbackStore::default();
    let service = Arc::new(service_with_feedback(
        &pool,
        fetcher.clone(),
        feedback.clone(),
    ));
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, None);
    exhausted_feedback(&feedback, scope);

    let automatic = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, false).await })
    };
    timeout(Duration::from_secs(3), started.notified())
        .await
        .expect("automatic fetch started");
    let manual = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    release.open();
    let (automatic, manual) = timeout(Duration::from_secs(5), async {
        tokio::join!(automatic, manual)
    })
    .await
    .expect("automatic and manual callers finish");
    assert!(automatic.expect("automatic task").is_ok());
    assert!(manual.expect("manual task").is_ok());
    assert_eq!(
        fetcher.calls_for(endpoint_id),
        1,
        "the manual arrival must reuse the automatic result"
    );
    assert_eq!(
        feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Cooling,
        "a reused automatic result can never recover manual feedback"
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn manual_waiter_reusing_an_automatic_failure_keeps_feedback() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let (fetcher, started, release) = FakeFetcher::gated_failure(ChatGptQuotaError::Upstream);
    let fetcher = Arc::new(fetcher);
    let feedback = QuotaFeedbackStore::default();
    let service = Arc::new(service_with_feedback(
        &pool,
        fetcher.clone(),
        feedback.clone(),
    ));
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, None);
    exhausted_feedback(&feedback, scope);

    let automatic = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, false).await })
    };
    timeout(Duration::from_secs(3), started.notified())
        .await
        .expect("automatic fetch started");
    let manual = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    release.open();
    let (automatic, manual) = timeout(Duration::from_secs(5), async {
        tokio::join!(automatic, manual)
    })
    .await
    .expect("automatic and manual callers finish");
    assert!(automatic.expect("automatic task").is_err());
    assert!(manual.expect("manual task").is_err());
    assert_eq!(
        fetcher.calls_for(endpoint_id),
        1,
        "the manual arrival must reuse the automatic failure"
    );
    assert_eq!(
        feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Cooling,
        "a reused automatic failure can never recover manual feedback"
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn real_manual_available_fetch_recovers_feedback() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let fetcher = Arc::new(fake_fetcher(quota(Some(20.0))));
    let feedback = QuotaFeedbackStore::default();
    let service = service_with_feedback(&pool, fetcher.clone(), feedback.clone());
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, None);
    exhausted_feedback(&feedback, scope);

    let response = service
        .read(&provider_endpoint, true)
        .await
        .expect("manual refresh succeeds");
    assert!(response.keys[0].ok);
    assert_eq!(fetcher.calls_for(endpoint_id), 1);
    assert_eq!(
        feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Eligible,
        "a real manual Available fetch clears the old exhaustion"
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn failure_recorded_after_manual_fetch_started_is_retained() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let endpoint_id = create_endpoint(&pool).await;
    let (fetcher, started, release) = FakeFetcher::gated(quota(Some(20.0)));
    let fetcher = Arc::new(fetcher);
    let feedback = QuotaFeedbackStore::default();
    let service = Arc::new(service_with_feedback(
        &pool,
        fetcher.clone(),
        feedback.clone(),
    ));
    let provider_endpoint = endpoint(&pool, endpoint_id).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, None);
    exhausted_feedback(&feedback, scope);

    let manual = {
        let service = service.clone();
        let endpoint = provider_endpoint.clone();
        tokio::spawn(async move { service.read(&endpoint, true).await })
    };
    timeout(Duration::from_secs(3), started.notified())
        .await
        .expect("manual fetch started");
    let newer = Instant::now();
    feedback.record_failure(scope, QuotaFeedbackCause::RateLimited, None, newer, newer);
    release.open();
    let response = timeout(Duration::from_secs(5), manual)
        .await
        .expect("manual refresh finishes")
        .expect("manual task")
        .expect("manual refresh succeeds");
    assert!(response.keys[0].ok);
    assert_eq!(
        feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Cooling,
        "a failure newer than the fetch start survives the success"
    );
    cleanup_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn still_empty_unknown_and_failed_manual_results_keep_feedback() {
    let Some(pool) = test_pool().await else {
        return;
    };
    for (label, fetcher, expect_error) in [
        (
            "exhausted",
            Arc::new(fake_fetcher(quota(Some(100.0)))) as Arc<dyn super::ChatGptQuotaFetcher>,
            false,
        ),
        (
            "unknown",
            Arc::new(fake_fetcher(quota(None))) as Arc<dyn super::ChatGptQuotaFetcher>,
            false,
        ),
        (
            "failed",
            Arc::new(FakeFetcher::failure(ChatGptQuotaError::Upstream))
                as Arc<dyn super::ChatGptQuotaFetcher>,
            true,
        ),
    ] {
        let endpoint_id = create_endpoint(&pool).await;
        let feedback = QuotaFeedbackStore::default();
        let service = service_with_feedback(&pool, fetcher.clone(), feedback.clone());
        let provider_endpoint = endpoint(&pool, endpoint_id).await;
        let scope = QuotaFeedbackScope::new(endpoint_id, None);
        exhausted_feedback(&feedback, scope);

        let response = service.read(&provider_endpoint, true).await;
        assert_eq!(
            response.is_err(),
            expect_error,
            "{label} manual result has the expected outcome"
        );
        assert_eq!(
            feedback.eligibility(scope, Instant::now()),
            QuotaFeedbackEligibility::Cooling,
            "{label} manual result must never recover feedback"
        );
        cleanup_endpoint(&pool, endpoint_id).await;
    }
}
