use super::super::{
    RequestExecutionContext, context::RuntimeServices, prompt_log::RequestPromptLog,
    request_assembly::BufferedBridgeRequest,
};
use super::key_pool;
use super::quota_selection::{refresh_candidate_quota, request_model};
use crate::{
    db, endpoint_models,
    worker_admin::{
        AdminState,
        token_plan_cache::{TokenPlanQuotaCache, estimate_input_tokens},
    },
};
use reqwest::Client;
use tracing::{info, warn};

pub(in crate::worker::runtime) struct SelectedRoute {
    pub(in crate::worker::runtime) route: db::RouteConfig,
}

struct CandidateSelection<'a> {
    target: &'a db::ModelRouteCandidateTarget,
    key: db::EndpointApiKeySelection,
    reason: db::RouteSelectionReason,
    invalid_override: bool,
}

pub(in crate::worker::runtime) async fn discover_dynamic_model_route(
    state: &AdminState,
    client: &Client,
    user_id: i64,
    request_model: Option<&str>,
    fallback_route: Option<&db::RouteConfig>,
) -> Option<db::RouteConfig> {
    let model = request_model?;
    let visible_routes = match db::list_visible_endpoints(&state.pool, user_id).await {
        Ok(routes) => routes,
        Err(err) => {
            warn!(
                user_id,
                model,
                error = %err,
                "failed to list visible endpoints for model discovery"
            );
            return None;
        }
    };
    if visible_routes.is_empty() {
        return None;
    }

    // Issue #375 Phase G: dynamic discovery reuses the per-route endpoint
    // proxy pool. Direct routes clone `shared` unchanged; proxy routes use
    // the pooled client. Invalid proxy fails closed (no silent direct).
    let discovered = endpoint_models::discover_route_for_model(
        &state.endpoint_model_cache,
        &visible_routes,
        fallback_route,
        model,
        |route| {
            let shared = client.clone();
            let route = route.clone();
            async move {
                let pooled = endpoint_models::client_for_route(&route, &shared)
                    .map_err(|message| anyhow::anyhow!("{message}"))?;
                endpoint_models::fetch_endpoint_model_ids(&pooled, &route).await
            }
        },
    )
    .await;

    if let Some(route) = &discovered
        && fallback_route.is_none_or(|fallback| fallback.route_id != route.route_id)
    {
        info!(
            user_id,
            model,
            endpoint_id = %route.route_id,
            "selected endpoint via dynamic model discovery"
        );
    }

    discovered
}

/// Unified routing entry: one eligibility filter and one deterministic
/// weighted draw over the whole candidate key pool. Session affinity keeps
/// its binding shape (`endpoint_id` + key) and pins the drawn unit.
pub(in crate::worker::runtime) async fn select_route_for_candidate(
    services: &RuntimeServices,
    request_ctx: &RequestExecutionContext,
    candidate: &db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    user_id: i64,
    routing_key: Option<&str>,
) -> anyhow::Result<Option<SelectedRoute>> {
    // Issue #378 Phase I: schedule fail-closed. Worker-local time; disabled
    // targets are always out. Empty after filtering carries route, time,
    // and per-target windows summaries (no silent fallback).
    let now_min = db::worker_local_minutes_now();
    if !candidate
        .targets
        .iter()
        .any(|target| db::candidate_target_is_active(target, now_min))
    {
        return Err(anyhow::anyhow!(db::schedule_unavailable_message(
            candidate, now_min
        )));
    }
    if services.standalone_state().is_none()
        && candidate.routing_strategy == db::ModelRouteRoutingStrategy::ResponsesSessionAffinity
        && (request.path == "/v1/responses" || request.path == "/v1/chat/completions")
    {
        let selected =
            super::session_affinity::select(services, request_ctx, candidate, request, user_id)
                .await?;
        return Ok(Some(SelectedRoute {
            route: route_from_target(
                selected.target,
                user_id,
                candidate.rule_id,
                selected.key_selection,
                selected.route_selection_reason,
            ),
        }));
    }

    let request_model = request_model(request);
    let quota_cache = services.admin_state().map(|state| &state.token_plan_quota);
    refresh_candidate_quota(services, candidate).await;
    let Some(selected) = select_unified_candidate(
        candidate,
        request,
        &request_ctx.request_prompt_log,
        quota_cache,
        request_model.as_deref(),
        routing_key,
    ) else {
        return Ok(None);
    };
    clear_invalid_conversation_endpoint_key_override(
        services,
        &request_ctx.request_prompt_log,
        selected.invalid_override,
    )
    .await;
    Ok(Some(SelectedRoute {
        route: route_from_target(
            selected.target,
            user_id,
            candidate.rule_id,
            selected.key,
            selected.reason,
        ),
    }))
}

