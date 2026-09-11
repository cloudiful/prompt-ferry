//! Unified key-pool routing failover: an endpoint is skipped only when all of
//! its units are exhausted, and an all-exhausted pool still resolves a route.
//! Shared fixtures live in `unified_pool_fixtures`.

use super::super::tests::session_affinity_services;
use super::unified_pool_fixtures::{
    candidate, endpoint_key, opencode_go_usage, route_selected_endpoint, target,
};
use crate::replay_cache::ReplayCache;

#[tokio::test]
async fn pool_falls_over_to_another_endpoint_only_when_all_its_units_are_dead() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state, replay_cache);
    let endpoint_a = uuid::Uuid::new_v4();
    let endpoint_b = uuid::Uuid::new_v4();
    let dead_a1 = uuid::Uuid::new_v4();
    let dead_a2 = uuid::Uuid::new_v4();
    let live_b = uuid::Uuid::new_v4();
    let candidate = candidate(vec![
        target(
            endpoint_a,
            0,
            true,
            vec![
                endpoint_key(endpoint_a, dead_a1, "dead-a1", 0),
                endpoint_key(endpoint_a, dead_a2, "dead-a2", 1),
            ],
        ),
        target(
            endpoint_b,
            1,
            true,
            vec![endpoint_key(endpoint_b, live_b, "live-b", 0)],
        ),
    ]);
    let quota = &services
        .admin_state()
        .expect("admin state")
        .token_plan_quota;
    quota
        .store_for_test(
            endpoint_a,
            opencode_go_usage(&[(dead_a1, "dead-a1", 0.0), (dead_a2, "dead-a2", 0.0)]),
        )
        .await;
    quota
        .store_for_test(endpoint_b, opencode_go_usage(&[(live_b, "live-b", 100.0)]))
        .await;

    for value in 0..40_u128 {
        let client_key = format!("client-{value}");
        let (endpoint_id, key_id, _) =
            route_selected_endpoint(&services, &candidate, &client_key).await;
        assert_eq!(
            endpoint_id, endpoint_b,
            "all-dead endpoint A must be skipped for endpoint B"
        );
        assert_eq!(key_id, Some(live_b));
    }
}

#[tokio::test]
async fn pool_with_every_unit_dead_still_resolves_a_route() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state, replay_cache);
    let endpoint_a = uuid::Uuid::new_v4();
    let endpoint_b = uuid::Uuid::new_v4();
    let dead_a = uuid::Uuid::new_v4();
    let dead_b = uuid::Uuid::new_v4();
    let candidate = candidate(vec![
        target(
            endpoint_a,
            0,
            true,
            vec![endpoint_key(endpoint_a, dead_a, "dead-a", 0)],
        ),
        target(
            endpoint_b,
            1,
            true,
            vec![endpoint_key(endpoint_b, dead_b, "dead-b", 0)],
        ),
    ]);
    let quota = &services
        .admin_state()
        .expect("admin state")
        .token_plan_quota;
    quota
        .store_for_test(endpoint_a, opencode_go_usage(&[(dead_a, "dead-a", 0.0)]))
        .await;
    quota
        .store_for_test(endpoint_b, opencode_go_usage(&[(dead_b, "dead-b", 0.0)]))
        .await;

    let (endpoint_id, _, _) = route_selected_endpoint(&services, &candidate, "client-1").await;
    assert!(
        endpoint_id == endpoint_a || endpoint_id == endpoint_b,
        "an all-exhausted pool must still resolve a route"
    );
}
