use super::schemas::ErrorEnvelope;
use crate::worker_admin_types::{SetupRequest, SetupStatus};

#[utoipa::path(
    get,
    path = "/api/v1/auth/setup",
    responses(
        (status = 200, body = SetupStatus, description = "Setup status"),
        (status = 503, body = ErrorEnvelope, description = "Setup unavailable")
    ),
    tag = "auth"
)]
pub(super) fn setup_status() {}

#[utoipa::path(
    post,
    path = "/api/v1/auth/setup",
    request_body = SetupRequest,
    responses(
        (status = 201, body = SetupStatus, description = "Administrator initialized"),
        (status = 400, body = ErrorEnvelope, description = "Invalid request"),
        (status = 401, body = ErrorEnvelope, description = "Invalid proof"),
        (status = 409, body = ErrorEnvelope, description = "Already initialized"),
        (status = 503, body = ErrorEnvelope, description = "Setup unavailable")
    ),
    tag = "auth"
)]
pub(super) fn setup_initialize() {}
