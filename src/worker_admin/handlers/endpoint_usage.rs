use super::*;

pub(super) async fn token_plan_usage(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let unified = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    // Issue #599 R2c: ChatGPT subscription endpoints have no provider
    // token-plan API. Their quota is the display-only 5h/week window pair from
    // the Codex backend, adapted into the shared window shape. It never enters
    // the routing weight cache and stays separate from Platform API usage.
    // Attempt 4: the unified read above is dual-backend (PG/SQLite) so the
    // SQLite-backed admin state reaches this branch; the PG-only read below
    // stays for the other providers to keep their behavior identical.
    if unified.provider == db::EndpointProvider::OpenAi {
        let endpoint = unified.into_pg();
        return chatgpt_subscription_usage(&state, endpoint_id, &endpoint).await;
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
        // OpenAI (issue #589 P1) has no token-plan surface yet; the
        // organization-level usage view lands in P2.
        db::EndpointProvider::Generic | db::EndpointProvider::OpenAi => {
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

/// Issue #599 R2c: display-only ChatGPT subscription quota. One synthetic key
/// entry carries the 5h/week rate-limit windows in `model_remains`, so the
/// existing window rendering and countdown helpers apply unchanged.
async fn chatgpt_subscription_usage(
    state: &AdminState,
    endpoint_id: Uuid,
    endpoint: &db::ProviderEndpoint,
) -> Response {
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
                "complete the ChatGPT OAuth login for this endpoint first",
            );
        }
        Err(err) => return internal(state, err),
    };
    let proxy_url = match state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
    {
        Ok(proxy_url) => proxy_url,
        Err(err) => return internal(state, err),
    };
    let client = match crate::endpoint_protocol::endpoint_protocol_client_for_endpoint(
        proxy_url.as_deref(),
        &chatgpt_backend::chatgpt_backend_base_url(),
    ) {
        Ok(client) => client,
        Err(message) => {
            return error(
                StatusCode::BAD_REQUEST,
                "invalid_proxy_url",
                &truncate_message(&maybe_redact(state, &message)),
            );
        }
    };
    let account_id = chatgpt_backend::codex_account_id_from_access_token(&token.access_token);
    match chatgpt_backend::fetch_chatgpt_quota(&client, &token.access_token, account_id.as_deref())
        .await
    {
        Ok(quota) => Json(chatgpt_quota_response(endpoint, quota)).into_response(),
        Err(err) => error(
            StatusCode::BAD_GATEWAY,
            "chatgpt_quota_unavailable",
            &truncate_message(&maybe_redact(state, &err.to_string())),
        ),
    }
}

fn chatgpt_quota_response(
    endpoint: &db::ProviderEndpoint,
    quota: chatgpt_backend::ChatgptQuota,
) -> TokenPlanUsageResponse {
    let key_label = quota
        .plan_type
        .clone()
        .map(|plan_type| format!("ChatGPT {plan_type}"))
        .unwrap_or_else(|| "ChatGPT subscription".to_string());
    let windows_present = quota.primary.is_some() || quota.secondary.is_some();
    let model_remains = if windows_present {
        vec![TokenPlanModelUsage {
            model_name: quota
                .plan_type
                .clone()
                .unwrap_or_else(|| "chatgpt".to_string()),
            interval: quota.primary.as_ref().map(chatgpt_quota_window),
            weekly: quota.secondary.as_ref().map(chatgpt_quota_window),
        }]
    } else {
        Vec::new()
    };
    TokenPlanUsageResponse {
        provider: endpoint.provider,
        provider_region: endpoint.provider_region,
        keys: vec![TokenPlanKeyUsage {
            key_id: endpoint
                .api_keys
                .first()
                .map(|key| key.key_id)
                .unwrap_or_default(),
            key_label,
            ok: true,
            status: None,
            error_code: None,
            error_message: None,
            model_remains,
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
        }],
        local_today_tokens: None,
    }
}

fn chatgpt_quota_window(window: &chatgpt_backend::ChatgptQuotaWindow) -> TokenPlanWindowUsage {
    TokenPlanWindowUsage {
        status: None,
        remaining_percent: window
            .used_percent
            .map(|used| (100.0 - used).clamp(0.0, 100.0)),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at: window.reset_at,
        remains_time_ms: window
            .reset_after_seconds
            .and_then(|seconds| seconds.checked_mul(1000)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin::chatgpt_backend::{ChatgptQuota, ChatgptQuotaWindow};

    fn endpoint_fixture() -> db::ProviderEndpoint {
        let now = Utc::now();
        db::ProviderEndpoint {
            endpoint_id: Uuid::new_v4(),
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "openai".to_string(),
            provider: db::EndpointProvider::OpenAi,
            provider_region: None,
            plan: db::EndpointPlan::ChatgptSubscription,
            service_tier: db::MinimaxServiceTier::Standard,
            base_url: "https://api.openai.com/v1".to_string(),
            native_api: "responses".to_string(),
            native_api_source: "manual".to_string(),
            api_key: "platform-key".to_string(),
            proxy_url: None,
            has_oauth_token: true,
            has_proxy_url: false,
            active_windows: Vec::new(),
            key_lb_enabled: false,
            enabled: true,
            mcp_enabled: false,
            created_at: now,
            updated_at: now,
            api_keys: Vec::new(),
        }
    }

    fn window(used_percent: f64, reset_after_seconds: i64) -> ChatgptQuotaWindow {
        ChatgptQuotaWindow {
            used_percent: Some(used_percent),
            limit_window_seconds: Some(18_000),
            reset_after_seconds: Some(reset_after_seconds),
            reset_at: Some(Utc::now() + chrono::Duration::seconds(reset_after_seconds)),
        }
    }

    fn quota(
        primary: Option<ChatgptQuotaWindow>,
        secondary: Option<ChatgptQuotaWindow>,
    ) -> ChatgptQuota {
        ChatgptQuota {
            plan_type: Some("plus".to_string()),
            limit_reached: Some(false),
            primary,
            secondary,
            has_credits: Some(true),
            unlimited_credits: Some(false),
            credits_balance: Some("12.5".to_string()),
        }
    }

    #[test]
    fn quota_response_maps_percent_windows_into_model_remains() {
        let response = chatgpt_quota_response(
            &endpoint_fixture(),
            quota(Some(window(25.0, 3_600)), Some(window(120.0, 86_400))),
        );
        assert_eq!(response.provider, db::EndpointProvider::OpenAi);
        let key = &response.keys[0];
        assert!(key.ok);
        assert_eq!(key.key_label, "ChatGPT plus");
        assert_eq!(key.key_id, Uuid::nil());
        let model = &key.model_remains[0];
        assert_eq!(model.model_name, "plus");
        let interval = model.interval.as_ref().expect("5h window");
        assert_eq!(interval.remaining_percent, Some(75.0));
        assert_eq!(interval.remains_time_ms, Some(3_600_000));
        let weekly = model.weekly.as_ref().expect("weekly window");
        // A used share above 100 clamps to zero remaining instead of going negative.
        assert_eq!(weekly.remaining_percent, Some(0.0));
        assert_eq!(weekly.remains_time_ms, Some(86_400_000));
    }

    #[test]
    fn quota_response_without_windows_stays_degraded() {
        let response = chatgpt_quota_response(&endpoint_fixture(), quota(None, None));
        assert!(response.keys[0].model_remains.is_empty());
        assert!(response.keys[0].ok);
    }
}
