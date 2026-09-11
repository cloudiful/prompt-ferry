use super::super::{
    WorkerRuntimeState,
    prompt_log::RequestPromptLog,
    tests::{sample_request, session_affinity_candidate, session_affinity_services},
};
use super::{select_route_for_candidate, session_affinity_tests::request_context};
use crate::replay_cache::ReplayCache;

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
async fn reports_unavailable_when_the_bound_endpoint_leaves_the_route() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
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
    changed_candidate
        .targets
        .first()
        .expect("route should keep one replacement target");

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
        Ok(_) => panic!("stale affinity must not silently rebind after the bound endpoint leaves"),
        Err(error) => error,
    };
    assert_eq!(
        error
            .downcast_ref::<super::RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_target_unavailable")
    );
}
