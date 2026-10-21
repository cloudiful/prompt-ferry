use super::*;

pub(super) async fn list_model_routes(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Query(query): Query<TablePageQuery>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match state
        .config_repository
        .list_model_routes_page(query.first.unwrap_or(0), query.rows.unwrap_or(10))
        .await
    {
        Ok(page) => Json(ModelRoutePageResponse::from(page)).into_response(),
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn create_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ModelRouteRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match body.validate_for_create(&state).await {
        Ok(()) => {}
        Err(response) => return response.into_response(),
    }
    let input = match body.into_create(&state).await {
        Ok(input) => input,
        Err(response) => return response.into_response(),
    };
    let rule_id = Uuid::new_v4();
    match state
        .config_repository
        .create_model_route(rule_id, input)
        .await
    {
        Ok(rule) => {
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after route create");
            }
            Json(rule).into_response()
        }
        Err(err) => internal(&state, err),
    }
}

pub(super) async fn update_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(rule_id): Path<Uuid>,
    Json(body): Json<ModelRouteRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match body.validate_for_update(&state, rule_id).await {
        Ok(()) => {}
        Err(response) => return response.into_response(),
    }
    // Issue #368 Phase B: PATCH carry for per-target overrides. The unified
    // route shape is redacted, so load the stored overrides separately;
    // omitted per-target `proxy_url_override` keeps the stored value.
    // Issue #378 Phase I: same carry for `active_windows` (omitted keeps).
    let (existing_overrides, existing_windows) = load_existing_route_carry(&state, rule_id).await;
    let input = match body
        .into_create_with_existing(&state, &existing_overrides, &existing_windows)
        .await
    {
        Ok(input) => input,
        Err(response) => return response.into_response(),
    };
    match state
        .config_repository
        .update_model_route(rule_id, input)
        .await
    {
        Ok(Some(rule)) => {
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after route update");
            }
            Json(rule).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "not_found", "model route not found"),
        Err(err) => internal(&state, err),
    }
}

/// Issue #368 Phase B: load stored per-target proxy overrides for PATCH carry.
/// Returns `endpoint_id -> override` (`None` means inherit). Best-effort:
/// missing rules or backends yield an empty map and the update then falls
/// back to inherit; the subsequent `update_model_route` still returns 404
/// for unknown rules.
/// Issue #378 Phase I: also loads stored `active_windows` for the same
/// omit-when-untouched carry (`None`/empty means all-day).
async fn load_existing_route_carry(
    state: &AdminState,
    rule_id: Uuid,
) -> (
    HashMap<Uuid, Option<String>>,
    HashMap<Uuid, Vec<db::ActiveWindow>>,
) {
    if let Some(pool) = state.config_repository.as_postgres() {
        if let Ok(Some(rule)) = db::get_model_endpoint_rule(pool, rule_id).await {
            return carry_from_targets(
                rule.targets
                    .iter()
                    .map(|target| {
                        (
                            target.endpoint_id,
                            target.proxy_url_override.clone(),
                            target.active_windows.clone(),
                        )
                    })
                    .collect(),
            );
        }
        return (HashMap::new(), HashMap::new());
    }
    if let Some(repo) = state.config_repository.as_sqlite()
        && let Ok(Some(route)) = repo
            .store()
            .get_route(repo.manager(), rule_id)
            .await
            .map_err(|_| anyhow::anyhow!("sqlite lookup failed"))
    {
        return carry_from_targets(
            route
                .targets
                .iter()
                .map(|target| {
                    (
                        target.endpoint_id,
                        target.proxy_url_override.clone(),
                        parse_sqlite_windows(&target.active_windows),
                    )
                })
                .collect(),
        );
    }
    (HashMap::new(), HashMap::new())
}

fn carry_from_targets(
    targets: Vec<(Uuid, Option<String>, Vec<db::ActiveWindow>)>,
) -> (
    HashMap<Uuid, Option<String>>,
    HashMap<Uuid, Vec<db::ActiveWindow>>,
) {
    let mut overrides = HashMap::new();
    let mut windows = HashMap::new();
    for (endpoint_id, proxy_override, active_windows) in targets {
        overrides.insert(
            endpoint_id,
            proxy_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        );
        windows.insert(endpoint_id, active_windows);
    }
    (overrides, windows)
}

