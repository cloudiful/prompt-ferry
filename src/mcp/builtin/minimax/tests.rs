use super::*;
use crate::mcp::call_record::{McpCallOutcome, McpCallTracker};
use crate::worker_admin::quota_feedback::QuotaFeedbackEligibility;
use crate::worker_admin::token_plan_cache::TokenPlanQuotaCache;
use crate::worker_admin_types::{
    TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse, TokenPlanWindowUsage,
};
use reqwest::StatusCode;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Mutex;

/// The stub responder: it answers one bearer secret.
type Responder = Box<dyn Fn(&str) -> anyhow::Result<transport::MiniMaxResponse> + Send + Sync>;

/// The in-process transport stub: it answers per bearer secret and records
/// every attempted key, so the selection and the failover are exercised
/// without an upstream call.
struct StubTransport {
    responder: Responder,
    delay: Duration,
    attempts: Mutex<Vec<String>>,
}

impl StubTransport {
    fn new(
        responder: impl Fn(&str) -> anyhow::Result<transport::MiniMaxResponse> + Send + Sync + 'static,
    ) -> Self {
        Self {
            responder: Box::new(responder),
            delay: Duration::ZERO,
            attempts: Mutex::new(Vec::new()),
        }
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    fn attempts(&self) -> Vec<String> {
        self.attempts
            .lock()
            .expect("attempt log is not poisoned")
            .clone()
    }
}

#[async_trait::async_trait]
impl transport::MiniMaxTransport for StubTransport {
    async fn send(
        &self,
        request: transport::MiniMaxRequest<'_>,
    ) -> anyhow::Result<transport::MiniMaxResponse> {
        self.attempts
            .lock()
            .expect("attempt log is not poisoned")
            .push(request.secret.to_string());
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        (self.responder)(request.secret)
    }
}

/// The documented `web_search` success payload.
fn ok_response() -> transport::MiniMaxResponse {
    transport::MiniMaxResponse {
        status: StatusCode::OK,
        payload: json!({ "base_resp": { "status_code": 0 }, "organic": [] }),
        retry_after: None,
    }
}

/// The documented `understand_image` success payload.
fn ok_image_response() -> transport::MiniMaxResponse {
    transport::MiniMaxResponse {
        status: StatusCode::OK,
        payload: json!({ "base_resp": { "status_code": 0 }, "content": "hello" }),
        retry_after: None,
    }
}

fn exhausted_response() -> transport::MiniMaxResponse {
    transport::MiniMaxResponse {
        status: StatusCode::OK,
        payload: json!({
            "base_resp": { "status_code": 2056, "status_msg": "usage limit reached" }
        }),
        retry_after: None,
    }
}

fn status_response(status: u16) -> transport::MiniMaxResponse {
    transport::MiniMaxResponse {
        status: StatusCode::from_u16(status).expect("a valid status code"),
        payload: json!({ "base_resp": { "status_code": 1000, "status_msg": "upstream said no" } }),
        retry_after: None,
    }
}

/// Run the tool call with a fresh tracker, the way the entry does.
async fn execute_call(
    plan: &Plan,
    transport: &StubTransport,
    cache: Option<&TokenPlanQuotaCache>,
) -> anyhow::Result<Value> {
    let tracker = McpCallTracker::new();
    execute(plan, transport, cache, None, &tracker).await
}

fn candidate(key_id: Uuid, label: &str, secret: &str, position: i32) -> Candidate {
    Candidate {
        key_id: Some(key_id),
        label: label.to_string(),
        secret: secret.to_string(),
        position,
    }
}

fn plan(candidates: Vec<Candidate>, lb_enabled: bool, tool: Tool) -> Plan {
    Plan {
        endpoint_id: Uuid::new_v4(),
        tool,
        path: match tool {
            Tool::WebSearch => "/v1/coding_plan/search",
            Tool::UnderstandImage => "/v1/coding_plan/vlm",
        },
        body: json!({ "q": "rust" }),
        id: json!(7),
        candidates,
        lb_enabled,
        scope: tool.scope(),
        deadline: tokio::time::Instant::now() + Duration::from_secs(30),
    }
}

fn window(remaining_percent: f64) -> TokenPlanWindowUsage {
    TokenPlanWindowUsage {
        status: Some(1),
        remaining_percent: Some(remaining_percent),
        total_count: None,
        usage_count: None,
        boost_permille: None,
        start_at: None,
        end_at: None,
        remains_time_ms: None,
    }
}

/// A cache that holds one general quota group per key.
async fn quota_cache(endpoint_id: Uuid, keys: &[(Uuid, f64, f64)]) -> TokenPlanQuotaCache {
    let cache = TokenPlanQuotaCache::default();
    let usage = TokenPlanUsageResponse {
        local_today_tokens: None,
        quota_inference_available: None,
        provider: db::EndpointProvider::Minimax,
        provider_region: None,
        keys: keys
            .iter()
            .map(|(key_id, interval, weekly)| TokenPlanKeyUsage {
                key_id: *key_id,
                key_label: "k".into(),
                ok: true,
                status: Some(200),
                error_code: None,
                error_message: None,
                quota_status: None,
                model_remains: vec![TokenPlanModelUsage {
                    model_name: "general".to_string(),
                    interval: Some(window(*interval)),
                    weekly: Some(window(*weekly)),
                    windows: None,
                    observation: None,
                }],
                balances: None,
                five_hour: None,
                weekly: None,
                opencodego_rolling: None,
                opencodego_weekly: None,
                opencodego_monthly: None,
                openrouter_balance: None,
                openrouter_spend: None,
                glm_five_hour: None,
                glm_weekly: None,
                deepseek_balance: None,
                quota_windows: None,
            })
            .collect(),
    };
    cache.store_for_test(endpoint_id, usage).await;
    cache
}

#[test]
fn catalog_exposes_only_supported_minimax_tools() {
    let catalog = catalog();
    let names = catalog
        .tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(names, ["web_search", "understand_image"]);
    assert!(catalog.resources.is_empty());
    assert!(catalog.prompts.is_empty());
}

#[tokio::test]
async fn an_unknown_write_tool_is_rejected_before_any_attempt() {
    let result = prepare_tool(
        Some("future_write"),
        &serde_json::Map::new(),
        tokio::time::Instant::now() + Duration::from_secs(1),
    )
    .await;
    assert!(result.is_err(), "an unknown tool has no retry plan");
}

#[test]
fn blocks_ipv4_loopback_and_private_ranges() {
    let cases = [
        ("127.0.0.1", true),
        ("127.255.255.254", true),
        ("10.0.0.1", true),
        ("10.255.255.254", true),
        ("172.16.0.1", true),
        ("172.31.255.254", true),
        ("192.168.0.1", true),
        ("192.168.255.254", true),
        ("0.0.0.0", true),
        ("169.254.0.1", true),
        ("224.0.0.1", true),
        ("239.255.255.255", true),
        ("240.0.0.1", true),
        ("255.255.255.255", true),
        ("1.1.1.1", false),
        ("8.8.8.8", false),
        ("172.15.255.254", false),
        ("172.32.0.1", false),
        ("192.169.0.1", false),
    ];
    for (text, expected) in cases {
        let ip: IpAddr = text.parse().unwrap();
        assert_eq!(
            image::is_blocked_ip(ip),
            expected,
            "is_blocked_ip({ip}) should be {expected}",
        );
    }
}

#[test]
fn blocks_ipv6_loopback_private_and_link_local_ranges() {
    let cases = [
        ("::1", true),
        ("::", true),
        ("fc00::1", true),
        ("fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", true),
        ("fe80::1", true),
        ("febf:ffff::", true),
        ("ff00::1", true),
        ("ff02::1", true),
        ("2001:db8::1", true),
        ("2606:4700:4700::1111", false),
    ];
    for (text, expected) in cases {
        let ip: IpAddr = text.parse().unwrap();
        assert_eq!(
            image::is_blocked_ip(ip),
            expected,
            "is_blocked_ip({ip}) should be {expected}",
        );
    }
}

#[test]
fn blocked_nets_cover_every_documented_category() {
    // Belt-and-braces: every category present in the production ranges must
    // be represented in the const list so that future edits cannot silently
    // drop a range.
    let categories: &[(IpAddr, u8, &str)] = &[
        (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8, "unspecified"),
        (IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8, "private-10"),
        (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8, "loopback"),
        (IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0)), 16, "link-local"),
        (IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0)), 12, "private-172"),
        (IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0)), 16, "private-192"),
        (IpAddr::V4(Ipv4Addr::new(224, 0, 0, 0)), 4, "multicast"),
        (IpAddr::V4(Ipv4Addr::new(240, 0, 0, 0)), 4, "reserved"),
        (IpAddr::V6(Ipv6Addr::UNSPECIFIED), 128, "v6-unspecified"),
        (IpAddr::V6(Ipv6Addr::LOCALHOST), 128, "v6-loopback"),
        (
            IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)),
            7,
            "v6-ula",
        ),
        (
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0)),
            10,
            "v6-link-local",
        ),
        (
            IpAddr::V6(Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0)),
            8,
            "v6-multicast",
        ),
    ];
    for (ip, prefix, label) in categories {
        let net = ipnet::IpNet::new_assert(*ip, *prefix);
        assert!(
            image::BLOCKED_NETS.iter().any(|existing| existing == &net),
            "missing blocked range {label} ({ip}/{prefix})",
        );
    }
}

