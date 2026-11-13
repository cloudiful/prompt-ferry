use super::super::{
    prompt_log::RequestPromptLog, request_assembly::BufferedBridgeRequest,
    tests::session_affinity_candidate,
};
use super::{session_affinity_quota::quota_failover_selection, test_support::window};
use crate::{
    db,
    response_affinity::{ResponseAffinityBinding, api_key_fingerprint},
    worker_admin::token_plan_cache::TokenPlanQuotaCache,
    worker_admin_types::{
        TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse, TokenPlanWindowUsage,
    },
};
use chrono::Utc;

#[tokio::test]
async fn subscription_initial_and_bound_selection_share_endpoint_quota() {
    use super::{
        key_pool,
        quota_eligibility::QuotaContext,
        session_affinity_quota::{BindingSelection, selection_for_binding},
    };
    use crate::worker_admin::chatgpt_quota_service::ChatGptQuotaRoutingSnapshot;
    let mut candidate = session_affinity_candidate();
    candidate.targets.truncate(1);
    candidate.targets[0].has_oauth_token = true;
    let target = &candidate.targets[0];
    let cache = TokenPlanQuotaCache::default();
    cache
        .store_for_test(
            target.endpoint_id,
            usage_with_keys(&[(uuid::Uuid::nil(), "nil", 0.0)]),
        )
        .await;
    let context = QuotaContext {
        model: Some("gpt-5.4"),
        path: "/v1/responses",
    };
    let binding =
        crate::routing::credential_binding(target, &crate::routing::subscription_selection());
    assert_eq!(
        key_pool::target_units(target, Some(&cache), context, true)
            .units
            .len(),
        1
    );
    assert!(matches!(
        selection_for_binding(&candidate, &binding, &request(), Some(&cache)),
        BindingSelection::Selected(_)
    ));
    cache.store_chatgpt_subscription_snapshot(
        target.endpoint_id,
        Some(ChatGptQuotaRoutingSnapshot {
            observed_at: Utc::now(),
            limit_reached: Some(true),
            windows: vec![],
            stale: false,
        }),
    );
    assert!(
        key_pool::target_units(target, Some(&cache), context, true)
            .units
            .is_empty()
    );
    assert!(matches!(
        selection_for_binding(&candidate, &binding, &request(), Some(&cache)),
        BindingSelection::QuotaExhausted
    ));
    assert!(
        quota_failover_selection(
            &candidate,
            &binding,
            &request(),
            &RequestPromptLog::default(),
            &cache,
            "stable",
            1
        )
        .is_none()
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
        local_today_tokens: None,
        quota_inference_available: None,
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
                quota_status: None,
                model_remains: vec![TokenPlanModelUsage {
                    model_name: "general".to_string(),
                    interval: Some(window(*remaining_percent)),
                    weekly: Some(window(*remaining_percent)),
                    windows: None,
                    observation: None,
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
                quota_windows: None,
            })
            .collect(),
    }
}

// CommandCode session-affinity consumption (issue #184 P5): the bound key
// is gated on the tighter USD window; PAYG keys without windows stay
// honored (no quota signal means no exhaustion).

fn model_key(
    key_id: uuid::Uuid,
    key_label: &str,
    model_remains: Vec<TokenPlanModelUsage>,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label: key_label.to_string(),
        ok: true,
        status: Some(200),
        error_code: None,
        error_message: None,
        quota_status: None,
        model_remains,
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
        quota_windows: None,
    }
}

fn model_window(
    remaining_percent: f64,
    total_count: Option<i64>,
    resets_in_ms: Option<i64>,
) -> TokenPlanModelUsage {
    TokenPlanModelUsage {
        model_name: "general".to_string(),
        interval: Some(TokenPlanWindowUsage {
            status: Some(1),
            remaining_percent: Some(remaining_percent),
            total_count,
            usage_count: None,
            boost_permille: None,
            start_at: None,
            end_at: None,
            remains_time_ms: resets_in_ms,
        }),
        weekly: None,
        windows: None,
        observation: None,
    }
}

fn model_usage(keys: Vec<TokenPlanKeyUsage>) -> TokenPlanUsageResponse {
    TokenPlanUsageResponse {
        local_today_tokens: None,
        quota_inference_available: None,
        provider: db::EndpointProvider::Minimax,
        provider_region: Some(db::EndpointRegion::Cn),
        keys,
    }
}

/// Regression (issue #310 Task 1): a legacy binding without `endpoint_key_id`
/// only carries the endpoint and a key fingerprint. The exhausted key must be
/// resolved back through that fingerprint and excluded from the failover
/// redraw, even when its quota filter still keeps it eligible because the
/// urgency-lifted pool weight stays positive while the raw remaining bottoms
/// out under outstanding reservations.
#[tokio::test]
async fn none_binding_fingerprint_migrates_off_exhausted_key() {
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
    // Keep the redraw on the bound endpoint so a healthy alternate key there
    // is the only correct migration.
    candidate.targets[1].enabled = false;

    let quota = TokenPlanQuotaCache::default();
    quota
        .store_for_test(
            endpoint_id,
            model_usage(vec![
                model_key(
                    bound_key_id,
                    "primary",
                    vec![model_window(10.0, Some(1_000), Some(60_000))],
                ),
                // Negligible but strictly positive weight: without the bound
                // unit exclusion the dominant primary would win the draw.
                model_key(
                    alternate_key_id,
                    "alternate",
                    vec![model_window(0.0001, None, None)],
                ),
            ]),
        )
        .await;
    // 100 reserved tokens == 10% of the 1000-token window, so the bound key's
    // raw remaining is 0 (quota-exhausted) while its 1-minute reset keeps the
    // urgency-lifted weight near 90.
    quota.reserve_estimated_tokens(endpoint_id, bound_key_id, 100);

    let binding = ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: api_key_fingerprint("key-a"),
        target: None,
    };
    let (selection, replacement) = quota_failover_selection(
        &candidate,
        &binding,
        &request(),
        &RequestPromptLog::default(),
        &quota,
        "session-stable-key",
        1,
    )
    .expect("exhausted bound key must migrate to the alternate key");
    assert_eq!(selection.key_selection.key_id, Some(alternate_key_id));
    assert_ne!(selection.key_selection.key_id, Some(bound_key_id));
    assert_eq!(
        selection.route_selection_reason,
        db::RouteSelectionReason::QuotaFailover
    );
    assert_eq!(replacement.endpoint_key_id, Some(alternate_key_id));
}

