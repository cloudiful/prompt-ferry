//! The safe, read-only catalog reads for ordinary authenticated users.
//!
//! These three `/me` routes answer with the dedicated allowlisted shapes:
//! they never expose the administrator DTOs, they never reach a control
//! action, and they never collect quota, refresh it, discover models, or
//! test an upstream.

use super::*;
use crate::openapi_export::schemas::ErrorEnvelope;

/// The upstream endpoints the current user may see.
///
/// The row set is filtered in the repository query, before the count and the
/// page, so the total always matches the visible rows.
#[utoipa::path(
    get,
    path = "/api/v1/me/endpoint-catalog",
    operation_id = "me_list_endpoint_catalog",
    params(TablePageQuery),
    responses(
        (status = 200, body = EndpointCatalogPageResponse, description = "Visible upstream endpoint catalog"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized"),
        (status = 403, body = ErrorEnvelope, description = "Forbidden")
    ),
    tag = "me"
)]
pub(super) async fn list_endpoint_catalog(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::EndpointRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match state
        .config_repository
        .list_endpoints_page(first, rows, Some(user.user_id))
        .await
    {
        Ok(page) => {
            let endpoints = page
                .endpoints
                .iter()
                .filter_map(|endpoint| {
                    let visibility = crate::db::visibility::catalog_visibility(
                        &endpoint.scope,
                        endpoint.owner_user_id,
                        user.user_id,
                    )?;
                    Some(EndpointCatalogRow::from_endpoint(
                        endpoint,
                        visibility.into(),
                    ))
                })
                .collect();
            Json(EndpointCatalogPageResponse {
                total: page.total,
                endpoints,
                first: page.first,
                rows: page.rows,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

/// The model routes the current user may see.
///
/// The route rows are filtered in the repository query, and every target is
/// filtered again against the visible endpoint set, so a target never names a
/// hidden endpoint.
#[utoipa::path(
    get,
    path = "/api/v1/me/model-route-catalog",
    operation_id = "me_list_model_route_catalog",
    params(TablePageQuery),
    responses(
        (status = 200, body = ModelRouteCatalogPageResponse, description = "Visible model route catalog"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized"),
        (status = 403, body = ErrorEnvelope, description = "Forbidden")
    ),
    tag = "me"
)]
pub(super) async fn list_model_route_catalog(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::ModelRouteRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    let visible_endpoint_ids = match state
        .config_repository
        .list_visible_endpoint_ids(Some(user.user_id))
        .await
    {
        Ok(ids) => ids,
        Err(err) => return internal(&state, err),
    };
    match state
        .config_repository
        .list_model_routes_page(first, rows, Some(user.user_id))
        .await
    {
        Ok(page) => {
            let routes = page
                .routes
                .iter()
                .filter_map(|route| {
                    let visibility = crate::db::visibility::catalog_visibility(
                        &route.scope,
                        route.owner_user_id,
                        user.user_id,
                    )?;
                    Some(ModelRouteCatalogRow::from_route(
                        route,
                        visibility.into(),
                        &visible_endpoint_ids,
                    ))
                })
                .collect();
            Json(ModelRouteCatalogPageResponse {
                total: page.total,
                routes,
                first: page.first,
                rows: page.rows,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

/// The cached quota of one visible endpoint.
///
/// The answer always comes from the cache: the read never collects quota,
/// never forces a refresh, and never reaches the upstream provider. An
/// endpoint the caller may not see is indistinguishable from a missing one.
#[utoipa::path(
    get,
    path = "/api/v1/me/endpoint-catalog/{endpoint_id}/quota",
    operation_id = "me_endpoint_quota",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, body = CachedQuotaResponse, description = "Cached quota of a visible endpoint"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized"),
        (status = 403, body = ErrorEnvelope, description = "Forbidden"),
        (status = 404, body = ErrorEnvelope, description = "Endpoint not found")
    ),
    tag = "me"
)]
pub(super) async fn endpoint_quota(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    let user = match ensure_user(&state, &headers, UserAction::QuotaRead).await {
        Ok(user) => user,
        Err(response) => return response.into_response(),
    };
    // The endpoint is resolved and checked for visibility before any quota
    // access happens, so a hidden endpoint never reaches the cache.
    let endpoint = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    if crate::db::visibility::catalog_visibility(
        &endpoint.scope,
        endpoint.owner_user_id,
        user.user_id,
    )
    .is_none()
    {
        return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found");
    }
    let provider = endpoint.provider;
    let plan = endpoint.plan;
    let observations = if !crate::worker_admin_types::quota_eligible(provider, plan) {
        Vec::new()
    } else if provider == db::EndpointProvider::OpenAi {
        // The ChatGPT quota lives in the persisted snapshot store. The read
        // answers from that snapshot only; it never refreshes it.
        match state.chatgpt_quota_service.as_ref() {
            Some(service) => {
                let endpoint = endpoint.clone().into_pg();
                match service.snapshot_only(&endpoint).await {
                    Ok(Some(usage)) => {
                        crate::worker_admin_types::cached_observations_from_usage(&usage)
                    }
                    Ok(None) => Vec::new(),
                    Err(err) => {
                        tracing::warn!(
                            endpoint_id = %endpoint_id,
                            error = %err,
                            "failed to read the persisted ChatGPT quota snapshot"
                        );
                        Vec::new()
                    }
                }
            }
            None => Vec::new(),
        }
    } else {
        // The token-plan quota lives in the in-memory cache. The read clones
        // the cached entry; it never refreshes it and never fetches upstream.
        match state.token_plan_quota.cached_snapshot(endpoint_id).await {
            Some(snapshot) => snapshot.safe_observations(),
            None => Vec::new(),
        }
    };
    Json(CachedQuotaResponse::new(
        endpoint_id,
        provider,
        endpoint.provider_region,
        observations,
    ))
    .into_response()
}
