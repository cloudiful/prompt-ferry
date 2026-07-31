use super::super::{
    admin_proxy::process_admin_request,
    context::{FailurePayload, RouteExecutionContext, RuntimeServices, is_bridge_send_error},
    error_handling::safe_error,
    request_assembly::BufferedBridgeRequest,
};
use super::{
    errors::{respond_with_budget_error, respond_with_client_error, respond_with_local_error},
    models::{ModelsRequestContext, process_models_request},
    request_attempts::{ForwardOutcome, RouteForwardRequest, forward_route_request},
    request_init::initialize_request,
    request_routes::{RouteResolution, resolve_route},
    request_support::mark_function_call_outputs_received,
    review::handle_llm_review_gate,
};
use crate::{
    config::{NativeApi, WorkerConfig},
    db,
    openai_compat::CompatError,
    worker_usage::record_usage_event,
};
use http::StatusCode;

pub(in crate::worker::runtime) async fn process_request(
    request: BufferedBridgeRequest,
    config: &WorkerConfig,
    services: &RuntimeServices,
) -> anyhow::Result<()> {
    if !request.path.starts_with("/v1/") {
        return process_admin_request(request, services).await;
    }

    let initialized = initialize_request(&request, services).await?;
    let content_logging_enabled = initialized.content_logging_enabled;
    let raw_content_logging_enabled = initialized.raw_content_logging_enabled;
    let method = initialized.method;
    let redact_content = initialized.redact_content;
    let request_ctx = initialized.request_ctx;
    let _request_lease = services
        .runtime_state
        .spawn_request_lease_guard(services.admin_state(), request_ctx.request_id);
    record_usage_event(
        services.admin_state(),
        request_ctx.ai_usage_log(&request, None),
    )
    .await;
    if request.path == "/v1/responses" {
        mark_function_call_outputs_received(
            services.admin_state(),
            request_ctx.request_prompt_log.parent_event_id,
            &request.body,
        )
        .await;
    }
    if let Some(state) = services.admin_state()
        && request.path == "/v1/models"
    {
        return process_models_request(ModelsRequestContext {
            state,
            client: &services.client,
            out_tx: &services.out_tx,
            request: &request,
            request_id: request_ctx.request_id,
            started: request_ctx.started,
            user_id: request.user_id.unwrap_or_default(),
            owner_worker_id: services.runtime_state.worker_instance_id(),
        })
        .await;
    }

    if services.admin_state().is_some()
        && !handle_llm_review_gate(services, &request, &request_ctx).await?
    {
        return Ok(());
    }

    let (mut route, _endpoint_load_guard) =
        match resolve_route(&request, config, services, &request_ctx).await? {
            RouteResolution::Ready { route, load_guard } => (*route, load_guard),
            RouteResolution::Responded => return Ok(()),
        };

    if let Err(err) = resolve_auto_protocol(&mut route, &request.path) {
        let route_ctx = RouteExecutionContext::new(&route);
        return respond_with_client_error(services, &request, &request_ctx, &route_ctx, err).await;
    }

    let outcome = forward_route_request(RouteForwardRequest {
        services,
        request: &request,
        request_ctx: &request_ctx,
        route: &route,
        method: &method,
        redact_content,
        content_logging_enabled,
        raw_content_logging_enabled,
    })
    .await?;
    let route_ctx = RouteExecutionContext::new(&route);
    let err = match outcome {
        ForwardOutcome::Handled => return Ok(()),
        ForwardOutcome::CompatError(err) => {
            return respond_with_client_error(services, &request, &request_ctx, &route_ctx, err)
                .await;
        }
        ForwardOutcome::BudgetError {
            endpoint_id,
            message,
            model_route_rule_id,
        } => {
            return respond_with_budget_error(
                services,
                &request,
                &request_ctx,
                RouteExecutionContext {
                    route: route.clone(),
                    endpoint_id,
                    model_route_rule_id,
                    route_selection_reason: route.route_selection_reason,
                },
                message,
            )
            .await;
        }
        ForwardOutcome::TransportError(err) => {
            if err.to_string().contains("upstream_response_too_large") {
                return respond_with_local_error(
                    services,
                    &request,
                    &request_ctx,
                    FailurePayload {
                        status: reqwest::StatusCode::BAD_GATEWAY,
                        error_code: "upstream_response_too_large".to_string(),
                        error_message: "upstream response exceeded the configured size limit"
                            .to_string(),
                        upstream_error_body: None,
                        response_body: None,
                    },
                )
                .await;
            }
            err
        }
    };
    let error_code = if is_bridge_send_error(&err) {
        "relay_bridge_error"
    } else {
        "upstream_error"
    };
    record_usage_event(
        services.admin_state(),
        request_ctx
            .ai_usage_log(&request, Some(route.user_id))
            .with_upstream_redaction(
                request_ctx.request_prompt_log.upstream_redaction_enabled,
                request_ctx
                    .request_prompt_log
                    .upstream_redacted_request_json
                    .clone(),
                request_ctx
                    .request_prompt_log
                    .upstream_restore_session
                    .clone(),
            )
            .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
            .with_route(route_ctx.endpoint_id, route_ctx.model_route_rule_id)
            .with_endpoint_key(
                route_ctx.route.endpoint_key_id,
                route_ctx.route.endpoint_key_label.clone(),
            )
            .with_status(None, Some(false), Some(request_ctx.elapsed_ms()), None)
            .with_error(
                Some(error_code.to_string()),
                Some(safe_error(&err, redact_content, request_ctx.user_id)),
                None,
            ),
    )
    .await;
    Err(err)
}