#[tokio::test]
async fn a_funded_key_is_selected_when_the_other_keys_are_exhausted() {
    let endpoint_id = Uuid::new_v4();
    let spent = Uuid::new_v4();
    let funded = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(spent, 100.0, 0.0), (funded, 79.0, 61.0)]).await;
    let candidates = vec![
        candidate(spent, "spent", "sk-spent", 0),
        candidate(funded, "funded", "sk-funded", 1),
    ];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(ok_response()));
    let result = execute_call(&plan, &transport, Some(&cache)).await;

    assert!(result.is_ok(), "the funded key must serve the call");
    assert_eq!(transport.attempts(), vec!["sk-funded".to_string()]);
}

#[tokio::test]
async fn the_third_key_serves_when_both_earlier_weekly_quotas_are_zero() {
    let endpoint_id = Uuid::new_v4();
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    let third = Uuid::from_u128(3);
    let cache = quota_cache(
        endpoint_id,
        &[
            (first, 100.0, 0.0),
            (second, 100.0, 0.0),
            (third, 79.0, 61.0),
        ],
    )
    .await;
    let candidates = vec![
        candidate(first, "first", "sk-first", 0),
        candidate(second, "second", "sk-second", 1),
        candidate(third, "third", "sk-third", 2),
    ];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.endpoint_id = endpoint_id;
    let transport = StubTransport::new(|_| Ok(ok_response()));

    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the funded third key serves the call");
    assert_eq!(transport.attempts(), vec!["sk-third".to_string()]);
}

#[tokio::test]
async fn absent_observations_spread_over_the_enabled_keys() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
        candidate(Uuid::new_v4(), "c", "sk-c", 2),
    ];
    let mut selected = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let plan = plan(candidates.clone(), true, Tool::WebSearch);
        let transport = StubTransport::new(|_| Ok(ok_response()));
        execute_call(&plan, &transport, None)
            .await
            .expect("a call succeeds");
        selected.extend(transport.attempts());
    }
    assert!(
        selected.len() > 1,
        "the selection must not pin one key: {selected:?}"
    );
}

#[tokio::test]
async fn a_cooling_key_is_skipped() {
    let endpoint_id = Uuid::new_v4();
    let cooling = Uuid::new_v4();
    let funded = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(funded, 100.0, 100.0)]).await;
    cache.feedback.record_429(
        QuotaFeedbackScope::new(endpoint_id, Some(cooling)),
        None,
        std::time::Instant::now(),
        std::time::Instant::now(),
    );
    let candidates = vec![
        candidate(cooling, "cooling", "sk-cooling", 0),
        candidate(funded, "funded", "sk-funded", 1),
    ];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(ok_response()));
    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the funded key serves the call");

    assert_eq!(transport.attempts(), vec!["sk-funded".to_string()]);
}

