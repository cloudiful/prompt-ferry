use super::*;

pub(super) async fn list_endpoints(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match state
        .config_repository
        .list_endpoints_page(query.first.unwrap_or(0), query.rows.unwrap_or(10))
        .await
    {
        Ok(page) => Json(EndpointPageResponse::from(page)).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_endpoint(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<EndpointRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if endpoint_base_url_has_version_path(&body.base_url) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_base_url",
            "base_url must be the provider base URL without /v1",
        );
    }
    let mcp_enabled = body
        .mcp_enabled
        .unwrap_or(body.provider == db::EndpointProvider::Minimax);
    let input = match resolve_endpoint_input(&state, body, None).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    let endpoint_id = uuid::Uuid::new_v4();
    match state
        .config_repository
        .create_endpoint(endpoint_id, input, mcp_enabled)
        .await
    {
        Ok(endpoint) => {
            let pg_endpoint = endpoint.clone().into_pg();
            if let Err(err) = state
                .config_repository
                .sync_minimax_mcp_server(&pg_endpoint, mcp_enabled)
                .await
            {
                return internal(&state, err);
            }
            refresh_managed_minimax_mcp(&state, pg_endpoint.endpoint_id).await;
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after endpoint create");
            }
            match state.config_repository.get_endpoint(endpoint_id).await {
                Ok(Some(endpoint)) => Json(endpoint).into_response(),
                Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
                Err(err) => internal(&state, err),
            }
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_endpoint(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Json(body): Json<EndpointRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let existing = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    if endpoint_base_url_has_version_path(&body.base_url) {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_base_url",
            "base_url must be the provider base URL without /v1",
        );
    }
    let provider_is_minimax = body.provider == db::EndpointProvider::Minimax;
    let mcp_enabled = if provider_is_minimax {
        body.mcp_enabled.unwrap_or(existing.mcp_enabled)
    } else {
        false
    };
    let existing_api_keys = match state
        .config_repository
        .endpoint_api_keys_for_update(endpoint_id)
        .await
    {
        Ok(keys) => keys,
        Err(err) => return internal(&state, err),
    };
    let input = match resolve_endpoint_input(&state, body, Some(existing_api_keys)).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match state
        .config_repository
        .update_endpoint(endpoint_id, input)
        .await
    {
        Ok(Some(endpoint)) => {
            let endpoint_id = endpoint.endpoint_id;
            if let Err(err) = state
                .config_repository
                .set_endpoint_mcp_enabled(endpoint_id, mcp_enabled)
                .await
            {
                return internal(&state, err);
            }
            let pg_endpoint = endpoint.clone().into_pg();
            if let Err(err) = state
                .config_repository
                .sync_minimax_mcp_server(&pg_endpoint, mcp_enabled)
                .await
            {
                return internal(&state, err);
            }
            refresh_managed_minimax_mcp(&state, pg_endpoint.endpoint_id).await;
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after endpoint update");
            }
            match state.config_repository.get_endpoint(endpoint_id).await {
                Ok(Some(endpoint)) => Json(endpoint).into_response(),
                Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
                Err(err) => internal(&state, err),
            }
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => internal(&state, err),
    }
}

async fn refresh_managed_minimax_mcp(state: &AdminState, endpoint_id: Uuid) {
    match state
        .config_repository
        .get_mcp_server_by_source_endpoint(endpoint_id)
        .await
    {
        Ok(Some(server)) if server.enabled => state.mcp_catalog_service.spawn_refresh(server),
        Ok(Some(server)) => state.mcp_catalog_service.invalidate(server.server_id).await,
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(error = %err, %endpoint_id, "failed to refresh managed MiniMax MCP")
        }
    }
}

pub(super) async fn delete_endpoint(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let managed_server = match state
        .config_repository
        .get_mcp_server_by_source_endpoint(endpoint_id)
        .await
    {
        Ok(server) => server,
        Err(err) => return internal(&state, err),
    };
    match state.config_repository.delete_endpoint(endpoint_id).await {
        Ok(true) => {
            if let Some(server) = managed_server {
                state.mcp_catalog_service.invalidate(server.server_id).await;
            }
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after endpoint delete");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn test_endpoint(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let endpoint = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    let api_key = match state
        .config_repository
        .first_endpoint_api_key(endpoint_id)
        .await
    {
        Ok(Some(key)) => key,
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "missing_endpoint_api_key",
                "endpoint is missing an API key",
            );
        }
        Err(err) => return internal(&state, err),
    };

    let client = endpoint_protocol_client();
    let url = format!("{}/v1/models", endpoint.base_url.trim_end_matches('/'));
    let started = Instant::now();
    let request = client.get(url);
    let request = if endpoint.native_api == NativeApi::AnthropicMessages {
        request
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        request.bearer_auth(&api_key)
    };
    let result = request.send().await;

    let elapsed = started.elapsed().as_millis();
    match result {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let model_count = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("data")
                        .and_then(|data| data.as_array())
                        .map(Vec::len)
                });
            if !status.is_success() {
                return Json(EndpointTestResponse {
                    ok: false,
                    status: Some(status.as_u16()),
                    duration_ms: elapsed,
                    model_count,
                    native_api: None,
                    native_api_source: None,
                    message: truncate_message(&maybe_redact(&state, body.trim())),
                })
                .into_response();
            }
            let native_api = endpoint.native_api;
            let native_api_source = endpoint.native_api_source.clone();
            let message = match model_count {
                Some(count) => format!(
                    "OK, models: {count}, protocol: {} ({})",
                    native_api.as_str(),
                    native_api_source
                ),
                None => format!(
                    "OK, protocol: {} ({})",
                    native_api.as_str(),
                    native_api_source
                ),
            };
            Json(EndpointTestResponse {
                ok: true,
                status: Some(status.as_u16()),
                duration_ms: elapsed,
                model_count,
                native_api: Some(native_api.as_str().to_string()),
                native_api_source: Some(native_api_source),
                message,
            })
            .into_response()
        }
        Err(err) => Json(EndpointTestResponse {
            ok: false,
            status: err.status().map(|status| status.as_u16()),
            duration_ms: elapsed,
            model_count: None,
            native_api: None,
            native_api_source: None,
            message: truncate_message(&maybe_redact(&state, &err.to_string())),
        })
        .into_response(),
    }
}
