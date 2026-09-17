use super::super::{
    WorkerRuntimeState,
    prompt_log::RequestPromptLog,
    tests::{sample_request, session_affinity_candidate, session_affinity_services},
};
use super::{select_route_for_candidate, session_affinity_tests::request_context};
use crate::{
    db,
    replay_cache::ReplayCache,
    response_affinity::{ResponseAffinityBinding, ResponseAffinityStore, api_key_fingerprint},
    worker_admin_types::{
        TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse, TokenPlanWindowUsage,
    },
};

#[tokio::test]
async fn ignores_preferred_endpoint_from_another_model_route() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let foreign_endpoint_id = uuid::Uuid::new_v4();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(2),
            preferred_endpoint_id: Some(foreign_endpoint_id),
            ..RequestPromptLog::default()
        },
    );

    let first = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("current model route should still be selected");
    let second = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("current model route should still be selected");

    assert_ne!(first.route.route_id, foreign_endpoint_id);
    assert!(
        candidate
            .targets
            .iter()
            .any(|target| target.endpoint_id == first.route.route_id),
        "route must come from the current candidate"
    );
    assert_eq!(first.route.route_id, second.route.route_id);
}

#[tokio::test]
async fn rebinds_when_the_bound_endpoint_leaves_the_route() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");
    let mut changed_candidate = candidate.clone();
    changed_candidate
        .targets
        .retain(|target| target.endpoint_id != first.route.route_id);
    let replacement = changed_candidate
        .targets
        .first()
        .expect("route should keep one replacement target");
    let replacement_endpoint_id = replacement.endpoint_id;
    let replacement_key_id = replacement.api_keys[0].key_id;

    // Issue #444: a stale (StaleEndpoint) binding is cleared and rebuilt on
    // the remaining target instead of failing with target_unavailable.
    let rebound = select_route_for_candidate(
        &services,
        &request_ctx,
        &changed_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("stale affinity must rebind to the remaining endpoint");
    assert_eq!(rebound.route.route_id, replacement_endpoint_id);
    assert_eq!(
        rebound.route.route_selection_reason,
        db::RouteSelectionReason::SessionAffinity
    );

    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let stored = replay_cache
        .response_affinity()
        .get(&cache_key)
        .await
        .unwrap()
        .expect("rebound binding exists");
    assert_eq!(stored.endpoint_id, replacement_endpoint_id);
    assert_eq!(stored.endpoint_key_id, Some(replacement_key_id));
}

#[tokio::test]
async fn reports_unavailable_when_no_replacement_unit_is_usable() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");
    // Keep only the bound target but strip every usable unit (no keys, empty
    // secret): the schedule filter still passes (enabled, no windows) while
    // the rebuild pool is empty, so the original target_unavailable stands.
    let mut changed_candidate = candidate.clone();
    changed_candidate
        .targets
        .retain(|target| target.endpoint_id == first.route.route_id);
    let bound_target = changed_candidate
        .targets
        .first_mut()
        .expect("route should keep the bound target");
    bound_target.api_keys.clear();
    bound_target.api_key.clear();

    let error = match select_route_for_candidate(
        &services,
        &request_ctx,
        &changed_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("rebind with no usable unit must still report unavailable"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<super::RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_target_unavailable")
    );

    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    assert!(
        replay_cache
            .response_affinity()
            .get(&cache_key)
            .await
            .unwrap()
            .is_none(),
        "failed rebuild must leave the stale binding cleared"
    );
}

#[tokio::test]
async fn auto_rebind_after_stale_endpoint() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");
    // Disable (instead of removing) the bound endpoint: StaleEndpoint must
    // still trigger delete + rebuild onto the healthy endpoint.
    let mut changed_candidate = candidate.clone();
    changed_candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == first.route.route_id)
        .expect("bound target exists")
        .enabled = false;
    let healthy = changed_candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id != first.route.route_id)
        .expect("route should keep one healthy target");

    let rebound = select_route_for_candidate(
        &services,
        &request_ctx,
        &changed_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("disabled bound endpoint must rebind to the healthy one");
    assert_eq!(rebound.route.route_id, healthy.endpoint_id);

    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let stored = replay_cache
        .response_affinity()
        .get(&cache_key)
        .await
        .unwrap()
        .expect("rebound binding exists");
    assert_eq!(stored.endpoint_id, healthy.endpoint_id);
}