fn select_unified_candidate<'a>(
    candidate: &'a db::ModelRouteCandidate,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
    routing_key: Option<&str>,
) -> Option<CandidateSelection<'a>> {
    let stable_key = routing_stable_key(request, request_prompt_log, routing_key);
    let estimated = estimate_input_tokens(&request.body);

    if let Some(endpoint_id) = request_prompt_log.conversation_override_endpoint_id
        && let Some(target) = candidate.targets.iter().find(|target| {
            // Issue #378 Phase I: window-inactive override targets do not
            // participate (same worker-local clock as the pool filter).
            // Issue #392 Phase K: effective windows (target-nonempty else
            // endpoint else all-day).
            target.enabled
                && target.endpoint_id == endpoint_id
                && db::effective_stored_is_active_at(
                    target.active_windows.as_deref(),
                    target.endpoint_active_windows.as_deref(),
                    db::worker_local_minutes_now(),
                )
        })
    {
        let units = key_pool::target_units(target, quota_cache, model);
        let drawn = key_pool::draw(&units, &stable_key, quota_cache, estimated);
        let key = drawn
            .map(|unit| unit.selection())
            .unwrap_or_else(|| target_secret(target));
        let (key, invalid_override) = key_pool::apply_override(
            target,
            key,
            request_prompt_log.conversation_override_endpoint_key_id,
        );
        return Some(CandidateSelection {
            target,
            key,
            reason: db::RouteSelectionReason::ConversationOverride,
            invalid_override,
        });
    }

    let mut units = key_pool::candidate_units(candidate, quota_cache, model, None);
    if units.is_empty() {
        units = key_pool::candidate_units_without_quota(candidate, None);
    }
    let unit = key_pool::draw(&units, &stable_key, quota_cache, estimated)?;
    let (key, invalid_override) = key_pool::apply_override(
        unit.target,
        unit.selection(),
        request_prompt_log.conversation_override_endpoint_key_id,
    );
    Some(CandidateSelection {
        target: unit.target,
        key,
        reason: db::RouteSelectionReason::Default,
        invalid_override,
    })
}

fn target_secret(target: &db::ModelRouteCandidateTarget) -> db::EndpointApiKeySelection {
    db::EndpointApiKeySelection {
        key_id: None,
        key_label: None,
        secret: target.api_key.clone(),
    }
}