fn parse_sqlite_windows(raw: &Option<String>) -> Vec<db::ActiveWindow> {
    db::parse_stored_windows(raw.as_deref()).unwrap_or_default()
}

pub(super) async fn delete_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Path(rule_id): Path<Uuid>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    match state.config_repository.delete_model_route(rule_id).await {
        Ok(true) => {
            if let Err(err) = publish_snapshot(&state).await {
                tracing::warn!(error = %err, "snapshot publication failed after route delete");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => error(StatusCode::NOT_FOUND, "not_found", "model route not found"),
        Err(err) => internal(&state, err),
    }
}

/// Issue #1004 P2: the per-target probe budget and the concurrency of the
/// collection. Every eligible target gets an outcome, so a target that runs
/// out of budget reports that instead of disappearing.
const MODEL_ROUTE_TEST_TARGET_BUDGET: Duration = Duration::from_secs(20);
const MODEL_ROUTE_TEST_ACTION_BUDGET: Duration = Duration::from_secs(60);
const MODEL_ROUTE_TEST_CONCURRENCY: usize = 4;

pub(super) async fn test_model_route(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<ModelRouteTestRequest>,
) -> Response {
    if let Err(response) = ensure_admin(&state, &headers).await {
        return response.into_response();
    }
    // For Phase 3 the test path is only implemented against PostgreSQL because
    // it needs `model_route_candidates` which still joins against the
    // `provider_endpoints` and `endpoint_api_keys` PostgreSQL tables.
    let Some(pool) = state.config_repository.as_postgres() else {
        return state.capability_unavailable(db::Capability::ModelRouteTest);
    };
    let rule = match db::get_model_endpoint_rule(pool, body.rule_id).await {
        Ok(Some(rule)) => rule,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_found", "model route not found"),
        Err(err) => return internal(&state, err),
    };
    let started = Instant::now();
    let candidate = match db::get_model_route_candidate(pool, body.rule_id).await {
        Ok(Some(candidate)) => candidate,
        // Issue #1004 P2: the rule exists, but none of its targets or
        // endpoints is enabled, so there is nothing to test. That is the same
        // explicit zero-eligible answer as a target outside its schedule
        // window, not a missing rule.
        Ok(None) => {
            return Json(zero_eligible_response(
                &rule,
                None,
                started.elapsed().as_millis(),
            ))
            .into_response();
        }
        Err(err) => return internal(&state, err),
    };
    // Issue #1004 P2: the candidate already holds the enabled rule, target and
    // endpoint rows. The schedule window is checked against one captured now,
    // so every outcome of this run answers the same question.
    let now_minutes = db::worker_local_minutes_now();
    let eligible = eligible_targets(&candidate, now_minutes);
    // The preferred target is metadata for the reader, never a filter.
    let preferred = choose_preferred_target(&candidate, model_route_test_routing_key(&candidate));
    if eligible.is_empty() {
        return Json(zero_eligible_response(
            &rule,
            preferred.as_ref(),
            started.elapsed().as_millis(),
        ))
        .into_response();
    }
    let results = probe_model_route_targets(&state, &candidate, eligible, preferred.as_ref()).await;
    let duration_ms = started.elapsed().as_millis();
    let passed = results.iter().filter(|result| result.ok).count() as u32;
    let total = results.len() as u32;
    // The legacy single-result fields describe the preferred target's outcome
    // when it was tested, and the first outcome otherwise.
    let representative = results
        .iter()
        .find(|result| result.preferred)
        .or_else(|| results.first());
    Json(ModelRouteTestResponse {
        ok: total > 0 && passed == total,
        status: representative.and_then(|result| result.status),
        duration_ms,
        endpoint_id: representative.map(|result| result.endpoint_id),
        endpoint_name: representative.map(|result| result.endpoint_name.clone()),
        preferred_endpoint_id: preferred.as_ref().map(|target| target.endpoint_id),
        preferred_endpoint_name: preferred
            .as_ref()
            .map(|target| target.endpoint_name.clone()),
        rule_id: Some(rule.rule_id),
        model_pattern: Some(rule.model_pattern),
        model: representative.and_then(|result| result.model.clone()),
        message: representative
            .map(|result| result.message.clone())
            .unwrap_or_default(),
        results,
        passed,
        total,
    })
    .into_response()
}

/// Issue #1004 P2: the message of a rule without an eligible target. It is
/// actionable and never a green result.
const NO_ELIGIBLE_TARGET_MESSAGE: &str = "no enabled target of this route is currently active; enable a target or widen its schedule window";

/// Issue #1004 P2: the answer of a rule without an eligible target.
///
/// A rule whose targets or endpoints are all disabled has the same answer as a
/// rule whose targets are all outside their schedule window: nothing was
/// tested, and the result is never green.
fn zero_eligible_response(
    rule: &db::ModelEndpointRule,
    preferred: Option<&db::ModelRouteCandidateTarget>,
    duration_ms: u128,
) -> ModelRouteTestResponse {
    ModelRouteTestResponse {
        ok: false,
        status: None,
        duration_ms,
        endpoint_id: None,
        endpoint_name: None,
        preferred_endpoint_id: preferred.map(|target| target.endpoint_id),
        preferred_endpoint_name: preferred.map(|target| target.endpoint_name.clone()),
        rule_id: Some(rule.rule_id),
        model_pattern: Some(rule.model_pattern.clone()),
        model: None,
        message: NO_ELIGIBLE_TARGET_MESSAGE.to_string(),
        results: Vec::new(),
        passed: 0,
        total: 0,
    }
}

/// Issue #1004 P2: the targets a route test may probe.
///
/// A candidate holds the enabled rule, target and endpoint rows. A target
/// outside its schedule window is not eligible, and the eligible targets keep
/// the configured order.
fn eligible_targets(
    candidate: &db::ModelRouteCandidate,
    now_minutes: u16,
) -> Vec<db::ModelRouteCandidateTarget> {
    candidate
        .targets
        .iter()
        .filter(|target| db::candidate_target_is_active(target, now_minutes))
        .cloned()
        .collect()
}

/// Issue #1004 P2: probe every eligible target once.
///
/// The probes run with a bounded concurrency. A target that exceeds its own
/// budget or the overall action budget still gets an explicit failure
/// outcome, and the outcomes keep the configured target order.
async fn probe_model_route_targets(
    state: &AdminState,
    candidate: &db::ModelRouteCandidate,
    eligible: Vec<db::ModelRouteCandidateTarget>,
    preferred: Option<&db::ModelRouteCandidateTarget>,
) -> Vec<ModelRouteTestTargetResult> {
    use futures::StreamExt;

    let preferred_id = preferred.map(|target| target.target_id);
    let order = eligible
        .iter()
        .map(|target| target.target_id)
        .collect::<Vec<_>>();
    let deadline = tokio::time::Instant::now() + MODEL_ROUTE_TEST_ACTION_BUDGET;
    let outcomes = futures::stream::iter(eligible.into_iter())
        .map(|target| probe_target_within_budget(state, candidate, target, deadline, preferred_id))
        .buffer_unordered(MODEL_ROUTE_TEST_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    // The outcomes arrive in completion order; the reader expects the
    // configured target order.
    let mut ordered = Vec::with_capacity(outcomes.len());
    for target_id in order {
        if let Some(result) = outcomes.iter().find(|result| result.target_id == target_id) {
            ordered.push(result.clone());
        }
    }
    ordered
}

/// Issue #1004 P2: probe one target within the collection budgets.
///
/// A target that exceeds its own budget, or the overall action budget, still
/// gets an explicit failure outcome.
async fn probe_target_within_budget(
    state: &AdminState,
    candidate: &db::ModelRouteCandidate,
    target: db::ModelRouteCandidateTarget,
    deadline: tokio::time::Instant,
    preferred_id: Option<Uuid>,
) -> ModelRouteTestTargetResult {
    let started = Instant::now();
    let probe = tokio::time::timeout(
        MODEL_ROUTE_TEST_TARGET_BUDGET,
        run_model_route_test(state, candidate, &target),
    );
    let mut result = match tokio::time::timeout_at(deadline, probe).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => failed_target_result(
            &target,
            StatusCode::GATEWAY_TIMEOUT,
            "the target did not answer within the test budget",
        ),
        Err(_) => failed_target_result(
            &target,
            StatusCode::GATEWAY_TIMEOUT,
            "the test budget for this route was exhausted before the target answered",
        ),
    };
    result.duration_ms = started.elapsed().as_millis();
    result.preferred = preferred_id == Some(target.target_id);
    result
}

/// Issue #1004 P2: a target outcome without an answer.
fn failed_target_result(
    target: &db::ModelRouteCandidateTarget,
    status: StatusCode,
    message: &str,
) -> ModelRouteTestTargetResult {
    ModelRouteTestTargetResult {
        target_id: target.target_id,
        endpoint_id: target.endpoint_id,
        endpoint_name: target.endpoint_name.clone(),
        model: None,
        ok: false,
        status: Some(status.as_u16()),
        duration_ms: 0,
        message: message.to_string(),
        preferred: false,
    }
}

/// Issue #1004 P2: the credential of one platform target.
///
/// A currently enabled configured key is preferred, so a disabled key is
/// never used. An endpoint without any configured key has no usable
/// credential and is reported as a failed target instead of being probed with
/// an empty bearer.
fn probe_credential(target: &db::ModelRouteCandidateTarget) -> Option<String> {
    let mut keys = target
        .api_keys
        .iter()
        .filter(|key| {
            key.endpoint_id == target.endpoint_id && key.enabled && !key.api_key.trim().is_empty()
        })
        .collect::<Vec<_>>();
    keys.sort_by(|a, b| {
        a.position
            .cmp(&b.position)
            .then_with(|| a.key_label.cmp(&b.key_label))
            .then_with(|| a.key_id.cmp(&b.key_id))
    });
    if let Some(key) = keys.first() {
        return Some(key.api_key.clone());
    }
    // Configured keys exist but none of them is enabled: there is no usable
    // credential, and the legacy value may be a disabled key.
    if !target.api_keys.is_empty() {
        return None;
    }
    let legacy = target.api_key.trim();
    (!legacy.is_empty()).then(|| legacy.to_string())
}

/// Issue #1004 P2: probe one model-route target.
///
/// The outcome never fails as a whole: a transport error becomes a failed
/// target, so the other targets of the same route keep their own outcome.
async fn run_model_route_test(
    state: &AdminState,
    candidate: &db::ModelRouteCandidate,
    target: &db::ModelRouteCandidateTarget,
) -> ModelRouteTestTargetResult {
    match probe_model_route_target(state, candidate, target).await {
        Ok((model, status, message)) => ModelRouteTestTargetResult {
            target_id: target.target_id,
            endpoint_id: target.endpoint_id,
            endpoint_name: target.endpoint_name.clone(),
            model: Some(model),
            ok: status.is_success(),
            status: Some(status.as_u16()),
            duration_ms: 0,
            message,
            preferred: false,
        },
        Err(err) => ModelRouteTestTargetResult {
            target_id: target.target_id,
            endpoint_id: target.endpoint_id,
            endpoint_name: target.endpoint_name.clone(),
            model: None,
            ok: false,
            status: None,
            duration_ms: 0,
            message: truncate_message(&maybe_redact(state, &err.to_string())),
            preferred: false,
        },
    }
}

async fn probe_model_route_target(
    state: &AdminState,
    candidate: &db::ModelRouteCandidate,
    target: &db::ModelRouteCandidateTarget,
) -> Result<(String, StatusCode, String), reqwest::Error> {
    // Issue #368 Phase D: the test probe uses the same resolved proxy
    // (`override ?? endpoint`) as live forwarding so the "test" button is
    // representative. Invalid proxy fails closed as BAD_GATEWAY without
    // echoing userinfo.
    let client = match probe_client_for_target(target) {
        Ok(client) => client,
        Err((status, message)) => {
            return Ok((candidate.model_pattern.clone(), status, message));
        }
    };
    // Issue #1004 P1: a ChatGPT subscription target carries no Platform API
    // key and has no Platform model listing. The probe answers from the
    // Codex model table and verifies the device credential instead of
    // sending a Platform request with a placeholder key.
    if target.has_oauth_token {
        let (model, status, message) =
            run_chatgpt_subscription_probe(state, candidate, target, &client).await;
        return Ok((model, status, message));
    }
    // Issue #1004 P2: a platform target is probed with one currently enabled
    // configured key. A target without a usable credential is a failed
    // outcome with an actionable message, never an empty bearer.
    let Some(credential) = probe_credential(target) else {
        return Ok((
            candidate.model_pattern.clone(),
            StatusCode::BAD_REQUEST,
            "no API key is configured for this endpoint; add a Platform API key or complete the ChatGPT login"
                .to_string(),
        ));
    };
    // GLM (issue #230 P2) lists models at `{base}/models` (the Coding
    // Plan base already encodes the protocol root); every other provider
    // keeps the plain `/v1/models` join. The probe URL composition
    // further down is routed through `upstream_url_for_route_parts`
    // so the per-`native_api` `/v1` strip applies uniformly.
    let models_request = client.get(crate::endpoint_models::models_url(
        &target.base_url,
        target.provider,
    ));
    let models_request = match target.native_api {
        NativeApi::AnthropicMessages => models_request
            .header("x-api-key", &credential)
            .header("anthropic-version", "2023-06-01"),
        _ => models_request.bearer_auth(&credential),
    };
    let models_response = models_request.send().await?;
    let models_status = models_response.status();
    let models_body = models_response.text().await?;
    if !models_status.is_success() {
        return Ok((
            candidate.model_pattern.clone(),
            models_status,
            truncate_message(&maybe_redact(state, models_body.trim())),
        ));
    }
    let Some(model) = target
        .upstream_model
        .clone()
        .or_else(|| select_test_model(&candidate.model_pattern, &models_body))
    else {
        return Ok((
            candidate.model_pattern.clone(),
            StatusCode::NOT_FOUND,
            "no upstream model matched rule".to_string(),
        ));
    };
    if target.native_api == NativeApi::Auto {
        return Ok((
            model,
            StatusCode::OK,
            "OK (auto; caller protocol is selected at request time)".to_string(),
        ));
    }
    if target.native_api == NativeApi::Realtime {
        // P4 (issue #230): route the probe through the runtime helper so
        // the GLM `/v1` strip applies; a GLM Realtime base
        // (`.../api/coding/paas/v4`) joins `/realtime` not `/v1/realtime`.
        let url = format!(
            "{}?model={}",
            crate::worker::upstream_url_for_route_parts(
                &target.base_url,
                target.provider,
                target.native_api,
                NativeApi::Realtime.path(),
            )
            .replace("https://", "wss://")
            .replace("http://", "ws://"),
            urlencoding::encode(&model)
        );
        let mut request =
            match tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
                url,
            ) {
                Ok(request) => request,
                Err(err) => {
                    return Ok((model, StatusCode::BAD_REQUEST, err.to_string()));
                }
            };
        if let Ok(value) = header::HeaderValue::from_str(&format!("Bearer {credential}")) {
            request.headers_mut().insert(header::AUTHORIZATION, value);
        }
        match tokio_tungstenite::connect_async_with_config(request, None, false).await {
            Ok((mut socket, _)) => {
                let _ = futures::SinkExt::send(
                    &mut socket,
                    tokio_tungstenite::tungstenite::Message::Close(None),
                )
                .await;
                return Ok((
                    model,
                    StatusCode::OK,
                    format!("OK ({})", target.native_api.as_str()),
                ));
            }
            Err(err) => {
                return Ok((
                    model,
                    StatusCode::BAD_GATEWAY,
                    truncate_message(&maybe_redact(state, &err.to_string())),
                ));
            }
        }
    }
    let (path, mut payload) = match target.native_api {
        NativeApi::AnthropicMessages => (
            NativeApi::AnthropicMessages.path(),
            serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "stream": false,
                "max_tokens": 1,
            }),
        ),
        NativeApi::Responses => (
            NativeApi::Responses.path(),
            serde_json::json!({
                "model": model,
                "input": "ping",
                "stream": false,
                "max_output_tokens": 1,
            }),
        ),
        NativeApi::Chat => (
            NativeApi::Chat.path(),
            serde_json::json!({
                "model": model,
                "stream": false,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 1,
            }),
        ),
        NativeApi::Auto => unreachable!("auto model routes return before protocol test"),
        NativeApi::Realtime => unreachable!(),
    };
    // Issue #644: keep probe behavior consistent with live forwarding. The
    // configured free-form tier is provider-agnostic
    // (`db::supports_service_tier_for`): every HTTP JSON protocol injects it
    // when configured, while Realtime — which carries WebSocket frames —
    // stays untouched so the "test" button remains representative.
    apply_service_tier_to_probe_payload(
        target.native_api,
        target.service_tier.as_deref(),
        &mut payload,
    );
    // P4 (issue #230): route the probe through the runtime helper so the
    // GLM `/v1` strip applies (a GLM Chat base
    // `.../api/coding/paas/v4` joins `/chat/completions`, not
    // `/v1/chat/completions`).
    let request = client
        .post(crate::worker::upstream_url_for_route_parts(
            &target.base_url,
            target.provider,
            target.native_api,
            path,
        ))
        .json(&payload);
    let request = match target.native_api {
        NativeApi::AnthropicMessages => request
            .header("x-api-key", &credential)
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(&credential),
    };
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let message = if status.is_success() {
        format!("OK ({})", target.native_api.as_str())
    } else {
        truncate_message(&maybe_redact(state, body.trim()))
    };
    Ok((model, status, message))
}

