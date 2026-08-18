use super::*;

pub(super) async fn token_plan_usage(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let endpoint = match db::get_endpoint(&state.pool, endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    if endpoint.provider != db::EndpointProvider::Minimax {
        return error(
            StatusCode::BAD_REQUEST,
            "unsupported_provider",
            "token plan usage is only available for MiniMax endpoints",
        );
    }
    if endpoint.provider_region.is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_provider_region",
            "MiniMax endpoint has no provider region",
        );
    }
    let has_enabled_key = endpoint
        .api_keys
        .iter()
        .any(|key| key.enabled && !key.api_key.trim().is_empty())
        || !endpoint.api_key.trim().is_empty();
    if !has_enabled_key {
        return error(
            StatusCode::BAD_REQUEST,
            "missing_api_key",
            "MiniMax endpoint has no enabled API key",
        );
    }

    match super::super::token_plan::fetch_endpoint_usage(&endpoint).await {
        Ok(usage) => Json(usage).into_response(),
        Err(err) => internal(&state, err),
    }
}