/// Session identity first (conversation / previous response / provider
/// conversation / session header), then the caller-supplied routing key
/// (the client key), then a stable constant.
fn routing_stable_key(
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    routing_key: Option<&str>,
) -> String {
    endpoint_key_stickiness_value(request, request_prompt_log)
        .or_else(|| {
            routing_key
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "default".to_string())
}

fn route_from_target(
    target: &db::ModelRouteCandidateTarget,
    user_id: i64,
    rule_id: uuid::Uuid,
    key_selection: db::EndpointApiKeySelection,
    route_selection_reason: db::RouteSelectionReason,
) -> db::RouteConfig {
    db::RouteConfig {
        route_id: target.endpoint_id,
        user_id,
        model_route_rule_id: Some(rule_id),
        base_url: target.base_url.clone(),
        api_key: key_selection.secret,
        endpoint_key_id: key_selection.key_id,
        endpoint_key_label: key_selection.key_label,
        api_keys: target.api_keys.clone(),
        key_lb_enabled: target.key_lb_enabled,
        // Issue #409 Phase 1: `target.native_api` is already resolved as
        // target-explicit else endpoint fallback (see `hydrate` and
        // standalone `target_from_endpoint`); `Auto` stays `Auto` for
        // per-caller `resolve_auto_protocol`.
        native_api: target.native_api,
        upstream_model: target.upstream_model.clone(),
        route_selection_reason,
        provider: target.provider,
        service_tier: target.service_tier,
        // Issue #368 Phase D: resolved proxy (override wins, empty means
        // direct); pooled client selection uses this value.
        proxy_url: db::resolve_proxy_url(
            target.proxy_url.as_deref(),
            target.proxy_url_override.as_deref(),
        ),
        // Issue #392 Phase K: carry the per-target normalize switch;
        // `false` skips Chat developer->system rewriting.
        dev_system_normalize: target.dev_system_normalize,
        // Issue #464: carry the per-target thinking effort override;
        // `None` means inherit (follow the caller).
        thinking_effort_override: target.thinking_effort_override.clone(),
    }
}

pub(super) fn endpoint_key_stickiness_value(
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> Option<String> {
    if let Some(conversation_id) = request_prompt_log.conversation_id {
        return Some(format!("conversation:{conversation_id}"));
    }
    if let Some(previous_response_id) = request_prompt_log
        .request_previous_response_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("previous_response_id:{previous_response_id}"));
    }
    if let Some(conversation_key) = request_prompt_log
        .request_conversation_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("provider_conversation:{conversation_key}"));
    }
    if let Some(session_header_id) = request_prompt_log
        .session_header_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(format!("session_header:{session_header_id}"));
    }
    request
        .client_key_hash
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("client_key:{value}"))
}

#[cfg(test)]
pub(in crate::worker::runtime) fn materialize_route_api_key_selection(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
) -> EndpointApiKeySelectionResult {
    materialize_route_api_key_selection_with_quota(route, request, request_prompt_log, None)
}

pub(in crate::worker::runtime) fn materialize_route_api_key_selection_with_quota(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: Option<&TokenPlanQuotaCache>,
) -> EndpointApiKeySelectionResult {
    let model = request_model(request);
    let model = model.as_deref();
    if let Some(override_key_id) = request_prompt_log.conversation_override_endpoint_key_id {
        if let Some(selection) = key_pool::override_route_key(route, override_key_id) {
            return EndpointApiKeySelectionResult {
                selection,
                invalid_conversation_override: false,
            };
        }
        let (selection, _) = draw_route_key(route, request, request_prompt_log, quota_cache, model);
        return EndpointApiKeySelectionResult {
            selection,
            invalid_conversation_override: true,
        };
    }
    let (selection, _) = draw_route_key(route, request, request_prompt_log, quota_cache, model);
    EndpointApiKeySelectionResult {
        selection,
        invalid_conversation_override: false,
    }
}

fn draw_route_key(
    route: &db::RouteConfig,
    request: &BufferedBridgeRequest,
    request_prompt_log: &RequestPromptLog,
    quota_cache: Option<&TokenPlanQuotaCache>,
    model: Option<&str>,
) -> (db::EndpointApiKeySelection, bool) {
    let units = key_pool::route_units(route, quota_cache, model);
    let stable_key = endpoint_key_stickiness_value(request, request_prompt_log);
    let estimated = estimate_input_tokens(&request.body);
    if let Some(stable_key) = stable_key
        && let Some(unit) =
            key_pool::draw_route(&units, &stable_key, route.route_id, quota_cache, estimated)
    {
        return (unit.key.clone(), false);
    }
    units
        .first()
        .map(|unit| (unit.key.clone(), false))
        .unwrap_or_else(|| {
            (
                db::EndpointApiKeySelection {
                    key_id: None,
                    key_label: None,
                    secret: route.api_key.clone(),
                },
                false,
            )
        })
}