/// Issue #1004 P1: probe a ChatGPT subscription model-route target.
///
/// The model comes from the Codex model table (the subscription backend has
/// no model listing), and the credential check reuses the OAuth
/// refresh/header/quota GET conventions. It is a credential and connectivity
/// probe, not a generation probe.
async fn run_chatgpt_subscription_probe(
    state: &AdminState,
    candidate: &db::ModelRouteCandidate,
    target: &db::ModelRouteCandidateTarget,
    client: &reqwest::Client,
) -> (String, StatusCode, String) {
    let Some(model) = target.upstream_model.clone().or_else(|| {
        chatgpt_backend::codex_models()
            .iter()
            .find(|model| db::model_pattern_matches(&candidate.model_pattern, model))
            .map(|model| (*model).to_string())
    }) else {
        return (
            candidate.model_pattern.clone(),
            StatusCode::NOT_FOUND,
            "no upstream model matched rule".to_string(),
        );
    };
    let token = match state
        .config_repository
        .get_endpoint_oauth_token(target.endpoint_id)
        .await
    {
        Ok(Some(token)) => token,
        Ok(None) => {
            return (
                model,
                StatusCode::BAD_REQUEST,
                "complete the ChatGPT login for this endpoint first".to_string(),
            );
        }
        Err(err) => {
            return (
                model,
                StatusCode::INTERNAL_SERVER_ERROR,
                truncate_message(&maybe_redact(state, &err.to_string())),
            );
        }
    };
    let (access_token, account_id) = match chatgpt_backend::resolve_subscription_access_token(
        &state.config_repository,
        client,
        target.endpoint_id,
        &token,
    )
    .await
    {
        Ok(resolved) => resolved,
        Err(err) => {
            return (
                model,
                StatusCode::UNAUTHORIZED,
                truncate_message(&maybe_redact(state, &err.to_string())),
            );
        }
    };
    match chatgpt_backend::fetch_chatgpt_quota(client, &access_token, account_id.as_deref()).await {
        Ok(_) => (
            model,
            StatusCode::OK,
            "OK (ChatGPT subscription credential verified)".to_string(),
        ),
        Err(err) => (
            model,
            StatusCode::BAD_GATEWAY,
            truncate_message(&maybe_redact(state, &err.to_string())),
        ),
    }
}