#[tokio::test]
async fn auto_rebind_after_stale_key() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");
    // Simulate bound-key deletion on the bound target (no key rows, empty
    // secret) so the rebuild pool can only resolve the other endpoint.
    let mut changed_candidate = candidate.clone();
    let other_endpoint_id = changed_candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id != first.route.route_id)
        .expect("candidate should have another endpoint")
        .endpoint_id;
    let bound_target = changed_candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == first.route.route_id)
        .expect("bound target exists");
    bound_target.api_keys.clear();
    bound_target.api_key.clear();

    let rebound = select_route_for_candidate(
        &services,
        &request_ctx,
        &changed_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("deleted bound key must rebind to the other endpoint");
    assert_eq!(rebound.route.route_id, other_endpoint_id);

    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let stored = replay_cache
        .response_affinity()
        .get(&cache_key)
        .await
        .unwrap()
        .expect("rebound binding exists");
    assert_eq!(stored.endpoint_id, other_endpoint_id);
}

#[tokio::test]
async fn does_not_rebind_when_quota_exhausted() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let candidate = session_affinity_candidate();
    let bound_target = candidate.targets.first().expect("bound target exists");
    let endpoint_id = bound_target.endpoint_id;
    let bound_key_id = bound_target.api_keys[0].key_id;
    let other_endpoint_id = candidate.targets[1].endpoint_id;
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
        .store_for_test(endpoint_id, exhausted_key_usage(bound_key_id))
        .await;

    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    // Quota exhaustion must keep the quota-failover path (replace +
    // QuotaFailover reason), never the stale delete + rebuild path (which
    // reports SessionAffinity).
    let migrated = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .expect("exhausted bound key must migrate")
    .expect("migration must select a route");
    assert_eq!(migrated.route.route_id, other_endpoint_id);
    assert_eq!(
        migrated.route.route_selection_reason,
        db::RouteSelectionReason::QuotaFailover
    );
}

#[tokio::test]
async fn auto_rebind_delete_failure_maps_to_backend_unavailable() {
    // A failing affinity store cannot delete the stale binding; select must
    // surface backend_unavailable (never target_unavailable) on that path.
    let store = ResponseAffinityStore::unavailable();
    store
        .delete("any-key")
        .await
        .expect_err("unavailable backend must fail delete");

    let services = session_affinity_services(WorkerRuntimeState::default(), ReplayCache::default());
    let candidate = session_affinity_candidate();
    let error = match select_route_for_candidate(
        &services,
        &request_context(
            uuid::Uuid::new_v4(),
            RequestPromptLog {
                conversation_id: Some(uuid::Uuid::new_v4()),
                conversation_seq: Some(1),
                ..RequestPromptLog::default()
            },
        ),
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("unavailable affinity backend must not select a route"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<super::RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_unavailable")
    );
}

fn exhausted_key_usage(key_id: uuid::Uuid) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        provider: db::EndpointProvider::Minimax,
        provider_region: Some(db::EndpointRegion::Cn),
        keys: vec![TokenPlanKeyUsage {
            key_id,
            key_label: "primary".to_string(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            model_remains: vec![TokenPlanModelUsage {
                model_name: "general".to_string(),
                interval: Some(TokenPlanWindowUsage {
                    status: Some(1),
                    remaining_percent: Some(0.0),
                    total_count: None,
                    usage_count: None,
                    boost_permille: None,
                    start_at: None,
                    end_at: None,
                    remains_time_ms: None,
                }),
                weekly: Some(TokenPlanWindowUsage {
                    status: Some(1),
                    remaining_percent: Some(0.0),
                    total_count: None,
                    usage_count: None,
                    boost_permille: None,
                    start_at: None,
                    end_at: None,
                    remains_time_ms: None,
                }),
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
            deepseek_balance: None,
        }],
    }
}
