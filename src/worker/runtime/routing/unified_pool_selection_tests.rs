//! Unified key-pool routing selection: the global weighted draw distributes by
//! remaining quota, ignores endpoint identity, honors quota urgency, and
//! deflects concurrent bursts. Shared fixtures live in `unified_pool_fixtures`.

use super::super::tests::session_affinity_services;
use super::unified_pool_fixtures::{
    candidate, endpoint_key, opencode_go_urgent_key, opencode_go_urgent_usage, opencode_go_usage,
    route_selected_endpoint, target,
};
use crate::replay_cache::ReplayCache;

#[tokio::test]
async fn pool_distributes_globally_by_remaining_quota() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state, replay_cache);
    let endpoint_a = uuid::Uuid::new_v4();
    let endpoint_b = uuid::Uuid::new_v4();
    let heavy_key = uuid::Uuid::new_v4();
    let dead_key = uuid::Uuid::new_v4();
    let light_key = uuid::Uuid::new_v4();
    let candidate = candidate(vec![
        target(
            endpoint_a,
            0,
            true,
            vec![
                endpoint_key(endpoint_a, dead_key, "dead", 0),
                endpoint_key(endpoint_a, heavy_key, "heavy", 1),
            ],
        ),
        target(
            endpoint_b,
            1,
            true,
            vec![endpoint_key(endpoint_b, light_key, "light", 0)],
        ),
    ]);
    let quota = &services
        .admin_state()
        .expect("admin state")
        .token_plan_quota;
    quota
        .store_for_test(
            endpoint_a,
            opencode_go_usage(&[(dead_key, "dead", 0.0), (heavy_key, "heavy", 90.0)]),
        )
        .await;
    quota
        .store_for_test(endpoint_b, opencode_go_usage(&[(light_key, "light", 10.0)]))
        .await;

    let mut endpoint_a_hits = 0;
    let mut endpoint_b_hits = 0;
    let mut dead_selected = false;
    for value in 0..200_u128 {
        let client_key = format!("client-{value}");
        let (endpoint_id, key_id, secret) =
            route_selected_endpoint(&services, &candidate, &client_key).await;
        if key_id == Some(dead_key) || secret == "dead-key" {
            dead_selected = true;
        }
        if endpoint_id == endpoint_a {
            endpoint_a_hits += 1;
        } else {
            endpoint_b_hits += 1;
        }
    }
    assert!(!dead_selected, "a quota-exhausted unit must never be drawn");
    assert!(endpoint_a_hits > 0, "endpoint A must receive traffic");
    assert!(endpoint_b_hits > 0, "endpoint B must receive traffic");
    assert!(
        endpoint_a_hits > endpoint_b_hits,
        "90% vs 10% must favor endpoint A (a={endpoint_a_hits}, b={endpoint_b_hits})"
    );
}

