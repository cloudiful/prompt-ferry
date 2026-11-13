use super::super::{
    candidate_quota_selection, key_pool, session_affinity_quota::quota_failover_selection,
    session_quota_selection,
};
use super::*;
use crate::{routing::credential_binding, worker::runtime::prompt_log::RequestPromptLog};

#[tokio::test]
async fn new_session_preference_steers_initial_binding_and_falls_back_when_unavailable() {
    let cache = TokenPlanQuotaCache::default();
    let preferred_endpoint = Uuid::new_v4();
    let fallback_endpoint = Uuid::new_v4();
    let preferred_key = Uuid::new_v4();
    let fallback_key = Uuid::new_v4();
    let mut candidate = candidate(vec![
        target(
            preferred_endpoint,
            0,
            false,
            vec![endpoint_key(
                preferred_endpoint,
                preferred_key,
                "preferred",
                0,
            )],
        ),
        target(
            fallback_endpoint,
            1,
            false,
            vec![endpoint_key(fallback_endpoint, fallback_key, "fallback", 0)],
        ),
    ]);
    candidate.targets[0].prefer_new_sessions = true;

    // A fresh unbound session uses the preferred target and records why.
    let selected = session_quota_selection::select(
        &candidate,
        &RequestPromptLog::default(),
        "session",
        &cache,
        text(),
        &Default::default(),
        session_quota_selection::NewSessionPreferencePermission::InitialBinding,
    )
    .unwrap();
    assert_eq!(selected.target.endpoint_id, preferred_endpoint);
    assert_eq!(
        selected.reason,
        crate::db::RouteSelectionReason::NewSessionPreference
    );

    // An explicit conversation override keeps precedence over the preference.
    let log = RequestPromptLog {
        conversation_override_endpoint_id: Some(fallback_endpoint),
        ..Default::default()
    };
    let selected = session_quota_selection::select(
        &candidate,
        &log,
        "session",
        &cache,
        text(),
        &Default::default(),
        session_quota_selection::NewSessionPreferencePermission::InitialBinding,
    )
    .unwrap();
    assert_eq!(selected.target.endpoint_id, fallback_endpoint);
    assert_eq!(
        selected.reason,
        crate::db::RouteSelectionReason::ConversationOverride
    );

    // An exhausted preferred target contributes no unit: the automatic draw
    // binds normally instead of failing the session.
    cache
        .store_for_test(
            preferred_endpoint,
            opencode_go_usage(&[(preferred_key, "preferred", 0.0)]),
        )
        .await;
    let selected = session_quota_selection::select(
        &candidate,
        &RequestPromptLog::default(),
        "session",
        &cache,
        text(),
        &Default::default(),
        session_quota_selection::NewSessionPreferencePermission::InitialBinding,
    )
    .unwrap();
    assert_eq!(selected.target.endpoint_id, fallback_endpoint);
    assert_eq!(
        selected.reason,
        crate::db::RouteSelectionReason::SessionAffinity
    );
}

/// The new-session preference only steers a session without a binding. A
/// session that already has one — a stale binding that is rebuilt, or an
/// existing binding that is rebound — follows the automatic rules.
#[tokio::test]
async fn an_observed_binding_withdraws_the_new_session_preference() {
    let cache = TokenPlanQuotaCache::default();
    let preferred_endpoint = Uuid::new_v4();
    let fallback_endpoint = Uuid::new_v4();
    let mut candidate = candidate(vec![
        target(
            preferred_endpoint,
            0,
            false,
            vec![endpoint_key(
                preferred_endpoint,
                Uuid::new_v4(),
                "preferred",
                0,
            )],
        ),
        target(
            fallback_endpoint,
            1,
            false,
            vec![endpoint_key(
                fallback_endpoint,
                Uuid::new_v4(),
                "fallback",
                0,
            )],
        ),
    ]);
    candidate.targets[0].prefer_new_sessions = true;

    // Without a binding the preference steers the session.
    let selected = session_quota_selection::select(
        &candidate,
        &RequestPromptLog::default(),
        "session",
        &cache,
        text(),
        &Default::default(),
        session_quota_selection::NewSessionPreferencePermission::InitialBinding,
    )
    .unwrap();
    assert_eq!(selected.target.endpoint_id, preferred_endpoint);
    assert_eq!(
        selected.reason,
        crate::db::RouteSelectionReason::NewSessionPreference
    );

    // An observed binding withdraws the permission, so the automatic rules
    // decide the selection.
    let selected = session_quota_selection::select(
        &candidate,
        &RequestPromptLog::default(),
        "session",
        &cache,
        text(),
        &Default::default(),
        session_quota_selection::NewSessionPreferencePermission::ExistingBinding,
    )
    .unwrap();
    assert_ne!(
        selected.reason,
        crate::db::RouteSelectionReason::NewSessionPreference
    );
    assert_eq!(
        selected.reason,
        crate::db::RouteSelectionReason::SessionAffinity
    );
}

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
        session_quota_selection::NewSessionPreferencePermission::InitialBinding,
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
    let binding = credential_binding(
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