#[tokio::test]
async fn a_probe_busy_key_is_skipped() {
    let endpoint_id = Uuid::new_v4();
    let busy = Uuid::new_v4();
    let funded = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(funded, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(busy));
    cache.feedback.record_429(
        scope,
        Some(Duration::ZERO),
        std::time::Instant::now(),
        std::time::Instant::now(),
    );
    let reserved = cache
        .feedback
        .reserve_probe_if_required(scope, std::time::Instant::now());
    assert!(matches!(
        reserved,
        crate::worker_admin::quota_feedback::QuotaProbeReservation::Reserved
    ));
    let candidates = vec![
        candidate(busy, "busy", "sk-busy", 0),
        candidate(funded, "funded", "sk-funded", 1),
    ];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(ok_response()));
    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the funded key serves the call");

    assert_eq!(transport.attempts(), vec!["sk-funded".to_string()]);
}

#[tokio::test]
async fn a_probe_scope_is_released_after_the_attempt() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    cache.feedback.record_429(
        scope,
        Some(Duration::ZERO),
        std::time::Instant::now(),
        std::time::Instant::now(),
    );
    let candidates = vec![candidate(key_id, "only", "sk-only", 0)];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(ok_response()));
    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the call succeeds");

    let eligibility = cache.feedback.eligibility(scope, std::time::Instant::now());
    assert!(
        !matches!(
            eligibility,
            crate::worker_admin::quota_feedback::QuotaFeedbackEligibility::ProbeBusy
        ),
        "the probe guard must be released: {eligibility:?}"
    );
}

#[tokio::test]
async fn a_quota_code_switches_to_the_next_key() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    // The first attempt reports the spent quota, the second one succeeds, so
    // the switch is observed whichever key the draw picked first.
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let transport = StubTransport::new(move |_| {
        let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if call == 0 {
            exhausted_response()
        } else {
            ok_response()
        })
    });

    let result = execute_call(&plan, &transport, None).await;

    assert!(result.is_ok(), "the second key must serve the call");
    let attempts = transport.attempts();
    assert_eq!(attempts.len(), 2, "the call switches to another key");
    assert_ne!(attempts[0], attempts[1], "a key is never tried twice");
}

#[tokio::test]
async fn a_rate_limit_switches_to_the_next_key() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let transport = StubTransport::new(move |_| {
        let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if call == 0 {
            status_response(429)
        } else {
            ok_response()
        })
    });

    let result = execute_call(&plan, &transport, None).await;

    assert!(result.is_ok(), "the second key must serve the call");
    let attempts = transport.attempts();
    assert_eq!(attempts.len(), 2, "the call switches to another key");
    assert_ne!(attempts[0], attempts[1], "a key is never tried twice");
}

#[tokio::test]
async fn an_authentication_failure_is_not_replayed() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let transport = StubTransport::new(|_| Ok(status_response(401)));

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("an authentication failure is returned");

    assert!(error.to_string().contains("401"), "{error}");
    assert_eq!(
        transport.attempts().len(),
        1,
        "an authentication failure is never replayed"
    );
}

#[tokio::test]
async fn a_server_failure_is_not_replayed() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let transport = StubTransport::new(|_| Ok(status_response(500)));

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("a server failure is returned");

    assert!(error.to_string().contains("500"), "{error}");
    assert_eq!(
        transport.attempts().len(),
        1,
        "a server failure is never replayed"
    );
}

#[tokio::test]
async fn a_transport_failure_is_not_replayed() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    // The transport exception repeats the credential and the query.
    let transport = StubTransport::new(|secret| {
        Err(anyhow!(
            "connection reset for {secret} while searching \"rust\""
        ))
    });

    let tracker = McpCallTracker::new();
    let error = execute(&plan, &transport, None, None, &tracker)
        .await
        .expect_err("a transport failure is returned");

    // The client sees a fixed diagnostic, never the transport text that
    // repeated the credential and the query.
    assert_eq!(error.to_string(), "MiniMax MCP upstream call failed");
    assert!(
        !format!("{error:?}").contains("connection reset"),
        "{error:?}"
    );
    assert!(!error.to_string().contains("sk-"), "{error}");
    assert!(!error.to_string().contains("rust"), "{error}");
    assert_eq!(
        transport.attempts().len(),
        1,
        "a transport failure is never replayed"
    );
    let record = tracker.snapshot();
    let (code, message) = record.failure().expect("a failure");
    assert_eq!(code, "mcp_call_failed");
    assert_eq!(message, "the upstream call failed");
    assert!(!message.contains("sk-"), "{message}");
    assert!(!message.contains("rust"), "{message}");
}

#[tokio::test]
async fn the_attempt_ceiling_is_three_and_no_key_is_tried_twice() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
        candidate(Uuid::new_v4(), "c", "sk-c", 2),
        candidate(Uuid::new_v4(), "d", "sk-d", 3),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let transport = StubTransport::new(|_| Ok(exhausted_response()));

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("every attempt fails");

    let attempts = transport.attempts();
    assert_eq!(attempts.len(), 3, "the ceiling is three attempts");
    let unique = attempts.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), attempts.len(), "a key is tried once");
    assert!(error.to_string().contains("2056"), "{error}");
}

#[tokio::test]
async fn an_exhausted_pool_sends_no_request() {
    let endpoint_id = Uuid::new_v4();
    let spent = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(spent, 100.0, 0.0)]).await;
    let candidates = vec![candidate(spent, "spent", "sk-spent", 0)];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(ok_response()));
    let error = execute_call(&plan, &transport, Some(&cache))
        .await
        .expect_err("an exhausted pool fails the call");

    assert!(
        error.to_string().contains("exhausted"),
        "the error must name the exhausted quota: {error}"
    );
    assert!(transport.attempts().is_empty(), "no upstream call is sent");
}

#[tokio::test]
async fn the_image_tool_never_claims_the_general_quota_is_exhausted() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let candidates = vec![candidate(key_id, "image", "sk-image", 0)];
    let mut plan = plan(candidates, true, Tool::UnderstandImage);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(exhausted_response()));
    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect_err("the image quota rejection is returned");

    let assessment = cache.assess_scope(QuotaScope::new(
        endpoint_id,
        Some(key_id),
        Some("general"),
        true,
    ));
    assert!(
        matches!(
            assessment,
            crate::worker_admin::token_plan_cache::QuotaAssessment::Eligible(_)
        ),
        "an image rejection must not mark the general quota exhausted: {assessment:?}"
    );
}

#[tokio::test]
async fn the_image_tool_never_guesses_a_quota_group() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    // The cached general group is spent, but the image tool bills an
    // undocumented group, so the key stays selectable.
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 0.0)]).await;
    let candidates = vec![candidate(key_id, "image", "sk-image", 0)];
    let mut plan = plan(candidates, true, Tool::UnderstandImage);
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(ok_image_response()));
    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the image tool still calls the key");

    assert_eq!(transport.attempts(), vec!["sk-image".to_string()]);
}