/// A route may use one endpoint for several upstream models. A binding that
/// records the target identity resolves the exact model, while a legacy
/// binding keeps the old first-endpoint match. The identity survives a route
/// save that regenerates the target ids.
#[tokio::test]
async fn a_target_identity_binding_resolves_the_exact_model_of_one_endpoint() {
    use super::session_affinity_quota::{BindingSelection, selection_for_binding};
    use super::unified_pool_fixtures::{candidate, endpoint_key, target};

    let endpoint_id = uuid::Uuid::new_v4();
    let key_id = uuid::Uuid::new_v4();
    let mut first = target(
        endpoint_id,
        0,
        false,
        vec![endpoint_key(endpoint_id, key_id, "key", 0)],
    );
    first.upstream_model = Some("model-a".to_string());
    let mut second = target(
        endpoint_id,
        1,
        false,
        vec![endpoint_key(endpoint_id, key_id, "key", 0)],
    );
    second.upstream_model = Some("model-b".to_string());
    let candidate = candidate(vec![first, second]);
    let cache = TokenPlanQuotaCache::default();

    let binding = ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: Some(key_id),
        endpoint_key_fingerprint: api_key_fingerprint("key-key"),
        target: Some(crate::response_affinity::ResponseAffinityTargetIdentity {
            upstream_model: Some("model-b".to_string()),
        }),
    };
    let BindingSelection::Selected(selection) =
        selection_for_binding(&candidate, &binding, &request(), Some(&cache))
    else {
        panic!("the bound target must stay selectable");
    };
    assert_eq!(selection.target.upstream_model.as_deref(), Some("model-b"));

    // A legacy binding keeps the old endpoint-only resolution.
    let mut legacy = binding.clone();
    legacy.target = None;
    let BindingSelection::Selected(selection) =
        selection_for_binding(&candidate, &legacy, &request(), Some(&cache))
    else {
        panic!("the legacy binding must stay selectable");
    };
    assert_eq!(selection.target.upstream_model.as_deref(), Some("model-a"));

    // A route save regenerates the target ids; the identity still resolves.
    let mut saved = candidate.clone();
    for (index, target) in saved.targets.iter_mut().enumerate() {
        target.target_id = uuid::Uuid::new_v4();
        target.prefer_new_sessions = index == 1;
    }
    let BindingSelection::Selected(selection) =
        selection_for_binding(&saved, &binding, &request(), Some(&cache))
    else {
        panic!("the bound target must stay selectable after a save");
    };
    assert_eq!(selection.target.upstream_model.as_deref(), Some("model-b"));
}

/// A `previous_response_id` continuation may not migrate to another model of
/// the same endpoint. A legacy binding stays endpoint-only.
#[tokio::test]
async fn a_chained_failover_keeps_the_bound_model_and_a_legacy_binding_stays_endpoint_only() {
    use super::unified_pool_fixtures::{candidate, endpoint_key, target};

    let endpoint_id = uuid::Uuid::new_v4();
    let bound_key_id = uuid::Uuid::new_v4();
    let other_key_id = uuid::Uuid::new_v4();
    let mut bound = target(
        endpoint_id,
        0,
        false,
        vec![endpoint_key(endpoint_id, bound_key_id, "bound", 0)],
    );
    bound.upstream_model = Some("model-b".to_string());
    let mut other = target(
        endpoint_id,
        1,
        false,
        vec![endpoint_key(endpoint_id, other_key_id, "other", 0)],
    );
    other.upstream_model = Some("model-a".to_string());
    let candidate = candidate(vec![bound, other]);
    let cache = TokenPlanQuotaCache::default();
    let log = RequestPromptLog {
        request_previous_response_id: Some("previous".into()),
        ..Default::default()
    };

    let binding = ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: Some(bound_key_id),
        endpoint_key_fingerprint: api_key_fingerprint("bound-key"),
        target: Some(crate::response_affinity::ResponseAffinityTargetIdentity {
            upstream_model: Some("model-b".to_string()),
        }),
    };
    assert!(
        quota_failover_selection(
            &candidate,
            &binding,
            &request(),
            &log,
            &cache,
            "session-stable-key",
            1
        )
        .is_none(),
        "a continuation must not migrate to another model of the same endpoint"
    );

    let mut legacy = binding.clone();
    legacy.target = None;
    let (selection, _) = quota_failover_selection(
        &candidate,
        &legacy,
        &request(),
        &log,
        &cache,
        "session-stable-key",
        1,
    )
    .expect("a legacy binding stays pinned to the endpoint");
    assert_eq!(selection.target.endpoint_id, endpoint_id);
    assert_eq!(selection.target.upstream_model.as_deref(), Some("model-a"));
}