/// Issue #368 Phase D: build the test-probe client with the resolved
/// proxy (`override ?? endpoint`). Direct targets reuse a plain client;
/// proxy targets add `Proxy::all`. Errors are redacted and fail closed.
fn probe_client_for_target(
    target: &db::ModelRouteCandidateTarget,
) -> Result<reqwest::Client, (StatusCode, String)> {
    let resolved = db::resolve_proxy_url(
        target.proxy_url.as_deref(),
        target.proxy_url_override.as_deref(),
    );
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(20));
    if let Some(proxy_url) = resolved {
        let trimmed = proxy_url.trim().to_string();
        if let Err(message) = db::validate_outbound_proxy_url(&trimmed) {
            tracing::warn!(
                endpoint_id = %target.endpoint_id,
                proxy = %db::redact_proxy_url_for_log(&trimmed),
                "rejecting model route test with invalid proxy configuration"
            );
            return Err((StatusCode::BAD_GATEWAY, message.to_string()));
        }
        match reqwest::Proxy::all(trimmed.clone()) {
            Ok(proxy) => {
                builder = builder.proxy(proxy);
                tracing::debug!(
                    endpoint_id = %target.endpoint_id,
                    proxy = %db::redact_proxy_url_for_log(&trimmed),
                    "model route test via configured proxy"
                );
            }
            Err(_) => {
                tracing::warn!(
                    endpoint_id = %target.endpoint_id,
                    proxy = %db::redact_proxy_url_for_log(&trimmed),
                    "rejecting model route test with unsupported proxy URL"
                );
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "invalid proxy configuration".to_string(),
                ));
            }
        }
    }
    builder.build().map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "failed to build HTTP client".to_string(),
        )
    })
}