pub(in crate::worker::runtime) async fn clear_invalid_conversation_endpoint_key_override(
    services: &RuntimeServices,
    request_prompt_log: &RequestPromptLog,
    invalid: bool,
) {
    if !invalid {
        return;
    }
    let (Some(admin_state), Some(conversation_id)) =
        (services.admin_state(), request_prompt_log.conversation_id)
    else {
        return;
    };
    if let Err(err) =
        db::clear_conversation_endpoint_key_override(&admin_state.pool, conversation_id).await
    {
        warn!(
            error = %err,
            conversation_id = %conversation_id,
            "failed to clear invalid conversation endpoint key override"
        );
    }
}

pub(in crate::worker::runtime) struct EndpointApiKeySelectionResult {
    pub(in crate::worker::runtime) selection: db::EndpointApiKeySelection,
    pub(in crate::worker::runtime) invalid_conversation_override: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_target(
        proxy_url: Option<&str>,
        proxy_override: Option<&str>,
    ) -> db::ModelRouteCandidateTarget {
        db::ModelRouteCandidateTarget {
            target_id: uuid::Uuid::new_v4(),
            endpoint_id: uuid::Uuid::new_v4(),
            endpoint_name: "e".to_string(),
            base_url: "https://api.example.test".to_string(),
            api_key: "k".to_string(),
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Chat,
            target_native_api: crate::config::NativeApi::Chat,
            position: 0,
            enabled: true,
            upstream_model: None,
            provider: db::EndpointProvider::Generic,
            service_tier: db::MinimaxServiceTier::Standard,
            proxy_url: proxy_url.map(str::to_string),
            proxy_url_override: proxy_override.map(str::to_string),
            active_windows: None,
            endpoint_active_windows: None,
            dev_system_normalize: false,
            thinking_effort_override: None,
        }
    }

    fn candidate_target_with_native_api(
        target_native_api: crate::config::NativeApi,
        endpoint_native_api: crate::config::NativeApi,
    ) -> db::ModelRouteCandidateTarget {
        let native_api = db::resolve_target_native_api(target_native_api, endpoint_native_api);
        db::ModelRouteCandidateTarget {
            target_id: uuid::Uuid::new_v4(),
            endpoint_id: uuid::Uuid::new_v4(),
            endpoint_name: "e".to_string(),
            base_url: "https://api.example.test".to_string(),
            api_key: "k".to_string(),
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api,
            target_native_api,
            position: 0,
            enabled: true,
            upstream_model: None,
            provider: db::EndpointProvider::Generic,
            service_tier: db::MinimaxServiceTier::Standard,
            proxy_url: None,
            proxy_url_override: None,
            active_windows: None,
            endpoint_active_windows: None,
            dev_system_normalize: false,
            thinking_effort_override: None,
        }
    }

    fn selected_proxy(target: &db::ModelRouteCandidateTarget) -> Option<String> {
        route_from_target(
            target,
            7,
            uuid::Uuid::new_v4(),
            db::EndpointApiKeySelection {
                key_id: None,
                key_label: None,
                secret: "k".to_string(),
            },
            db::RouteSelectionReason::Default,
        )
        .proxy_url
    }

    #[test]
    fn override_wins_over_endpoint_default() {
        let target = candidate_target(
            Some("http://endpoint-proxy.test:8080"),
            Some("http://override-proxy.test:8080"),
        );
        assert_eq!(
            selected_proxy(&target).as_deref(),
            Some("http://override-proxy.test:8080")
        );
    }

    #[test]
    fn falls_back_to_endpoint_default_when_override_missing() {
        let target = candidate_target(Some("http://endpoint-proxy.test:8080"), None);
        assert_eq!(
            selected_proxy(&target).as_deref(),
            Some("http://endpoint-proxy.test:8080")
        );
    }

