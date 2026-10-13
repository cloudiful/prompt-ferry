use std::time::{Duration, Instant};

#[path = "quota_feedback_fixtures.rs"]
mod quota_feedback_fixtures;

use crate::{
    config::NativeApi,
    protocol::BridgeMessage,
    worker::runtime::ai::quota_feedback::AttemptFeedbackFactory,
    worker_admin::{
        quota_feedback::{QuotaFeedbackCause, QuotaFeedbackEligibility, QuotaFeedbackScope},
        token_plan_cache::TokenPlanQuotaCache,
    },
};

use self::quota_feedback_fixtures::{
    forward_with_feedback, keyed_route, spawn_http_response, spawn_responses_failed_stream,
};
use super::{
    responses_framing::spawn_split_event_stream_upstream, spawn_bridge_log, test_request,
    test_request_ctx, test_route, test_services, wait_for_bridge, wait_for_count,
};

#[tokio::test]
async fn generic_429_is_forwarded_without_same_request_retry() {
    let (address, count) = spawn_http_response(
        429,
        "Too Many Requests",
        "Content-Type: application/json\r\nRetry-After: 8\r\n",
        br#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#,
    )
    .await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{address}"), NativeApi::Responses);

    let outcome = super::forward_test_request(&services, &route)
        .await
        .expect("forward");

    assert!(matches!(outcome, super::ForwardOutcome::Handled));
    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    let messages = bridge_log.lock().await;
    let status = messages.iter().find_map(|message| match message {
        BridgeMessage::ResponseStart(start) => Some(start.status),
        _ => None,
    });
    assert_eq!(status, Some(429));
    let body = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(String::from_utf8_lossy(&body).contains("rate limited"));
}

#[tokio::test]
async fn upstream_429_records_key_cooldown_and_preserves_client_error() {
    let (address, count) = spawn_http_response(
        429,
        "Too Many Requests",
        "Content-Type: application/json\r\nRetry-After: 8\r\n",
        br#"{"error":{"message":"rate limited","type":"rate_limit_error"}}"#,
    )
    .await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let mut route = keyed_route(&format!("http://{address}"));
    route.provider = crate::db::EndpointProvider::Generic;
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
        .await
        .expect("forward upstream error");

    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let status = messages.iter().find_map(|message| match message {
        BridgeMessage::ResponseStart(start) => Some(start.status),
        _ => None,
    });
    assert_eq!(status, Some(429));
    assert_eq!(
        cache
            .feedback
            .eligibility(scope, Instant::now() + Duration::from_secs(1)),
        QuotaFeedbackEligibility::Cooling
    );
    assert_eq!(
        cache.feedback.eligibility(
            QuotaFeedbackScope::new(route.route_id, Some(uuid::Uuid::new_v4())),
            Instant::now(),
        ),
        QuotaFeedbackEligibility::Eligible
    );
}

#[tokio::test]
async fn every_429_cause_blocks_routing_but_keeps_its_own_label() {
    // A 429 always backs the route off; only the recorded cause differs, and
    // only a confirmed exhaustion may rewrite the status the client sees.
    let cases: &[(&str, &[u8], QuotaFeedbackCause, u16)] = &[
        (
            "generic",
            br#"{"error":{"message":"Too Many Requests","type":"rate_limit_error"}}"#,
            QuotaFeedbackCause::RateLimited,
            429,
        ),
        (
            "in-flight",
            br#"{"error":{"message":"in-flight credits reserved"}}"#,
            QuotaFeedbackCause::RateLimited,
            429,
        ),
        (
            "unavailable",
            br#"{"error":{"message":"Service Unavailable"}}"#,
            QuotaFeedbackCause::UpstreamUnavailable,
            429,
        ),
        (
            "exhausted",
            br#"{"error":{"code":2056,"message":"usage limit reached"}}"#,
            QuotaFeedbackCause::Exhausted,
            429,
        ),
    ];
    for (name, body, expected_cause, expected_status) in cases {
        let (address, count) = spawn_http_response(
            429,
            "Too Many Requests",
            "Content-Type: application/json\r\nRetry-After: 8\r\n",
            body,
        )
        .await;
        let (out_tx, bridge_log) = spawn_bridge_log();
        let services = test_services(out_tx);
        let mut route = keyed_route(&format!("http://{address}"));
        route.provider = crate::db::EndpointProvider::Generic;
        let request = test_request();
        let request_ctx = test_request_ctx(&services.runtime_state);
        let cache = TokenPlanQuotaCache::default();
        let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
        let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

        forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
            .await
            .expect("forward upstream error");

        wait_for_count(&count, 1).await;
        wait_for_bridge(&bridge_log, 3).await;
        let messages = bridge_log.lock().await;
        let status = messages.iter().find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start.status),
            _ => None,
        });
        assert_eq!(status, Some(*expected_status), "{name}");
        assert_eq!(
            cache.feedback.eligibility(scope, Instant::now()),
            QuotaFeedbackEligibility::Cooling,
            "{name} must block routing whatever its cause"
        );
        assert_eq!(
            cache
                .feedback
                .active_cooldown(scope, Instant::now())
                .map(|(cause, _)| cause),
            Some(*expected_cause),
            "{name}"
        );
    }
}

