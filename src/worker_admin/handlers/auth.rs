use super::*;

pub(super) async fn login(
    State(state): State<AdminState>,
    Json(body): Json<LoginRequest>,
) -> Response {
    match db::get_user_password_by_login(&state.pool, &body.login_name).await {
        Ok(Some(user))
            if user.is_active && verify_password(&body.password, &user.password_hash) =>
        {
            let session_id = new_session_id();
            let session_user = SessionUser {
                user_id: user.user_id,
                login_name: user.login_name,
                display_name: user.display_name,
                is_admin: user.is_admin,
            };
            if let Err(err) = state
                .replay_cache
                .write_session(&session_id, &session_user)
                .await
            {
                tracing::warn!(error = %maybe_redact(&state, &err.to_string()), "session backend unavailable");
                return error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "service_unavailable",
                    "session backend unavailable",
                );
            }
            (
                StatusCode::NO_CONTENT,
                [(
                    header::SET_COOKIE,
                    format!("{SESSION_COOKIE_NAME}={session_id}; Path=/; HttpOnly; SameSite=Lax"),
                )],
            )
                .into_response()
        }
        _ => error(StatusCode::UNAUTHORIZED, "unauthorized", "invalid login"),
    }
}

pub(super) async fn logout(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Some(session_id) = session_id(&headers)
        && let Err(err) = state.replay_cache.delete_session(session_id).await
    {
        tracing::warn!(error = %maybe_redact(&state, &err.to_string()), "failed to delete session");
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "session backend unavailable",
        );
    }
    (
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            format!("{SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"),
        )],
    )
        .into_response()
}

pub(super) async fn me(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    match current_user(&state, &headers).await {
        Ok(user) => Json(MeResponse {
            user_id: user.user_id,
            login_name: user.login_name,
            display_name: user.display_name,
            is_admin: user.is_admin,
        })
        .into_response(),
        Err(response) => response,
    }
}
