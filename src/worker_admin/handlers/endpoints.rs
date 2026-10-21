use super::*;

pub(super) async fn list_endpoints(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
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
        return response.into_response();
    }
    let mcp_enabled = body
        .mcp_enabled
        .unwrap_or(body.provider == db::EndpointProvider::Minimax);
    let (input, admin_api_key) =
        match resolve_endpoint_input(&state, body, None, None, None, false, None).await {
            Ok(input) => input,
            Err(response) => return response.into_response(),
        };
    let endpoint_id = uuid::Uuid::new_v4();
    match state
        .config_repository
        .create_endpoint(endpoint_id, input, mcp_enabled)
        .await
    {
        Ok(endpoint) => {
            // Issue #589: persist the optional Admin API key before the
            // response is built so `has_admin_api_key` is accurate.
            if let Err(err) = state
                .config_repository
                .set_endpoint_admin_api_key(endpoint_id, admin_api_key.as_deref())
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
                tracing::warn!(error = %err, "snapshot publication failed after endpoint create");
            }
            // Issue #599 R2e.2: answer from the committed write result. A
            // freshly created endpoint never carries an OAuth token, so the
            // effective plan is the platform default. The removed post-save
            // re-fetch could turn a committed write into a false 404.
            saved_endpoint_response(endpoint, false, mcp_enabled, admin_api_key.is_some())
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
        return response.into_response();
    }
    let existing = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
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
    // Issue #368 Phase B: PATCH carry for the proxy secret. The unified
    // endpoint shape is redacted, so fetch the decrypted value separately;
    // `None` (omitted) keeps it, `Some("")` clears to direct.
    let existing_proxy_url = match state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
    {
        Ok(value) => value,
        Err(err) => return internal(&state, err),
    };
    // Issue #392 Phase K: same carry for `active_windows` (omitted keeps).
    let existing_active_windows = Some(existing.active_windows.clone());
    // Issue #599 R2a: plan-switch hygiene needs the requested values after
    // `body` moves into `resolve_endpoint_input` (both are `Copy`).
    let requested_plan = body.plan;
    let new_provider = body.provider;
    // Issue #599 R2b: an existing stored token makes the subscription plan
    // claimable on PATCH (`validate_endpoint_plan`).
    let has_oauth_token = existing.has_oauth_token;
    // Issue #589: PATCH carry for the optional Admin API key. It is
    // OpenAI-only, so only an OpenAI update needs the stored secret; any
    // other provider resolves to clear.
    let existing_admin_api_key = if body.provider == db::EndpointProvider::OpenAi {
        match state
            .config_repository
            .endpoint_admin_api_key(endpoint_id)
            .await
        {
            Ok(value) => value,
            Err(err) => return internal(&state, err),
        }
    } else {
        None
    };
    let (input, admin_api_key) = match resolve_endpoint_input(
        &state,
        body,
        Some(existing_api_keys),
        existing_proxy_url,
        existing_active_windows,
        has_oauth_token,
        existing_admin_api_key,
    )
    .await
    {
        Ok(input) => input,
        Err(response) => return response.into_response(),
    };
    match state
        .config_repository
        .update_endpoint(endpoint_id, input)
        .await
    {
        Ok(Some(endpoint)) => {
            // Issue #599 R2a: plan switch clears the counterpart credential.
            // An explicit platform plan, or a provider move away from OpenAI,
            // drops any stored ChatGPT OAuth token (the derived plan falls
            // back to `platform_api_key`). No-op when nothing is stored.
            let switch_clears_token = existing.has_oauth_token
                && (requested_plan == Some(db::EndpointPlan::PlatformApiKey)
                    || new_provider != db::EndpointProvider::OpenAi);
            if switch_clears_token
                && let Err(err) = state
                    .config_repository
                    .clear_endpoint_oauth_token(endpoint_id)
                    .await
            {
                return internal(&state, err);
            }
            let endpoint_id = endpoint.endpoint_id;
            // Issue #589: apply the resolved Admin API key before the response
            // is built so `has_admin_api_key` is accurate; a non-OpenAI
            // update resolves to `None` and clears any stored key.
            if let Err(err) = state
                .config_repository
                .set_endpoint_admin_api_key(endpoint_id, admin_api_key.as_deref())
                .await
            {
                return internal(&state, err);
            }
            // Issue #589 P2b: a changed (or cleared) Admin API key must not be
            // served from the previous organization-usage snapshot.
            state.openai_org_usage.invalidate(endpoint_id).await;
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
            // Issue #599 R2e.2: answer from the committed write result plus
            // the in-handler transitions (`set_endpoint_mcp_enabled`, token
            // clear) instead of re-reading the row; the removed post-save
            // re-fetch could turn a committed write into a false 404.
            let has_oauth_token = existing.has_oauth_token && !switch_clears_token;
            saved_endpoint_response(
                endpoint,
                has_oauth_token,
                mcp_enabled,
                admin_api_key.is_some(),
            )
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => internal(&state, err),
    }
}

