use std::time::Instant;

use chrono::Utc;

use super::super::{
    RequestExecutionContext, WorkerRuntimeState,
    context::RequestExecutionContextParams,
    prompt_log::RequestPromptLog,
    tests::{
        querying_test_services, sample_request, session_affinity_candidate,
        session_affinity_services,
    },
};
use super::{RouteAffinityError, select_route_for_candidate};
use crate::{db, replay_cache::ReplayCache};

pub(super) fn request_context(
    worker_id: uuid::Uuid,
    prompt_log: RequestPromptLog,
) -> RequestExecutionContext {
    RequestExecutionContext::new(RequestExecutionContextParams {
        request_id: uuid::Uuid::new_v4(),
        started: Instant::now(),
        request_model: Some("gpt-4.1-mini".to_string()),
        client_key_id: None,
        client_key_label: None,
        user_id: Some(1),
        owner_worker_id: worker_id,
        request_prompt_log: prompt_log,
    })
}

#[tokio::test]
async fn rejects_unavailable_shared_backend() {
    let runtime_state = WorkerRuntimeState::default();
    // The affinity store is unavailable here, so selection fails before any
    // quota refresh: this test issues no query and keeps the lazy fixture.
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
    let services_a = querying_test_services(runtime_state_a.clone(), replay_cache.clone()).await;
    let services_b = querying_test_services(runtime_state_b.clone(), replay_cache).await;
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

    let stale_endpoint_id = left.route.route_id;
    let other_endpoint_id = rotated_candidate
        .targets
        .iter()
        .find(|target| target.endpoint_id != stale_endpoint_id)
        .expect("candidate should have another endpoint")
        .endpoint_id;
    let bound_target = rotated_candidate
        .targets
        .iter_mut()
        .find(|target| target.endpoint_id == stale_endpoint_id)
        .expect("bound target exists");
    bound_target.api_keys[0].enabled = false;
    // Clear the legacy secret as well: otherwise the stale endpoint would
    // linger in the rebuild pool via the secret fallback and the draw could
    // stay on it (see key_pool::target_units_mode). Clearing mirrors the
    // key-deletion path covered by auto_rebind_after_stale_key.
    bound_target.api_key.clear();
    // Issue #444: disabled bound key is StaleKey and must auto-rebind to the
    // remaining healthy endpoint instead of reporting target_unavailable.
    let rebound = select_route_for_candidate(
        &services_b,
        &request_ctx_b,
        &rotated_candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .expect("disabled bound key must auto-rebind");
    assert_eq!(rebound.route.route_id, other_endpoint_id);
}

#[tokio::test]
async fn rejects_endpoint_override_conflicts() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = querying_test_services(runtime_state.clone(), replay_cache).await;
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
    let conflict = match select_route_for_candidate(
        &services,
        &override_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("endpoint override must not replace an existing binding"),
        Err(error) => error,
    };
    assert_eq!(
        conflict
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_conflict")
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
    .expect("original binding should stay usable after a rejected override");
    assert_eq!(follow_up.route.route_id, first.route.route_id);
}

#[tokio::test]
async fn rejects_key_override_conflicts() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = querying_test_services(runtime_state.clone(), replay_cache).await;
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
    let conflict = match select_route_for_candidate(
        &services,
        &key_override_context,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("key override must not replace an existing binding"),
        Err(error) => error,
    };
    assert_eq!(
        conflict
            .downcast_ref::<RouteAffinityError>()
            .map(|error| error.code),
        Some("responses_session_affinity_conflict")
    );
}