    #[test]
    fn empty_override_falls_back_and_empty_default_means_direct() {
        let target = candidate_target(Some("http://endpoint-proxy.test:8080"), Some("   "));
        assert_eq!(
            selected_proxy(&target).as_deref(),
            Some("http://endpoint-proxy.test:8080")
        );
        let direct = candidate_target(Some("  "), Some(""));
        assert_eq!(selected_proxy(&direct), None);
        let none = candidate_target(None, None);
        assert_eq!(selected_proxy(&none), None);
    }

    #[test]
    fn trims_whitespace_around_proxy_values() {
        let target = candidate_target(None, Some("  http://proxy.test:8080  "));
        assert_eq!(
            selected_proxy(&target).as_deref(),
            Some("http://proxy.test:8080")
        );
    }

    fn route_for_target(target: &db::ModelRouteCandidateTarget) -> db::RouteConfig {
        route_from_target(
            target,
            7,
            uuid::Uuid::new_v4(),
            db::EndpointApiKeySelection {
                key_id: None,
                key_label: None,
                secret: "k".to_string(),
            },
            db::RouteSelectionReason::Default,
        )
    }

    #[test]
    fn discovery_callback_selects_per_route_proxy_fail_closed() {
        // Issue #375 Phase G P2-1: the dynamic-discovery callback must
        // resolve each route through `endpoint_models::client_for_route`
        // with the shared client, fail closed on invalid proxy, and never
        // fall back to direct.
        let shared = Client::new();
        let proxied = route_for_target(&candidate_target(
            Some("http://proxy-discovery-caller.test:8080"),
            None,
        ));
        let direct = route_for_target(&candidate_target(None, None));
        crate::endpoint_models::client_for_route(&proxied, &shared)
            .expect("proxied discovery route must resolve pooled client");
        crate::endpoint_models::client_for_route(&direct, &shared)
            .expect("direct discovery route must reuse shared client");
        let key = db::proxy_pool_key("http://proxy-discovery-caller.test:8080", &proxied.base_url)
            .expect("proxied key");
        assert_eq!(key.0, "http://proxy-discovery-caller.test:8080");
        assert_eq!(key.1, "api.example.test");
        assert!(db::proxy_pool_key("", &direct.base_url).is_none());
        let invalid = route_for_target(&candidate_target(
            Some("ftp://user:secret@proxy-discovery-invalid.test:21"),
            None,
        ));
        let err = crate::endpoint_models::client_for_route(&invalid, &shared)
            .expect_err("invalid discovery proxy must fail closed");
        assert!(err.contains("scheme"));
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }

    #[test]
    fn target_explicit_native_api_wins_over_endpoint() {
        // Issue #409 Phase 1: target explicit value has priority over the
        // upstream endpoint setting.
        use crate::config::NativeApi;
        let explicit = candidate_target_with_native_api(NativeApi::Chat, NativeApi::Responses);
        assert_eq!(explicit.native_api, NativeApi::Chat);
        assert_eq!(explicit.target_native_api, NativeApi::Chat);
        assert_eq!(route_for_target(&explicit).native_api, NativeApi::Chat);

        let realtime = candidate_target_with_native_api(NativeApi::Realtime, NativeApi::Chat);
        assert_eq!(realtime.native_api, NativeApi::Realtime);
    }

    #[test]
    fn target_auto_falls_back_to_endpoint_native_api() {
        // Issue #409 Phase 1: target `Auto` keeps endpoint/global logic
        // unchanged; both `Auto` stays `Auto` for per-caller resolution.
        use crate::config::NativeApi;
        let fallback = candidate_target_with_native_api(NativeApi::Auto, NativeApi::Responses);
        assert_eq!(fallback.native_api, NativeApi::Responses);
        assert_eq!(route_for_target(&fallback).native_api, NativeApi::Responses);

        let auto = candidate_target_with_native_api(NativeApi::Auto, NativeApi::Auto);
        assert_eq!(auto.native_api, NativeApi::Auto);
    }
}
