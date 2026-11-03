//! The builtin MiniMax MCP tools (issue #1179 P1).
//!
//! `web_search` and `understand_image` are read-only tools, so a quota or
//! rate rejection may switch to another endpoint key. The candidate
//! selection reuses the shared quota cache and the shared weighted pool
//! draw, the response feedback reuses the shared per-key feedback store, and
//! the whole call shares one deadline: the first attempt plus at most two
//! switches to another key.

mod classify;
mod diagnostic;
mod image;
mod selection;
pub(crate) mod transport;

#[cfg(test)]
mod tests;

use anyhow::anyhow;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;
use uuid::Uuid;

use crate::db;
use crate::mcp::ServerCatalogSnapshot;
use crate::mcp::call_record::McpCallFailure;
use crate::worker_admin::{
    quota_feedback::{QuotaFeedbackCause, QuotaFeedbackScope, QuotaProbeClaim},
    token_plan_cache::{QuotaScope, TokenPlanQuotaCache},
};

use diagnostic::Diagnostic;
use selection::{Candidate, PoolState, ToolScope};

pub(super) const SOURCE_HEADER: &str = "prompt-ferry-MiniMax-MCP";

/// The upstream attempts one tool call may spend: the first attempt plus two
/// switches to another key.
const MAX_ATTEMPTS: usize = 3;

/// The read-only tools the builtin provider exposes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tool {
    WebSearch,
    UnderstandImage,
}

impl Tool {
    fn scope(self) -> ToolScope {
        match self {
            Self::WebSearch => selection::WEB_SEARCH_SCOPE,
            Self::UnderstandImage => selection::UNDERSTAND_IMAGE_SCOPE,
        }
    }

    /// The tool name, for a bounded diagnostic.
    fn name(self) -> &'static str {
        match self {
            Self::WebSearch => "web_search",
            Self::UnderstandImage => "understand_image",
        }
    }
}

/// One prepared tool call.
struct Plan {
    endpoint_id: Uuid,
    tool: Tool,
    path: &'static str,
    body: Value,
    id: Value,
    candidates: Vec<Candidate>,
    /// Whether the operator enabled the key balancing for this endpoint.
    lb_enabled: bool,
    scope: ToolScope,
    deadline: tokio::time::Instant,
}

/// The probe claim of one attempt.
enum ProbeOutcome {
    NotRequired,
    Reserved(crate::worker_admin::quota_feedback::QuotaProbeGuard),
    Busy,
}

pub(crate) fn catalog() -> ServerCatalogSnapshot {
    ServerCatalogSnapshot {
        tools: vec![
            json!({
                "name": "web_search",
                "description": "Search the web with MiniMax Coding Plan.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "query": { "type": "string" } },
                    "required": ["query"]
                }
            }),
            json!({
                "name": "understand_image",
                "description": "Analyze an image with MiniMax Coding Plan.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string" },
                        "image_source": { "type": "string" }
                    },
                    "required": ["prompt", "image_source"]
                }
            }),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    }
}

