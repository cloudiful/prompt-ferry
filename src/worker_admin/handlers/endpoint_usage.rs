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
    let provider_label = match endpoint.provider {
        db::EndpointProvider::Minimax => "MiniMax",
        db::EndpointProvider::CommandCode => "CommandCode",
        db::EndpointProvider::OpencodeGo => "OpencodeGo",
        db::EndpointProvider::OpenRouter => "OpenRouter",
        // GLM (issue #230 P2): now has its own Coding Plan usage fetcher
        // (quota/limit); the cache allowlist lets the request flow through
        // the same way as the other four providers. Generic still has no
        // token plan API.
        db::EndpointProvider::Glm => "GLM",
        // DeepSeek (issue #287 P0): account balance fetcher (`/user/balance`).
        db::EndpointProvider::DeepSeek => "DeepSeek",
        db::EndpointProvider::Generic => {
            return error(
                StatusCode::BAD_REQUEST,
                "unsupported_provider",
                "token plan usage is only available for MiniMax, CommandCode, OpencodeGo, OpenRouter, GLM and DeepSeek endpoints",
            );
        }
    };
    // Region stays mandatory only for MiniMax; the other presets
    // (CommandCode, OpencodeGo, OpenRouter, GLM, DeepSeek) carry no region
    // (NULL) and must not be rejected here.
    if endpoint.provider == db::EndpointProvider::Minimax && endpoint.provider_region.is_none() {
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
            &format!("{provider_label} endpoint has no enabled API key"),
        );
    }

    match state
        .token_plan_quota
        .refresh_if_due(&state.pool, endpoint_id)
        .await
    {
        Ok(Some(mut usage)) => {
            // Balance-based providers (issue #287 P2) pair the account balance
            // with a locally aggregated "today usage" pill. OpenRouter reports
            // its own spend so this is only a fallback there; DeepSeek has no
            // spend endpoint and always relies on the local figure. The value
            // is attached to the returned clone so the routing cache keeps the
            // provider-only snapshot.
            if matches!(
                endpoint.provider,
                db::EndpointProvider::OpenRouter | db::EndpointProvider::DeepSeek,
            ) {
                match db::endpoint_today_tokens(&state.pool, endpoint_id, chrono::Utc::now()).await
                {
                    Ok(tokens) => usage.local_today_tokens = Some(tokens),
                    Err(err) => tracing::warn!(
                        endpoint_id = %endpoint_id,
                        error = %err,
                        "failed to aggregate local today tokens for token-plan badge"
                    ),
                }
            }
            Json(usage).into_response()
        }
        Ok(None) => error(
            StatusCode::BAD_REQUEST,
            "unsupported_provider",
            "token plan usage is only available for MiniMax, CommandCode, OpencodeGo, OpenRouter, GLM and DeepSeek endpoints",
        ),
        Err(err) => internal(&state, err),
    }
}
