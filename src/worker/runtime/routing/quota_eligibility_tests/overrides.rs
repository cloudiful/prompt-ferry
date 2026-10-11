use super::super::{
    candidate_quota_selection, key_pool,
    session_affinity_quota::{binding_for_selection, quota_failover_selection},
    session_quota_selection,
};
use super::*;
use crate::worker::runtime::prompt_log::RequestPromptLog;

#[tokio::test]
async fn blocked_override_and_preference_use_alternate_without_secret_bypass() {
    let cache = TokenPlanQuotaCache::default();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let dead = Uuid::new_v4();
    let live = Uuid::new_v4();
    let candidate = candidate(vec![
        target(a, 0, false, vec![endpoint_key(a, dead, "dead", 0)]),
        target(b, 1, false, vec![endpoint_key(b, live, "live", 0)]),
    ]);
    cache
        .store_for_test(a, opencode_go_usage(&[(dead, "dead", 0.0)]))
        .await;
    let selected = candidate_quota_selection::select(
        &candidate,
        Some(&cache),
        text(),
        "client",
        Some(a),
        Some(dead),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.target.endpoint_id, b);
    let log = RequestPromptLog {
        preferred_endpoint_id: Some(a),
        ..Default::default()
    };
    let selected = session_quota_selection::select(
        &candidate,
        &log,
        "session",
        &cache,
        text(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(selected.target.endpoint_id, b);
}

#[tokio::test]
async fn primary_policy_and_independent_explicit_override_are_preserved() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let candidate = candidate(vec![target(
        endpoint,
        0,
        false,
        vec![
            endpoint_key(endpoint, a, "primary", 0),
            endpoint_key(endpoint, b, "secondary", 1),
        ],
    )]);
    cache
        .store_for_test(
            endpoint,
            opencode_go_usage(&[(a, "primary", 0.0), (b, "secondary", 100.0)]),
        )
        .await;
    assert!(
        candidate_quota_selection::select(&candidate, Some(&cache), text(), "client", None, None)
            .is_err()
    );
    let selected = candidate_quota_selection::select(
        &candidate,
        Some(&cache),
        text(),
        "client",
        Some(endpoint),
        Some(b),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.key.key_id, Some(b));
    let binding = binding_for_selection(
        &candidate.targets[0],
        &key_pool::selection_from_key(&candidate.targets[0].api_keys[0]),
    );
    let log = RequestPromptLog {
        request_previous_response_id: Some("previous".into()),
        ..Default::default()
    };
    let (selected, _) = quota_failover_selection(
        &candidate,
        &binding,
        &request("client"),
        &log,
        &cache,
        "client",
        1,
    )
    .unwrap();
    assert_eq!(selected.key_selection.key_id, Some(b));
    assert_eq!(selected.target.endpoint_id, endpoint);
}

#[tokio::test]
async fn synthetic_secret_scope_is_not_a_quota_escape() {
    let cache = TokenPlanQuotaCache::default();
    let endpoint = Uuid::new_v4();
    let mut synthetic = target(endpoint, 0, false, Vec::new());
    synthetic.api_key = "fixture-key".into();
    cache
        .store_for_test(
            endpoint,
            opencode_go_usage(&[(Uuid::nil(), "synthetic", 0.0)]),
        )
        .await;
    let pool = key_pool::target_units(&synthetic, Some(&cache), text(), false);
    assert!(pool.checked().is_err());
    assert!(pool.units.is_empty());
}
