use super::*;
use crate::{
    db::{self, McpQuotaGroupInput},
    worker_admin_types::{
        CredentialPageResponse, CredentialQuotaBindingRequest, QuotaGroupRequest,
        QuotaGroupUsageResponse,
    },
};
pub(super) async fn list_quota_groups(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    match db::list_quota_groups(&state.pool).await {
        Ok(groups) => Json(groups).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_quota_group(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<QuotaGroupRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    if let Err(response) = validate_quota_group(&body, None) {
        return response;
    }
    match db::create_quota_group(&state.pool, McpQuotaGroupInput::from(body)).await {
        Ok(group) => Json(group).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_quota_group(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(group_id): Path<uuid::Uuid>,
    Json(body): Json<QuotaGroupRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    if let Err(response) = validate_quota_group(&body, Some(group_id)) {
        return response;
    }
    match db::update_quota_group(&state.pool, group_id, McpQuotaGroupInput::from(body)).await {
        Ok(Some(group)) => Json(group).into_response(),
        Ok(None) => not_found(&state, "quota group not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_quota_group(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(group_id): Path<uuid::Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    match db::delete_quota_group(&state.pool, group_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(&state, "quota group not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn quota_group_usage(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(group_id): Path<uuid::Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    let Some(group) = (match db::get_quota_group(&state.pool, group_id).await {
        Ok(group) => group,
        Err(err) => return internal(&state, err),
    }) else {
        return not_found(&state, "quota group not found");
    };
    let now = chrono::Utc::now();
    let day_period = db::current_day_period(now);
    let month_period = db::current_month_period(&group, now);
    let (day, month) = match (
        db::load_accounts_for_group(&state.pool, group_id, "day", day_period.start).await,
        db::load_accounts_for_group(&state.pool, group_id, "month", month_period.start).await,
    ) {
        (Ok(day), Ok(month)) => (day.into_iter().next(), month.into_iter().next()),
        (Err(err), _) | (_, Err(err)) => return internal(&state, err),
    };
    Json(QuotaGroupUsageResponse { group, day, month }).into_response()
}

pub(super) async fn list_server_credentials(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(server_id): Path<uuid::Uuid>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    match state
        .config_repository
        .list_mcp_credentials(server_id)
        .await
    {
        Ok(credentials) => {
            let total = credentials.len() as i64;
            let credentials = credentials
                .into_iter()
                .map(db::McpCredentialView::from)
                .collect();
            Json(CredentialPageResponse { credentials, total }).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn bind_credential_group(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path((server_id, credential_id)): Path<(uuid::Uuid, uuid::Uuid)>,
    Json(body): Json<CredentialQuotaBindingRequest>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    if let Some(group_id) = body.quota_group_id {
        match db::get_quota_group(&state.pool, group_id).await {
            Ok(Some(_)) => {}
            Ok(None) => return not_found(&state, "quota group not found"),
            Err(err) => return internal(&state, err),
        }
    }
    let credentials = match db::list_credentials_by_server(&state.pool, server_id).await {
        Ok(credentials) => credentials,
        Err(err) => return internal(&state, err),
    };
    if !credentials
        .iter()
        .any(|credential| credential.credential_id == credential_id)
    {
        return not_found(&state, "credential not found");
    }
    let bound =
        match db::set_credential_quota_group(&state.pool, credential_id, body.quota_group_id).await
        {
            Ok(bound) => bound,
            Err(err) => return internal(&state, err),
        };
    if !bound {
        return not_found(&state, "credential not found");
    }
    match db::list_credentials_by_server(&state.pool, server_id).await {
        Ok(credentials) => {
            let Some(credential) = credentials
                .into_iter()
                .find(|credential| credential.credential_id == credential_id)
            else {
                return not_found(&state, "credential not found");
            };
            Json(db::McpCredentialView::from(credential)).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

/// Refresh the provider balance for one credential through its provider's
/// balance adapter. Only a canonical hosted provider whose registry descriptor
/// reports balance support (currently Firecrawl) may be refreshed. Generic,
/// Context7, and MiniMax credentials are rejected without touching the secret,
/// and the SQLite capability gate keeps the endpoint unavailable there.
pub(super) async fn refresh_server_credential_balance(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path((server_id, credential_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Response {
    let user = match current_user(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    if !user.is_admin {
        return forbidden(&state, &user);
    }
    let server = match state.config_repository.get_mcp_server(server_id).await {
        Ok(Some(server)) => server,
        Ok(None) => return not_found(&state, "MCP server not found"),
        Err(err) => return internal(&state, err),
    };
    let provider_kind = db::canonical_mcp_provider_kind(server.provider_kind.as_deref());
    if !provider_balance_refresh_supported(provider_kind) {
        return error(
            StatusCode::BAD_REQUEST,
            "provider_balance_unsupported",
            "provider balance refresh is not supported for this provider",
        );
    }
    // The owning server is the source of truth for the credential provider;
    // reconcile pre-existing rows before the provider-specific lookup so a
    // stale credential kind can never redirect the call.
    if let Err(err) = db::backfill_credential_provider_kinds(&state.pool).await {
        return internal(&state, err);
    }
    let credential = match db::list_credentials_by_server(&state.pool, server_id).await {
        Ok(credentials) => credentials
            .into_iter()
            .find(|credential| credential.credential_id == credential_id),
        Err(err) => return internal(&state, err),
    };
    let Some(credential) = credential else {
        return not_found(&state, "credential not found");
    };
    let secret = db::McpProviderSecret {
        credential_id: credential.credential_id,
        secret: credential.secret.clone(),
    };
    let client = crate::mcp::FirecrawlBalanceClient::new();
    match crate::mcp::refresh_firecrawl_credential(
        &state.pool,
        &state.mcp_quota_valkey,
        &client,
        &secret,
    )
    .await
    {
        Ok(_) => match db::list_credentials_by_server(&state.pool, server_id).await {
            Ok(credentials) => credentials
                .into_iter()
                .find(|credential| credential.credential_id == credential_id)
                .map(|credential| Json(db::McpCredentialView::from(credential)).into_response())
                .unwrap_or_else(|| not_found(&state, "credential not found")),
            Err(err) => internal(&state, err),
        },
        Err(crate::mcp::ProviderBalanceError::Persistence) => internal(
            &state,
            anyhow::anyhow!("provider balance could not be persisted"),
        ),
        Err(err) => error(
            StatusCode::BAD_GATEWAY,
            "provider_balance_refresh_failed",
            &err.to_string(),
        ),
    }
}

fn validate_quota_group(
    body: &QuotaGroupRequest,
    group_id: Option<uuid::Uuid>,
) -> Result<(), Response> {
    if body.name.trim().is_empty() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_name",
            "quota group name is required",
        ));
    }
    if body.daily_limit.is_some_and(|value| value < 0.0)
        || body.monthly_limit.is_some_and(|value| value < 0.0)
        || body.default_cost.is_some_and(|value| value < 0.0)
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_limit",
            "quota limits and default cost must be non-negative",
        ));
    }
    if let (Some(start), Some(end)) = (body.billing_period_start, body.billing_period_end)
        && end <= start
    {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "invalid_period",
            "billing_period_end must be after billing_period_start",
        ));
    }
    let _ = group_id;
    Ok(())
}

fn forbidden(_state: &AdminState, user: &SessionUser) -> Response {
    tracing::warn!(user_id = user.user_id, "quota group admin access denied");
    error(
        StatusCode::FORBIDDEN,
        "forbidden",
        "admin required for quota group management",
    )
}

fn not_found(_state: &AdminState, message: &str) -> Response {
    error(StatusCode::NOT_FOUND, "not_found", message)
}

fn internal(state: &AdminState, err: anyhow::Error) -> Response {
    super::internal(state, err)
}

/// Only a canonical hosted provider whose registry descriptor reports balance
/// support (currently Firecrawl) may run an on-demand balance refresh. Generic,
/// legacy/unknown, Context7, and MiniMax servers are rejected.
fn provider_balance_refresh_supported(provider_kind: Option<&str>) -> bool {
    provider_kind == Some(db::MCP_PROVIDER_FIRECRAWL)
        && db::mcp_provider_info(provider_kind).is_some_and(|info| info.provider_balance_supported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_firecrawl_supports_a_provider_balance_refresh() {
        assert!(provider_balance_refresh_supported(Some("firecrawl")));
        // Canonical Context7/MiniMax are known presets but expose no balance.
        assert!(!provider_balance_refresh_supported(Some("context7")));
        assert!(!provider_balance_refresh_supported(Some("minimax")));
        // Generic/legacy/unknown never trigger a provider call.
        assert!(!provider_balance_refresh_supported(Some("generic")));
        assert!(!provider_balance_refresh_supported(Some("legacy-unknown")));
        assert!(!provider_balance_refresh_supported(None));
    }
}
