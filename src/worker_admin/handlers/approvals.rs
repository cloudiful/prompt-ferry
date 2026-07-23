use super::*;

pub(super) async fn list_approvals(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<ApprovalPageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::list_approval_requests_page(
        &state.pool,
        query.status.unwrap_or_default(),
        query.first.unwrap_or(0),
        query.rows.unwrap_or(10),
    )
    .await
    {
        Ok(page) => Json(ApprovalPageResponse::from(page)).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn get_approval(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(approval_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::get_approval_request(&state.pool, approval_id).await {
        Ok(Some(approval)) => Json(approval).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "approval not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn approve_approval(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(approval_id): Path<Uuid>,
) -> Response {
    let user = match ensure_admin(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    resolve_approval(
        &state,
        approval_id,
        ApprovalStatus::Approved,
        ApprovalResolution::Approved,
        user.user_id,
    )
    .await
}

pub(super) async fn reject_approval(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(approval_id): Path<Uuid>,
) -> Response {
    let user = match ensure_admin(&state, &headers).await {
        Ok(user) => user,
        Err(response) => return response,
    };
    resolve_approval(
        &state,
        approval_id,
        ApprovalStatus::Rejected,
        ApprovalResolution::Rejected,
        user.user_id,
    )
    .await
}

async fn resolve_approval(
    state: &AdminState,
    approval_id: Uuid,
    status: ApprovalStatus,
    resolution: ApprovalResolution,
    decided_by_user_id: i64,
) -> Response {
    let approval = match db::resolve_approval_request(
        &state.pool,
        approval_id,
        status,
        Some(decided_by_user_id),
    )
    .await
    {
        Ok(Some(approval)) => approval,
        Ok(None) => match db::approval_request_status(&state.pool, approval_id).await {
            Ok(Some((current_status, _))) => {
                return error(
                    StatusCode::CONFLICT,
                    "invalid_state",
                    &format!("approval is already {current_status}"),
                );
            }
            Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "approval not found"),
            Err(err) => return internal(state, err),
        },
        Err(err) => return internal(state, err),
    };

    if let Some(waiter) = state.approval_waiters.lock().await.remove(&approval_id) {
        let _ = waiter.send(resolution);
    }
    let llm_review_settings = state.llm_review_settings.read().await.clone();
    spawn_approval_webhook(
        state.pool.clone(),
        reqwest::Client::new(),
        llm_review_settings,
        match status {
            ApprovalStatus::Approved => "approval.approved",
            ApprovalStatus::Rejected => "approval.rejected",
            ApprovalStatus::Pending => "approval.pending",
            ApprovalStatus::Expired => "approval.expired",
            ApprovalStatus::Aborted => "approval.aborted",
        },
        approval.clone(),
    );
    Json(approval).into_response()
}
