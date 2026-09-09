use super::super::{
    prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
    tests::{session_affinity_candidate, session_affinity_services},
};
use super::{
    RouteAffinityError, select_route_for_candidate, session_affinity_tests::request_context,
};
use crate::{
    db,
    replay_cache::ReplayCache,
    response_affinity::{ResponseAffinityBinding, ResponseAffinityStore, api_key_fingerprint},
    worker_admin_types::{
        CommandCodeBalances, CommandCodeWindowUsage, TokenPlanKeyUsage, TokenPlanModelUsage,
        TokenPlanUsageResponse, TokenPlanWindowUsage,
    },
};
use chrono::Utc;

#[tokio::test]
async fn exhausted_bound_key_returns_target_unavailable() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;
    let alternate_key_id = uuid::Uuid::new_v4();
    target.api_keys.push(db::EndpointApiKey {
        key_id: alternate_key_id,
        endpoint_id,
        key_label: "alternate".to_string(),
        api_key: "alternate-key".to_string(),
        position: 1,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    let conversation_id = uuid::Uuid::new_v4();
    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    replay_cache
        .response_affinity()
        .get_or_create(
            &cache_key,
            &ResponseAffinityBinding {
                endpoint_id,
                endpoint_key_id: Some(bound_key_id),
                endpoint_key_fingerprint: api_key_fingerprint("key-a"),
            },
        )
        .await
        .expect("binding should be stored");
    services
        .admin_state()
        .expect("admin state")
        .token_plan_quota
        .store_for_test(
            endpoint_id,
            usage_with_keys(&[
                (bound_key_id, "primary", 0.0),
                (alternate_key_id, "alternate", 100.0),
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
        Ok(_) => panic!("exhausted bound key must not fail over to another key"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_target_unavailable")
    );
}

pub(super) fn request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        method: "POST".to_string(),
        path: "/v1/responses".to_string(),
        headers: Vec::new(),
        body: br#"{"model":"gpt-5.4","input":"ping"}"#.to_vec(),
        request_deadline_unix_ms: 0,
        user_id: Some(1),
        client_key_hash: Some("key-a".to_string()),
        request_user_agent: Some("Codex Desktop".to_string()),
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    }
}

fn usage_with_keys(keys: &[(uuid::Uuid, &str, f64)]) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        provider: db::EndpointProvider::Minimax,
        provider_region: Some(db::EndpointRegion::Cn),
        keys: keys
            .iter()
            .map(|(key_id, key_label, remaining_percent)| TokenPlanKeyUsage {
                key_id: *key_id,
                key_label: (*key_label).to_string(),
                ok: true,
                status: Some(200),
                error_code: None,
                error_message: None,
                model_remains: vec![TokenPlanModelUsage {
                    model_name: "general".to_string(),
                    interval: Some(window(*remaining_percent)),
                    weekly: Some(window(*remaining_percent)),
                }],
                balances: None,
                five_hour: None,
                weekly: None,
                opencodego_rolling: None,
                opencodego_weekly: None,
                opencodego_monthly: None,
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
            })
            .collect(),
    }
}

fn window(remaining_percent: f64) -> TokenPlanWindowUsage {
    TokenPlanWindowUsage {
        status: Some(1),
        remaining_percent: Some(remaining_percent),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at: None,
        remains_time_ms: None,
    }
}

// CommandCode session-affinity consumption (issue #184 P5): the bound key
// is gated on the tighter USD window; PAYG keys without windows stay
// honored (no quota signal means no exhaustion).
fn command_code_window(remaining_percent: f64) -> CommandCodeWindowUsage {
    CommandCodeWindowUsage {
        used: 10.0 - remaining_percent / 10.0,
        cap: 10.0,
        used_percent: Some(100.0 - remaining_percent),
        remaining_percent: Some(remaining_percent),
        reset_at: None,
    }
}

fn command_code_key(
    key_id: uuid::Uuid,
    key_label: &str,
    five_hour: Option<f64>,
    weekly: Option<f64>,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: key_label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        model_remains: Vec::new(),
        balances: Some(CommandCodeBalances {
            monthly_credits: 80.0,
            purchased_credits: 0.0,
            free_credits: 0.0,
            remaining_credits: 80.0,
        }),
        five_hour: five_hour.map(command_code_window),
        weekly: weekly.map(command_code_window),
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
        openrouter_balance: None,
        openrouter_spend: None,
        glm_five_hour: None,
        glm_weekly: None,
    }
}

fn command_code_usage(
    keys: &[(uuid::Uuid, &str, Option<f64>, Option<f64>)],
) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        provider: db::EndpointProvider::CommandCode,
        provider_region: None,
        keys: keys
            .iter()
            .map(|(key_id, key_label, five_hour, weekly)| {
                command_code_key(*key_id, key_label, *five_hour, *weekly)
            })
            .collect(),
    }
}

pub(super) async fn bind_key(
    replay_cache: &ReplayCache,
    rule_id: uuid::Uuid,
    conversation_id: uuid::Uuid,
    endpoint_id: uuid::Uuid,
    key_id: uuid::Uuid,
    fingerprint_secret: &str,
) {
    let cache_key =
        ResponseAffinityStore::cache_key(1, rule_id, &format!("conversation:{conversation_id}"));
    replay_cache
        .response_affinity()
        .get_or_create(
            &cache_key,
            &ResponseAffinityBinding {
                endpoint_id,
                endpoint_key_id: Some(key_id),
                endpoint_key_fingerprint: api_key_fingerprint(fingerprint_secret),
            },
        )
        .await
        .expect("binding should be stored");
}

#[tokio::test]
async fn exhausted_command_code_bound_key_returns_target_unavailable() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;
    let alternate_key_id = uuid::Uuid::new_v4();
    target.api_keys.push(db::EndpointApiKey {
        key_id: alternate_key_id,
        endpoint_id,
        key_label: "alternate".to_string(),
        api_key: "alternate-key".to_string(),
        position: 1,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });

    let conversation_id = uuid::Uuid::new_v4();
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
            command_code_usage(&[
                (bound_key_id, "primary", Some(0.0), Some(0.0)),
                (alternate_key_id, "alternate", Some(100.0), Some(100.0)),
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
        Ok(_) => panic!("exhausted command_code bound key must not fail over to another key"),
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
async fn payg_command_code_bound_key_without_windows_is_still_honored() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = session_affinity_candidate();
    let target = candidate.targets.first_mut().expect("target exists");
    target.key_lb_enabled = true;
    let endpoint_id = target.endpoint_id;
    let bound_key_id = target.api_keys[0].key_id;

    let conversation_id = uuid::Uuid::new_v4();
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
            command_code_usage(&[(bound_key_id, "primary", None, None)]),
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
