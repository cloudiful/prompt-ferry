use super::*;

pub(super) async fn list_model_routes(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::list_model_endpoint_rules_page(
        &state.pool,
        query.first.unwrap_or(0),
        query.rows.unwrap_or(10),
    )
    .await
    {
        Ok(page) => Json(ModelRoutePageResponse::from(page)).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ModelRouteRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match body.validate_for_create(&state).await {
        Ok(()) => {}
        Err(response) => return response,
    }
    let input = match body.into_create(&state).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::create_model_endpoint_rule(&state.pool, input).await {
        Ok(rule) => {
            let _ = publish_snapshot(&state).await;
            Json(rule).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(rule_id): Path<Uuid>,
    Json(body): Json<ModelRouteRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match body.validate_for_update(&state, rule_id).await {
        Ok(()) => {}
        Err(response) => return response,
    }
    let input = match body.into_create(&state).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    match db::update_model_endpoint_rule(&state.pool, rule_id, input).await {
        Ok(Some(rule)) => {
            let _ = publish_snapshot(&state).await;
            Json(rule).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "model route not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(rule_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::delete_model_endpoint_rule(&state.pool, rule_id).await {
        Ok(true) => {
            let _ = publish_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "model route not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn test_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ModelRouteTestRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let rule = match db::get_model_endpoint_rule(&state.pool, body.rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "model route not found"),
        Err(err) => return internal(&state, err),
    };
    let candidate = match db::get_model_route_candidate(&state.pool, body.rule_id).await {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "not_found",
                "model route target not found",
            );
        }
        Err(err) => return internal(&state, err),
    };
    let preferred = choose_preferred_target(&candidate, model_route_test_routing_key(&candidate));
    let started = Instant::now();
    let result = run_model_route_test(&state, &candidate).await;
    let duration_ms = started.elapsed().as_millis();
    match result {
        Ok((endpoint, model, status, message)) => Json(ModelRouteTestResponse {
            ok: status.is_success(),
            status: Some(status.as_u16()),
            duration_ms,
            endpoint_id: Some(endpoint.endpoint_id).filter(|id| !id.is_nil()),
            endpoint_name: (!endpoint.name.is_empty()).then_some(endpoint.name),
            preferred_endpoint_id: preferred.as_ref().map(|target| target.endpoint_id),
            preferred_endpoint_name: preferred.map(|target| target.endpoint_name),
            rule_id: Some(rule.rule_id),
            model_pattern: Some(rule.model_pattern),
            model: Some(model),
            message,
        })
        .into_response(),
        Err(err) => Json(ModelRouteTestResponse {
            ok: false,
            status: err.status().map(|status| status.as_u16()),
            duration_ms,
            endpoint_id: None,
            endpoint_name: None,
            preferred_endpoint_id: preferred.as_ref().map(|target| target.endpoint_id),
            preferred_endpoint_name: preferred.map(|target| target.endpoint_name),
            rule_id: Some(rule.rule_id),
            model_pattern: Some(rule.model_pattern),
            model: None,
            message: truncate_message(&maybe_redact(&state, &err.to_string())),
        })
        .into_response(),
    }
}

async fn run_model_route_test(
    state: &AdminState,
    candidate: &db::ModelRouteCandidate,
) -> Result<(db::RouteTestEndpoint, String, StatusCode, String), reqwest::Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("static reqwest client config is valid");
    let routing_key = model_route_test_routing_key(candidate);
    let target = choose_preferred_target(candidate, routing_key)
        .expect("validated model route has at least one target");
    let base = target.base_url.trim_end_matches('/');
    let models_request = client.get(format!("{base}/v1/models"));
    let models_request = match target.native_api {
        NativeApi::AnthropicMessages => models_request
            .header("x-api-key", &target.api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => models_request.bearer_auth(&target.api_key),
    };
    let models_response = models_request.send().await?;
    let models_status = models_response.status();
    let models_body = models_response.text().await?;
    if !models_status.is_success() {
        return Ok((
            db::RouteTestEndpoint {
                endpoint_id: target.endpoint_id,
                name: target.endpoint_name.clone(),
            },
            candidate.model_pattern.clone(),
            models_status,
            truncate_message(&maybe_redact(state, models_body.trim())),
        ));
    }
    let Some(model) = target
        .upstream_model
        .clone()
        .or_else(|| select_test_model(&candidate.model_pattern, &models_body))
    else {
        return Ok((
            db::RouteTestEndpoint {
                endpoint_id: target.endpoint_id,
                name: target.endpoint_name.clone(),
            },
            candidate.model_pattern.clone(),
            StatusCode::NOT_FOUND,
            "no upstream model matched rule".to_string(),
        ));
    };
    if target.native_api == NativeApi::Realtime {
        let url = format!(
            "{}?model={}",
            format!("{}/v1/realtime", base)
                .replace("https://", "wss://")
                .replace("http://", "ws://"),
            urlencoding::encode(&model)
        );
        let mut request =
            match tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
                url,
            ) {
                Ok(request) => request,
                Err(err) => {
                    return Ok((
                        db::RouteTestEndpoint {
                            endpoint_id: target.endpoint_id,
                            name: target.endpoint_name.clone(),
                        },
                        model,
                        StatusCode::BAD_REQUEST,
                        err.to_string(),
                    ));
                }
            };
        if let Ok(value) = header::HeaderValue::from_str(&format!("Bearer {}", target.api_key)) {
            request.headers_mut().insert(header::AUTHORIZATION, value);
        }
        match tokio_tungstenite::connect_async_with_config(request, None, false).await {
            Ok((mut socket, _)) => {
                let _ = futures::SinkExt::send(
                    &mut socket,
                    tokio_tungstenite::tungstenite::Message::Close(None),
                )
                .await;
                return Ok((
                    db::RouteTestEndpoint {
                        endpoint_id: target.endpoint_id,
                        name: target.endpoint_name.clone(),
                    },
                    model,
                    StatusCode::OK,
                    format!("OK ({})", target.native_api.as_str()),
                ));
            }
            Err(err) => {
                return Ok((
                    db::RouteTestEndpoint {
                        endpoint_id: target.endpoint_id,
                        name: target.endpoint_name.clone(),
                    },
                    model,
                    StatusCode::BAD_GATEWAY,
                    truncate_message(&maybe_redact(state, &err.to_string())),
                ));
            }
        }
    }
    let (path, payload) = match target.native_api {
        NativeApi::AnthropicMessages => (
            NativeApi::AnthropicMessages.path(),
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": false,
                "max_tokens": 1,
            }),
        ),
        NativeApi::Responses => (
            NativeApi::Responses.path(),
            serde_json::json!({
                "model": model,
                "input": "ping",
                "stream": false,
                "max_output_tokens": 1,
            }),
        ),
        NativeApi::Chat => (
            NativeApi::Chat.path(),
            serde_json::json!({
                "model": model,
                "stream": false,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 1,
            }),
        ),
        NativeApi::Realtime => unreachable!(),
    };
    let request = client.post(format!("{base}{path}")).json(&payload);
    let request = match target.native_api {
        NativeApi::AnthropicMessages => request
            .header("x-api-key", &target.api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(&target.api_key),
    };
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = if status.is_success() {
        format!("OK ({})", target.native_api.as_str())
    } else {
        truncate_message(&maybe_redact(state, body.trim()))
    };
    Ok((
        db::RouteTestEndpoint {
            endpoint_id: target.endpoint_id,
            name: target.endpoint_name.clone(),
        },
        model,
        status,
        message,
    ))
}

fn select_test_model(pattern: &str, models_body: &str) -> Option<String> {
    let body = serde_json::from_str::<serde_json::Value>(models_body).ok()?;
    let models = body.get("data")?.as_array()?;
    models
        .iter()
        .filter_map(|item| item.get("id").and_then(|value| value.as_str()))
        .find(|model| db::model_pattern_matches(pattern, model))
        .map(str::to_string)
}

fn model_route_test_routing_key(candidate: &db::ModelRouteCandidate) -> Option<&'static str> {
    match candidate.routing_strategy {
        db::ModelRouteRoutingStrategy::ClientKeyRendezvous => Some(MODEL_ROUTE_TEST_ROUTING_KEY),
        db::ModelRouteRoutingStrategy::ResponsesSessionAffinity => {
            Some(MODEL_ROUTE_TEST_SESSION_KEY)
        }
    }
}
