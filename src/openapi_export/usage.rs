use crate::{
    db,
    worker_admin_types::{
        ConversationEndpointOverrideRequest, RequestRecordFullResponse, RequestRecordOverviewQuery,
        RequestRecordPruneResponse, RequestRecordSeriesQuery, RequestRecordSummaryQuery,
        RequestRecordsClearRequest, RequestRecordsClearResponse, RequestRecordsQuery,
        SessionRouteOptionsResponse,
    },
};

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/overview",
    params(RequestRecordOverviewQuery),
    responses((status = 200, body = db::RequestRecordOverviewResponse, description = "Request record overview")),
    tag = "request-records"
)]
pub(super) fn request_record_overview() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/summary",
    params(RequestRecordSummaryQuery),
    responses((status = 200, body = db::RequestRecordSummary, description = "Request record summary")),
    tag = "request-records"
)]
pub(super) fn request_record_summary() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records",
    params(RequestRecordsQuery),
    responses((status = 200, body = db::RequestRecordPage, description = "Request record page")),
    tag = "request-records"
)]
pub(super) fn list_request_records() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/facets",
    params(crate::worker_admin_types::RequestRecordFacetsQuery),
    responses((status = 200, body = db::RequestRecordFacets, description = "Request record facets")),
    tag = "request-records"
)]
pub(super) fn request_record_facets() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/request-records/clear",
    request_body = RequestRecordsClearRequest,
    responses((status = 200, body = RequestRecordsClearResponse, description = "Request record clear result")),
    tag = "request-records"
)]
pub(super) fn clear_request_records() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/{record_id}",
    params(("record_id" = i64, Path, description = "Request record ID")),
    responses((status = 200, body = db::RequestRecordDetail, description = "Request record detail")),
    tag = "request-records"
)]
pub(super) fn request_record_detail() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/{record_id}/session-route-options",
    params(("record_id" = i64, Path, description = "Request record ID")),
    responses((status = 200, body = SessionRouteOptionsResponse, description = "Session route options for request record")),
    tag = "request-records"
)]
pub(super) fn request_record_session_route_options() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/request-records/{record_id}/reset-session-affinity",
    params(("record_id" = i64, Path, description = "Request record ID")),
    responses(
        (status = 204, description = "Session affinity binding cleared"),
        (status = 400, body = super::schemas::ErrorEnvelope, description = "Request record has no conversation or no route resolved"),
        (status = 403, body = super::schemas::ErrorEnvelope),
        (status = 404, body = super::schemas::ErrorEnvelope),
        (status = 503, body = super::schemas::ErrorEnvelope, description = "Response affinity backend unavailable")
    ),
    tag = "request-records"
)]
pub(super) fn request_record_reset_session_affinity() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/{record_id}/request-full",
    params(("record_id" = i64, Path, description = "Request record ID")),
    responses((status = 200, body = RequestRecordFullResponse, description = "Full stored request record payload")),
    tag = "request-records"
)]
pub(super) fn request_record_full() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/request-records/series",
    params(RequestRecordSeriesQuery),
    responses((status = 200, body = [db::RequestRecordBucket], description = "Request record series")),
    tag = "request-records"
)]
pub(super) fn request_record_series() {}

#[utoipa::path(
    post,
    path = "/api/v1/admin/request-records/prune",
    responses((status = 200, body = RequestRecordPruneResponse, description = "Request record prune result")),
    tag = "request-records"
)]
pub(super) fn prune_request_records() {}

#[utoipa::path(
    get,
    path = "/api/v1/admin/conversations/{conversation_id}/endpoint-override",
    params(("conversation_id" = uuid::Uuid, Path, description = "Conversation ID")),
    responses(
        (status = 200, body = db::ConversationEndpointOverride, description = "Conversation endpoint override"),
        (status = 404, body = super::schemas::ErrorEnvelope)
    ),
    tag = "request-records"
)]
pub(super) fn get_conversation_endpoint_override() {}

#[utoipa::path(
    put,
    path = "/api/v1/admin/conversations/{conversation_id}/endpoint-override",
    params(("conversation_id" = uuid::Uuid, Path, description = "Conversation ID")),
    request_body = ConversationEndpointOverrideRequest,
    responses(
        (status = 200, body = db::ConversationEndpointOverride, description = "Updated conversation endpoint override"),
        (status = 400, body = super::schemas::ErrorEnvelope)
    ),
    tag = "request-records"
)]
pub(super) fn set_conversation_endpoint_override() {}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/conversations/{conversation_id}/endpoint-override",
    params(("conversation_id" = uuid::Uuid, Path, description = "Conversation ID")),
    responses(
        (status = 204, description = "Deleted conversation endpoint override"),
        (status = 404, body = super::schemas::ErrorEnvelope)
    ),
    tag = "request-records"
)]
pub(super) fn delete_conversation_endpoint_override() {}
