use super::*;

pub(super) async fn list_endpoints(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::list_endpoints_page(
        &state.pool,
        query.first.unwrap_or(0),
        query.rows.unwrap_or(10),
    )
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
    match db::create_endpoint_with_mcp(&state.pool, input, mcp_enabled).await {
        Ok(endpoint) => {
            if let Err(err) = db::sync_minimax_mcp_server(&state.pool, &endpoint, mcp_enabled).await
            {
                return internal(&state, err);
            }
            refresh_managed_minimax_mcp(&state, endpoint.endpoint_id).await;
            let _ = publish_snapshot(&state).await;
            // The first INSERT already set `mcp_enabled`; reload so the
            // synchronous response reflects the canonical row plus any
            // API-key side effects.
            match db::get_endpoint(&state.pool, endpoint.endpoint_id).await {
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
    let existing = match db::get_endpoint(&state.pool, endpoint_id).await {
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
    // When the provider is changed away from MiniMax, MCP exposure is no
    // longer valid; resolve_endpoint_input already rejects an explicit
    // `mcp_enabled: true` for non-MiniMax providers, so any other value
    // (None, Some(false)) must collapse to false here.
    let provider_is_minimax = body.provider == db::EndpointProvider::Minimax;
    let mcp_enabled = if provider_is_minimax {
        body.mcp_enabled.unwrap_or(existing.mcp_enabled)
    } else {
        false
    };
    let input = match resolve_endpoint_input(&state, body, Some(existing.api_keys)).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::update_endpoint(&state.pool, endpoint_id, input).await {
        Ok(Some(endpoint)) => {
            if let Err(err) =
                db::set_endpoint_mcp_enabled(&state.pool, endpoint.endpoint_id, mcp_enabled).await
            {
                return internal(&state, err);
            }
            let endpoint = match db::get_endpoint(&state.pool, endpoint.endpoint_id).await {
                Ok(Some(endpoint)) => endpoint,
                Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
                Err(err) => return internal(&state, err),
            };
            if let Err(err) = db::sync_minimax_mcp_server(&state.pool, &endpoint, mcp_enabled).await
            {
                return internal(&state, err);
            }
            refresh_managed_minimax_mcp(&state, endpoint.endpoint_id).await;
            let _ = publish_snapshot(&state).await;
            match db::get_endpoint(&state.pool, endpoint.endpoint_id).await {
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
    match db::get_mcp_server_by_source_endpoint(&state.pool, endpoint_id).await {
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
    // Prefetch the managed MCP row so we can invalidate its local and
    // Valkey-backed catalog cache after the endpoint is gone. A missing row
    // (no managed MCP for this endpoint) is treated as a no-op so the
    // delete path is not coupled to MCP exposure.
    let managed_server = match db::get_mcp_server_by_source_endpoint(&state.pool, endpoint_id).await
    {
        Ok(server) => server,
        Err(err) => return internal(&state, err),
    };
    match db::delete_endpoint(&state.pool, endpoint_id).await {
        Ok(true) => {
            if let Some(server) = managed_server {
                state.mcp_catalog_service.invalidate(server.server_id).await;
            }
            let _ = publish_snapshot(&state).await;
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
    let endpoint = match db::get_endpoint(&state.pool, endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };

    let client = endpoint_protocol_client();
    let url = format!("{}/v1/models", endpoint.base_url.trim_end_matches('/'));
    let started = Instant::now();
    let request = client.get(url);
    let request = if parse_native_api(&endpoint.native_api) == NativeApi::AnthropicMessages {
        request
            .header("x-api-key", &endpoint.api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        request.bearer_auth(&endpoint.api_key)
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
            let native_api = parse_native_api(&endpoint.native_api);
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
