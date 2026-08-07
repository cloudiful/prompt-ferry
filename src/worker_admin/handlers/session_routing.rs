use super::usage_support::{
    build_session_route_options_response, request_record_not_found, session_affinity_user_ids,
};
use super::*;
use crate::response_affinity::ResponseAffinityStore;

pub(super) async fn usage_event_session_route_options(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(record_id): Path<i64>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    match build_session_route_options_response(&state, record_id, visible_user_id, user.user_id)
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(response) => response,
    }
}

pub(super) async fn usage_event_session_affinity_reset(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(record_id): Path<i64>,
) -> Response {
    let admin = match ensure_admin(&state, &headers).await {
        Ok(admin) => admin,
        Err(response) => return response,
    };
    let locator = match db::get_request_record_route_locator(&state.pool, record_id).await {
        Ok(Some(locator)) => locator,
        Ok(None) => return request_record_not_found(),
        Err(err) => return internal(&state, err),
    };
    let Some(conversation_id) = locator.conversation_id else {
        return error(
            StatusCode::BAD_REQUEST,
            "no_conversation_id",
            "request record has no conversation_id",
        );
    };
    let route_user_id = locator.user_id.unwrap_or_default();
    let route_user_ids = session_affinity_user_ids(locator.user_id, admin.user_id);
    let candidate = match db::resolve_model_route_with_fallback(
        &state.pool,
        route_user_id,
        locator.model.as_deref(),
        true,
    )
    .await
    {
        Ok((_, candidate)) => candidate,
        Err(err) => return internal(&state, err),
    };
    let mut rule_ids = Vec::with_capacity(2);
    if let Some(rule_id) = locator.model_route_rule_id {
        rule_ids.push(rule_id);
    }
    if let Some(candidate) = candidate {
        rule_ids.push(candidate.rule_id);
    }
    rule_ids.sort();
    rule_ids.dedup();

    let stable_identity = format!("conversation:{conversation_id}");
    let store = state.replay_cache.response_affinity();
    let mut cleared_count = 0u32;
    for route_user_id in &route_user_ids {
        for rule_id in &rule_ids {
            let cache_key =
                ResponseAffinityStore::cache_key(*route_user_id, *rule_id, &stable_identity);
            match store.delete(&cache_key).await {
                Ok(true) => cleared_count += 1,
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "failed to reset session affinity binding");
                    return error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "responses_session_affinity_unavailable",
                        "response affinity backend is unavailable",
                    );
                }
            }
        }
    }
    tracing::info!(
        record_id,
        conversation_id = %conversation_id,
        user_id = route_user_id,
        attempted_rule_count = rule_ids.len(),
        cleared_count,
        "reset session affinity binding"
    );
    Json(SessionAffinityResetResponse {
        cleared: cleared_count > 0,
        cleared_count,
    })
    .into_response()
}

pub(super) async fn get_conversation_endpoint_override(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::get_conversation_endpoint_override(&state.pool, conversation_id).await {
        Ok(Some(override_entry)) => Json(override_entry).into_response(),
        Ok(None) => error(
            StatusCode::NOT_FOUND,
            "not_found",
            "conversation override not found",
        ),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn set_conversation_endpoint_override(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<ConversationEndpointOverrideRequest>,
) -> Response {
    let user = match ensure_admin(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    match db::get_endpoint(&state.pool, body.endpoint_id).await {
        Ok(Some(endpoint)) => {
            if let Some(endpoint_key_id) = body.endpoint_key_id
                && !endpoint.api_keys.iter().any(|key| {
                    !key.key_id.is_nil()
                        && key.key_id == endpoint_key_id
                        && key.endpoint_id == body.endpoint_id
                        && key.enabled
                })
            {
                return error(
                    StatusCode::BAD_REQUEST,
                    "invalid_endpoint_key",
                    "endpoint key not found, disabled, or does not belong to endpoint",
                );
            }
        }
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_endpoint",
                "endpoint not found",
            );
        }
        Err(err) => return internal(&state, err),
    }
    match db::upsert_conversation_endpoint_override(
        &state.pool,
        conversation_id,
        body.endpoint_id,
        body.endpoint_key_id,
        user.user_id,
    )
    .await
    {
        Ok(override_entry) => Json(override_entry).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_conversation_endpoint_override(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(conversation_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::delete_conversation_endpoint_override(&state.pool, conversation_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "not_found",
            "conversation override not found",
        ),
        Err(err) => internal(&state, err),
    }
}