#[tokio::test]
async fn draw_does_not_depend_on_endpoint_id() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state, replay_cache);
    let shared_target_a = uuid::Uuid::new_v4();
    let shared_target_b = uuid::Uuid::new_v4();
    let shared_key_a = uuid::Uuid::new_v4();
    let shared_key_b = uuid::Uuid::new_v4();
    let build = |endpoint_a: uuid::Uuid, endpoint_b: uuid::Uuid| {
        let mut candidate = candidate(vec![
            target(
                endpoint_a,
                0,
                true,
                vec![endpoint_key(endpoint_a, shared_key_a, "a", 0)],
            ),
            target(
                endpoint_b,
                1,
                true,
                vec![endpoint_key(endpoint_b, shared_key_b, "b", 0)],
            ),
        ]);
        candidate.targets[0].target_id = shared_target_a;
        candidate.targets[1].target_id = shared_target_b;
        candidate
    };
    let left = build(uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let right = build(uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let quota = &services
        .admin_state()
        .expect("admin state")
        .token_plan_quota;
    let usage = opencode_go_usage(&[(shared_key_a, "a", 100.0), (shared_key_b, "b", 100.0)]);
    for candidate in [&left, &right] {
        for target in &candidate.targets {
            quota
                .store_for_test(target.endpoint_id, usage.clone())
                .await;
        }
    }

    for value in 0..32_u128 {
        let client_key = format!("client-{value}");
        let (_, left_key, _) = route_selected_endpoint(&services, &left, &client_key).await;
        let (_, right_key, _) = route_selected_endpoint(&services, &right, &client_key).await;
        assert_eq!(
            left_key, right_key,
            "the draw must ignore endpoint_id (client {client_key})"
        );
    }
}

#[tokio::test]
async fn urgency_favors_a_bottleneck_that_resets_soon() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state, replay_cache);
    let endpoint_x = uuid::Uuid::new_v4();
    let endpoint_y = uuid::Uuid::new_v4();
    let key_x = uuid::Uuid::new_v4();
    let key_y = uuid::Uuid::new_v4();
    let candidate = candidate(vec![
        target(
            endpoint_x,
            0,
            true,
            vec![endpoint_key(endpoint_x, key_x, "x", 0)],
        ),
        target(
            endpoint_y,
            1,
            true,
            vec![endpoint_key(endpoint_y, key_y, "y", 0)],
        ),
    ]);
    let quota = &services
        .admin_state()
        .expect("admin state")
        .token_plan_quota;
    // X: 3% left on a 5-hour window that resets in 25 minutes, month nearly
    // full. Y: 90% rolling but a weekly window at 6% that resets in 6 days.
    // Raw min favors Y (6 > 3); urgency must favor X.
    quota
        .store_for_test(
            endpoint_x,
            opencode_go_urgent_usage(vec![opencode_go_urgent_key(
                key_x,
                "x",
                Some((97.0, chrono::Duration::minutes(25))),
                None,
                Some((5.0, chrono::Duration::days(29))),
            )]),
        )
        .await;
    quota
        .store_for_test(
            endpoint_y,
            opencode_go_urgent_usage(vec![opencode_go_urgent_key(
                key_y,
                "y",
                Some((10.0, chrono::Duration::hours(4))),
                Some((94.0, chrono::Duration::days(6))),
                None,
            )]),
        )
        .await;

    let mut x_hits = 0;
    let mut y_hits = 0;
    for value in 0..200_u128 {
        let client_key = format!("client-{value}");
        let (endpoint_id, _, _) = route_selected_endpoint(&services, &candidate, &client_key).await;
        if endpoint_id == endpoint_x {
            x_hits += 1;
        } else {
            y_hits += 1;
        }
    }
    assert!(x_hits > 0 && y_hits > 0, "both keys must stay reachable");
    assert!(
        x_hits > y_hits,
        "the fast-resetting bottleneck must win (x={x_hits}, y={y_hits})"
    );
}

#[tokio::test]
async fn reservations_deflect_a_concurrent_burst() {
    let replay_cache = ReplayCache::for_tests();
    let runtime_state = super::super::WorkerRuntimeState::default();
    let services = session_affinity_services(runtime_state, replay_cache);
    let endpoint = uuid::Uuid::new_v4();
    let hot_key = uuid::Uuid::new_v4();
    let cool_key = uuid::Uuid::new_v4();
    let candidate = candidate(vec![target(
        endpoint,
        0,
        true,
        vec![
            endpoint_key(endpoint, hot_key, "hot", 0),
            endpoint_key(endpoint, cool_key, "cool", 1),
        ],
    )]);
    let quota = &services
        .admin_state()
        .expect("admin state")
        .token_plan_quota;
    quota
        .store_for_test(
            endpoint,
            opencode_go_usage(&[(hot_key, "hot", 100.0), (cool_key, "cool", 100.0)]),
        )
        .await;
    // A concurrent burst already in flight on `hot`: those reservations must
    // push the next batch away instead of overshooting the same key.
    for _ in 0..400 {
        quota.reserve_estimated_tokens(endpoint, hot_key, 1_000);
    }

    let mut hot_hits = 0;
    let mut cool_hits = 0;
    for value in 0..60_u128 {
        let client_key = format!("client-{value}");
        let (_, key_id, _) = route_selected_endpoint(&services, &candidate, &client_key).await;
        if key_id == Some(hot_key) {
            hot_hits += 1;
        } else {
            cool_hits += 1;
        }
    }
    assert!(
        cool_hits > hot_hits,
        "reservations must deflect the burst (hot={hot_hits}, cool={cool_hits})"
    );
}
