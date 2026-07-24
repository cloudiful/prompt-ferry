use super::schemas::ErrorEnvelope;
use crate::{
    db,
    worker_admin_types::{
        ClientKeyPageResponse, CreateClientKeyRequest, CreateClientKeyResponse, CreateUserRequest,
        ResetPasswordRequest, TablePageQuery, UpdateClientKeyRequest, UserOptionsResponse,
        UserPageResponse,
    },
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/users",
    params(TablePageQuery),
    responses(
        (status = 200, body = UserPageResponse, description = "Users"),
        (status = 401, body = ErrorEnvelope),
        (status = 403, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn list_users() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users/options",
    responses(
        (status = 200, body = UserOptionsResponse, description = "User options"),
        (status = 401, body = ErrorEnvelope),
        (status = 403, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn list_user_options() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users",
    request_body = CreateUserRequest,
    responses(
        (status = 200, body = db::User, description = "Created user"),
        (status = 400, body = ErrorEnvelope),
        (status = 401, body = ErrorEnvelope),
        (status = 403, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn create_user() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/users/{user_id}",
    params(("user_id" = i64, Path, description = "User ID")),
    request_body = db::UserUpdate,
    responses(
        (status = 200, body = db::User, description = "Updated user"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn update_user() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/users/{user_id}",
    params(("user_id" = i64, Path, description = "User ID")),
    responses(
        (status = 204, description = "Deleted user"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn delete_user() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/reset-password",
    params(("user_id" = i64, Path, description = "User ID")),
    request_body = ResetPasswordRequest,
    responses(
        (status = 204, description = "Password reset"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn reset_password() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users/{user_id}/client-keys",
    params(("user_id" = i64, Path, description = "User ID"), TablePageQuery),
    responses(
        (status = 200, body = ClientKeyPageResponse, description = "Client keys"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn list_client_keys() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/client-keys",
    params(("user_id" = i64, Path, description = "User ID")),
    request_body = CreateClientKeyRequest,
    responses(
        (status = 200, body = CreateClientKeyResponse, description = "Created client key"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn create_client_key() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/users/{user_id}/client-keys/{key_id}",
    params(
        ("user_id" = i64, Path, description = "User ID"),
        ("key_id" = i64, Path, description = "Client key ID")
    ),
    request_body = UpdateClientKeyRequest,
    responses(
        (status = 200, body = db::ClientKey, description = "Updated client key"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn update_client_key() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/users/{user_id}/client-keys/{key_id}",
    params(
        ("user_id" = i64, Path, description = "User ID"),
        ("key_id" = i64, Path, description = "Client key ID")
    ),
    responses(
        (status = 204, description = "Deleted client key"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "users"
)]
pub(super) fn delete_client_key() {}
