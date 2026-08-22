use super::schemas::ErrorEnvelope;
use crate::worker_admin_types::{
    AvailableModelsResponse, ClientKey, ClientKeyPageResponse, CreateClientKeyRequest,
    CreateClientKeyResponse, TablePageQuery, UpdateClientKeyRequest,
};

#[utoipa::path(
    get,
    path = "/api/v1/me/client-keys",
    params(TablePageQuery),
    responses(
        (status = 200, body = ClientKeyPageResponse, description = "Current user client keys"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized")
    ),
    tag = "me"
)]
pub(super) fn me_list_client_keys() {}

#[utoipa::path(
    post,
    path = "/api/v1/me/client-keys",
    request_body = CreateClientKeyRequest,
    responses(
        (status = 200, body = CreateClientKeyResponse, description = "Created current user client key"),
        (status = 400, body = ErrorEnvelope, description = "Client key limit exceeded"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized")
    ),
    tag = "me"
)]
pub(super) fn me_create_client_key() {}

#[utoipa::path(
    patch,
    path = "/api/v1/me/client-keys/{key_id}",
    params(("key_id" = String, Path, description = "Client key UUID; legacy numeric identifiers are also accepted")),
    request_body = UpdateClientKeyRequest,
    responses(
        (status = 200, body = ClientKey, description = "Updated current user client key"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized"),
        (status = 404, body = ErrorEnvelope, description = "Key not found")
    ),
    tag = "me"
)]
pub(super) fn me_update_client_key() {}

#[utoipa::path(
    delete,
    path = "/api/v1/me/client-keys/{key_id}",
    params(("key_id" = String, Path, description = "Client key UUID; legacy numeric identifiers are also accepted")),
    responses(
        (status = 204, description = "Deleted current user client key"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized"),
        (status = 404, body = ErrorEnvelope, description = "Key not found")
    ),
    tag = "me"
)]
pub(super) fn me_delete_client_key() {}

#[utoipa::path(
    get,
    path = "/api/v1/me/models",
    params(TablePageQuery),
    responses(
        (status = 200, body = AvailableModelsResponse, description = "Available models for current user"),
        (status = 401, body = ErrorEnvelope, description = "Unauthorized")
    ),
    tag = "me"
)]
pub(super) fn me_list_models() {}