#[tokio::test]
async fn the_image_tool_ignores_a_shared_rate_cooldown_and_probe() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    let now = std::time::Instant::now();
    // The search path put the key into a long rate cooldown, which makes a
    // shared recovery probe pending as well.
    cache
        .feedback
        .record_429(scope, Some(Duration::from_secs(60)), now, now);
    assert_ne!(
        cache.feedback.eligibility(scope, now),
        QuotaFeedbackEligibility::Eligible,
        "the search path sees the cooldown"
    );

    let mut plan = plan(
        vec![candidate(key_id, "image", "sk-image", 0)],
        true,
        Tool::UnderstandImage,
    );
    plan.endpoint_id = endpoint_id;
    let transport = StubTransport::new(|_| Ok(ok_image_response()));

    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the image tool never reads the shared feedback");

    assert_eq!(transport.attempts(), vec!["sk-image".to_string()]);
}

#[tokio::test]
async fn the_image_tool_runs_beside_a_live_search_probe() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    let now = std::time::Instant::now();
    // The search path owns a live recovery guard on the key.
    cache
        .feedback
        .record_429(scope, Some(Duration::ZERO), now, now);
    let probe_scope = QuotaScope::new(endpoint_id, Some(key_id), Some("general"), true);
    let guard = match cache.claim_scope_probe(probe_scope) {
        crate::worker_admin::quota_feedback::QuotaProbeClaim::Reserved(guard) => guard,
        _ => panic!("the search path must own the live probe"),
    };

    let mut plan = plan(
        vec![candidate(key_id, "image", "sk-image", 0)],
        true,
        Tool::UnderstandImage,
    );
    plan.endpoint_id = endpoint_id;
    let transport = StubTransport::new(|_| Ok(ok_image_response()));
    execute_call(&plan, &transport, Some(&cache))
        .await
        .expect("the image tool runs beside a live search probe");
    assert_eq!(transport.attempts(), vec!["sk-image".to_string()]);

    assert_eq!(
        cache.feedback.eligibility(scope, std::time::Instant::now()),
        QuotaFeedbackEligibility::ProbeBusy,
        "the image call neither claims nor settles the search probe"
    );
    assert_eq!(
        cache
            .feedback
            .active_cooldown(scope, std::time::Instant::now())
            .map(|(cause, _)| cause),
        Some(crate::worker_admin::quota_feedback::QuotaFeedbackCause::RateLimited),
        "the search feedback cause is unchanged"
    );

    drop(guard);
    assert!(
        matches!(
            cache.feedback.claim_probe(scope, std::time::Instant::now()),
            crate::worker_admin::quota_feedback::QuotaProbeClaim::Reserved(_)
        ),
        "dropping the search guard releases exactly its own claim"
    );
}

#[tokio::test]
async fn an_invalid_business_status_is_not_a_success_for_the_image_tool() {
    // The image tool uses the same strict status branch: an invalid present
    // envelope fails the call even with a valid content shape, is never
    // replayed, and never touches the shared search feedback.
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    let now = std::time::Instant::now();
    cache.feedback.record_failure(
        scope,
        crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted,
        Some(Duration::ZERO),
        now,
        now,
    );

    for payload in [
        json!({ "base_resp": null, "content": "an answer" }),
        json!({ "base_resp": { "status_code": "invalid" }, "content": "an answer" }),
        json!({ "base_resp": { "status_code": 2067.5 }, "content": "an answer" }),
        json!({ "base_resp": { "status_code": [] }, "content": "an answer" }),
    ] {
        let mut plan = plan(
            vec![candidate(key_id, "image", "sk-image", 0)],
            true,
            Tool::UnderstandImage,
        );
        plan.endpoint_id = endpoint_id;
        let echoed = payload.clone();
        let transport = StubTransport::new(move |_| Ok(response(200, echoed.clone())));

        let error = execute_call(&plan, &transport, Some(&cache))
            .await
            .expect_err("an invalid business status fails the image call");

        assert!(
            error
                .to_string()
                .contains("not the documented understand_image payload"),
            "{payload} -> {error}"
        );
        assert_eq!(transport.attempts().len(), 1, "{payload} is never replayed");
        assert_eq!(
            shared_cause(&cache, scope),
            Some(crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted),
            "{payload} never clears the search feedback"
        );
    }
}

#[tokio::test]
async fn a_credential_echoed_by_the_provider_never_reaches_the_client() {
    let candidates = vec![candidate(Uuid::new_v4(), "a", "sk-secret", 0)];
    let plan = plan(candidates, true, Tool::WebSearch);
    let transport = StubTransport::new(|_| {
        Ok(transport::MiniMaxResponse {
            status: StatusCode::OK,
            payload: json!({
                "base_resp": { "status_code": 2056, "status_msg": "key sk-secret is spent" }
            }),
            retry_after: None,
        })
    });

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("the quota rejection is returned");

    // The provider status text is dropped entirely rather than scrubbed: the
    // fixed diagnostic keeps only the known code.
    assert_eq!(
        error.to_string(),
        "MiniMax API reported an exhausted plan quota (2056)"
    );
    assert!(!format!("{error:?}").contains("sk-secret"), "{error:?}");
    assert!(!error.to_string().contains("sk-secret"), "{error}");
}

#[tokio::test]
async fn the_second_quota_code_switches_to_the_next_key() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let transport = StubTransport::new(move |_| {
        let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if call == 0 {
            transport::MiniMaxResponse {
                status: StatusCode::OK,
                payload: json!({
                    "base_resp": { "status_code": 2067, "status_msg": "plan quota spent" }
                }),
                retry_after: None,
            }
        } else {
            ok_response()
        })
    });

    let result = execute_call(&plan, &transport, None).await;

    assert!(result.is_ok(), "the second key must serve the call");
    let attempts = transport.attempts();
    assert_eq!(attempts.len(), 2, "the call switches to another key");
    assert_ne!(attempts[0], attempts[1], "a key is never tried twice");
}

