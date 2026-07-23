use super::schemas::ErrorEnvelope;
use crate::worker_admin_types::{LoginRequest, MeResponse};

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 204, description = "Login successful"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized")
    ),
    tag = "auth"
)]
pub(super) fn auth_login() {}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    responses((status = 204, description = "Logged out")),
    tag = "auth"
)]
pub(super) fn auth_logout() {}

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses(
        (status = 200, body = MeResponse, description = "Current user"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized")
    ),
    tag = "auth"
)]
pub(super) fn auth_me() {}
