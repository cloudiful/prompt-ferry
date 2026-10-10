use super::super::chatgpt_quota_normalize;
use super::super::openai_org_usage;
use super::super::token_plan;
use super::*;

/// Token-plan usage exists for the six preset providers with a fetcher; every
/// other provider gets this one answer, whether it is rejected up front or
/// drops out of the quota cache.
const TOKEN_PLAN_UNSUPPORTED_MESSAGE: &str = "token plan usage is only available for MiniMax, CommandCode, OpencodeGo, OpenRouter, GLM and DeepSeek endpoints";

fn unsupported_token_plan_provider() -> Response {
    error(
        StatusCode::BAD_REQUEST,
        "unsupported_provider",
        TOKEN_PLAN_UNSUPPORTED_MESSAGE,
    )
}

pub(super) async fn token_plan_usage(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(endpoint_id): Path<Uuid>,
    Query(query): Query<TokenPlanUsageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    let unified = match state.config_repository.get_endpoint(endpoint_id).await {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "endpoint not found"),
        Err(err) => return internal(&state, err),
    };
    // ChatGPT subscription quota is display-only and remains separate from
    // routing weights and Platform API usage.
    // Attempt 4: the unified read above is dual-backend (PG/SQLite) so the
    // SQLite-backed admin state reaches this branch; the PG-only read below
    // stays for the other providers to keep their behavior identical.
    if unified.provider == db::EndpointProvider::OpenAi {
        let endpoint = unified.into_pg();
        return chatgpt_subscription_usage(
            &state,
            endpoint_id,
            &endpoint,
            query.force.unwrap_or(false),
        )
        .await;
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
            return unsupported_token_plan_provider();
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
    if !token_plan::has_enabled_key(&endpoint) {
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
        Ok(None) => unsupported_token_plan_provider(),
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
    force: bool,
) -> Response {
    if let Some(service) = state.chatgpt_quota_service.as_ref() {
        return match service.read(endpoint, force).await {
            Ok(usage) => Json(usage).into_response(),
            Err(crate::worker_admin::chatgpt_quota_service::ChatGptQuotaError::NotConfigured) => {
                error(
                    StatusCode::BAD_REQUEST,
                    "oauth_login_required",
                    "complete the ChatGPT OAuth login for this endpoint first",
                )
            }
            Err(error_code) => {
                let status = if error_code
                    == crate::worker_admin::chatgpt_quota_service::ChatGptQuotaError::Storage
                {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::BAD_GATEWAY
                };
                error(
                    status,
                    &format!("chatgpt_quota_{}", error_code.code()),
                    "ChatGPT subscription quota is unavailable",
                )
            }
        };
    }
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
    chatgpt_quota_normalize::response_from_quota(endpoint, quota)
}

/// Issue #589 P2b: OpenAI Platform organization usage and spend. Reads the
/// official `usage/completions` and `costs` endpoints with the endpoint's
/// dedicated Admin API key and returns UTC month-to-date token and USD totals.
/// Display-only: it never feeds routing weights or quota.
pub(super) async fn organization_usage(
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
    if endpoint.provider != db::EndpointProvider::OpenAi {
        return error(
            StatusCode::BAD_REQUEST,
            "unsupported_provider",
            "organization usage is only available for OpenAI endpoints",
        );
    }
    let admin_api_key = match state
        .config_repository
        .endpoint_admin_api_key(endpoint_id)
        .await
    {
        Ok(Some(key)) => key,
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "missing_admin_api_key",
                "configure an OpenAI Admin API key for this endpoint to read organization usage",
            );
        }
        Err(err) => return internal(&state, err),
    };
    let proxy_url = match state
        .config_repository
        .endpoint_proxy_url(endpoint_id)
        .await
    {
        Ok(proxy_url) => proxy_url,
        Err(err) => return internal(&state, err),
    };
    match state
        .openai_org_usage
        .get_or_refresh(endpoint_id, || {
            // The stored inference base is passed through but ignored: the
            // organization Admin API is pinned to the official Platform
            // origin so the privileged Admin key never follows a custom base.
            openai_org_usage::fetch_organization_usage(
                &endpoint.base_url,
                &admin_api_key,
                proxy_url.as_deref(),
                Utc::now(),
            )
        })
        .await
    {
        Ok(usage) => Json(usage).into_response(),
        Err(err) => error(
            StatusCode::BAD_GATEWAY,
            "organization_usage_unavailable",
            &truncate_message(&maybe_redact(&state, &err.to_string())),
        ),
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
            service_tier: None,
            base_url: "https://api.openai.com/v1".to_string(),
            native_api: "responses".to_string(),
            native_api_source: "manual".to_string(),
            api_key: "platform-key".to_string(),
            proxy_url: None,
            has_oauth_token: true,
            has_proxy_url: false,
            has_admin_api_key: false,
            active_windows: Vec::new(),
            key_lb_enabled: false,
            enabled: true,
            mcp_enabled: false,
            created_at: now,
            updated_at: now,
            api_keys: Vec::new(),
        }
    }

    fn window(
        used_percent: Option<f64>,
        window_seconds: i64,
        reset_after_seconds: i64,
    ) -> ChatgptQuotaWindow {
        ChatgptQuotaWindow {
            used_percent,
            limit_window_seconds: Some(window_seconds),
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
    fn quota_response_emits_canonical_windows_and_no_positional_slots() {
        let response = chatgpt_quota_response(
            &endpoint_fixture(),
            quota(
                Some(window(Some(25.0), 18_000, 3_600)),
                Some(window(Some(100.0), 604_800, 86_400)),
            ),
        );
        assert_eq!(response.provider, db::EndpointProvider::OpenAi);
        let key = &response.keys[0];
        assert!(key.ok);
        assert_eq!(key.key_label, "ChatGPT plus");
        assert_eq!(key.key_id, Uuid::nil());
        let model = &key.model_remains[0];
        assert_eq!(model.model_name, "plus");
        // The positional slots are never fabricated for ChatGPT.
        assert!(model.interval.is_none());
        assert!(model.weekly.is_none());
        let windows = model.windows.as_ref().expect("canonical windows");
        assert_eq!(windows.len(), 2);
        let primary = &windows[0];
        assert_eq!(primary.source_window, "primary");
        assert_eq!(primary.window_seconds, Some(18_000));
        assert_eq!(primary.used_percent, Some(25.0));
        assert_eq!(primary.remaining_percent, Some(75.0));
        assert_eq!(primary.availability, SubscriptionWindowAvailability::Known);
        assert!(primary.reset_at.is_some());
        assert_eq!(primary.reset_after_seconds, Some(3_600));
        let secondary = &windows[1];
        assert_eq!(secondary.source_window, "secondary");
        assert_eq!(secondary.window_seconds, Some(604_800));
        // A genuine exhausted window keeps its real zero.
        assert_eq!(secondary.used_percent, Some(100.0));
        assert_eq!(secondary.remaining_percent, Some(0.0));
        assert_eq!(
            secondary.availability,
            SubscriptionWindowAvailability::Known
        );
    }

    #[test]
    fn quota_response_with_a_weekly_only_window_invents_no_five_hour_slot() {
        let response = chatgpt_quota_response(
            &endpoint_fixture(),
            quota(Some(window(Some(2.0), 604_800, 86_400)), None),
        );
        let model = &response.keys[0].model_remains[0];
        let windows = model.windows.as_ref().expect("canonical windows");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window_seconds, Some(604_800));
        assert_eq!(windows[0].remaining_percent, Some(98.0));
    }

    #[test]
    fn quota_response_keeps_an_unknown_percentage_unknown() {
        let response = chatgpt_quota_response(
            &endpoint_fixture(),
            quota(Some(window(None, 604_800, 0)), None),
        );
        let model = &response.keys[0].model_remains[0];
        let windows = model.windows.as_ref().expect("canonical windows");
        assert_eq!(windows[0].used_percent, None);
        assert_eq!(windows[0].remaining_percent, None);
        assert_eq!(
            windows[0].availability,
            SubscriptionWindowAvailability::Unknown
        );
    }

    #[test]
    fn quota_response_without_windows_stays_degraded() {
        let response = chatgpt_quota_response(&endpoint_fixture(), quota(None, None));
        assert!(response.keys[0].model_remains.is_empty());
        assert!(response.keys[0].ok);
    }
}
