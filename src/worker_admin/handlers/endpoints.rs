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
    let input = match resolve_endpoint_input(&state, body, None, None).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::create_endpoint(&state.pool, input).await {
        Ok(endpoint) => {
            let _ = publish_snapshot(&state).await;
            Json(endpoint).into_response()
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
    let input = match resolve_endpoint_input(
        &state,
        body,
        Some(existing.api_key),
        Some(existing.api_keys),
    )
    .await
    {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::update_endpoint(&state.pool, endpoint_id, input).await {
        Ok(Some(endpoint)) => {
            let _ = publish_snapshot(&state).await;
            Json(endpoint).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => internal(&state, err),
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
    match db::delete_endpoint(&state.pool, endpoint_id).await {
        Ok(true) => {
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
            let protocol = if endpoint.native_api_source == NativeApiSource::Detected.as_str() {
                match detect_endpoint_protocol(&client, &endpoint.base_url, &endpoint.api_key).await
                {
                    Ok(native_api) => EndpointProtocolResolution {
                        native_api,
                        native_api_source: NativeApiSource::Detected,
                    },
                    Err(message) => {
                        return Json(EndpointTestResponse {
                            ok: false,
                            status: Some(StatusCode::BAD_GATEWAY.as_u16()),
                            duration_ms: elapsed,
                            model_count,
                            native_api: None,
                            native_api_source: Some(NativeApiSource::Detected.as_str().to_string()),
                            message: truncate_message(&maybe_redact(&state, &message)),
                        })
                        .into_response();
                    }
                }
            } else {
                EndpointProtocolResolution {
                    native_api: parse_native_api(&endpoint.native_api),
                    native_api_source: NativeApiSource::Manual,
                }
            };
            let message = match model_count {
                Some(count) => format!(
                    "OK, models: {count}, protocol: {} ({})",
                    protocol.native_api.as_str(),
                    protocol.native_api_source.as_str()
                ),
                None => format!(
                    "OK, protocol: {} ({})",
                    protocol.native_api.as_str(),
                    protocol.native_api_source.as_str()
                ),
            };
            if protocol.native_api == NativeApi::Realtime {
                let model = serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|value| value.get("data").and_then(|data| data.as_array()).cloned())
                    .and_then(|items| {
                        items.into_iter().find_map(|item| {
                            item.get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string)
                        })
                    });
                let Some(model) = model else {
                    return Json(EndpointTestResponse {
                        ok: false,
                        status: Some(StatusCode::NOT_FOUND.as_u16()),
                        duration_ms: elapsed,
                        model_count,
                        native_api: Some(protocol.native_api.as_str().to_string()),
                        native_api_source: Some(protocol.native_api_source.as_str().to_string()),
                        message: "realtime handshake requires at least one listed model"
                            .to_string(),
                    })
                    .into_response();
                };
                let ws_url = format!(
                    "{}?model={}",
                    format!("{}/v1/realtime", endpoint.base_url.trim_end_matches('/'))
                        .replace("https://", "wss://")
                        .replace("http://", "ws://"),
                    urlencoding::encode(&model)
                );
                let mut ws_request = match tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(ws_url) {
                    Ok(request) => request,
                    Err(err) => {
                        return Json(EndpointTestResponse {
                            ok: false,
                            status: Some(StatusCode::BAD_REQUEST.as_u16()),
                            duration_ms: elapsed,
                            model_count,
                            native_api: Some(protocol.native_api.as_str().to_string()),
                            native_api_source: Some(protocol.native_api_source.as_str().to_string()),
                            message: truncate_message(&maybe_redact(&state, &err.to_string())),
                        }).into_response();
                    }
                };
                if let Ok(value) =
                    header::HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                {
                    ws_request
                        .headers_mut()
                        .insert(header::AUTHORIZATION, value);
                }
                match tokio_tungstenite::connect_async_with_config(ws_request, None, false).await {
                    Ok((mut socket, _)) => {
                        let _ = futures::SinkExt::send(
                            &mut socket,
                            tokio_tungstenite::tungstenite::Message::Close(None),
                        )
                        .await;
                    }
                    Err(err) => {
                        return Json(EndpointTestResponse {
                            ok: false,
                            status: Some(StatusCode::BAD_GATEWAY.as_u16()),
                            duration_ms: elapsed,
                            model_count,
                            native_api: Some(protocol.native_api.as_str().to_string()),
                            native_api_source: Some(
                                protocol.native_api_source.as_str().to_string(),
                            ),
                            message: truncate_message(&maybe_redact(&state, &err.to_string())),
                        })
                        .into_response();
                    }
                }
            }
            Json(EndpointTestResponse {
                ok: true,
                status: Some(status.as_u16()),
                duration_ms: elapsed,
                model_count,
                native_api: Some(protocol.native_api.as_str().to_string()),
                native_api_source: Some(protocol.native_api_source.as_str().to_string()),
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