#[tokio::test]
async fn a_forbidden_response_is_not_replayed() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let transport = StubTransport::new(|_| Ok(status_response(403)));

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("a forbidden response is returned");

    assert!(error.to_string().contains("403"), "{error}");
    assert_eq!(
        transport.attempts().len(),
        1,
        "a forbidden response is never replayed"
    );
}

#[tokio::test]
async fn the_image_tool_spreads_over_the_enabled_keys() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
        candidate(Uuid::new_v4(), "c", "sk-c", 2),
    ];
    let mut selected = std::collections::BTreeSet::new();
    for _ in 0..64 {
        let plan = plan(candidates.clone(), true, Tool::UnderstandImage);
        let transport = StubTransport::new(|_| Ok(ok_image_response()));
        execute_call(&plan, &transport, None)
            .await
            .expect("a call succeeds");
        selected.extend(transport.attempts());
    }
    assert!(
        selected.len() > 1,
        "the image tool must spread over the enabled keys: {selected:?}"
    );
}

#[tokio::test]
async fn the_key_balancing_flag_keeps_the_pinned_key() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let plan = plan(candidates, false, Tool::WebSearch);
    let transport = StubTransport::new(|_| Ok(exhausted_response()));

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("the pinned key fails");

    assert_eq!(
        transport.attempts(),
        vec!["sk-a".to_string()],
        "the pinned key must not be substituted"
    );
    assert!(error.to_string().contains("2056"), "{error}");
}

#[tokio::test]
async fn the_call_shares_one_deadline() {
    let candidates = vec![
        candidate(Uuid::new_v4(), "a", "sk-a", 0),
        candidate(Uuid::new_v4(), "b", "sk-b", 1),
    ];
    let mut plan = plan(candidates, true, Tool::WebSearch);
    plan.deadline = tokio::time::Instant::now() + Duration::from_millis(60);
    let transport =
        StubTransport::new(|_| Ok(ok_response())).with_delay(Duration::from_millis(500));

    let error = execute_call(&plan, &transport, None)
        .await
        .expect_err("the deadline ends the call");

    assert!(error.to_string().contains("timed out"), "{error}");
    assert_eq!(
        transport.attempts().len(),
        1,
        "the deadline is not restarted per key"
    );
}

