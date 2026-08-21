use anyhow::Context;
use http::Method;

use crate::{
    usage::model_from_body,
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
};

use super::super::{
    RequestExecutionContext, RequestPromptLog, context::RuntimeServices,
    error_handling::redaction_enabled, prepare_request_prompt_log,
    request_assembly::BufferedBridgeRequest,
};

pub(super) struct InitializedRequest {
    pub(super) content_logging_enabled: bool,
    pub(super) raw_content_logging_enabled: bool,
    pub(super) method: Method,
    pub(super) redact_content: bool,
    pub(super) request_ctx: RequestExecutionContext,
}

pub(super) async fn initialize_request(
    request: &BufferedBridgeRequest,
    services: &RuntimeServices,
) -> anyhow::Result<InitializedRequest> {
    let started = std::time::Instant::now();
    let request_id =
        uuid::Uuid::parse_str(&request.request_id).unwrap_or_else(|_| uuid::Uuid::new_v4());
    let request_model = model_from_body(&request.body);
    let (client_key_id, client_key_label, client_key_user_id) =
        if let (Some(state), Some(key_hash)) = (
            services.standalone_state(),
            request.client_key_hash.as_deref(),
        ) {
            state
                .client_key_identity(key_hash)
                .await
                .map(|identity| (None, Some(identity.label), Some(identity.user_id)))
                .unwrap_or_default()
        } else if let (Some(state), Some(key_hash)) =
            (services.admin_state(), request.client_key_hash.as_deref())
        {
            crate::db::get_client_key_identity_by_hash(&state.pool, key_hash)
                .await?
                .map(|identity| (Some(identity.key_id), Some(identity.label), None))
                .unwrap_or_default()
        } else {
            (None, None, None)
        };
    let user_id = client_key_user_id.or(request.user_id.filter(|id| *id > 0));
    let request_content_logging = if let Some(state) = services.admin_state() {
        state.request_content_logging.read().await.clone()
    } else {
        RequestContentLoggingResponse {
            mode: RequestContentLoggingMode::Off,
            raw_retention_days: 3,
        }
    };
    let content_logging_enabled = request_content_logging.mode.captures_normalized();
    let raw_content_logging_enabled = request_content_logging.mode.captures_raw();
    let redact_content = services.standalone_state().map_or_else(
        || redaction_enabled(services.admin_state()),
        |state| state.redaction_enabled(),
    );
    let request_prompt_log = if let Some(state) = services.admin_state() {
        prepare_request_prompt_log(
            state,
            request,
            request.user_id.filter(|id| *id > 0),
            request_model.as_deref(),
            &request_content_logging,
            redact_content,
        )
        .await?
    } else {
        RequestPromptLog::default()
    };
    let method = Method::from_bytes(request.method.as_bytes()).context("invalid method")?;
    Ok(InitializedRequest {
        content_logging_enabled,
        raw_content_logging_enabled,
        method,
        redact_content,
        request_ctx: RequestExecutionContext::new(
            request_id,
            started,
            request_model,
            client_key_id,
            client_key_label,
            user_id,
            services.runtime_state.worker_instance_id(),
            request_prompt_log,
        ),
    })
}
