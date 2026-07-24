use super::schemas::ErrorEnvelope;
use crate::worker_admin_types::{
    ManagedRelay, ManagedRelayListResponse, ManagedRelayPatchRequest, ManagedRelayRequest,
    TablePageQuery,
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/relays",
    params(TablePageQuery),
    responses((status = 200, body = ManagedRelayListResponse, description = "Managed relay list")),
    tag = "relays"
)]
pub(super) fn list_relays() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/relays",
    request_body = ManagedRelayRequest,
    responses(
        (status = 200, body = ManagedRelay, description = "Created relay"),
        (status = 400, body = ErrorEnvelope)
    ),
    tag = "relays"
)]
pub(super) fn create_relay() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/relays/{relay_id}",
    params(("relay_id" = uuid::Uuid, Path, description = "Relay ID")),
    responses(
        (status = 200, body = ManagedRelay, description = "Managed relay detail"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "relays"
)]
pub(super) fn get_relay() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/relays/{relay_id}",
    params(("relay_id" = uuid::Uuid, Path, description = "Relay ID")),
    request_body = ManagedRelayPatchRequest,
    responses(
        (status = 200, body = ManagedRelay, description = "Updated relay"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "relays"
)]
pub(super) fn update_relay() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/relays/{relay_id}",
    params(("relay_id" = uuid::Uuid, Path, description = "Relay ID")),
    responses(
        (status = 204, description = "Deleted relay"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "relays"
)]
pub(super) fn delete_relay() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/relays/{relay_id}/reconnect",
    params(("relay_id" = uuid::Uuid, Path, description = "Relay ID")),
    responses(
        (status = 200, body = ManagedRelay, description = "Reconnect requested"),
        (status = 400, body = ErrorEnvelope),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "relays"
)]
pub(super) fn reconnect_relay() {}
