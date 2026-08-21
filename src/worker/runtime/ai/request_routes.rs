use anyhow::anyhow;

use crate::{config::WorkerConfig, db};

use super::super::{
    RequestExecutionContext, check_named_request_budget,
    context::{RouteExecutionContext, RuntimeServices},
    discover_dynamic_model_route, materialize_route_api_key_selection_with_quota,
    request_assembly::BufferedBridgeRequest,
    routing::clear_invalid_conversation_endpoint_key_override,
    select_route_for_candidate,
};
use super::errors::respond_with_budget_error;

pub(super) enum RouteResolution {
    Ready { route: Box<db::RouteConfig> },
    Responded,
}

pub(super) async fn resolve_route(
    request: &BufferedBridgeRequest,
    config: &WorkerConfig,
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
) -> anyhow::Result<RouteResolution> {
    if let Some(state) = services.standalone_state() {
        let user_id = request_ctx.user_id.or(request.user_id).unwrap_or_default();
        let snapshot = state.snapshot().await;
        if let Some(candidate) =
            crate::worker::runtime::standalone::standalone_model_route_candidate(
                &snapshot,
                user_id,
                request_ctx.request_model.as_deref(),
            )
        {
            let selected = select_route_for_candidate(
                services,
                request_ctx,
                &candidate,
                request,
                user_id,
                request.client_key_hash.as_deref(),
            )
            .await?
            .ok_or_else(|| anyhow!("route not found"))?;
            return Ok(RouteResolution::Ready {
                route: Box::new(selected.route),
            });
        }
        return Ok(RouteResolution::Ready {
            route: Box::new(default_route_for_user(config, user_id)),
        });
    }
    let Some(state) = services.admin_state() else {
        return Ok(RouteResolution::Ready {
            route: Box::new(default_route(config, request)),
        });
    };

    let user_id = request.user_id.unwrap_or_default();
    let whitelist_enabled = state
        .model_route_whitelist_enabled
        .load(std::sync::atomic::Ordering::SeqCst);
    let use_fallback = !whitelist_enabled;
    let (fallback_route, candidate) = db::resolve_model_route_with_fallback(
        &state.pool,
        user_id,
        request_ctx.request_model.as_deref(),
        use_fallback,
    )
    .await?;

    if let Some(candidate) = candidate {
        if let Some(message) = check_named_request_budget(
            &state.pool,
            db::RequestRecordCategory::Ai,
            db::RequestBudgetScope::ModelRoute(candidate.rule_id),
            "model route",
            &candidate.model_pattern,
            candidate.daily_max_requests,
            candidate.monthly_max_requests,
        )
        .await?
        {
            respond_with_budget_error(
                services,
                request,
                request_ctx,
                RouteExecutionContext {
                    route: db::RouteConfig {
                        route_id: uuid::Uuid::nil(),
                        user_id,
                        model_route_rule_id: Some(candidate.rule_id),
                        base_url: String::new(),
                        api_key: String::new(),
                        endpoint_key_id: None,
                        endpoint_key_label: None,
                        api_keys: Vec::new(),
                        key_lb_enabled: false,
                        native_api: config.upstream_native_api,
                        upstream_model: None,
                        responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                        route_selection_reason: db::RouteSelectionReason::Default,
                    },
                    endpoint_id: None,
                    model_route_rule_id: Some(candidate.rule_id),
                    route_selection_reason: db::RouteSelectionReason::Default,
                },
                message,
            )
            .await?;
            return Ok(RouteResolution::Responded);
        }
        let selected = select_route_for_candidate(
            services,
            request_ctx,
            &candidate,
            request,
            user_id,
            request.client_key_hash.as_deref(),
        )
        .await?
        .ok_or_else(|| anyhow!("route not found"))?;
        return Ok(RouteResolution::Ready {
            route: Box::new(selected.route),
        });
    }

    let dynamic_route = if !whitelist_enabled {
        discover_dynamic_model_route(
            state,
            &services.client,
            user_id,
            request_ctx.request_model.as_deref(),
            fallback_route.as_ref(),
        )
        .await
    } else {
        None
    };

    let mut route = dynamic_route
        .or(fallback_route)
        .ok_or_else(|| anyhow!("route not found"))?;
    if let Err(err) = state
        .token_plan_quota
        .refresh_if_due(&state.pool, route.route_id)
        .await
    {
        tracing::warn!(
            endpoint_id = %route.route_id,
            error = %err,
            "MiniMax quota refresh failed for fallback route; retaining the previous snapshot"
        );
    }
    let key_selection = materialize_route_api_key_selection_with_quota(
        &route,
        request,
        &request_ctx.request_prompt_log,
        Some(&state.token_plan_quota),
    );
    clear_invalid_conversation_endpoint_key_override(
        services,
        &request_ctx.request_prompt_log,
        key_selection.invalid_conversation_override,
    )
    .await;
    route.api_key = key_selection.selection.secret;
    route.endpoint_key_id = key_selection.selection.key_id;
    route.endpoint_key_label = key_selection.selection.key_label;
    Ok(RouteResolution::Ready {
        route: Box::new(route),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{NativeApi, NativeApiSource, WorkerConfig},
        relay_secrets::RelaySecretManager,
        standalone_config::StandaloneConfigStore,
        standalone_config::{
            ContinuationPolicy, EndpointProvider, ModelRouteConfig, ModelRouteTargetConfig,
            ProviderEndpointConfig, RouteScope, RoutingStrategy, StandaloneConfig,
        },
        worker::runtime::{
            WorkerRuntimeState, prompt_log::RequestPromptLog,
            request_assembly::BufferedBridgeRequest, standalone::StandaloneRuntimeState,
            tests::session_affinity_services,
        },
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    fn request() -> BufferedBridgeRequest {
        BufferedBridgeRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            method: "POST".to_string(),
            path: "/v1/responses".to_string(),
            headers: Vec::new(),
            body: br#"{"model":"local-model","input":"hello"}"#.to_vec(),
            request_deadline_unix_ms: 0,
            user_id: Some(7),
            client_key_hash: None,
            request_user_agent: None,
            http_request_content_encoding: None,
            http_request_compressed: false,
            http_request_compressed_bytes: None,
            http_request_decompressed_bytes: None,
            http_request_compression_ratio: None,
        }
    }

    fn database_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("prompt-ferry-runtime-route-{suffix}.sqlite"))
    }

    #[tokio::test]
    async fn standalone_route_resolution_uses_local_snapshot_and_default_fallback() {
        let _redaction_guard = crate::redact_test_support::lock();
        let path = database_path();
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let endpoint_id = uuid::Uuid::new_v4();
        let snapshot = StandaloneConfig {
            endpoints: vec![ProviderEndpointConfig {
                endpoint_id,
                name: "local endpoint".to_string(),
                provider: EndpointProvider::Generic,
                provider_region: None,
                base_url: "https://local.example".to_string(),
                native_api: NativeApi::Responses,
                native_api_source: NativeApiSource::Manual,
                key_lb_enabled: false,
                enabled: true,
                mcp_enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                api_key: "local-key".to_string(),
                api_keys: Vec::new(),
            }],
            routes: vec![ModelRouteConfig {
                rule_id: uuid::Uuid::new_v4(),
                scope: RouteScope::Admin,
                owner_user_id: None,
                model_pattern: "local-*".to_string(),
                routing_strategy: RoutingStrategy::ClientKeyRendezvous,
                daily_max_requests: None,
                monthly_max_requests: None,
                enabled: true,
                targets: vec![ModelRouteTargetConfig {
                    target_id: uuid::Uuid::new_v4(),
                    endpoint_id,
                    position: 0,
                    enabled: true,
                    upstream_model: Some("provider-local".to_string()),
                    responses_continuation_policy: ContinuationPolicy::ForcePassthrough,
                }],
            }],
            ..StandaloneConfig::default()
        };
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let standalone = StandaloneRuntimeState::new(store.clone(), manager, snapshot);
        let runtime_state = WorkerRuntimeState::default();
        let services = session_affinity_services(
            runtime_state.clone(),
            crate::replay_cache::ReplayCache::default(),
        )
        .with_standalone_state(standalone);
        let request = request();
        let request_ctx = RequestExecutionContext::new(
            uuid::Uuid::new_v4(),
            Instant::now(),
            Some("local-model".to_string()),
            None,
            None,
            Some(7),
            runtime_state.worker_instance_id(),
            RequestPromptLog::default(),
        );
        let route = resolve_route(&request, &WorkerConfig::default(), &services, &request_ctx)
            .await
            .expect("local route")
            .ready_route();
        assert_eq!(route.route_id, endpoint_id);
        assert_eq!(route.upstream_model.as_deref(), Some("provider-local"));

        let fallback_request = BufferedBridgeRequest {
            body: br#"{"model":"other-model"}"#.to_vec(),
            ..request
        };
        let fallback_context = RequestExecutionContext::new(
            uuid::Uuid::new_v4(),
            Instant::now(),
            Some("other-model".to_string()),
            None,
            None,
            Some(7),
            runtime_state.worker_instance_id(),
            RequestPromptLog::default(),
        );
        let fallback = resolve_route(
            &fallback_request,
            &WorkerConfig::default(),
            &services,
            &fallback_context,
        )
        .await
        .expect("fallback route")
        .ready_route();
        assert!(fallback.route_id.is_nil());
        let _ = std::fs::remove_file(path);
    }

    trait ReadyRoute {
        fn ready_route(self) -> crate::db::RouteConfig;
    }

    impl ReadyRoute for RouteResolution {
        fn ready_route(self) -> crate::db::RouteConfig {
            match self {
                Self::Ready { route } => *route,
                Self::Responded => panic!("route unexpectedly responded"),
            }
        }
    }
}

fn default_route(config: &WorkerConfig, request: &BufferedBridgeRequest) -> db::RouteConfig {
    default_route_for_user(config, request.user_id.unwrap_or_default())
}

fn default_route_for_user(config: &WorkerConfig, user_id: i64) -> db::RouteConfig {
    db::RouteConfig {
        route_id: uuid::Uuid::nil(),
        user_id,
        model_route_rule_id: None,
        base_url: config.upstream_base_url.clone(),
        api_key: config.upstream_api_key.clone(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: config.upstream_native_api,
        upstream_model: None,
        responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
        route_selection_reason: db::RouteSelectionReason::Default,
    }
}
