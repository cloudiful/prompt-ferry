use super::usage_support::{
    build_request_record_query, build_usage_clear_query, build_usage_request_full_response,
    combine_record_date_range, get_visible_usage_event_detail_or_not_found, parse_overview_window,
    parse_usage_date_range, parse_usage_series_bucket, parse_usage_summary_days,
    resolve_record_range_bounds,
};
use super::*;

#[utoipa::path(
    get,
    path = "/api/v1/bridge/status",
    responses((status = 200, body = BridgeStatus, description = "Bridge status")),
    tag = "bridge"
)]
pub(super) async fn bridge_status(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(response) = ensure_user(&state, &headers, UserAction::BridgeRead).await {
        return response.into_response();
    }
    let relay_senders = state.relay_senders.lock().await;
    let relays = state
        .configured_relays
        .iter()
        .map(|relay_url| RelayBridgeStatus {
            relay_url: relay_url.clone(),
            connected: relay_senders.contains_key(relay_url),
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/summary",
    operation_id = "request_record_summary",
    params(RequestRecordSummaryQuery),
    responses((status = 200, body = db::RequestRecordSummary, description = "Request record summary")),
    tag = "request-records"
)]
pub(super) async fn usage_summary(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<UsageSummaryQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let days = match parse_usage_summary_days(query.days) {
        Ok(days) => days,
        Err(response) => return response.into_response(),
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    match db::request_record_summary(&state.pool, days, visible_user_id).await {
        Ok(summary) => Json(summary).into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/overview",
    operation_id = "request_record_overview",
    params(RequestRecordOverviewQuery),
    responses((status = 200, body = db::RequestRecordOverviewResponse, description = "Request record overview")),
    tag = "request-records"
)]
pub(super) async fn usage_overview(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RequestRecordOverviewQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let window = match parse_overview_window(query.range, query.start, query.end) {
        Ok(window) => window,
        Err(response) => return response.into_response(),
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
        query.perspective.unwrap_or_default(),
        query.mcp_scope.unwrap_or_default(),
    )
    .await
    {
        Ok(overview) => Json(overview).into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records",
    operation_id = "list_request_records",
    params(RequestRecordsQuery),
    responses((status = 200, body = db::RequestRecordPage, description = "Request record page")),
    tag = "request-records"
)]
pub(super) async fn usage_events(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<UsageEventsQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let legacy_date_range = match query.date.as_deref() {
        Some(value) if !value.is_empty() => match parse_usage_date_range(value) {
            Ok(range) => range,
            Err(response) => return response.into_response(),
        },
        _ => (None, None),
    };
    let preset_range_bounds = match resolve_record_range_bounds(
        query.range,
        query.start,
        query.end,
        chrono::Utc::now(),
    ) {
        Ok(range) => range,
        Err(response) => return response.into_response(),
    };
    let (date_start, date_end) = match combine_record_date_range(
        legacy_date_range,
        preset_range_bounds.0,
        preset_range_bounds.1,
    ) {
        Ok(range) => range,
        Err(response) => return response.into_response(),
    };
    let request_query = build_request_record_query(&user, query, date_start, date_end);
    match db::list_request_records(&state.pool, request_query).await {
        Ok(page) => Json(page).into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/facets",
    operation_id = "request_record_facets",
    params(crate::worker_admin_types::RequestRecordFacetsQuery),
    responses((status = 200, body = db::RequestRecordFacets, description = "Request record facets")),
    tag = "request-records"
)]
pub(super) async fn usage_facets(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<RequestRecordFacetsQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let (start, end) = match resolve_record_range_bounds(
        query.range,
        query.start,
        query.end,
        chrono::Utc::now(),
    ) {
        Ok(bounds) => bounds,
        Err(response) => return response.into_response(),
    };
    match db::list_request_record_facets(
        &state.pool,
        (!user.is_admin).then_some(user.user_id),
        query
            .request_category
            .unwrap_or(db::RequestRecordCategory::Ai),
        user.is_admin,
        start,
        end,
        query.mcp_scope.unwrap_or_default(),
    )
    .await
    {
        Ok(facets) => Json(facets).into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/{record_id}",
    operation_id = "request_record_detail",
    params(("record_id" = i64, Path, description = "Request record ID")),
    responses((status = 200, body = db::RequestRecordDetail, description = "Request record detail")),
    tag = "request-records"
)]
pub(super) async fn usage_event_detail(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(record_id): Path<i64>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    let mut event =
        match get_visible_usage_event_detail_or_not_found(&state, record_id, visible_user_id).await
        {
            Ok(event) => event,
            Err(response) => return response.into_response(),
        };
    match db::list_request_record_tool_calls(&state.pool, event.record_id).await {
        Ok(tool_call_events) => {
            event.tool_call_events = tool_call_events;
            Json(event).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/{record_id}/request-full",
    operation_id = "request_record_full",
    params(RequestRecordFullQuery, ("record_id" = i64, Path, description = "Request record ID")),
    responses((status = 200, body = RequestRecordFullResponse, description = "Full stored request record payload")),
    tag = "request-records"
)]
pub(super) async fn usage_request_full(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(record_id): Path<i64>,
    Query(query): Query<RequestRecordFullQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let visible_user_id = (!user.is_admin).then_some(user.user_id);
    match build_usage_request_full_response(&state, record_id, visible_user_id, &query).await {
        Ok(response) => Json(response).into_response(),
        Err(response) => response.into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/series",
    operation_id = "request_record_series",
    params(RequestRecordSeriesQuery),
    responses((status = 200, body = [db::RequestRecordBucket], description = "Request record series")),
    tag = "request-records"
)]
pub(super) async fn usage_series(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<UsageSeriesQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let bucket = match parse_usage_series_bucket(query.bucket) {
        Ok(bucket) => bucket,
        Err(response) => return response.into_response(),
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

#[utoipa::path(
    post,
    path = "/api/v1/admin/request-records/clear",
    operation_id = "clear_request_records",
    request_body = RequestRecordsClearRequest,
    responses((status = 200, body = RequestRecordsClearResponse, description = "Request record clear result")),
    tag = "request-records"
)]
pub(super) async fn clear_usage_events(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<UsageClearRequest>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::UsageManage).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let clear_query = match build_usage_clear_query(&user, body) {
        Ok(query) => query,
        Err(response) => return response.into_response(),
    };
    match db::clear_usage_events(&state.pool, clear_query).await {
        Ok(report) => Json(UsageClearResponse {
            deleted: report.deleted,
            deleted_prompt_blocks: report.deleted_prompt_blocks,
            chunks_dropped: report.chunks_dropped,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/request-records/prune",
    operation_id = "prune_request_records",
    responses((status = 200, body = RequestRecordPruneResponse, description = "Request record prune result")),
    tag = "request-records"
)]
pub(super) async fn prune_usage_events(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::Usage).await {
        return response.into_response();
    }
    let retention = state.usage_retention.read().await.clone();
    match db::prune_usage_events(
        &state.pool,
        i64::from(retention.metadata_retention_days),
        i64::from(retention.content_retention_days),
    )
    .await
    {
        Ok(report) => Json(UsagePruneResponse {
            chunks_dropped: report.chunks_dropped,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}
