use super::schemas::ErrorEnvelope;
use crate::{
    db,
    worker_admin_types::{
        EndpointPageResponse, EndpointRequest, EndpointTestResponse, TablePageQuery,
        TokenPlanUsageResponse,
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