#[tokio::test]
async fn disabled_and_blank_keys_are_excluded() {
    let endpoint_id = Uuid::new_v4();
    let enabled = Uuid::new_v4();
    let mut endpoint = db::ProviderEndpoint {
        endpoint_id,
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "minimax".to_string(),
        provider: db::EndpointProvider::Minimax,
        provider_region: Some(db::EndpointRegion::Global),
        plan: db::EndpointPlan::PlatformApiKey,
        service_tier: None,
        base_url: "https://api.minimax.io".to_string(),
        native_api: "responses".to_string(),
        native_api_source: "test".to_string(),
        api_key: String::new(),
        proxy_url: None,
        has_oauth_token: false,
        has_proxy_url: false,
        has_admin_api_key: false,
        active_windows: Vec::new(),
        key_lb_enabled: true,
        enabled: true,
        mcp_enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        api_keys: vec![
            db::EndpointApiKey {
                key_id: enabled,
                endpoint_id,
                key_label: "enabled".to_string(),
                api_key: "sk-enabled".to_string(),
                position: 0,
                enabled: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            db::EndpointApiKey {
                key_id: Uuid::new_v4(),
                endpoint_id,
                key_label: "disabled".to_string(),
                api_key: "sk-disabled".to_string(),
                position: 1,
                enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            db::EndpointApiKey {
                key_id: Uuid::new_v4(),
                endpoint_id,
                key_label: "blank".to_string(),
                api_key: "   ".to_string(),
                position: 2,
                enabled: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
        ],
    };
    endpoint.api_key = "sk-inline".to_string();

    let candidates = selection::candidates(&endpoint);
    let secrets = candidates
        .iter()
        .map(|candidate| candidate.secret.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        secrets,
        vec!["sk-enabled"],
        "a disabled or blank key row is never a candidate"
    );

    // Issue #1179 P2 repair: rows exist but none qualifies, so the legacy
    // inline column — which the ordinary endpoint save derives from the first
    // row, even a disabled one — must not be sent.
    endpoint.api_keys.retain(|key| !key.enabled);
    assert!(
        selection::candidates(&endpoint).is_empty(),
        "a disabled key row never falls back to the inline secret"
    );
    endpoint.api_key = "   ".to_string();
    assert!(selection::candidates(&endpoint).is_empty());
}

/// An endpoint with the given key rows and legacy inline column.
fn endpoint_with(
    keys: Vec<(String, bool)>,
    inline: &str,
    lb_enabled: bool,
) -> db::ProviderEndpoint {
    let endpoint_id = Uuid::new_v4();
    db::ProviderEndpoint {
        endpoint_id,
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "minimax".to_string(),
        provider: db::EndpointProvider::Minimax,
        provider_region: Some(db::EndpointRegion::Global),
        plan: db::EndpointPlan::PlatformApiKey,
        service_tier: None,
        base_url: "https://api.minimax.io".to_string(),
        native_api: "responses".to_string(),
        native_api_source: "test".to_string(),
        api_key: inline.to_string(),
        proxy_url: None,
        has_oauth_token: false,
        has_proxy_url: false,
        has_admin_api_key: false,
        active_windows: Vec::new(),
        key_lb_enabled: lb_enabled,
        enabled: true,
        mcp_enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        api_keys: keys
            .into_iter()
            .enumerate()
            .map(|(position, (secret, enabled))| db::EndpointApiKey {
                key_id: Uuid::new_v4(),
                endpoint_id,
                key_label: format!("key-{position}"),
                api_key: secret,
                position: position as i32,
                enabled,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn disabled_rows_never_fall_back_to_the_matching_inline_secret() {
    // The ordinary endpoint save copies the first key row into the legacy
    // inline column, so a disabled row and the inline secret can be identical.
    // No request may be sent for either key-balancing setting.
    for lb_enabled in [true, false] {
        let endpoint = endpoint_with(
            vec![("sk-disabled".to_string(), false)],
            "sk-disabled",
            lb_enabled,
        );
        assert!(
            selection::candidates(&endpoint).is_empty(),
            "lb={lb_enabled}: a disabled row is never replaced by its inline secret"
        );

        let endpoint = endpoint_with(
            vec![("   ".to_string(), true), (String::new(), true)],
            "sk-blank-inline",
            lb_enabled,
        );
        assert!(
            selection::candidates(&endpoint).is_empty(),
            "lb={lb_enabled}: blank rows never fall back to the inline secret"
        );
    }

    // The inline secret is still the fallback of an endpoint with no key row.
    let endpoint = endpoint_with(Vec::new(), "sk-inline", true);
    let secrets = selection::candidates(&endpoint)
        .into_iter()
        .map(|candidate| candidate.secret)
        .collect::<Vec<_>>();
    assert_eq!(secrets, vec!["sk-inline".to_string()]);
}

#[tokio::test]
async fn the_tracker_records_the_selected_key_and_the_outcome() {
    let key_id = Uuid::new_v4();
    let candidates = vec![candidate(key_id, "primary", "sk-a", 0)];
    let plan = plan(candidates, true, Tool::WebSearch);
    let tracker = McpCallTracker::new();
    let transport = StubTransport::new(|_| Ok(ok_response()));

    execute(&plan, &transport, None, None, &tracker)
        .await
        .expect("a call succeeds");

    let record = tracker.snapshot();
    assert_eq!(record.endpoint_key_id, Some(key_id));
    assert_eq!(record.endpoint_key_label.as_deref(), Some("primary"));
    assert_eq!(record.mcp_bearer_token_slot, Some(1));
    assert_eq!(record.outcome, McpCallOutcome::Completed);
}

#[tokio::test]
async fn the_tracker_records_the_final_key_of_a_failover() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let candidates = vec![
        candidate(first, "first", "sk-a", 0),
        candidate(second, "second", "sk-b", 1),
    ];
    let plan = plan(candidates, true, Tool::WebSearch);
    let tracker = McpCallTracker::new();
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let transport = StubTransport::new(move |_| {
        let call = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(if call == 0 {
            exhausted_response()
        } else {
            ok_response()
        })
    });

    execute(&plan, &transport, None, None, &tracker)
        .await
        .expect("the second key serves the call");

    let record = tracker.snapshot();
    assert_eq!(record.attempt_failures, 1, "the first attempt failed");
    assert_eq!(record.outcome, McpCallOutcome::Completed);
    let attempts = transport.attempts();
    assert_eq!(attempts.len(), 2, "the call switched to another key");
    let failed_key = if attempts[0] == "sk-a" { first } else { second };
    let final_key = record.endpoint_key_id.expect("the final key is recorded");
    assert_ne!(final_key, failed_key, "the failed key is not the final one");
    assert_eq!(
        record.endpoint_key_label.as_deref(),
        if final_key == first {
            Some("first")
        } else {
            Some("second")
        }
    );
}

#[tokio::test]
async fn the_tracker_records_a_failed_call() {
    let key_id = Uuid::new_v4();
    let candidates = vec![candidate(key_id, "primary", "sk-a", 0)];
    let plan = plan(candidates, true, Tool::WebSearch);
    let tracker = McpCallTracker::new();
    let transport = StubTransport::new(|_| Ok(exhausted_response()));

    execute(&plan, &transport, None, None, &tracker)
        .await
        .expect_err("the quota rejection is returned");

    let record = tracker.snapshot();
    assert_eq!(record.endpoint_key_id, Some(key_id));
    assert!(
        matches!(record.outcome, McpCallOutcome::Failed { .. }),
        "a failed call is recorded: {:?}",
        record.outcome
    );
}

// ---------------------------------------------------------------------------
// Issue #1179 P2 repair: malformed bodies, safe record text, probe lifetime
// and the image/feedback boundary.
// ---------------------------------------------------------------------------

/// A response with a chosen status and body.
fn response(status: u16, payload: serde_json::Value) -> transport::MiniMaxResponse {
    transport::MiniMaxResponse {
        status: StatusCode::from_u16(status).expect("a valid status code"),
        payload,
        retry_after: None,
    }
}

/// The shared feedback cause of a scope, read through the public store API.
///
/// A probe reservation exposes the recorded cause with `active_cooldown`;
/// a cleared entry exposes nothing.
fn shared_cause(
    cache: &TokenPlanQuotaCache,
    scope: QuotaFeedbackScope,
) -> Option<crate::worker_admin::quota_feedback::QuotaFeedbackCause> {
    let now = std::time::Instant::now();
    let _ = cache.feedback.reserve_probe(scope, now);
    let cause = cache
        .feedback
        .active_cooldown(scope, now)
        .map(|(cause, _)| cause);
    cache.feedback.release_probe(scope, now, now);
    cause
}

#[tokio::test]
async fn an_empty_body_is_a_failed_call_not_a_success() {
    let key_id = Uuid::new_v4();
    let endpoint_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    let mut plan = plan(
        vec![candidate(key_id, "only", "sk-only", 0)],
        true,
        Tool::WebSearch,
    );
    plan.endpoint_id = endpoint_id;

    let transport = StubTransport::new(|_| Ok(response(200, json!({}))));
    let tracker = McpCallTracker::new();
    execute(&plan, &transport, Some(&cache), None, &tracker)
        .await
        .expect_err("an empty body is not a successful search");

    assert!(matches!(
        tracker.snapshot().outcome,
        McpCallOutcome::Failed { .. }
    ));
    let _ = scope;
    assert!(
        transport.attempts().len() == 1,
        "a malformed body is never replayed"
    );
}

#[tokio::test]
async fn a_malformed_body_is_never_replayed_and_never_clears_feedback() {
    let key_id = Uuid::new_v4();
    let endpoint_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    cache.feedback.record_failure(
        scope,
        crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted,
        Some(Duration::ZERO),
        std::time::Instant::now(),
        std::time::Instant::now(),
    );
    let mut plan = plan(
        vec![candidate(key_id, "only", "sk-only", 0)],
        true,
        Tool::WebSearch,
    );
    plan.endpoint_id = endpoint_id;

    // A success status whose body is not the documented search shape.
    let transport = StubTransport::new(|_| {
        Ok(response(
            200,
            json!({ "base_resp": { "status_code": 0 }, "content": "hello" }),
        ))
    });
    let tracker = McpCallTracker::new();
    execute(&plan, &transport, Some(&cache), None, &tracker)
        .await
        .expect_err("the body is not the documented search payload");

    assert_eq!(transport.attempts().len(), 1, "no replay");
    assert_eq!(
        shared_cause(&cache, scope),
        Some(crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted),
        "a malformed body never counts as a success observation"
    );
    assert!(matches!(
        tracker.snapshot().outcome,
        McpCallOutcome::Failed { .. }
    ));
}

#[tokio::test]
async fn a_missing_image_content_is_not_a_success() {
    let key_id = Uuid::new_v4();
    let mut plan = plan(
        vec![candidate(key_id, "only", "sk-only", 0)],
        true,
        Tool::UnderstandImage,
    );
    plan.endpoint_id = Uuid::new_v4();

    let transport = StubTransport::new(|_| {
        Ok(response(
            200,
            json!({ "base_resp": { "status_code": 0 }, "organic": [] }),
        ))
    });
    let tracker = McpCallTracker::new();
    execute(&plan, &transport, None, None, &tracker)
        .await
        .expect_err("the VLM response has no content");

    assert!(matches!(
        tracker.snapshot().outcome,
        McpCallOutcome::Failed { .. }
    ));
    assert_eq!(transport.attempts().len(), 1, "no replay");
}

#[tokio::test]
async fn a_present_but_invalid_business_status_is_never_a_success() {
    // The envelope is present, so it must be valid: a null, a non-object, a
    // missing field, a non-integral number, a list or a non-numeric string is
    // a malformed response and is never read as an absent envelope — not even
    // when the tool's own success shape is intact.
    for payload in [
        json!({ "base_resp": null, "organic": [] }),
        json!({ "base_resp": "ok", "organic": [] }),
        json!({ "base_resp": [], "organic": [] }),
        json!({ "base_resp": {}, "organic": [] }),
        json!({ "base_resp": { "status_code": null }, "organic": [] }),
        json!({ "base_resp": { "status_code": "invalid" }, "organic": [] }),
        json!({ "base_resp": { "status_code": 2067.5 }, "organic": [] }),
        json!({ "base_resp": { "status_code": ["0"] }, "organic": [] }),
    ] {
        let call_plan = plan(
            vec![candidate(Uuid::new_v4(), "only", "sk-only", 0)],
            true,
            Tool::WebSearch,
        );
        let echoed = payload.clone();
        let transport = StubTransport::new(move |_| Ok(response(200, echoed.clone())));
        let tracker = McpCallTracker::new();

        let error = execute(&call_plan, &transport, None, None, &tracker)
            .await
            .expect_err("an invalid business status is a failure");

        assert!(
            error
                .to_string()
                .contains("not the documented web_search payload"),
            "{payload} -> {error}"
        );
        assert_eq!(transport.attempts().len(), 1, "{payload} is never replayed");
        assert!(matches!(
            tracker.snapshot().outcome,
            McpCallOutcome::Failed { .. }
        ));
    }
}

#[tokio::test]
async fn an_exact_integer_string_status_keeps_its_meaning() {
    // The provider sometimes writes the code as an exact digit string: zero is
    // a success, a non-zero code is a provider failure with its numeric code,
    // and the client never sees the provider text.
    let zero_plan = plan(
        vec![candidate(Uuid::new_v4(), "only", "sk-only", 0)],
        true,
        Tool::WebSearch,
    );
    let transport = StubTransport::new(|_| {
        Ok(response(
            200,
            json!({ "base_resp": { "status_code": "0" }, "organic": [] }),
        ))
    });
    execute_call(&zero_plan, &transport, None)
        .await
        .expect("an exact zero status string is a success");

    let failed_plan = plan(
        vec![candidate(Uuid::new_v4(), "only", "sk-only", 0)],
        true,
        Tool::WebSearch,
    );
    let transport = StubTransport::new(|_| {
        Ok(response(
            200,
            json!({
                "base_resp": { "status_code": "1000", "status_msg": "key sk-secret for query rust" }
            }),
        ))
    });
    let error = execute_call(&failed_plan, &transport, None)
        .await
        .expect_err("a non-zero status string is a provider failure");
    assert_eq!(error.to_string(), "MiniMax API returned error 1000");
    assert!(!error.to_string().contains("sk-secret"), "{error}");
}

#[tokio::test]
async fn the_documented_success_shapes_are_accepted() {
    for (tool, payload) in [
        (
            Tool::WebSearch,
            json!({ "base_resp": { "status_code": 0 }, "organic": [{ "title": "t" }] }),
        ),
        // Missing base_resp alone stays compatible when the documented shape
        // itself validates.
        (Tool::WebSearch, json!({ "organic": [] })),
        (
            Tool::UnderstandImage,
            json!({ "base_resp": { "status_code": 0 }, "content": "an answer" }),
        ),
        (Tool::UnderstandImage, json!({ "content": "an answer" })),
    ] {
        let plan = plan(
            vec![candidate(Uuid::new_v4(), "only", "sk-only", 0)],
            true,
            tool,
        );
        let transport = StubTransport::new(move |_| Ok(response(200, payload.clone())));
        execute_call(&plan, &transport, None)
            .await
            .expect("the documented success shape is accepted");
    }
}

#[tokio::test]
async fn a_live_guarded_probe_stays_busy_until_the_attempt_is_cancelled() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));
    let now = std::time::Instant::now();
    cache
        .feedback
        .record_429(scope, Some(Duration::ZERO), now, now);
    let mut plan = plan(
        vec![candidate(key_id, "only", "sk-only", 0)],
        true,
        Tool::WebSearch,
    );
    plan.endpoint_id = endpoint_id;
    let probe_scope = QuotaScope::new(endpoint_id, Some(key_id), Some("general"), true);

    let transport = SlowTransport::default();
    let tracker = McpCallTracker::new();
    let mut call = Box::pin(execute(&plan, &transport, Some(&cache), None, &tracker));
    tokio::select! {
        _ = transport.started.notified() => {}
        result = &mut call => panic!("the attempt must still be in flight: {result:?}"),
    }

    assert!(
        matches!(
            cache.claim_scope_probe(probe_scope),
            crate::worker_admin::quota_feedback::QuotaProbeClaim::Busy
        ),
        "the live guarded attempt owns the probe"
    );
    // The attempt outlives the legacy probe TTL: an eligibility read and the
    // unguarded legacy reservation both stay busy far beyond 30 seconds while
    // the guarded attempt is genuinely in flight.
    let beyond_ttl = now + Duration::from_secs(31);
    assert_eq!(
        cache.feedback.eligibility(scope, beyond_ttl),
        QuotaFeedbackEligibility::ProbeBusy,
        "a live attempt is never expired by the legacy TTL"
    );
    assert!(
        matches!(
            cache.feedback.reserve_probe_if_required(scope, beyond_ttl),
            crate::worker_admin::quota_feedback::QuotaProbeReservation::Busy
        ),
        "the legacy reservation is refused while the guarded attempt is live"
    );
    assert!(
        matches!(
            cache.feedback.claim_probe(scope, beyond_ttl),
            crate::worker_admin::quota_feedback::QuotaProbeClaim::Busy
        ),
        "a second guarded claimant cannot steal the live probe after the TTL"
    );

    // A newer explicit failure supersedes the cause while the attempt runs.
    cache.feedback.record_failure(
        scope,
        crate::worker_admin::quota_feedback::QuotaFeedbackCause::RateLimited,
        Some(Duration::from_secs(90)),
        beyond_ttl,
        beyond_ttl,
    );

    // Cancellation drops the attempt and releases only its own token: the
    // newer cause and its deadline survive, and the released guard stops
    // holding the scope once that cooldown ends.
    drop(call);
    assert_eq!(
        cache.feedback.active_cooldown(scope, beyond_ttl),
        Some((
            crate::worker_admin::quota_feedback::QuotaFeedbackCause::RateLimited,
            Duration::from_secs(90)
        )),
        "the newer feedback cause and deadline survive the cancellation"
    );
    let after_cooldown = beyond_ttl + Duration::from_secs(91);
    assert_eq!(
        cache.feedback.eligibility(scope, after_cooldown),
        QuotaFeedbackEligibility::ProbeRequired,
        "the cancelled attempt released its live probe; only the newer feedback remains"
    );
    assert!(
        matches!(
            cache.feedback.claim_probe(scope, after_cooldown),
            crate::worker_admin::quota_feedback::QuotaProbeClaim::Reserved(_)
        ),
        "the newer feedback still requires its own probe"
    );
}

/// A transport whose attempt never returns on its own.
#[derive(Default)]
struct SlowTransport {
    started: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl transport::MiniMaxTransport for SlowTransport {
    async fn send(
        &self,
        _request: transport::MiniMaxRequest<'_>,
    ) -> anyhow::Result<transport::MiniMaxResponse> {
        self.started.notify_one();
        std::future::pending::<()>().await;
        unreachable!("the pending attempt is only cancelled")
    }
}

#[tokio::test]
async fn an_image_outcome_never_changes_the_shared_search_feedback() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let cache = quota_cache(endpoint_id, &[(key_id, 100.0, 100.0)]).await;
    let scope = QuotaFeedbackScope::new(endpoint_id, Some(key_id));

    // The search path recorded an exhausted quota; its cooldown is over but
    // the entry (with its cause) remains.
    let now = std::time::Instant::now();
    cache.feedback.record_failure(
        scope,
        crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted,
        Some(Duration::ZERO),
        now,
        now,
    );
    let mut image_plan = plan(
        vec![candidate(key_id, "image", "sk-image", 0)],
        true,
        Tool::UnderstandImage,
    );
    image_plan.endpoint_id = endpoint_id;

    let success = StubTransport::new(|_| Ok(ok_image_response()));
    execute_call(&image_plan, &success, Some(&cache))
        .await
        .expect("the image call runs on its unknown billing group");
    assert_eq!(
        shared_cause(&cache, scope),
        Some(crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted),
        "an image success must not clear the search exhaustion"
    );

    let rate_limited = StubTransport::new(|_| Ok(status_response(429)));
    execute_call(&image_plan, &rate_limited, Some(&cache))
        .await
        .expect_err("the image 429 is returned");
    assert_eq!(
        shared_cause(&cache, scope),
        Some(crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted),
        "an image 429 must not overwrite the search exhaustion cause"
    );

    // The search path still assesses the key with its own state and records
    // its own outcome.
    let mut search_plan = plan(
        vec![candidate(key_id, "search", "sk-search", 0)],
        true,
        Tool::WebSearch,
    );
    search_plan.endpoint_id = endpoint_id;
    let search = StubTransport::new(|_| Ok(exhausted_response()));
    execute_call(&search_plan, &search, Some(&cache))
        .await
        .expect_err("the search quota rejection is returned");
    assert_eq!(
        shared_cause(&cache, scope),
        Some(crate::worker_admin::quota_feedback::QuotaFeedbackCause::Exhausted),
        "the search path still records its own exhaustion"
    );
}

#[tokio::test]
async fn the_builtin_echo_never_reaches_the_tracker() {
    // The provider repeats the credential and the query in its message.
    let candidates = vec![candidate(Uuid::new_v4(), "a", "sk-secret", 0)];
    let plan = plan(candidates, true, Tool::WebSearch);
    let transport = StubTransport::new(|_| {
        Ok(response(
            200,
            json!({
                "base_resp": {
                    "status_code": 2056,
                    "status_msg": "key sk-secret for query rust is spent"
                }
            }),
        ))
    });

    let tracker = McpCallTracker::new();
    execute(&plan, &transport, None, None, &tracker)
        .await
        .expect_err("the quota rejection is returned");

    let record = tracker.snapshot();
    let (code, message) = record.failure().expect("a failure");
    assert_eq!(code, "mcp_quota_exhausted");
    assert_eq!(
        message,
        "the provider reported an exhausted plan quota (2056)"
    );
    assert!(!message.contains("sk-secret"), "{message}");
    assert!(!message.contains("rust"), "{message}");
}

#[tokio::test]
async fn an_endpoint_without_candidates_sends_no_request() {
    for lb_enabled in [true, false] {
        let mut plan = plan(Vec::new(), lb_enabled, Tool::WebSearch);
        plan.endpoint_id = Uuid::new_v4();
        let transport = StubTransport::new(|_| Ok(ok_response()));
        let tracker = McpCallTracker::new();

        let error = execute(&plan, &transport, None, None, &tracker)
            .await
            .expect_err("no credential is available");

        assert!(error.to_string().contains("no enabled API key"), "{error}");
        assert!(
            transport.attempts().is_empty(),
            "no upstream request is sent without a credential"
        );
        assert!(matches!(
            tracker.snapshot().outcome,
            McpCallOutcome::Failed { .. }
        ));
    }
}
