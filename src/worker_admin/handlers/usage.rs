use super::usage_support::{
    build_request_record_query, build_session_route_options_response, build_usage_clear_query,
    build_usage_request_full_response, get_visible_usage_event_detail_or_not_found,
    parse_overview_window, parse_usage_date_range, parse_usage_series_bucket,
    parse_usage_summary_days,
};
use super::*;

pub(super) async fn bridge_status(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(response) = current_user(&state, &headers).await {
        return response;
    }
    if state.managed_mode {
        let relays = match db::list_managed_relays(&state.pool).await {
            Ok(relays) => relays,
            Err(err) => return internal(&state, err),
        };
        let statuses = state.managed_relay_statuses.read().await.clone();
        let relays = relays
            .into_iter()
            .map(|relay| {
                let runtime = statuses.get(&relay.relay_id).cloned().unwrap_or_default();
                RelayBridgeStatus {
                    relay_id: Some(relay.relay_id),
                    relay_url: relay.relay_url,
                    enabled: relay.enabled,
                    connected: runtime.connected,
                    last_error: runtime.last_error,
                    last_snapshot_version: runtime.last_snapshot_version,
                }
            })
            .collect::<Vec<_>>();
        return Json(BridgeStatus {
            configured_relays: relays.iter().filter(|relay| relay.enabled).count(),
            connected_relays: relays.iter().filter(|relay| relay.connected).count(),
            snapshot_version: state.snapshot_version.load(Ordering::SeqCst),
            relays,
        })
        .into_response();
    }
    let relay_senders = state.relay_senders.lock().await;
    let relays = state
        .configured_relays
        .iter()
        .map(|relay_url| RelayBridgeStatus {
            relay_id: None,
            relay_url: relay_url.clone(),
            enabled: true,
            connected: relay_senders.contains_key(relay_url),
            last_error: None,
            last_snapshot_version: None,
        })
        .collect::<Vec<_>>();
    Json(BridgeStatus {
        configured_relays: relays.len(),
        connected_relays: relays.iter().filter(|relay| relay.connected).count(),
        snapshot_version: state.snapshot_version.load(Ordering::SeqCst),
        relays,
    })
    .into_response()
}

pub(super) async fn usage_summary(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<UsageSummaryQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let days = match parse_usage_summary_days(query.days) {
        Ok(days) => days,
        Err(response) => return *response,
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    match db::request_record_summary(&state.pool, days, visible_user_id).await {
        Ok(summary) => Json(summary).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn usage_overview(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RequestRecordOverviewQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let window = match parse_overview_window(query.range, query.start, query.end) {
        Ok(window) => window,
        Err(response) => return *response,
    };
    match db::request_records_overview(
        &state.pool,
        (!user.is_admin).then_some(user.user_id),
        query
            .request_category
            .unwrap_or(db::RequestRecordCategory::Ai),
        window,
        if user.is_admin {
            query
                .user
                .as_deref()
                .filter(|value| !value.trim().is_empty())
        } else {
            None
        },
    )
    .await
    {
        Ok(overview) => Json(overview).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn usage_events(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<UsageEventsQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let (date_start, date_end) = match query.date.as_deref() {
        Some(value) if !value.is_empty() => match parse_usage_date_range(value) {
            Ok(range) => range,
            Err(response) => return *response,
        },
        _ => (None, None),
    };
    let request_query = build_request_record_query(&user, query, date_start, date_end);
    match db::list_request_records(&state.pool, request_query).await {
        Ok(page) => Json(page).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn usage_facets(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RequestRecordFacetsQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    match db::list_request_record_facets(
        &state.pool,
        (!user.is_admin).then_some(user.user_id),
        query
            .request_category
            .unwrap_or(db::RequestRecordCategory::Ai),
    )
    .await
    {
        Ok(facets) => Json(facets).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn usage_event_detail(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(record_id): Path<i64>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    let mut event =
        match get_visible_usage_event_detail_or_not_found(&state, record_id, visible_user_id).await
        {
            Ok(event) => event,
            Err(response) => return response,
        };
    match db::list_request_record_tool_calls(&state.pool, event.record_id).await {
        Ok(tool_call_events) => {
            event.tool_call_events = tool_call_events;
            Json(event).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

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

pub(super) async fn usage_request_full(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(record_id): Path<i64>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    match build_usage_request_full_response(&state, record_id, visible_user_id).await {
        Ok(response) => Json(response).into_response(),
        Err(response) => response,
    }
}

pub(super) async fn usage_series(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<UsageSeriesQuery>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let bucket = match parse_usage_series_bucket(query.bucket) {
        Ok(bucket) => bucket,
        Err(response) => return *response,
    };
    let limit = query.limit.unwrap_or(24).clamp(1, 120);
    match db::usage_buckets(
        &state.pool,
        &bucket,
        limit,
        query.start,
        query.end,
        (!user.is_admin).then_some(user.user_id),
        query.request_category,
    )
    .await
    {
        Ok(series) => Json(series).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn clear_usage_events(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<UsageClearRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    let clear_query = match build_usage_clear_query(&user, body) {
        Ok(query) => query,
        Err(response) => return *response,
    };
    match db::clear_usage_events(&state.pool, clear_query).await {
        Ok(report) => Json(UsageClearResponse {
            deleted: report.deleted,
            deleted_prompt_blocks: report.deleted_prompt_blocks,
            protected_by_billing: report.protected_by_billing,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn prune_usage_events(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let retention_days = state.usage_retention.read().await.metadata_retention_days;
    match db::prune_usage_events(&state.pool, i64::from(retention_days)).await {
        Ok(report) => Json(UsagePruneResponse {
            deleted: report.deleted,
            protected_by_billing: report.protected_by_billing,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}