pub(crate) async fn call(
    storage: &crate::mcp::McpRuntimeStorage,
    server: &db::McpServer,
    request: &Value,
    conversation_id: Option<&str>,
) -> anyhow::Result<Value> {
    // One deadline covers the payload preparation, the selection, the
    // feedback and every upstream attempt. It never restarts per key.
    let deadline =
        tokio::time::Instant::now() + Duration::from_millis(server.timeout_ms.max(1) as u64);
    let endpoint_id = server
        .source_endpoint_id
        .ok_or_else(|| Diagnostic::SourceEndpointMissing.into_error())?;
    // The lookup error may carry database detail, so only a fixed diagnostic
    // reaches the client; the detail stays in the log.
    let endpoint = match storage.repository().get_endpoint_for_mcp(endpoint_id).await {
        Ok(endpoint) => {
            endpoint.ok_or_else(|| Diagnostic::SourceEndpointUnavailable.into_error())?
        }
        Err(error) => {
            tracing::warn!(error = %error, "MiniMax MCP source endpoint lookup failed");
            return Err(Diagnostic::SourceEndpointUnavailable.into_error());
        }
    };
    if endpoint.provider != db::EndpointProvider::Minimax {
        return Err(Diagnostic::SourceEndpointNotMinimax.into_error());
    }
    if !endpoint.enabled {
        return Err(Diagnostic::SourceEndpointDisabled.into_error());
    }
    let host = match endpoint.provider_region {
        Some(db::EndpointRegion::Cn) => "https://api.minimaxi.com",
        Some(db::EndpointRegion::Global) => "https://api.minimax.io",
        None => return Err(Diagnostic::SourceEndpointRegionMissing.into_error()),
    };
    // Issue #375 Phase F+G: builtin inherits its source endpoint proxy
    // (endpoint → env → direct) for the MiniMax API request via the endpoint
    // proxy pool (`mcp::transport::proxy::client_for_builtin`). Invalid
    // endpoint proxy fails closed with userinfo scrubbed; empty falls
    // through to env/direct. The row's own proxy_url is ignored by design
    // (managed rows carry none). Image-URL fetches stay direct (pinned DNS,
    // see `image::normalize_image_source`).
    let client = crate::mcp::transport::proxy::client_for_builtin(
        endpoint.proxy_url.as_deref(),
        host,
        server.timeout_ms,
    )
    .map_err(|message| {
        tracing::warn!(server_name = %server.name, "{message}");
        Diagnostic::ProxyRejected.into_error()
    })?;
    let name = request.pointer("/params/name").and_then(Value::as_str);
    let arguments = request
        .pointer("/params/arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let tool = prepare_tool(name, &arguments, deadline).await?;
    let candidates = selection::candidates(&endpoint);
    if candidates.is_empty() {
        return Err(Diagnostic::NoEndpointKey.into_error());
    }
    let plan = Plan {
        endpoint_id,
        tool: tool.tool,
        path: tool.path,
        body: tool.body,
        id: request.get("id").cloned().unwrap_or(Value::Null),
        candidates,
        lb_enabled: endpoint.key_lb_enabled,
        scope: tool.tool.scope(),
        deadline,
    };
    // Issue #1179 P2: the in-process tests install a stub transport, so the
    // builtin selection and its failover can run without an upstream call.
    let transport: std::sync::Arc<dyn transport::MiniMaxTransport> =
        match storage.builtin_transport() {
            Some(transport) => transport.clone(),
            None => std::sync::Arc::new(transport::HttpTransport::new(client, host.to_string())),
        };
    execute(
        &plan,
        transport.as_ref(),
        storage.quota(),
        conversation_id,
        storage.tracker(),
    )
    .await
}

/// The prepared payload of one tool.
struct PreparedTool {
    tool: Tool,
    path: &'static str,
    body: Value,
}

/// Prepare the validated tool payload once, before any upstream attempt.
async fn prepare_tool(
    name: Option<&str>,
    arguments: &serde_json::Map<String, Value>,
    deadline: tokio::time::Instant,
) -> anyhow::Result<PreparedTool> {
    match name {
        Some("web_search") => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| Diagnostic::QueryRequired.into_error())?;
            Ok(PreparedTool {
                tool: Tool::WebSearch,
                path: "/v1/coding_plan/search",
                body: json!({ "q": query }),
            })
        }
        Some("understand_image") => {
            let prompt = arguments
                .get("prompt")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| Diagnostic::PromptRequired.into_error())?;
            let image_source = arguments
                .get("image_source")
                .and_then(Value::as_str)
                .ok_or_else(|| Diagnostic::ImageSourceRequired.into_error())?;
            let budget = deadline.saturating_duration_since(tokio::time::Instant::now());
            let image_url = image::normalize_image_source(image_source, budget)
                .await
                .map_err(|_| Diagnostic::ImageSourceRejected.into_error())?;
            Ok(PreparedTool {
                tool: Tool::UnderstandImage,
                path: "/v1/coding_plan/vlm",
                body: image::image_body(prompt, &image_url),
            })
        }
        Some(_) => Err(Diagnostic::UnsupportedTool.into_error()),
        None => Err(Diagnostic::MissingToolName.into_error()),
    }
}

