//! OpencodeGo session-affinity routing consumption (issue #193 P4).
//!
//! The bound key is gated on the tightest remaining percent window; PAYG
//! keys without windows stay honored (no quota signal means no exhaustion).
//! These exercise the real `select_route_for_candidate` against the
//! production `TokenPlanQuotaCache`, mirroring the CommandCode arms in
//! `session_affinity_quota_tests.rs`.

use super::session_affinity_quota_tests::{bind_key, request};
use super::session_affinity_tests::request_context;
use super::{RouteAffinityError, select_route_for_candidate};
use crate::{
    db,
    replay_cache::ReplayCache,
    worker::runtime::prompt_log::RequestPromptLog,
    worker_admin_types::{OpencodeGoWindowUsage, TokenPlanKeyUsage, TokenPlanUsageResponse},
};
use uuid::Uuid;

fn opencode_go_window(percent: f64) -> OpencodeGoWindowUsage {
    OpencodeGoWindowUsage {
        status: None,
        percent: Some(percent),
        resets_at: None,
    }
}

fn opencode_go_key(
    key_id: Uuid,
    key_label: &str,
    rolling: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: key_label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        model_remains: Vec::new(),
        balances: None,
        five_hour: None,
        weekly: None,
        opencodego_rolling: rolling.map(opencode_go_window),
        opencodego_weekly: weekly.map(opencode_go_window),
        opencodego_monthly: monthly.map(opencode_go_window),
        openrouter_balance: None,
        openrouter_spend: None,
    }
}

fn opencode_go_usage(
    keys: &[(Uuid, &str, Option<f64>, Option<f64>, Option<f64>)],
) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        provider: db::EndpointProvider::OpencodeGo,
        provider_region: None,
        keys: keys
            .iter()
            .map(|(key_id, key_label, rolling, weekly, monthly)| {
                opencode_go_key(*key_id, key_label, *rolling, *weekly, *monthly)
            })
            .collect(),
    }
}

#[tokio::test]
async fn exhausted_opencode_go_bound_key_returns_target_unavailable() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services =
        super::super::tests::session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = super::super::tests::session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;
    let alternate_key_id = Uuid::new_v4();
    target.api_keys.push(db::EndpointApiKey {
        key_id: alternate_key_id,
        endpoint_id,
        key_label: "alternate".to_string(),
        api_key: "alternate-key".to_string(),
        position: 1,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });

    let conversation_id = Uuid::new_v4();
    bind_key(
        &replay_cache,
        candidate.rule_id,
        conversation_id,
        endpoint_id,
        bound_key_id,
        "key-a",
    )
    .await;
    services
        .admin_state()
        .expect("admin state")
        .token_plan_quota
        .store_for_test(
            endpoint_id,
            opencode_go_usage(&[
                (bound_key_id, "primary", Some(100.0), Some(100.0), Some(100.0)),
                (alternate_key_id, "alternate", Some(50.0), None, None),
            ]),
        )
        .await;

    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            preferred_endpoint_id: Some(endpoint_id),
            ..RequestPromptLog::default()
        },
    );
    let error = match select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("exhausted opencode_go bound key must not fail over to another key"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_target_unavailable")
    );
}

#[tokio::test]
async fn payg_opencode_go_bound_key_without_windows_is_still_honored() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services =
        super::super::tests::session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = super::super::tests::session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;

    let conversation_id = Uuid::new_v4();
    bind_key(
        &replay_cache,
        candidate.rule_id,
        conversation_id,
        endpoint_id,
        bound_key_id,
        "key-a",
    )
    .await;
    services
        .admin_state()
        .expect("admin state")
        .token_plan_quota
        .store_for_test(
            endpoint_id,
            opencode_go_usage(&[(bound_key_id, "primary", None, None, None)]),
        )
        .await;

    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            preferred_endpoint_id: Some(endpoint_id),
            ..RequestPromptLog::default()
        },
    );
    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &request(),
        1,
        Some("key-a"),
    )
    .await
    .expect("payg bound key without windows must still route")
    .expect("payg bound key must select a route");
    assert_eq!(selected.route.endpoint_key_id, Some(bound_key_id));
}
