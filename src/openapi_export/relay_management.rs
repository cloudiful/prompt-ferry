//! The relay's own management API, as the generated spec describes it.
//!
//! These endpoints are served by the relay's management listener rather than by
//! the worker, but they live under the same `/api/v1` prefix and are part of the
//! one admin API surface, so they are exported here alongside the worker's. The
//! bodies are the relay's own types; nothing here describes worker business
//! resources, which stay behind the bridge on their existing routes.

use utoipa::OpenApi;

use crate::{
    config::HostRole,
    relay::admin::dto::{
        RelayAuthResponse, RelayConfigView, RelayHostResponse, RelayHostRoleRequest,
        RelayLoginRequest, RelaySettingsResponse, RelaySettingsUpdate, RelayStatusResponse,
        WorkerStatusResponse,
    },
};

#[utoipa::path(
    post,
    path = "/api/v1/relay/auth/login",
    request_body = RelayLoginRequest,
    responses(
        (status = 204, description = "Management session started"),
        (status = 401, description = "Invalid management token", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_auth_login() {}

#[utoipa::path(
    post,
    path = "/api/v1/relay/auth/logout",
    responses((status = 204, description = "Management session ended")),
    tag = "relay"
)]
pub(super) fn relay_auth_logout() {}

#[utoipa::path(
    get,
    path = "/api/v1/relay/auth/me",
    responses(
        (status = 200, description = "Management session state", body = RelayAuthResponse)
    ),
    tag = "relay"
)]
pub(super) fn relay_auth_me() {}

#[utoipa::path(
    get,
    path = "/api/v1/relay/status",
    responses(
        (status = 200, description = "Relay status", body = RelayStatusResponse),
        (status = 401, description = "Unauthorized", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_status() {}

#[utoipa::path(
    get,
    path = "/api/v1/relay/settings",
    responses(
        (status = 200, description = "Relay management settings", body = RelaySettingsResponse),
        (status = 401, description = "Unauthorized", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_get_settings() {}

#[utoipa::path(
    patch,
    path = "/api/v1/relay/settings",
    request_body = RelaySettingsUpdate,
    responses(
        (status = 200, description = "Updated relay management settings", body = RelaySettingsResponse),
        (status = 400, description = "Invalid setting", body = super::schemas::ErrorEnvelope),
        (status = 401, description = "Unauthorized", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_set_settings() {}

#[utoipa::path(
    get,
    path = "/api/v1/relay/host",
    responses(
        (status = 200, description = "Host role and restart state", body = RelayHostResponse),
        (status = 401, description = "Unauthorized", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_host() {}

#[utoipa::path(
    put,
    path = "/api/v1/relay/host/role",
    request_body = RelayHostRoleRequest,
    responses(
        (status = 200, description = "Host role recorded", body = RelayHostResponse),
        (status = 400, description = "Unknown role", body = super::schemas::ErrorEnvelope),
        (status = 401, description = "Unauthorized", body = super::schemas::ErrorEnvelope),
        (status = 503, description = "The role could not be saved", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_set_host_role() {}

#[utoipa::path(
    post,
    path = "/api/v1/relay/host/restart",
    responses(
        (status = 202, description = "Restart requested"),
        (status = 401, description = "Unauthorized", body = super::schemas::ErrorEnvelope)
    ),
    tag = "relay"
)]
pub(super) fn relay_request_restart() {}

#[derive(OpenApi)]
#[openapi(
    paths(
        relay_auth_login,
        relay_auth_logout,
        relay_auth_me,
        relay_status,
        relay_get_settings,
        relay_set_settings,
        relay_host,
        relay_set_host_role,
        relay_request_restart
    ),
    components(schemas(
        RelayAuthResponse,
        RelayConfigView,
        RelayHostResponse,
        RelayHostRoleRequest,
        RelayLoginRequest,
        RelaySettingsResponse,
        RelaySettingsUpdate,
        RelayStatusResponse,
        WorkerStatusResponse,
        HostRole
    )),
    tags(
        (
            name = "relay",
            description = "Relay management served by the relay's own management listener"
        )
    )
)]
pub(super) struct RelayApiDoc;