/// Issue #599 R2e.2: render the create/update success response from the
/// committed repository write result. The effective plan and token presence
/// are derived in memory, so a committed write never turns into a false 404
/// through a redundant post-save `get_endpoint` miss. `has_admin_api_key`
/// mirrors the just-resolved value (issue #589); the secret itself is never
/// part of the response.
fn saved_endpoint_response(
    mut endpoint: db::UnifiedProviderEndpoint,
    has_oauth_token: bool,
    mcp_enabled: bool,
    has_admin_api_key: bool,
) -> Response {
    endpoint.has_oauth_token = has_oauth_token;
    endpoint.plan = db::EndpointPlan::resolve(endpoint.provider, has_oauth_token);
    endpoint.has_admin_api_key = has_admin_api_key;
    endpoint.mcp_enabled = mcp_enabled;
    Json(endpoint).into_response()
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
        return response.into_response();
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
            // Issue #589 P2b: drop any cached organization usage for the
            // deleted endpoint.
            state.openai_org_usage.invalidate(endpoint_id).await;
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after endpoint delete");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => internal(&state, err),
    }
}

/// Issue #993 P1: the model identifiers one endpoint advertises.
///
/// The listing is an upstream discovery read, so it follows the endpoint's
/// own base URL, proxy, and protocol auth instead of a provider allowlist.
/// The response carries sorted, de-duplicated identifiers; a failure is a
/// bounded admin error that never echoes the credential or the upstream body.
pub(super) async fn list_endpoint_models(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let endpoint = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    // Issue #1004 P1: a ChatGPT subscription endpoint has no Platform API key
    // and no Platform model listing. Its suggestions come from the Codex
    // model table, so the request never reaches `api.openai.com` and the
    // platform model cache stays free of subscription snapshots.
    if endpoint.plan == db::EndpointPlan::ChatgptSubscription {
        let mut models = chatgpt_backend::codex_models()
            .iter()
            .map(|model| (*model).to_string())
            .collect::<Vec<_>>();
        models.sort();
        return Json(EndpointModelsResponse { models }).into_response();
    }
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
    let proxy_url = match state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
    {
        Ok(value) => value,
        Err(err) => return internal(&state, err),
    };
    // The endpoint proxy applies to the discovery request as well. An invalid
    // proxy fails closed instead of falling back to a direct request.
    let client = match crate::endpoint_protocol::endpoint_protocol_client_for_endpoint(
        proxy_url.as_deref(),
        &endpoint.base_url,
    ) {
        Ok(client) => client,
        Err(_) => {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                "rejecting endpoint model discovery with an invalid proxy configuration"
            );
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_endpoint_proxy",
                "endpoint proxy configuration is invalid",
            );
        }
    };

    let cached = state
        .endpoint_model_cache
        .lookup_endpoint(
            endpoint_id,
            &endpoint.base_url,
            &api_key,
            endpoint.native_api,
        )
        .await;
    if let crate::endpoint_models::CacheLookup::Fresh(snapshot) = cached {
        return Json(EndpointModelsResponse {
            models: snapshot.sorted_model_ids(),
        })
        .into_response();
    }

    let url = crate::endpoint_models::models_url(&endpoint.base_url, endpoint.provider);
    let request = client.get(url);
    let request = if endpoint.native_api == NativeApi::AnthropicMessages {
        request
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        request.bearer_auth(&api_key)
    };
    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                error = %err,
                "upstream model discovery request failed"
            );
            return discovery_failed();
        }
    };
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            endpoint_id = %endpoint_id,
            status = status.as_u16(),
            "upstream model discovery answered with an error status"
        );
        return discovery_failed();
    }
    let payload = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(payload) => payload,
        Err(err) => {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                error = %err,
                "upstream model discovery answered with an unreadable body"
            );
            return discovery_failed();
        }
    };
    let Some(items) = payload.get("data").and_then(serde_json::Value::as_array) else {
        tracing::warn!(
            endpoint_id = %endpoint_id,
            "upstream model discovery answered without a model list"
        );
        return discovery_failed();
    };
    let snapshot = crate::endpoint_models::EndpointModelSnapshot::from_model_ids(
        items
            .iter()
            .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
            .map(str::trim)
            .filter(|id| !id.is_empty()),
    );
    state
        .endpoint_model_cache
        .put_endpoint(
            endpoint_id,
            &endpoint.base_url,
            &api_key,
            endpoint.native_api,
            snapshot.clone(),
        )
        .await;
    Json(EndpointModelsResponse {
        models: snapshot.sorted_model_ids(),
    })
    .into_response()
}

/// Bounded answer for a failed upstream discovery. The upstream body and the
/// endpoint credential stay in the server log, never in the response.
fn discovery_failed() -> Response {
    error(
        StatusCode::BAD_GATEWAY,
        "model_discovery_failed",
        "upstream model discovery failed",
    )
}