#[tokio::test]
async fn an_unrelated_non_429_failure_starts_no_backoff() {
    let (address, count) = spawn_http_response(
        503,
        "Service Unavailable",
        "Content-Type: application/json\r\n",
        br#"{"error":{"message":"temporary provider failure"}}"#,
    )
    .await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let mut route = keyed_route(&format!("http://{address}"));
    route.provider = crate::db::EndpointProvider::Generic;
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
        .await
        .expect("forward upstream error");

    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let status = messages.iter().find_map(|message| match message {
        BridgeMessage::ResponseStart(start) => Some(start.status),
        _ => None,
    });
    assert_eq!(status, Some(503));
    assert_eq!(
        cache.feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Eligible
    );
}

#[tokio::test]
async fn a_confirmed_exhaustion_on_a_non_429_still_records_and_rewrites_the_status() {
    let (address, count) = spawn_http_response(
        400,
        "Bad Request",
        "Content-Type: application/json\r\n",
        br#"{"error":{"message":"Insufficient Balance","type":"unknown_error"}}"#,
    )
    .await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let mut route = keyed_route(&format!("http://{address}"));
    route.provider = crate::db::EndpointProvider::Generic;
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
        .await
        .expect("forward upstream error");

    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    let messages = bridge_log.lock().await;
    let status = messages.iter().find_map(|message| match message {
        BridgeMessage::ResponseStart(start) => Some(start.status),
        _ => None,
    });
    assert_eq!(status, Some(429));
    assert_eq!(
        cache
            .feedback
            .active_cooldown(scope, Instant::now())
            .map(|(cause, _)| cause),
        Some(QuotaFeedbackCause::Exhausted)
    );
}

#[tokio::test]
async fn completed_nonstream_response_recovers_key() {
    let (address, count) = spawn_http_response(
        200,
        "OK",
        "Content-Type: application/json\r\n",
        br#"{"id":"resp_1","object":"response","status":"completed","output":[],"usage":{"total_tokens":5}}"#,
    )
    .await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = keyed_route(&format!("http://{address}"));
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let failed_at = Instant::now() - Duration::from_secs(60);
    cache.feedback.record_429(scope, None, failed_at, failed_at);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
        .await
        .expect("forward completed response");

    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    assert_eq!(
        cache.feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Eligible
    );
}

#[tokio::test]
async fn completed_stream_recovers_key_but_failed_terminal_does_not() {
    let (address, count) = spawn_split_event_stream_upstream(Some("text/event-stream")).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = keyed_route(&format!("http://{address}"));
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let failed_at = Instant::now() - Duration::from_secs(60);
    cache.feedback.record_429(scope, None, failed_at, failed_at);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
        .await
        .expect("forward completed stream");
    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;
    assert_eq!(
        cache.feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Eligible
    );

    let (address, failed_count) = spawn_responses_failed_stream().await;
    let (out_tx, failed_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let mut route = keyed_route(&format!("http://{address}"));
    route.route_id = scope.endpoint_id;
    route.endpoint_key_id = scope.key_id;
    let request = test_request();
    let request_ctx = test_request_ctx(&services.runtime_state);
    let failed_at = Instant::now() - Duration::from_secs(60);
    cache.feedback.record_429(scope, None, failed_at, failed_at);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    forward_with_feedback(address, &route, &request, &request_ctx, &services, feedback)
        .await
        .expect("forward failed terminal");
    wait_for_count(&failed_count, 1).await;
    wait_for_bridge(&failed_log, 3).await;
    assert_eq!(
        cache.feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::ProbeRequired,
        "a response.failed terminal is not a successful completion"
    );
}
