use crate::{
    db::StreamDeltaBatchingSettings,
    llm_review::LlmReviewSettings,
    protocol::RelayIpPolicy,
    worker_admin_types::{
        EndpointSettingRequest, ModelRouteWhitelistRequest, ModelRouteWhitelistResponse,
        RelayIpPolicyResponse, RequestContentLoggingRequest, RequestContentLoggingResponse,
    },
};

use super::schemas::{
    EndpointSettingResponse, RedactionConfigSchema, RedactionCustomStringRulePageResponseSchema,
    RedactionPreviewRequestSchema, RedactionPreviewResponseSchema, RedactionScopeSchema,
    RedactionSettingResponseSchema,
};

#[utoipa::path(
    get,
    path = "/api/v1/settings/endpoint",
    responses((status = 200, body = EndpointSettingResponse, description = "Endpoint preference")),
    tag = "settings"
)]
pub(super) fn get_endpoint_setting() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/endpoint",
    request_body = EndpointSettingRequest,
    responses((status = 204, description = "Endpoint preference updated")),
    tag = "settings"
)]
pub(super) fn set_endpoint_setting() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/redaction",
    params(
        ("scope" = Option<RedactionScopeSchema>, Query, description = "Redaction rule scope"),
        ("user_id" = Option<i64>, Query, description = "Target user id for user-scoped rules")
    ),
    responses(
        (status = 200, body = RedactionSettingResponseSchema, description = "Redaction settings")
    ),
    tag = "settings"
)]
pub(super) fn get_redaction_setting() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/redaction",
    params(
        ("scope" = Option<RedactionScopeSchema>, Query, description = "Redaction rule scope"),
        ("user_id" = Option<i64>, Query, description = "Target user id for user-scoped rules")
    ),
    request_body = RedactionConfigSchema,
    responses(
        (status = 200, body = RedactionSettingResponseSchema, description = "Updated redaction settings")
    ),
    tag = "settings"
)]
pub(super) fn set_redaction_setting() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/redaction/custom-strings",
    params(
        ("scope" = Option<RedactionScopeSchema>, Query, description = "Redaction rule scope"),
        ("user_id" = Option<i64>, Query, description = "Target user id for user-scoped rules"),
        ("first" = Option<i64>, Query, description = "Zero-based offset"),
        ("rows" = Option<i64>, Query, description = "Page size, default 10"),
        ("search" = Option<String>, Query, description = "Pattern substring search")
    ),
    responses(
        (status = 200, body = RedactionCustomStringRulePageResponseSchema, description = "Paged custom string rules")
    ),
    tag = "settings"
)]
pub(super) fn list_redaction_custom_strings() {}

#[utoipa::path(
    post,
    path = "/api/v1/settings/redaction/preview",
    request_body = RedactionPreviewRequestSchema,
    responses(
        (status = 200, body = RedactionPreviewResponseSchema, description = "Redaction preview")
    ),
    tag = "settings"
)]
pub(super) fn preview_redaction() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/request-content-logging",
    responses((status = 200, body = RequestContentLoggingResponse, description = "Request content logging")),
    tag = "settings"
)]
pub(super) fn get_request_content_logging() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/request-content-logging",
    request_body = RequestContentLoggingRequest,
    responses((status = 200, body = RequestContentLoggingResponse, description = "Updated request content logging")),
    tag = "settings"
)]
pub(super) fn set_request_content_logging() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/stream-delta-batching",
    responses((status = 200, body = StreamDeltaBatchingSettings, description = "Stream delta batching settings")),
    tag = "settings"
)]
pub(super) fn get_stream_delta_batching() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/stream-delta-batching",
    request_body = StreamDeltaBatchingSettings,
    responses((status = 200, body = StreamDeltaBatchingSettings, description = "Updated stream delta batching settings")),
    tag = "settings"
)]
pub(super) fn set_stream_delta_batching() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/model-route-whitelist",
    responses((status = 200, body = ModelRouteWhitelistResponse, description = "Model route whitelist")),
    tag = "settings"
)]
pub(super) fn get_model_route_whitelist() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/model-route-whitelist",
    request_body = ModelRouteWhitelistRequest,
    responses((status = 200, body = ModelRouteWhitelistResponse, description = "Updated model route whitelist")),
    tag = "settings"
)]
pub(super) fn set_model_route_whitelist() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/relay-ip-whitelist",
    responses((status = 200, body = RelayIpPolicyResponse, description = "Relay IP whitelist")),
    tag = "settings"
)]
pub(super) fn get_relay_ip_whitelist() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/relay-ip-whitelist",
    request_body = RelayIpPolicy,
    responses((status = 200, body = RelayIpPolicyResponse, description = "Updated relay IP whitelist")),
    tag = "settings"
)]
pub(super) fn set_relay_ip_whitelist() {}

#[utoipa::path(
    get,
    path = "/api/v1/settings/llm-review",
    responses((status = 200, body = LlmReviewSettings, description = "LLM review settings")),
    tag = "settings"
)]
pub(super) fn get_llm_review_setting() {}

#[utoipa::path(
    patch,
    path = "/api/v1/settings/llm-review",
    request_body = LlmReviewSettings,
    responses((status = 200, body = LlmReviewSettings, description = "Updated LLM review settings")),
    tag = "settings"
)]
pub(super) fn set_llm_review_setting() {}
