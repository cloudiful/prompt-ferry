use std::time::Instant;

use chrono::Utc;

use super::super::{
    RequestExecutionContext, WorkerRuntimeState,
    prompt_log::RequestPromptLog,
    tests::{sample_request, session_affinity_candidate, session_affinity_services},
};
use super::{RouteAffinityError, select_route_for_candidate};
use crate::{db, replay_cache::ReplayCache};

pub(super) fn request_context(
    worker_id: uuid::Uuid,
    prompt_log: RequestPromptLog,
) -> RequestExecutionContext {
    RequestExecutionContext::new(
        uuid::Uuid::new_v4(),
        Instant::now(),
        Some("gpt-4.1-mini".to_string()),
        None,
        None,
        Some(1),
        worker_id,
        prompt_log,
    )
}

#[tokio::test]
async fn rejects_unavailable_shared_backend() {
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), ReplayCache::default());
    let candidate = session_affinity_candidate();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );

    let error = match select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("strict affinity must not use a process-local fallback"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_unavailable")
    );
}

#[tokio::test]
async fn concurrent_first_requests_share_binding_and_follow_key_rotation() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state_a = WorkerRuntimeState::default();
    let runtime_state_b = WorkerRuntimeState::default();
    let services_a = session_affinity_services(runtime_state_a.clone(), replay_cache.clone());
    let services_b = session_affinity_services(runtime_state_b.clone(), replay_cache);
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_a = sample_request();
    let request_b = sample_request();
    let request_ctx_a = request_context(
        runtime_state_a.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let request_ctx_b = request_context(
        runtime_state_b.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );

    let (left, right) = tokio::join!(
        select_route_for_candidate(
            &services_a,
            &request_ctx_a,
            &candidate,
            &request_a,
            1,
            Some("key-a"),
        ),
        select_route_for_candidate(
            &services_b,
            &request_ctx_b,
            &candidate,
            &request_b,
            1,
            Some("key-a"),
        )
    );
    let left = left.unwrap().expect("first worker should select a route");
    let right = right.unwrap().expect("second worker should select a route");
    assert_eq!(left.route.route_id, right.route.route_id);
    assert_eq!(left.route.endpoint_key_id, right.route.endpoint_key_id);
    assert_eq!(left.route.api_key, right.route.api_key);

    let mut rotated_candidate = candidate.clone();
    let bound_target = rotated_candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == left.route.route_id)
        .expect("bound target exists");
    bound_target.api_key = "rotated-key".to_string();
    bound_target.api_keys[0].api_key = "rotated-key".to_string();
    let rotated = select_route_for_candidate(
        &services_b,
        &request_ctx_b,
        &rotated_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("rotated key should remain usable");
    assert_eq!(rotated.route.route_id, left.route.route_id);
    assert_eq!(rotated.route.endpoint_key_id, left.route.endpoint_key_id);
    assert_eq!(rotated.route.api_key, "rotated-key");

    rotated_candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == left.route.route_id)
        .expect("bound target exists")
        .api_keys[0]
        .enabled = false;
    let unavailable = match select_route_for_candidate(
        &services_b,
        &request_ctx_b,
        &rotated_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("disabled bound key must not fail over"),
        Err(error) => error,
    };
    assert_eq!(
        unavailable
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_target_unavailable")
    );
}

#[tokio::test]
async fn rebinds_to_force_replay_endpoint_override() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let first_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &first_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");

    let other_endpoint_id = candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id != first.route.route_id)
        .expect("candidate should have another endpoint")
        .endpoint_id;
    let override_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_override_endpoint_id: Some(other_endpoint_id),
            ..RequestPromptLog::default()
        },
    );
    let rebound = select_route_for_candidate(
        &services,
        &override_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("force_replay override should rebind the session");
    assert_eq!(rebound.route.route_id, other_endpoint_id);
    assert_eq!(
        rebound.route.route_selection_reason,
        db::RouteSelectionReason::ConversationOverride
    );

    let follow_up = select_route_for_candidate(
        &services,
        &first_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("rebound session should stay on the override target");
    assert_eq!(follow_up.route.route_id, other_endpoint_id);
}

#[tokio::test]
async fn rebinds_to_force_replay_key_override() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
    let mut candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let first_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &first_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");

    let secondary_key_id = uuid::Uuid::new_v4();
    let bound_target = candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == first.route.route_id)
        .expect("bound target exists");
    bound_target.api_keys.push(db::EndpointApiKey {
        key_id: secondary_key_id,
        endpoint_id: first.route.route_id,
        key_label: "secondary".to_string(),
        api_key: "secondary-key".to_string(),
        position: 1,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    let key_override_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_override_endpoint_id: Some(first.route.route_id),
            conversation_override_endpoint_key_id: Some(secondary_key_id),
            ..RequestPromptLog::default()
        },
    );
    let rebound = select_route_for_candidate(
        &services,
        &key_override_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("force_replay key override should rebind the session");
    assert_eq!(rebound.route.route_id, first.route.route_id);
    assert_eq!(
        rebound.route.endpoint_key_id,
        Some(secondary_key_id),
        "key override should rebind to the requested key"
    );
}

