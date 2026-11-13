use super::*;
use crate::{
    db::quota_inference::{self as evidence_db, QuotaInferenceSnapshot, SnapshotEvidence},
    worker_admin::{
        quota_inference::{compute, cycle, response::InferenceResponse},
        quota_inference_cache::{AccountIdentity, Identity},
    },
};
use chrono::{DateTime, Duration as ChronoDuration};

pub(super) async fn quota_inference(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let _guard = state.quota_inference.lock(endpoint_id).await;
    let endpoint = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    if endpoint.provider != db::EndpointProvider::OpenAi
        || endpoint.plan != db::EndpointPlan::ChatgptSubscription
    {
        state.quota_inference.invalidate(endpoint_id).await;
        return error(
            StatusCode::BAD_REQUEST,
            "unsupported_provider",
            "quota inference is only available for ChatGPT subscription endpoints",
        );
    }
    let token = match state
        .config_repository
        .get_endpoint_oauth_token(endpoint_id)
        .await
    {
        Ok(Some(token)) => token,
        Ok(None) => {
            state.quota_inference.invalidate(endpoint_id).await;
            return error(
                StatusCode::BAD_REQUEST,
                "oauth_login_required",
                "complete the ChatGPT OAuth login for this endpoint first",
            );
        }
        Err(err) => return internal(&state, err),
    };
    let account = chatgpt_backend::codex_account_id_from_access_token(&token.access_token);
    if !crate::worker_admin_state::quota_inference_available(
        endpoint.provider,
        endpoint.plan,
        state.chatgpt_quota_service.is_some(),
    ) {
        let evidence = SnapshotEvidence {
            snapshots: Vec::new(),
            overflow: false,
            observation_provenance: "success_only",
            account_history_assumed_stable: true,
            refresh_state: None,
        };
        let mut result = compute::compute(&evidence, None, [&[], &[], &[]], Utc::now());
        result.account = crate::worker_admin::quota_inference::response::Estimate::unavailable(
            "storage_unavailable",
        );
        result.account_identity_known = account.is_some();
        return Json(result).into_response();
    }
    let account_identity = AccountIdentity::from_credentials(
        account.as_deref(),
        &token.access_token,
        &token.refresh_token,
    );
    let retention_days = state
        .usage_retention
        .read()
        .await
        .metadata_retention_days
        .max(1);
    let now = Utc::now();
    let lower = now - ChronoDuration::days(i64::from(retention_days).min(8));
    let latest = match db::quota_snapshots::latest_snapshot(&state.pool, endpoint_id).await {
        Ok(latest) => latest.filter(|s| s.observed_at >= lower && s.observed_at <= now),
        Err(err) => return internal(&state, err),
    };
    let latest_at = latest.as_ref().map(|s| s.observed_at);
    let refresh = match db::quota_snapshots::get_refresh_state(&state.pool, endpoint_id).await {
        Ok(refresh) => refresh,
        Err(err) => return internal(&state, err),
    };
    let latest_evidence = SnapshotEvidence {
        snapshots: latest
            .into_iter()
            .map(|s| QuotaInferenceSnapshot {
                observed_at: s.observed_at,
                plan_type: s.plan_type,
                limit_reached: s.limit_reached,
                windows: s.windows,
            })
            .collect(),
        overflow: false,
        observation_provenance: "success_only",
        account_history_assumed_stable: true,
        refresh_state: refresh,
    };
    let identity = cycle::build(&latest_evidence, lower).map(|cycle| Identity {
        account: account_identity.clone(),
        plan: cycle.plan,
        reset: cycle.reset,
        observed_at: cycle.end,
        remaining: cycle.last_remaining,
        retention_days,
    });
    if let Some(identity) = &identity {
        if let Some(mut result) = state.quota_inference.get(endpoint_id, identity).await
            && result.span_start.is_none_or(|start| start >= lower)
        {
            let unchanged = if let Some(as_of) = result.as_of {
                if Some(as_of) == latest_at {
                    true
                } else {
                    match evidence_db::snapshots(&state.pool, endpoint_id, as_of, now).await {
                        Ok(recent) => {
                            recent
                                .snapshots
                                .first()
                                .is_some_and(|s| s.observed_at == as_of)
                                && cycle::build(&recent, lower).is_some_and(|c| c.start <= as_of)
                        }
                        Err(err) => return internal(&state, err),
                    }
                }
            } else {
                false
            };
            if unchanged {
                update_freshness(
                    &mut result,
                    &latest_evidence,
                    latest_at,
                    now,
                    account.is_some(),
                );
                return Json(result).into_response();
            }
        }
    } else {
        state.quota_inference.invalidate(endpoint_id).await;
    }
    let evidence = match evidence_db::snapshots(&state.pool, endpoint_id, lower, now).await {
        Ok(evidence) => evidence,
        Err(err) => return internal(&state, err),
    };
    let cycle = cycle::build(&evidence, lower);
    let mut sets = [Vec::new(), Vec::new(), Vec::new()];
    if let Some(cycle) = cycle.as_ref().filter(|cycle| cycle.start < cycle.end) {
        for (index, shift) in [0, -60, 60].into_iter().enumerate() {
            let shift = ChronoDuration::seconds(shift);
            let mut starts = vec![cycle.start];
            let mut ends = vec![cycle.end];
            starts.extend(cycle.blocks.iter().map(|block| block.start + shift));
            ends.extend(cycle.blocks.iter().map(|block| block.end + shift));
            if starts.iter().any(|at| *at < lower) || ends.iter().any(|at| *at > now) {
                continue;
            }
            match evidence_db::aggregate(&state.pool, endpoint_id, lower, now, &starts, &ends).await
            {
                Ok(rows) => sets[index] = rows,
                Err(err) => return internal(&state, err),
            }
        }
    }
    let mut result = compute::compute(
        &evidence,
        cycle.as_ref(),
        [&sets[0], &sets[1], &sets[2]],
        now,
    );
    let observed = evidence.snapshots.last().map(|s| s.observed_at);
    update_freshness(&mut result, &evidence, observed, now, account.is_some());
    if let Some(cycle) = cycle {
        let identity = Identity {
            account: account_identity,
            plan: cycle.plan,
            reset: cycle.reset,
            observed_at: cycle.end,
            remaining: cycle.last_remaining,
            retention_days,
        };
        state
            .quota_inference
            .store(endpoint_id, identity, result.clone())
            .await;
    }
    Json(result).into_response()
}

fn update_freshness(
    result: &mut InferenceResponse,
    evidence: &SnapshotEvidence,
    latest: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    account_known: bool,
) {
    result.latest_snapshot_at = latest;
    result.account_identity_known = account_known;
    result.current_refresh_failed = evidence
        .refresh_state
        .as_ref()
        .is_some_and(|state| state.consecutive_failures > 0 && state.last_attempt_at > latest);
    let stale = result.current_refresh_failed
        || result
            .as_of
            .is_none_or(|at| now - at > ChronoDuration::minutes(10));
    result.status = if stale {
        "stale"
    } else if result.account.reason.is_none() {
        "available"
    } else {
        "insufficient_signal"
    };
}
