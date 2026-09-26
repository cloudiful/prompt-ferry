use super::schemas::ErrorEnvelope;
use crate::{
    db,
    worker_admin_types::{
        EndpointOAuthStatusResponse, EndpointPageResponse, EndpointRequest, EndpointTestResponse,
        OAuthBrowserCompleteRequest, OAuthBrowserStartResponse, OAuthDeviceStartResponse,
        OAuthFlowRequest, OAuthLoginResponse, TablePageQuery, TokenPlanUsageResponse,
    },
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/endpoints",
    params(TablePageQuery),
    responses(
        (status = 200, body = EndpointPageResponse, description = "Endpoint page")
    ),
    tag = "endpoints"
)]
pub(super) fn list_endpoints() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints",
    request_body = EndpointRequest,
    responses(
        (status = 200, body = db::ProviderEndpoint, description = "Created endpoint"),
        (status = 400, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn create_endpoint() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/endpoints/{endpoint_id}",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    request_body = EndpointRequest,
    responses(
        (status = 200, body = db::ProviderEndpoint, description = "Updated endpoint"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn update_endpoint() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/endpoints/{endpoint_id}",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 204, description = "Deleted endpoint"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn delete_endpoint() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints/{endpoint_id}/test",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses((status = 200, body = EndpointTestResponse, description = "Endpoint test result")),
    tag = "endpoints"
)]
pub(super) fn test_endpoint() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/endpoints/{endpoint_id}/token-plan-usage",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses((status = 200, body = TokenPlanUsageResponse, description = "Token plan usage")),
    tag = "endpoints"
)]
pub(super) fn token_plan_usage() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, body = EndpointOAuthStatusResponse, description = "Stored ChatGPT OAuth login state"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_status() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 204, description = "Stored ChatGPT OAuth token cleared"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_clear() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth/device",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, body = OAuthDeviceStartResponse, description = "Device-code login started"),
        (status = 400, body = ErrorEnvelope),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_device_start() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth/device/poll",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    request_body = OAuthFlowRequest,
    responses(
        (status = 200, body = OAuthLoginResponse, description = "Device-code login poll result"),
        (status = 400, body = ErrorEnvelope),
        (status = 404, body = ErrorEnvelope),
        (status = 410, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_device_poll() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth/browser",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, body = OAuthBrowserStartResponse, description = "Browser login started"),
        (status = 400, body = ErrorEnvelope),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_browser_start() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth/browser/complete",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    request_body = OAuthBrowserCompleteRequest,
    responses(
        (status = 200, body = OAuthLoginResponse, description = "Browser login completed from the pasted redirect"),
        (status = 400, body = ErrorEnvelope),
        (status = 404, body = ErrorEnvelope),
        (status = 410, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_browser_complete() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/endpoints/{endpoint_id}/oauth/refresh",
    params(("endpoint_id" = uuid::Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, body = EndpointOAuthStatusResponse, description = "ChatGPT OAuth login state after refresh"),
        (status = 400, body = ErrorEnvelope),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "endpoints"
)]
pub(super) fn oauth_refresh() {}