#[tokio::test]
async fn rejects_force_passthrough_endpoint_and_key_override_conflicts() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
    let mut candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let first_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let first = select_route_for_candidate(
        &services,
        &first_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("initial route should be selected");

    let other_endpoint_id = candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id != first.route.route_id)
        .expect("candidate should have another endpoint")
        .endpoint_id;
    candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == other_endpoint_id)
        .expect("override target exists")
        .responses_continuation_policy = db::ResponsesContinuationPolicy::ForcePassthrough;
    let endpoint_override_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_override_endpoint_id: Some(other_endpoint_id),
            ..RequestPromptLog::default()
        },
    );
    let endpoint_conflict = match select_route_for_candidate(
        &services,
        &endpoint_override_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("force_passthrough endpoint override must not replace an existing binding"),
        Err(error) => error,
    };
    assert_eq!(
        endpoint_conflict
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_conflict")
    );

    let conflicting_key_id = uuid::Uuid::new_v4();
    let bound_target = candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == first.route.route_id)
        .expect("bound target exists");
    bound_target.responses_continuation_policy = db::ResponsesContinuationPolicy::ForcePassthrough;
    bound_target.api_keys.push(db::EndpointApiKey {
        key_id: conflicting_key_id,
        endpoint_id: first.route.route_id,
        key_label: "secondary".to_string(),
        api_key: "secondary-key".to_string(),
        position: 1,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    });
    let key_conflict_context = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_override_endpoint_id: Some(first.route.route_id),
            conversation_override_endpoint_key_id: Some(conflicting_key_id),
            ..RequestPromptLog::default()
        },
    );
    let key_conflict = match select_route_for_candidate(
        &services,
        &key_conflict_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("force_passthrough key override must not replace an existing binding"),
        Err(error) => error,
    };
    assert_eq!(
        key_conflict
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_conflict")
    );
}

#[tokio::test]
async fn heals_stale_key_id_when_secret_fingerprint_matches() {
    use crate::response_affinity::{ResponseAffinityStore, api_key_fingerprint};

    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache.clone());
    let mut candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let bound_target = candidate.targets.first_mut().expect("bound target exists");
    let endpoint_id = bound_target.endpoint_id;
    let old_key_id = bound_target.api_keys[0].key_id;
    let stale_binding = crate::response_affinity::ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: Some(old_key_id),
        endpoint_key_fingerprint: api_key_fingerprint("key-a"),
    };
    let store = replay_cache.response_affinity();
    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    store
        .get_or_create(&cache_key, &stale_binding)
        .await
        .unwrap();

    bound_target.api_keys[0].key_id = uuid::Uuid::new_v4();
    let new_key_id = bound_target.api_keys[0].key_id;
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..RequestPromptLog::default()
        },
    );
    let healed = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("stale key_id with matching fingerprint should heal to the new key");
    assert_eq!(healed.route.endpoint_key_id, Some(new_key_id));

    let healed_binding = store
        .get(&cache_key)
        .await
        .unwrap()
        .expect("binding exists");
    assert_eq!(
        healed_binding.endpoint_key_id,
        Some(new_key_id),
        "binding should be healed to the new key_id"
    );

    let follow_up = select_route_for_candidate(
        &services,
        &request_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("healed binding should stay usable");
    assert_eq!(follow_up.route.endpoint_key_id, Some(new_key_id));
}