/// Run the tool call.
///
/// The candidates are reassessed before every attempt, so a key that became
/// unusable while the call was running drops out of the pool. A quota or rate
/// rejection switches to the next eligible key; every other failure returns
/// to the caller without an automatic replay. Every failure the client sees is
/// a fixed diagnostic: no provider message, transport exception, trace header,
/// query, image source or credential is ever part of it.
async fn execute(
    plan: &Plan,
    transport: &dyn transport::MiniMaxTransport,
    cache: Option<&TokenPlanQuotaCache>,
    conversation_id: Option<&str>,
    tracker: &crate::mcp::call_record::McpCallTracker,
) -> anyhow::Result<Value> {
    if plan.candidates.is_empty() {
        // No usable credential was selected: no upstream request is allowed.
        tracker.record_failure(McpCallFailure::CallFailed { status: None });
        return Err(Diagnostic::NoEndpointKey.into_error());
    }
    let mut attempted: BTreeSet<usize> = BTreeSet::new();
    let mut attempts = 0usize;
    let mut last_diagnostic: Option<Diagnostic> = None;
    let mut last_failure: Option<McpCallFailure> = None;
    let stable_key = selection::draw_key(conversation_id, selection::nonce());
    let max_attempts = if plan.lb_enabled { MAX_ATTEMPTS } else { 1 };

    while attempts < max_attempts {
        let index = if plan.lb_enabled {
            match selection::draw(
                cache,
                plan.endpoint_id,
                &plan.candidates,
                plan.scope,
                &attempted,
                &stable_key,
            ) {
                selection::Selection::Drawn(drawn) => drawn.index,
                selection::Selection::Unavailable(state) => {
                    let diagnostic = last_diagnostic.unwrap_or_else(|| pool_diagnostic(state));
                    tracker.record_failure(
                        last_failure.unwrap_or(McpCallFailure::CallFailed { status: None }),
                    );
                    return Err(diagnostic.into_error());
                }
            }
        } else {
            // The operator pinned one key: the legacy single-key selection is
            // preserved, so no other key is ever substituted for it.
            0
        };
        let candidate = &plan.candidates[index];
        let mut probe = match claim_probe(cache, plan, candidate, plan.lb_enabled) {
            ProbeOutcome::Busy => {
                // Another caller probes this key: skip it for this call
                // without spending an attempt.
                attempted.insert(index);
                continue;
            }
            ProbeOutcome::Reserved(guard) => Some(guard),
            ProbeOutcome::NotRequired => None,
        };
        attempts += 1;
        // The tracker records the key that served this attempt, so the
        // settlement of the request knows which credential was used even
        // though the attempt runs in the rmcp session task.
        tracker.record_endpoint_key(candidate.key_id, Some(candidate.label.clone()));
        tracker.record_token_slot(i16::try_from(candidate.position.max(0) as usize + 1).ok());
        let attempt_start = std::time::Instant::now();
        let remaining = plan
            .deadline
            .saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            tracker.record_failure(McpCallFailure::CallFailed { status: None });
            return Err(Diagnostic::Deadline.into_error());
        }
        let response = match tokio::time::timeout(
            remaining,
            transport.send(transport::MiniMaxRequest {
                path: plan.path,
                body: &plan.body,
                secret: &candidate.secret,
            }),
        )
        .await
        {
            Ok(Ok(response)) => response,
            // A transport failure is not a quota signal, so the call returns
            // without an automatic replay. The original error is dropped: its
            // text may repeat the credential, the query or an image payload.
            Ok(Err(_error)) => {
                tracker.record_failure(McpCallFailure::CallFailed { status: None });
                return Err(Diagnostic::TransportFailed { status: None }.into_error());
            }
            Err(_elapsed) => {
                tracker.record_failure(McpCallFailure::CallFailed { status: None });
                return Err(Diagnostic::RequestTimedOut.into_error());
            }
        };

        match classify::classify(&response, plan.tool) {
            classify::Outcome::Success => {
                record_success(cache, plan, candidate, attempt_start);
                drop(probe.take());
                tracker.record_success();
                return success_result(plan, &response.payload);
            }
            classify::Outcome::Exhausted { code } => {
                // The plan quota of the tried key is spent. The general quota
                // group is the one the search tool bills, so only that tool
                // feeds the shared exhaustion signal: the image tool bills a
                // group the provider does not document and must not claim
                // the general quota is exhausted.
                record_failure(
                    cache,
                    plan,
                    candidate,
                    QuotaFeedbackCause::Exhausted,
                    None,
                    attempt_start,
                );
                drop(probe.take());
                tracing::warn!(
                    endpoint_id = %plan.endpoint_id,
                    key_label = %candidate.label,
                    attempt = attempts,
                    reason = "quota_exhausted",
                    "MiniMax MCP key reported an exhausted quota"
                );
                last_diagnostic = Some(Diagnostic::QuotaExhausted {
                    provider_code: code,
                });
                last_failure = Some(McpCallFailure::QuotaExhausted {
                    provider_code: code,
                });
                attempted.insert(index);
                tracker.record_attempt_failure();
                if !plan.lb_enabled {
                    tracker.record_failure(
                        last_failure.unwrap_or(McpCallFailure::CallFailed { status: None }),
                    );
                    return Err(last_diagnostic
                        .unwrap_or(Diagnostic::Superseded)
                        .into_error());
                }
            }
            classify::Outcome::RateLimited { retry_after } => {
                // The rate limit feeds the shared cooldown, which honours the
                // bounded `Retry-After`. The call switches to another key
                // instead of sleeping through the caller budget.
                record_failure(
                    cache,
                    plan,
                    candidate,
                    QuotaFeedbackCause::RateLimited,
                    retry_after,
                    attempt_start,
                );
                drop(probe.take());
                tracing::warn!(
                    endpoint_id = %plan.endpoint_id,
                    key_label = %candidate.label,
                    attempt = attempts,
                    reason = "rate_limited",
                    "MiniMax MCP key was rate limited"
                );
                let status = reqwest::StatusCode::TOO_MANY_REQUESTS.as_u16();
                last_diagnostic = Some(Diagnostic::RateLimited { status });
                last_failure = Some(McpCallFailure::CallFailed {
                    status: Some(status),
                });
                attempted.insert(index);
                tracker.record_attempt_failure();
                if !plan.lb_enabled {
                    tracker.record_failure(
                        last_failure.unwrap_or(McpCallFailure::CallFailed { status: None }),
                    );
                    return Err(last_diagnostic
                        .unwrap_or(Diagnostic::Superseded)
                        .into_error());
                }
            }
            classify::Outcome::Failed { status } => {
                drop(probe.take());
                tracker.record_failure(McpCallFailure::CallFailed {
                    status: Some(status),
                });
                return Err(Diagnostic::UpstreamStatus { status }.into_error());
            }
            classify::Outcome::ProviderFailed { code } => {
                drop(probe.take());
                tracker.record_failure(McpCallFailure::CallFailed { status: None });
                return Err(Diagnostic::ProviderError {
                    provider_code: code,
                }
                .into_error());
            }
            classify::Outcome::Malformed => {
                // An unreadable or unexpected body never counts as a
                // completed call and never feeds a success observation.
                drop(probe.take());
                tracker.record_failure(McpCallFailure::CallFailed { status: None });
                return Err(Diagnostic::MalformedResponse {
                    tool: plan.tool.name(),
                }
                .into_error());
            }
        }
    }

    tracker.record_failure(last_failure.unwrap_or(McpCallFailure::CallFailed { status: None }));
    Err(last_diagnostic
        .unwrap_or(Diagnostic::Superseded)
        .into_error())
}

