use super::*;

use crate::db::config_repository::client_keys::parse_client_key_identifier;
use crate::openapi_export::schemas::ErrorEnvelope;

impl From<db::UnifiedClientKey> for ClientKey {
    fn from(key: db::UnifiedClientKey) -> Self {
        Self {
            key_id: key.key_id,
            user_id: key.user_id,
            key_prefix: key.key_prefix,
            label: key.label,
            enabled: key.enabled,
            last_used_at: None,
            created_at: key.created_at,
            secret: key.secret,
        }
    }
}

impl From<db::UnifiedClientKeyCreated> for CreateClientKeyResponse {
    fn from(created: db::UnifiedClientKeyCreated) -> Self {
        Self {
            key_id: created.key.key_id,
            user_id: created.key.user_id,
            key_prefix: created.key.key_prefix,
            label: created.key.label,
            enabled: created.key.enabled,
            created_at: created.key.created_at,
            secret: created.secret,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users",
    params(TablePageQuery),
    responses(
        (status = 200, body = UserPageResponse, description = "Users"),
        (status = 401, body = ErrorEnvelope, description = "Not authenticated"),
        (status = 403, body = ErrorEnvelope, description = "Administrator access required")
    ),
    tag = "users"
)]
pub(super) async fn list_users(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::User).await {
        return response.into_response();
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

#[utoipa::path(
    get,
    path = "/api/v1/admin/users/options",
    responses(
        (status = 200, body = UserOptionsResponse, description = "User options"),
        (status = 401, body = ErrorEnvelope, description = "Not authenticated"),
        (status = 403, body = ErrorEnvelope, description = "Administrator access required")
    ),
    tag = "users"
)]
pub(super) async fn list_user_options(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::User).await {
        return response.into_response();
    }
    match state.user_store.list_users().await {
        Ok(users) => Json(UserOptionsResponse { users }).into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users",
    request_body = CreateUserRequest,
    responses(
        (status = 200, body = db::User, description = "Created user"),
        (status = 400, body = ErrorEnvelope, description = "Invalid user request"),
        (status = 401, body = ErrorEnvelope, description = "Not authenticated"),
        (status = 403, body = ErrorEnvelope, description = "Administrator access required")
    ),
    tag = "users"
)]
pub(super) async fn create_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CreateUserRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::User).await {
        return response.into_response();
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

#[utoipa::path(
    patch,
    path = "/api/v1/admin/users/{user_id}",
    params(("user_id" = i64, Path, description = "User ID")),
    request_body = db::UserUpdate,
    responses(
        (status = 200, body = db::User, description = "Updated user"),
        (status = 404, body = ErrorEnvelope, description = "User not found")
    ),
    tag = "users"
)]
pub(super) async fn update_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<UserUpdate>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::User).await {
        return response.into_response();
    }
    match state.user_store.update_user(user_id, body).await {
        Ok(Some(user)) => Json(user).into_response(),
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "user not found"),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/reset-password",
    params(("user_id" = i64, Path, description = "User ID")),
    request_body = ResetPasswordRequest,
    responses(
        (status = 204, description = "Password reset"),
        (status = 404, body = ErrorEnvelope, description = "User not found")
    ),
    tag = "users"
)]
pub(super) async fn reset_password(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<ResetPasswordRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::User).await {
        return response.into_response();
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

#[utoipa::path(
    delete,
    path = "/api/v1/admin/users/{user_id}",
    params(("user_id" = i64, Path, description = "User ID")),
    responses(
        (status = 204, description = "Deleted user"),
        (status = 404, body = ErrorEnvelope, description = "User not found")
    ),
    tag = "users"
)]
pub(super) async fn delete_user(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::User).await {
        return response.into_response();
    }
    match state.user_store.delete_user(user_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "user not found"),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users/{user_id}/client-keys",
    params(("user_id" = i64, Path, description = "User ID"), TablePageQuery),
    responses(
        (status = 200, body = ClientKeyPageResponse, description = "Client keys"),
        (status = 404, body = ErrorEnvelope, description = "User not found")
    ),
    tag = "users"
)]
pub(super) async fn list_client_keys(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::ClientKey).await {
        return response.into_response();
    }
    let first = query.first.unwrap_or(0).max(0);
    let rows = query.rows.unwrap_or(20).clamp(1, 200);
    match state
        .config_repository
        .list_client_keys_page(user_id, first, rows)
        .await
    {
        Ok((total, keys)) => Json(ClientKeyPageResponse {
            keys: keys.into_iter().map(Into::into).collect(),
            total,
            first,
            rows,
        })
        .into_response(),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/client-keys",
    params(("user_id" = i64, Path, description = "User ID")),
    request_body = CreateClientKeyRequest,
    responses(
        (status = 200, body = CreateClientKeyResponse, description = "Created client key"),
        (status = 404, body = ErrorEnvelope, description = "User not found")
    ),
    tag = "users"
)]
pub(super) async fn create_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(user_id): Path<i64>,
    Json(body): Json<CreateClientKeyRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::ClientKey).await {
        return response.into_response();
    }
    let label = body.label.as_deref();
    match state
        .config_repository
        .create_client_key(user_id, label, true)
        .await
    {
        Ok(created) => {
            let _ = publish_snapshot(&state).await;
            Json(CreateClientKeyResponse::from(created)).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/users/{user_id}/client-keys/{key_id}",
    params(
        ("user_id" = i64, Path, description = "User ID"),
        ("key_id" = String, Path, description = "Client key UUID; legacy numeric identifiers are also accepted")
    ),
    request_body = UpdateClientKeyRequest,
    responses(
        (status = 200, body = ClientKey, description = "Updated client key"),
        (status = 404, body = ErrorEnvelope, description = "User or client key not found")
    ),
    tag = "users"
)]
pub(super) async fn update_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path((user_id, key_id)): Path<(i64, String)>,
    Json(body): Json<UpdateClientKeyRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::ClientKey).await {
        return response.into_response();
    }
    let identifier = match parse_client_key_identifier(&key_id) {
        Ok(identifier) => identifier,
        Err(_) => return bad_request("invalid client key identifier"),
    };
    let uuid_key_id = match state
        .config_repository
        .resolve_client_key_identifier(user_id, identifier)
        .await
    {
        Ok(Some(uuid)) => uuid,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => return internal(&state, err),
    };
    match state
        .config_repository
        .update_client_key(user_id, uuid_key_id, body.label, body.enabled)
        .await
    {
        Ok(Some(key)) => {
            let _ = publish_snapshot(&state).await;
            Json(ClientKey::from(key)).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/users/{user_id}/client-keys/{key_id}",
    params(
        ("user_id" = i64, Path, description = "User ID"),
        ("key_id" = String, Path, description = "Client key UUID; legacy numeric identifiers are also accepted")
    ),
    responses(
        (status = 204, description = "Deleted client key"),
        (status = 404, body = ErrorEnvelope, description = "User or client key not found")
    ),
    tag = "users"
)]
pub(super) async fn delete_client_key(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path((user_id, key_id)): Path<(i64, String)>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers, ControlAction::ClientKey).await {
        return response.into_response();
    }
    let identifier = match parse_client_key_identifier(&key_id) {
        Ok(identifier) => identifier,
        Err(_) => return bad_request("invalid client key identifier"),
    };
    let uuid_key_id = match state
        .config_repository
        .resolve_client_key_identifier(user_id, identifier)
        .await
    {
        Ok(Some(uuid)) => uuid,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => return internal(&state, err),
    };
    match state
        .config_repository
        .delete_client_key(user_id, uuid_key_id)
        .await
    {
        Ok(true) => {
            let _ = publish_snapshot(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "key not found"),
        Err(err) => internal(&state, err),
    }
}