fn select_test_model(pattern: &str, models_body: &str) -> Option<String> {
    let body = serde_json::from_str::<serde_json::Value>(models_body).ok()?;
    let models = body.get("data")?.as_array()?;
    models
        .iter()
        .filter_map(|item| item.get("id").and_then(|value| value.as_str()))
        .find(|model| db::model_pattern_matches(pattern, model))
        .map(str::to_string)
}

fn model_route_test_routing_key(candidate: &db::ModelRouteCandidate) -> Option<&'static str> {
    // Issue #466: only `responses_session_affinity` remains.
    match candidate.routing_strategy {
        db::ModelRouteRoutingStrategy::ResponsesSessionAffinity => {
            Some(MODEL_ROUTE_TEST_SESSION_KEY)
        }
    }
}

/// Issue #644: inject the configured free-form service tier into a probe
/// payload the same way live forwarding does. The gate is provider-agnostic
/// (any endpoint may carry a configured value) and covers every HTTP JSON
/// protocol; Realtime and blank/unset values leave the payload unchanged.
fn apply_service_tier_to_probe_payload(
    native_api: NativeApi,
    tier: Option<&str>,
    payload: &mut serde_json::Value,
) {
    if !db::supports_service_tier_for(native_api) {
        return;
    }
    let Some(tier) = tier.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    object.insert(
        db::SERVICE_TIER_WIRE_KEY.to_string(),
        serde_json::Value::String(tier.to_string()),
    );
}