/// The fixed diagnostic of a pool that cannot serve the call.
fn pool_diagnostic(state: PoolState) -> Diagnostic {
    match state {
        PoolState::Empty => Diagnostic::NoEndpointKey,
        PoolState::Exhausted => Diagnostic::PoolExhausted,
        PoolState::Cooling => Diagnostic::PoolCooling,
        PoolState::ProbeBusy => Diagnostic::PoolProbeBusy,
        PoolState::Blocked => Diagnostic::PoolBlocked,
    }
}

/// Claim the probe slot of the drawn candidate.
///
/// The image tool never claims a shared probe: the quota group it bills is not
/// documented, so a guarded claim would let an image call block the search
/// path on a signal the provider never confirmed.
fn claim_probe(
    cache: Option<&TokenPlanQuotaCache>,
    plan: &Plan,
    candidate: &Candidate,
    lb_enabled: bool,
) -> ProbeOutcome {
    if !lb_enabled || plan.tool != Tool::WebSearch {
        return ProbeOutcome::NotRequired;
    }
    let Some(cache) = cache else {
        return ProbeOutcome::NotRequired;
    };
    let scope = QuotaScope::new(
        plan.endpoint_id,
        candidate.key_id,
        plan.scope.model,
        plan.scope.allow_general,
    );
    match cache.claim_scope_probe(scope) {
        QuotaProbeClaim::NotRequired => ProbeOutcome::NotRequired,
        QuotaProbeClaim::Reserved(guard) => ProbeOutcome::Reserved(guard),
        QuotaProbeClaim::Busy => ProbeOutcome::Busy,
    }
}

