use std::time::Instant;

use super::super::{
    RequestExecutionContext, WorkerRuntimeState,
    prompt_log::RequestPromptLog,
    tests::{sample_request, session_affinity_candidate, session_affinity_services},
};
use super::{rendezvous_target, select_route_for_candidate};
use crate::replay_cache::ReplayCache;

fn request_context(worker_id: uuid::Uuid, prompt_log: RequestPromptLog) -> RequestExecutionContext {
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
async fn ignores_preferred_endpoint_from_another_model_route() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state.clone(), replay_cache);
    let candidate = session_affinity_candidate();
    let conversation_id = uuid::Uuid::new_v4();
    let request_ctx = request_context(
        runtime_state.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(2),
            preferred_endpoint_id: Some(uuid::Uuid::new_v4()),
            ..RequestPromptLog::default()
        },
    );

    let selected = select_route_for_candidate(
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
    let expected = rendezvous_target(&candidate, Some(&format!("conversation:{conversation_id}")))
        .expect("candidate should have a target")
        .endpoint_id;

    assert_eq!(selected.route.route_id, expected);
}

#[tokio::test]
async fn rebinds_when_the_bound_endpoint_leaves_the_route() {
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
    let replacement = changed_candidate
        .targets
        .first()
        .expect("route should keep one replacement target")
        .endpoint_id;

    let selected = select_route_for_candidate(
        &services,
        &request_ctx,
        &changed_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("stale affinity should be rebound");

    assert_eq!(selected.route.route_id, replacement);
}