#[cfg(test)]
mod service_tier_probe_tests {
    use super::*;

    #[test]
    fn probe_injects_configured_tier_on_every_json_protocol() {
        // Issue #644: provider-agnostic — the helper takes only the resolved
        // protocol, and every JSON protocol receives the configured value.
        for native_api in [
            NativeApi::Chat,
            NativeApi::Responses,
            NativeApi::AnthropicMessages,
        ] {
            let mut payload = serde_json::json!({"model": "m", "service_tier": "standard"});
            apply_service_tier_to_probe_payload(native_api, Some("priority"), &mut payload);
            assert_eq!(payload["service_tier"], "priority", "{native_api:?}");
            assert_eq!(payload["model"], "m");
        }
    }

    #[test]
    fn probe_leaves_realtime_and_unset_tiers_unchanged() {
        let mut realtime = serde_json::json!({"model": "m", "service_tier": "standard"});
        apply_service_tier_to_probe_payload(NativeApi::Realtime, Some("priority"), &mut realtime);
        assert_eq!(realtime["service_tier"], "standard");

        for tier in [None, Some(""), Some("   ")] {
            let mut payload = serde_json::json!({"model": "m", "service_tier": "standard"});
            apply_service_tier_to_probe_payload(NativeApi::Chat, tier, &mut payload);
            assert_eq!(payload["service_tier"], "standard", "{tier:?}");
        }
    }
}