pub(super) async fn test_endpoint(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let endpoint = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    // Issue #1004 P1: a ChatGPT subscription endpoint authenticates with its
    // device credential, so the probe uses that credential instead of a
    // Platform API key.
    if endpoint.plan == db::EndpointPlan::ChatgptSubscription {
        return test_chatgpt_subscription_endpoint(
            &state,
            endpoint_id,
            endpoint.native_api,
            endpoint.native_api_source.clone(),
        )
        .await;
    }
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

    // Issue #375 Phase G: endpoint protocol check reuses the endpoint proxy
    // via the pooled client. The unified row never echoes the secret, so
    // fetch the decrypted value separately (same PATCH-carry helper as
    // `update_endpoint`). Invalid proxy fails closed (no silent direct).
    let proxy_url = match state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
    {
        Ok(value) => value,
        Err(err) => return internal(&state, err),
    };
    let client = match crate::endpoint_protocol::endpoint_protocol_client_for_endpoint(
        proxy_url.as_deref(),
        &endpoint.base_url,
    ) {
        Ok(client) => client,
        Err(message) => {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                proxy = %crate::db::redact_proxy_url_for_log(
                    proxy_url.as_deref().unwrap_or_default()
                ),
                "rejecting endpoint test with invalid proxy configuration"
            );
            return Json(EndpointTestResponse {
                ok: false,
                status: None,
                duration_ms: 0,
                model_count: None,
                native_api: None,
                native_api_source: None,
                message: truncate_message(&maybe_redact(&state, &message)),
            })
            .into_response();
        }
    };
    let url = crate::endpoint_models::models_url(&endpoint.base_url, endpoint.provider);
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

/// Issue #1004 P1: probe a ChatGPT subscription endpoint with its device
/// credential.
///
/// The probe refreshes an expired access token and then performs the Codex
/// usage GET with the account headers, so it is a credential and
/// connectivity check. It is not a generation probe and never claims that a
/// chat or responses turn would succeed. The token, the account claims and
/// the refresh secret stay inside the request path; only the outcome reaches
/// the response.
async fn test_chatgpt_subscription_endpoint(
    state: &AdminState,
    endpoint_id: Uuid,
    native_api: NativeApi,
    native_api_source: String,
) -> Response {
    let proxy_url = match state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
    {
        Ok(value) => value,
        Err(err) => return internal(state, err),
    };
    let client = match crate::endpoint_protocol::endpoint_protocol_client_for_endpoint(
        proxy_url.as_deref(),
        &chatgpt_backend::chatgpt_backend_base_url(),
    ) {
        Ok(client) => client,
        Err(message) => {
            tracing::warn!(
                endpoint_id = %endpoint_id,
                "rejecting the ChatGPT subscription probe with an invalid proxy configuration"
            );
            return Json(EndpointTestResponse {
                ok: false,
                status: None,
                duration_ms: 0,
                model_count: None,
                native_api: None,
                native_api_source: None,
                message: truncate_message(&maybe_redact(state, &message)),
            })
            .into_response();
        }
    };
    let token = match state
        .config_repository
        .get_endpoint_oauth_token(endpoint_id)
        .await
    {
        Ok(Some(token)) => token,
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "oauth_login_required",
                "complete the ChatGPT login for this endpoint first",
            );
        }
        Err(err) => return internal(state, err),
    };
    let started = Instant::now();
    let (access_token, account_id) = match chatgpt_backend::resolve_subscription_access_token(
        &state.config_repository,
        &client,
        endpoint_id,
        &token,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(err) => {
            return Json(EndpointTestResponse {
                ok: false,
                status: None,
                duration_ms: started.elapsed().as_millis(),
                model_count: None,
                native_api: None,
                native_api_source: None,
                message: truncate_message(&maybe_redact(state, &err.to_string())),
            })
            .into_response();
        }
    };
    match chatgpt_backend::fetch_chatgpt_quota(&client, &access_token, account_id.as_deref()).await
    {
        Ok(_) => Json(EndpointTestResponse {
            ok: true,
            status: Some(200),
            duration_ms: started.elapsed().as_millis(),
            model_count: None,
            native_api: Some(native_api.as_str().to_string()),
            native_api_source: Some(native_api_source),
            message: "OK, ChatGPT subscription credential verified".to_string(),
        })
        .into_response(),
        Err(err) => Json(EndpointTestResponse {
            ok: false,
            status: match &err {
                chatgpt_backend::ChatgptBackendError::Upstream { status, .. } => *status,
                _ => None,
            },
            duration_ms: started.elapsed().as_millis(),
            model_count: None,
            native_api: None,
            native_api_source: None,
            message: truncate_message(&maybe_redact(state, &err.to_string())),
        })
        .into_response(),
    }
}