/// The feedback scope of one candidate.
fn feedback_scope(plan: &Plan, candidate: &Candidate) -> QuotaFeedbackScope {
    QuotaFeedbackScope::new(plan.endpoint_id, candidate.key_id)
}

/// Record the success of one attempt for the tried key.
///
/// Only the search tool feeds the shared feedback store: an image success says
/// nothing about the general quota group, so it must never clear an exhaustion
/// the search path recorded.
fn record_success(
    cache: Option<&TokenPlanQuotaCache>,
    plan: &Plan,
    candidate: &Candidate,
    attempt_start: std::time::Instant,
) {
    if plan.tool != Tool::WebSearch {
        return;
    }
    let Some(cache) = cache else {
        return;
    };
    cache.feedback.record_success(
        feedback_scope(plan, candidate),
        attempt_start,
        std::time::Instant::now(),
    );
}

/// Record a failure of one attempt for the tried key.
///
/// Only the search tool feeds the shared feedback store; an image rejection
/// neither claims nor clears the general quota signal.
fn record_failure(
    cache: Option<&TokenPlanQuotaCache>,
    plan: &Plan,
    candidate: &Candidate,
    cause: QuotaFeedbackCause,
    retry_after: Option<Duration>,
    attempt_start: std::time::Instant,
) {
    if plan.tool != Tool::WebSearch {
        return;
    }
    let Some(cache) = cache else {
        return;
    };
    cache.feedback.record_failure(
        feedback_scope(plan, candidate),
        cause,
        retry_after,
        std::time::Instant::now(),
        attempt_start,
    );
}

/// The tool result of a successful response.
fn success_result(plan: &Plan, payload: &Value) -> anyhow::Result<Value> {
    let text = if plan.tool == Tool::UnderstandImage {
        payload
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("MiniMax VLM returned no content"))?
            .to_string()
    } else {
        serde_json::to_string_pretty(payload)?
    };
    Ok(json!({
        "jsonrpc": "2.0",
        "id": plan.id,
        "result": { "content": [{ "type": "text", "text": text }] }
    }))
}