#[cfg(test)]
mod route_test_probe_tests {
    use super::*;

    fn endpoint_key(
        endpoint_id: uuid::Uuid,
        key_id: uuid::Uuid,
        secret: &str,
        enabled: bool,
        position: i32,
    ) -> db::EndpointApiKey {
        db::EndpointApiKey {
            key_id,
            endpoint_id,
            key_label: "primary".to_string(),
            api_key: secret.to_string(),
            position,
            enabled,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn target(
        endpoint_id: uuid::Uuid,
        enabled: bool,
        active_windows: Option<&str>,
        keys: Vec<db::EndpointApiKey>,
    ) -> db::ModelRouteCandidateTarget {
        db::ModelRouteCandidateTarget {
            target_id: uuid::Uuid::new_v4(),
            endpoint_id,
            endpoint_name: "endpoint".to_string(),
            base_url: "https://api.example.test".to_string(),
            api_key: keys
                .first()
                .map(|key| key.api_key.clone())
                .unwrap_or_default(),
            api_keys: keys,
            key_lb_enabled: false,
            native_api: NativeApi::Responses,
            target_native_api: NativeApi::Responses,
            position: 0,
            enabled,
            upstream_model: None,
            provider: db::EndpointProvider::Generic,
            service_tier: None,
            proxy_url: None,
            proxy_url_override: None,
            active_windows: active_windows.map(str::to_string),
            endpoint_active_windows: None,
            dev_system_normalize: false,
            thinking_downgrade_enabled: false,
            thinking_effort_override: None,
            compact_mode: db::CompactMode::Passthrough,
            has_oauth_token: false,
        }
    }

    fn candidate(targets: Vec<db::ModelRouteCandidateTarget>) -> db::ModelRouteCandidate {
        db::ModelRouteCandidate {
            rule_id: uuid::Uuid::new_v4(),
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            updated_at: chrono::Utc::now(),
            targets,
        }
    }

    #[test]
    fn a_disabled_target_is_not_eligible() {
        let endpoint_id = uuid::Uuid::new_v4();
        let candidate = candidate(vec![target(endpoint_id, false, None, Vec::new())]);
        assert!(eligible_targets(&candidate, 600).is_empty());
    }

    #[test]
    fn a_target_outside_its_window_is_not_eligible() {
        let endpoint_id = uuid::Uuid::new_v4();
        // A window that never covers the captured minute of the day.
        let candidate = candidate(vec![target(
            endpoint_id,
            true,
            Some(r#"[{"start":"00:00","end":"00:01"}]"#),
            Vec::new(),
        )]);
        assert!(eligible_targets(&candidate, 600).is_empty());
    }

    #[test]
    fn the_eligible_targets_keep_the_configured_order() {
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let candidate = candidate(vec![
            target(first, true, None, Vec::new()),
            target(second, true, None, Vec::new()),
        ]);
        let eligible = eligible_targets(&candidate, 600);
        assert_eq!(eligible.len(), 2);
        assert_eq!(eligible[0].endpoint_id, first);
        assert_eq!(eligible[1].endpoint_id, second);
    }

    #[test]
    fn the_probe_uses_an_enabled_configured_key() {
        let endpoint_id = uuid::Uuid::new_v4();
        let enabled = endpoint_key(endpoint_id, uuid::Uuid::new_v4(), "sk-enabled", true, 1);
        let disabled = endpoint_key(endpoint_id, uuid::Uuid::new_v4(), "sk-disabled", false, 0);
        let target = target(
            endpoint_id,
            true,
            None,
            vec![disabled.clone(), enabled.clone()],
        );
        assert_eq!(probe_credential(&target).as_deref(), Some("sk-enabled"));
    }

    #[test]
    fn the_probe_reports_no_credential_when_every_key_is_disabled() {
        let endpoint_id = uuid::Uuid::new_v4();
        let disabled = endpoint_key(endpoint_id, uuid::Uuid::new_v4(), "sk-disabled", false, 0);
        let target = target(endpoint_id, true, None, vec![disabled]);
        assert_eq!(probe_credential(&target), None);
    }

    #[test]
    fn the_probe_uses_the_legacy_key_when_no_key_row_is_configured() {
        let endpoint_id = uuid::Uuid::new_v4();
        let mut target = target(endpoint_id, true, None, Vec::new());
        target.api_key = "sk-legacy".to_string();
        assert_eq!(probe_credential(&target).as_deref(), Some("sk-legacy"));
    }
}
