use super::*;

const CLIENT_KEY_LIMIT: i64 = 10;

pub(super) async fn list_users(State(state): State<AdminState>, headers: HeaderMap) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::list_users(&state.pool).await {
        Ok(users) => Json(users).into_response(),
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
    let password_hash = match hash_password(&body.password) {
        Ok(hash) => hash,
        Err(err) => return internal(&state, err),
    };
    match db::create_user(
        &state.pool,
        UserCreate {
            login_name: body.login_name,
            password_hash,
            display_name: body.display_name,
            is_admin: body.is_admin.unwrap_or(false),
        },
    )
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
    match db::update_user(&state.pool, user_id, body).await {
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
    let password_hash = match hash_password(&body.password) {
        Ok(hash) => hash,
        Err(err) => return internal(&state, err),
    };
    match db::reset_password(&state.pool, user_id, password_hash).await {
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
    match db::delete_user(&state.pool, user_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "user not found"),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn list_client_keys(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response;
    }
    match db::list_client_keys(&state.pool, user_id).await {
        Ok(keys) => Json(keys).into_response(),
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
    match db::count_client_keys(&state.pool, user_id).await {
        Ok(count) if count >= CLIENT_KEY_LIMIT => {
            return error(
                StatusCode::BAD_REQUEST,
                "client_key_limit_exceeded",
                "client key limit exceeded",
            );
        }
        Ok(_) => {}
        Err(err) => return internal(&state, err),
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
    match db::delete_client_key(&state.pool, user_id, key_id).await {
        Ok(true) => {
            let _ = publish_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}
