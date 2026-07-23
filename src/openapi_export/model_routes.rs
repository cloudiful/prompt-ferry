use super::schemas::ErrorEnvelope;
use crate::{
    db,
    worker_admin_types::{
        ModelRoutePageResponse, ModelRouteRequest, ModelRouteTestRequest, ModelRouteTestResponse,
        TablePageQuery,
    },
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/model-routes",
    params(TablePageQuery),
    responses((status = 200, body = ModelRoutePageResponse, description = "Model route page")),
    tag = "model-routes"
)]
pub(super) fn list_model_routes() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/model-routes",
    request_body = ModelRouteRequest,
    responses((status = 200, body = db::ModelEndpointRule, description = "Created model route")),
    tag = "model-routes"
)]
pub(super) fn create_model_route() {}

#[utoipa::path(
    patch,
    path = "/api/v1/admin/model-routes/{rule_id}",
    params(("rule_id" = uuid::Uuid, Path, description = "Rule ID")),
    request_body = ModelRouteRequest,
    responses((status = 200, body = db::ModelEndpointRule, description = "Updated model route")),
    tag = "model-routes"
)]
pub(super) fn update_model_route() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/model-routes/{rule_id}",
    params(("rule_id" = uuid::Uuid, Path, description = "Rule ID")),
    responses(
        (status = 204, description = "Deleted model route"),
        (status = 404, body = ErrorEnvelope)
    ),
    tag = "model-routes"
)]
pub(super) fn delete_model_route() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/model-routes/test",
    request_body = ModelRouteTestRequest,
    responses((status = 200, body = ModelRouteTestResponse, description = "Route test result")),
    tag = "model-routes"
)]
pub(super) fn test_model_route() {}