pub(super) fn resolve_auto_protocol(
    route: &mut db::RouteConfig,
    request_path: &str,
) -> Result<(), CompatError> {
    if route.native_api != NativeApi::Auto {
        return Ok(());
    }
    route.native_api = match request_path {
        "/v1/chat/completions" => NativeApi::Chat,
        "/v1/responses" => NativeApi::Responses,
        _ => {
            return Err(CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_auto_protocol",
                "automatic endpoints support only /v1/chat/completions and /v1/responses",
            ));
        }
    };
    Ok(())
}

#[cfg(test)]
mod auto_protocol_tests {
    use super::resolve_auto_protocol;
    use crate::{config::NativeApi, db};

    fn route(native_api: NativeApi) -> db::RouteConfig {
        db::RouteConfig {
            route_id: uuid::Uuid::nil(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://example.test".to_string(),
            api_key: "key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api,
            upstream_model: None,
            responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: db::RouteSelectionReason::Default,
        }
    }

    #[test]
    fn resolves_auto_from_request_path() {
        let mut chat_route = route(NativeApi::Auto);
        resolve_auto_protocol(&mut chat_route, "/v1/chat/completions").unwrap();
        assert_eq!(chat_route.native_api, NativeApi::Chat);

        let mut responses_route = route(NativeApi::Auto);
        resolve_auto_protocol(&mut responses_route, "/v1/responses").unwrap();
        assert_eq!(responses_route.native_api, NativeApi::Responses);
    }

    #[test]
    fn rejects_unsupported_auto_path() {
        let mut route = route(NativeApi::Auto);
        let error = resolve_auto_protocol(&mut route, "/v1/embeddings").unwrap_err();
        assert_eq!(error.code, "unsupported_auto_protocol");
        assert_eq!(route.native_api, NativeApi::Auto);
    }

    #[test]
    fn rejects_auto_for_realtime() {
        let mut route = route(NativeApi::Auto);
        let error = resolve_auto_protocol(&mut route, "/v1/realtime").unwrap_err();
        assert_eq!(error.code, "unsupported_auto_protocol");
        assert_eq!(route.native_api, NativeApi::Auto);
    }

    #[test]
    fn leaves_fixed_protocols_unchanged() {
        let mut route = route(NativeApi::Responses);
        resolve_auto_protocol(&mut route, "/v1/chat/completions").unwrap();
        assert_eq!(route.native_api, NativeApi::Responses);
    }
}
