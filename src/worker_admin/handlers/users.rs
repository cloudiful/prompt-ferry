use super::*;

pub(super) async fn list_users(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match state.user_store.list_users_page(first, rows).await {
        Ok((total, users)) => Json(UserPageResponse {
            users,
            total,
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn list_user_options(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match state.user_store.list_users().await {
        Ok(users) => Json(UserOptionsResponse { users }).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CreateUserRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if body.login_name.trim().is_empty() {
        return bad_request("login_name must not be empty");
    }
    if body.password.trim().is_empty() {
        return bad_request("password must not be empty");
    }
    if body.display_name.trim().is_empty() {
        return bad_request("display_name must not be empty");
    }
    let password_hash = match hash_password(&body.password) {
        Ok(hash) => hash,
        Err(err) => return internal(&state, err),
    };
    match state
        .user_store
        .create_user(UserCreate {
            login_name: body.login_name,
            password_hash,
            display_name: body.display_name,
            is_admin: body.is_admin.unwrap_or(false),
        })
        .await
    {
        Ok(user) => Json(user).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<UserUpdate>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match state.user_store.update_user(user_id, body).await {
        Ok(Some(user)) => Json(user).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "user not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn reset_password(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<ResetPasswordRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if body.password.trim().is_empty() {
        return bad_request("password must not be empty");
    }
    let password_hash = match hash_password(&body.password) {
        Ok(hash) => hash,
        Err(err) => return internal(&state, err),
    };
    match state
        .user_store
        .reset_password(user_id, password_hash)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "user not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match state.user_store.delete_user(user_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "user not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn list_client_keys(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match db::list_client_keys_page(&state.pool, user_id, first, rows).await {
        Ok((total, keys)) => Json(ClientKeyPageResponse {
            keys,
            total,
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<CreateClientKeyRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    let (secret, prefix, hash) = generate_client_key();
    match db::create_client_key(
        &state.pool,
        user_id,
        body.label.as_deref().unwrap_or("Codex key"),
        &prefix,
        &hash,
        &secret,
    )
    .await
    {
        Ok(key) => {
            let _ = publish_snapshot(&state).await;
            Json(CreateClientKeyResponse {
                key_id: key.key_id,
                user_id: key.user_id,
                key_prefix: key.key_prefix,
                label: key.label,
                enabled: key.enabled,
                created_at: key.created_at,
                secret,
            })
            .into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path((user_id, key_id)): Path<(i64, i64)>,
    Json(body): Json<UpdateClientKeyRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    match db::update_client_key(&state.pool, user_id, key_id, body.label, body.enabled).await {
        Ok(Some(key)) => {
            let _ = publish_snapshot(&state).await;
            Json(key).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn delete_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path((user_id, key_id)): Path<(i64, i64)>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    if state.user_store.is_sqlite() {
        return state.sqlite_capability_unavailable();
    }
    match db::delete_client_key(&state.pool, user_id, key_id).await {
        Ok(true) => {
            let _ = publish_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}
