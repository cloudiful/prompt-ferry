use std::time::{Duration, Instant};

use crate::{
    config::NativeApi,
    db::{CompactMode, EndpointProvider, RouteConfig, RouteSelectionReason},
    worker::runtime::{
        WorkerRuntimeState,
        ai::quota_feedback::AttemptFeedbackFactory,
        context::{BridgeSender, ResponseLimits, RuntimeServices},
    },
    worker_admin::{
        quota_feedback::{QuotaFeedbackEligibility, QuotaFeedbackScope},
        token_plan_cache::TokenPlanQuotaCache,
    },
};

use super::settle_handshake;

fn route() -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: "http://127.0.0.1:1".to_string(),
        api_key: "fixture-key".to_string(),
        endpoint_key_id: Some(uuid::Uuid::new_v4()),
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: NativeApi::Realtime,
        upstream_model: None,
        route_selection_reason: RouteSelectionReason::Default,
        provider: EndpointProvider::Generic,
        service_tier: None,
        proxy_url: None,
        dev_system_normalize: false,
        thinking_effort_override: None,
        compact_mode: CompactMode::Passthrough,
        thinking_downgrade_enabled: false,
        has_oauth_token: false,
    }
}

fn services() -> RuntimeServices {
    RuntimeServices::new(
        None,
        BridgeSender::test_sender(),
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
}

#[test]
fn successful_handshake_clears_only_feedback_older_than_the_attempt() {
    let route = route();
    let services = services();
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let failed_at = Instant::now() - Duration::from_secs(60);
    cache.feedback.record_429(scope, None, failed_at, failed_at);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();

    settle_handshake::<()>(Ok(()), &feedback, &services, &route).expect("handshake");

    assert_eq!(
        cache.feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::Eligible
    );

    let failed_at = Instant::now() - Duration::from_secs(60);
    cache.feedback.record_429(scope, None, failed_at, failed_at);
    let feedback = AttemptFeedbackFactory::new(Some(&cache), &route, None).begin_attempt();
    assert!(
        settle_handshake::<()>(
            Err(tokio_tungstenite::tungstenite::Error::ConnectionClosed),
            &feedback,
            &services,
            &route,
        )
        .is_err()
    );
    assert_eq!(
        cache.feedback.eligibility(scope, Instant::now()),
        QuotaFeedbackEligibility::ProbeRequired,
        "a failed handshake must not record success"
    );
}