#[tokio::test]
async fn rejects_endpoint_and_key_override_conflicts() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = querying_test_services(runtime_state.clone(), replay_cache).await;
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
        Ok(_) => panic!("endpoint override must not replace an existing binding"),
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
        Ok(_) => panic!("key override must not replace an existing binding"),
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
async fn credential_lifecycle_emits_only_genuine_stale_rebind() {
    use crate::response_affinity::{
        ResponseAffinityBinding, ResponseAffinityStore, api_key_fingerprint,
    };
    use std::sync::{Arc, Mutex};
    use tracing::{
        Event, Subscriber,
        field::{Field, Visit},
    };
    use tracing_subscriber::{
        Layer,
        layer::{Context, SubscriberExt},
    };

    #[derive(Default)]
    struct Fields {
        event: Option<String>,
        state: Option<String>,
    }
    impl Visit for Fields {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "event" {
                self.event = Some(value.to_owned());
            }
        }
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "binding_state" {
                self.state = Some(format!("{value:?}"));
            }
        }
    }
    struct Capture(Arc<Mutex<Vec<String>>>);
    impl<S: Subscriber> Layer<S> for Capture {
        fn register_callsite(
            &self,
            _: &'static tracing::Metadata<'static>,
        ) -> tracing::subscriber::Interest {
            tracing::subscriber::Interest::sometimes()
        }
        fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
            let mut fields = Fields::default();
            event.record(&mut fields);
            if fields.event.as_deref() == Some("session_affinity_auto_rebind") {
                self.0
                    .lock()
                    .unwrap()
                    .push(fields.state.unwrap_or_default());
            }
        }
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(Capture(events.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);
    tracing::callsite::rebuild_interest_cache();
    {
        let replay = ReplayCache::for_tests();
        let runtime = WorkerRuntimeState::default();
        let services = querying_test_services(runtime.clone(), replay.clone()).await;
        let store = replay.response_affinity();
        let mut candidate = session_affinity_candidate();
        candidate.targets.truncate(1);
        candidate.targets[0].has_oauth_token = true;
        let conversation = uuid::Uuid::new_v4();
        let key = ResponseAffinityStore::cache_key(
            1,
            candidate.rule_id,
            &format!("conversation:{conversation}"),
        );
        let ordinary = request_context(
            runtime.worker_instance_id(),
            RequestPromptLog {
                conversation_id: Some(conversation),
                ..Default::default()
            },
        );
        let chained = request_context(
            runtime.worker_instance_id(),
            RequestPromptLog {
                conversation_id: Some(conversation),
                request_previous_response_id: Some("resp_previous".into()),
                ..Default::default()
            },
        );
        select_route_for_candidate(
            &services,
            &ordinary,
            &candidate,
            &sample_request(),
            1,
            Some("key-a"),
        )
        .await
        .unwrap()
        .unwrap();
        let original = store.get(&key).await.unwrap().unwrap();
        let mut refreshed = candidate.clone();
        refreshed.targets[0].api_key = "different-unused-endpoint".into();
        refreshed.targets[0].api_keys[0].api_key = "different-unused-row".into();
        refreshed.targets[0].api_keys[0].key_id = uuid::Uuid::new_v4();
        let chosen = select_route_for_candidate(
            &services,
            &chained,
            &refreshed,
            &sample_request(),
            1,
            Some("key-a"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(chosen.route.endpoint_key_id, None);
        assert!(chosen.route.api_key.is_empty());
        assert_eq!(store.get(&key).await.unwrap(), Some(original));
        assert!(events.lock().unwrap().is_empty());

        let mut api = session_affinity_candidate();
        api.targets.truncate(1);
        let conversation = uuid::Uuid::new_v4();
        let key = ResponseAffinityStore::cache_key(
            1,
            api.rule_id,
            &format!("conversation:{conversation}"),
        );
        let ctx = request_context(
            runtime.worker_instance_id(),
            RequestPromptLog {
                conversation_id: Some(conversation),
                ..Default::default()
            },
        );
        select_route_for_candidate(&services, &ctx, &api, &sample_request(), 1, Some("key-a"))
            .await
            .unwrap()
            .unwrap();
        let initial = store.get(&key).await.unwrap().unwrap();
        api.targets[0].api_key = "rotated-secret".into();
        api.targets[0].api_keys[0].api_key = "rotated-secret".into();
        select_route_for_candidate(&services, &ctx, &api, &sample_request(), 1, Some("key-a"))
            .await
            .unwrap()
            .unwrap();
        let rotated = store.get(&key).await.unwrap().unwrap();
        assert_eq!(rotated.endpoint_key_id, initial.endpoint_key_id);
        assert_eq!(
            rotated.endpoint_key_fingerprint,
            api_key_fingerprint("rotated-secret")
        );
        let replacement_id = uuid::Uuid::new_v4();
        api.targets[0].api_keys[0].key_id = replacement_id;
        select_route_for_candidate(&services, &ctx, &api, &sample_request(), 1, Some("key-a"))
            .await
            .unwrap()
            .unwrap();
        let healed = store.get(&key).await.unwrap().unwrap();
        assert_eq!(healed.endpoint_key_id, Some(replacement_id));
        assert_eq!(
            healed.endpoint_key_fingerprint,
            rotated.endpoint_key_fingerprint
        );
        assert!(events.lock().unwrap().is_empty());

        let conversation = uuid::Uuid::new_v4();
        let key = ResponseAffinityStore::cache_key(
            1,
            candidate.rule_id,
            &format!("conversation:{conversation}"),
        );
        let stale = ResponseAffinityBinding {
            endpoint_id: candidate.targets[0].endpoint_id,
            endpoint_key_id: None,
            endpoint_key_fingerprint: api_key_fingerprint("foreign"),
        };
        store.get_or_create(&key, &stale).await.unwrap();
        let ctx = request_context(
            runtime.worker_instance_id(),
            RequestPromptLog {
                conversation_id: Some(conversation),
                ..Default::default()
            },
        );
        tracing::callsite::rebuild_interest_cache();
        for _ in 0..2 {
            select_route_for_candidate(
                &services,
                &ctx,
                &candidate,
                &sample_request(),
                1,
                Some("key-a"),
            )
            .await
            .unwrap()
            .unwrap();
        }
        assert_eq!(
            store
                .get(&key)
                .await
                .unwrap()
                .unwrap()
                .endpoint_key_fingerprint,
            api_key_fingerprint("")
        );
        assert_eq!(*events.lock().unwrap(), vec!["StaleKey"]);
    }
}

#[tokio::test]
async fn subscription_requested_platform_key_conflicts_without_using_the_key() {
    let replay_cache = ReplayCache::for_tests();
    let runtime = WorkerRuntimeState::default();
    let services = querying_test_services(runtime.clone(), replay_cache).await;
    let mut candidate = session_affinity_candidate();
    candidate.targets.truncate(1);
    candidate.targets[0].has_oauth_token = true;
    let ctx = request_context(
        runtime.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(uuid::Uuid::new_v4()),
            conversation_override_endpoint_id: Some(candidate.targets[0].endpoint_id),
            conversation_override_endpoint_key_id: Some(candidate.targets[0].api_keys[0].key_id),
            ..Default::default()
        },
    );
    let error = match select_route_for_candidate(
        &services,
        &ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    {
        Ok(_) => panic!("subscription must not accept a platform key override"),
        Err(error) => error,
    };
    assert_eq!(
        error.downcast_ref::<RouteAffinityError>().unwrap().code,
        "responses_session_affinity_conflict"
    );
}

#[tokio::test]
async fn subscription_binding_reused_for_chained_request_with_unused_keys() {
    use crate::response_affinity::ResponseAffinityStore;
    let replay_cache = ReplayCache::for_tests();
    let runtime = WorkerRuntimeState::default();
    let services = querying_test_services(runtime.clone(), replay_cache.clone()).await;
    let mut candidate = session_affinity_candidate();
    candidate.targets.truncate(1);
    candidate.targets[0].has_oauth_token = true;
    let conversation_id = uuid::Uuid::new_v4();
    let cache_key = ResponseAffinityStore::cache_key(
        1,
        candidate.rule_id,
        &format!("conversation:{conversation_id}"),
    );
    let first_ctx = request_context(
        runtime.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(1),
            ..Default::default()
        },
    );
    let chained_ctx = request_context(
        runtime.worker_instance_id(),
        RequestPromptLog {
            conversation_id: Some(conversation_id),
            conversation_seq: Some(2),
            request_previous_response_id: Some("resp_previous".to_string()),
            ..Default::default()
        },
    );
    let store = replay_cache.response_affinity();
    let first = select_route_for_candidate(
        &services,
        &first_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .unwrap();
    let binding = store.get(&cache_key).await.unwrap().unwrap();
    let second = select_route_for_candidate(
        &services,
        &chained_ctx,
        &candidate,
        &sample_request(),
        1,
        Some("key-a"),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(first.route.route_id, second.route.route_id);
    assert_eq!(second.route.endpoint_key_id, None);
    assert!(second.route.api_key.is_empty());
    assert_eq!(store.get(&cache_key).await.unwrap(), Some(binding));
}

#[tokio::test]
async fn heals_stale_key_id_when_secret_fingerprint_matches() {
    use crate::response_affinity::{ResponseAffinityStore, api_key_fingerprint};

    let replay_cache = ReplayCache::for_tests();
    let runtime_state = WorkerRuntimeState::default();
    let services = querying_test_services(runtime_state.clone(), replay_cache.clone()).await;
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
