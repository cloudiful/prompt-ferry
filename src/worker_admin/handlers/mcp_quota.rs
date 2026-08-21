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
