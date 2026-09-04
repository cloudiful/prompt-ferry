use crate::{db, upstream_adapter::PreparedRequestBody};
use bytes::Bytes;
use futures::StreamExt;
use http::header;
use reqwest::{Client, Method};

#[cfg(test)]
pub(super) async fn send_upstream_request(
    client: &Client,
    method: &Method,
    url: &str,
    route: &db::RouteConfig,
    body: &PreparedRequestBody,
) -> Result<reqwest::Response, reqwest::Error> {
    build_upstream_request(client, method, url, route, body, &[], None)
        .send()
        .await
}

pub(super) fn build_upstream_request(
    client: &Client,
    method: &Method,
    url: &str,
    route: &db::RouteConfig,
    body: &PreparedRequestBody,
    request_headers: &[(String, String)],
    conversation_id: Option<uuid::Uuid>,
) -> reqwest::RequestBuilder {
    let request_builder = client
        .request(method.clone(), url)
        .header(header::CONTENT_TYPE, "application/json");
    let request_builder = match route.native_api {
        crate::config::NativeApi::AnthropicMessages => with_anthropic_headers(
            request_builder.header("x-api-key", &route.api_key),
            request_headers,
        ),
        _ => request_builder.bearer_auth(&route.api_key),
    };
    let request_builder = if is_opencode_host(&route.base_url) || is_opencode_host(url) {
        with_opencode_headers(request_builder, request_headers, conversation_id)
    } else {
        request_builder
    };
    match body {
        PreparedRequestBody::PassthroughStream(bytes) => {
            request_builder.body(apply_minimax_service_tier(route, bytes))
        }
        PreparedRequestBody::BufferedBytes(bytes) => {
            request_builder.body(apply_minimax_service_tier(route, bytes))
        }
    }
}

/// Inject the endpoint-configured MiniMax `service_tier` into an upstream
/// JSON request body. Only MiniMax endpoints are modified; generic
/// endpoints return the body unchanged so client-supplied values are
/// never forwarded or overridden. The configured value overwrites any
/// existing `service_tier` field. Non-JSON or non-object bodies are
/// returned unchanged.
pub(super) fn apply_minimax_service_tier(route: &db::RouteConfig, body: &[u8]) -> Vec<u8> {
    if route.provider != crate::db::EndpointProvider::Minimax {
        return body.to_vec();
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };
    let Some(object) = value.as_object_mut() else {
        return body.to_vec();
    };
    object.insert(
        "service_tier".to_string(),
        serde_json::Value::String(route.service_tier.as_str().to_string()),
    );
    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

pub(super) fn is_opencode_host(url: &str) -> bool {
    let host = if let Ok(parsed) = reqwest::Url::parse(url) {
        parsed.host_str().map(|h| h.to_ascii_lowercase())
    } else {
        // Fallback manual parsing for robustness in tests with non-standard urls.
        let trimmed = url.trim();
        let without_scheme = if let Some(idx) = trimmed.find("://") {
            &trimmed[idx + 3..]
        } else {
            trimmed
        };
        let host_part = without_scheme
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("");
        if host_part.is_empty() {
            None
        } else {
            Some(host_part.to_ascii_lowercase())
        }
    };
    match host {
        Some(host) => host == "opencode.ai" || host.ends_with(".opencode.ai"),
        None => false,
    }
}

/// Build the upstream URL for a resolved route.
///
/// Only MiniMax routes with the fixed Anthropic path `/v1/messages` are
/// remapped to `/anthropic/v1/messages`. All other providers and paths use
/// the plain `base + path` join so Generic and MiniMax Chat/Responses
/// behavior is unchanged. Caller paths stay fixed and allowlisted; this
/// helper never accepts arbitrary suffixes.
///
/// A MiniMax base already carrying an `/anthropic` prefix is joined without
/// duplication. Only known official MiniMax roots gain the prefix; custom
/// MiniMax bases without that prefix are left unchanged instead of being
/// blindly rewritten. No credentials are logged here.
pub(in crate::worker::runtime) fn upstream_url_for_route(
    route: &db::RouteConfig,
    path: &str,
) -> String {
    if route.provider != crate::db::EndpointProvider::Minimax {
        return join_base_path(&route.base_url, path);
    }
    if path != "/v1/messages" {
        return join_base_path(&route.base_url, path);
    }
    if base_has_anthropic_prefix(&route.base_url) {
        return join_base_path(&route.base_url, path);
    }
    if is_minimax_official_root(&route.base_url) {
        return format!("{}/anthropic{}", route.base_url.trim_end_matches('/'), path);
    }
    join_base_path(&route.base_url, path)
}

fn join_base_path(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn is_minimax_official_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "api.minimaxi.com" | "api.minimax.io"
    )
}

/// Return true when the configured base URL path already starts with the
/// fixed `/anthropic` segment. Uses URL parsing so only a real path segment
/// matches; a host suffix such as `api.minimaxi.com.evil.com` never matches.
fn base_has_anthropic_prefix(base_url: &str) -> bool {
    if let Ok(url) = reqwest::Url::parse(base_url.trim()) {
        if let Some(mut segments) = url.path_segments() {
            if let Some(first) = segments.next() {
                return first == "anthropic";
            }
        }
        return false;
    }
    let without_query = base_url
        .split(['?', '#'])
        .next()
        .unwrap_or(base_url)
        .trim()
        .trim_end_matches('/');
    without_query.ends_with("/anthropic")
}

/// Return true for known official MiniMax roots with no extra path. Custom
/// MiniMax bases return false so they are never blindly rewritten.
fn is_minimax_official_root(base_url: &str) -> bool {
    let trimmed = base_url.trim();
    if let Ok(url) = reqwest::Url::parse(trimmed) {
        let Some(host) = url.host_str() else {
            return false;
        };
        if !is_minimax_official_host(host) {
            return false;
        }
        return url.path() == "/" || url.path().is_empty();
    }
    let without_scheme = if let Some(idx) = trimmed.find("://") {
        &trimmed[idx + 3..]
    } else {
        trimmed
    };
    let host_part = without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    if !is_minimax_official_host(host_part) {
        return false;
    }
    let path_part = without_scheme
        .find('/')
        .map(|idx| &without_scheme[idx..])
        .unwrap_or("/");
    let path_no_query = path_part
        .split(['?', '#'])
        .next()
        .unwrap_or(path_part)
        .trim_end_matches('/');
    path_no_query.is_empty()
}

fn with_opencode_headers(
    builder: reqwest::RequestBuilder,
    request_headers: &[(String, String)],
    conversation_id: Option<uuid::Uuid>,
) -> reqwest::RequestBuilder {
    // Preserve caller-supplied non-empty x-opencode-session, else synthesize deterministically.
    let session_value = request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-opencode-session"))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let builder = if let Some(value) = session_value {
        builder.header("x-opencode-session", value)
    } else if let Some(id) = conversation_id.filter(|id| !id.is_nil()) {
        builder.header("x-opencode-session", format!("ses_{id}"))
    } else {
        builder
    };

    // Preserve caller User-Agent, else synthesize prompt-ferry/<version>.
    let user_agent_value = request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());

    let builder = if let Some(value) = user_agent_value {
        builder.header(header::USER_AGENT, value)
    } else {
        builder.header(
            header::USER_AGENT,
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION")),
        )
    };

    // Forward other safe OpenCode identity headers (x-opencode-*) that are non-empty.
    // This keeps the relay's safe header policy for OpenCode without leaking auth/hop-by-hop.
    let mut builder = builder;
    for (name, value) in request_headers {
        if name.eq_ignore_ascii_case("x-opencode-session")
            || name.eq_ignore_ascii_case("user-agent")
        {
            continue;
        }
        if name.to_ascii_lowercase().starts_with("x-opencode-") {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Avoid forwarding auth/hop-by-hop even if mis-prefixed; x-opencode-* is safe, but keep guard.
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization" | "cookie" | "x-api-key" | "host" | "connection"
            ) {
                continue;
            }
            builder = builder.header(name.as_str(), trimmed);
        }
    }
    builder
}

pub(super) fn with_anthropic_headers(
    builder: reqwest::RequestBuilder,
    request_headers: &[(String, String)],
) -> reqwest::RequestBuilder {
    let version = request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-version"))
        .map(|(_, value)| value.as_str());
    let builder = if let Some(version) = version {
        builder.header("anthropic-version", version)
    } else {
        builder.header("anthropic-version", "2023-06-01")
    };
    let beta = request_headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(",");
    if beta.is_empty() {
        builder
    } else {
        builder.header("anthropic-beta", beta)
    }
}

pub(super) async fn read_response_sample(response: reqwest::Response, max_bytes: usize) -> Vec<u8> {
    let mut collected = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            break;
        };
        append_sample(&mut collected, &chunk, max_bytes);
        if collected.len() >= max_bytes {
            break;
        }
    }
    collected
}

fn append_sample(collected: &mut Vec<u8>, chunk: &Bytes, max_bytes: usize) {
    if collected.len() >= max_bytes {
        return;
    }
    let remaining = max_bytes - collected.len();
    collected.extend(chunk.iter().copied().take(remaining));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::NativeApi,
        db::{ResponsesContinuationPolicy, RouteConfig, RouteSelectionReason},
    };
    use axum::{Router, http::StatusCode, routing::post};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn bad_gateway_is_returned_without_retry() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let counter = request_count.clone();
        let app = Router::new().route(
            "/v1/responses",
            post(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    StatusCode::BAD_GATEWAY
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let address = listener.local_addr().expect("mock upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve mock upstream");
        });

        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: format!("http://{address}"),
            api_key: "test-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Responses,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let response = send_upstream_request(
            &Client::new(),
            &Method::POST,
            &format!("http://{address}/v1/responses"),
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
        )
        .await
        .expect("upstream response");

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(request_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn forwards_anthropic_version_and_beta_without_forwarding_client_key() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://example.test".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::AnthropicMessages,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://example.test/v1/messages",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[
                ("anthropic-version".to_string(), "2024-10-22".to_string()),
                ("anthropic-beta".to_string(), "tools-2024-04-04".to_string()),
                ("x-api-key".to_string(), "client-key".to_string()),
            ],
            None,
        )
        .build()
        .unwrap();

        assert_eq!(request.headers().get("x-api-key").unwrap(), "upstream-key");
        assert_eq!(
            request.headers().get("anthropic-version").unwrap(),
            "2024-10-22"
        );
        assert_eq!(
            request.headers().get("anthropic-beta").unwrap(),
            "tools-2024-04-04"
        );
        // Non-OpenCode hosts must not synthesize opencode headers.
        assert!(request.headers().get("x-opencode-session").is_none());
        assert!(request.headers().get(header::USER_AGENT).is_none());
    }

    #[test]
    fn is_opencode_host_detects_root_and_subdomains() {
        assert!(is_opencode_host("https://opencode.ai"));
        assert!(is_opencode_host("https://opencode.ai/zen/go/v1"));
        assert!(is_opencode_host("https://api.opencode.ai"));
        assert!(is_opencode_host("https://foo.bar.opencode.ai"));
        assert!(is_opencode_host(
            "https://opencode.ai:8443/v1/chat/completions"
        ));
        assert!(is_opencode_host("https://OPENC0DE.AI") == false);
        assert!(is_opencode_host("https://OPencode.AI"));
        assert!(is_opencode_host("https://opencode.AI/"));
        assert!(!is_opencode_host("https://notopencode.ai"));
        assert!(!is_opencode_host("https://opencode.ai.evil.com"));
        assert!(!is_opencode_host("https://evilopencode.ai"));
        assert!(!is_opencode_host("https://example.com"));
        assert!(!is_opencode_host("https://opencode.ai.evil.com/v1"));
        assert!(!is_opencode_host("not-a-url"));
        assert!(!is_opencode_host(""));
        // boundary: host with prefix attacker
        assert!(!is_opencode_host("https://attacker-opencode.ai"));
        // exact subdomain boundary requires dot
        assert!(is_opencode_host(
            "https://deep.nested.sub.opencode.ai/path?query=1"
        ));
    }

    #[test]
    fn opencode_passthrough_preserves_caller_session_and_user_agent() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://api.opencode.ai".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let conversation_id =
            uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://api.opencode.ai/v1/responses",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[
                (
                    "x-opencode-session".to_string(),
                    "ses_caller123".to_string(),
                ),
                ("User-Agent".to_string(), "opencode/1.0 test".to_string()),
                ("x-opencode-project".to_string(), "proj_123".to_string()),
            ],
            Some(conversation_id),
        )
        .build()
        .unwrap();

        assert_eq!(
            request.headers().get("x-opencode-session").unwrap(),
            "ses_caller123"
        );
        assert_eq!(
            request.headers().get(header::USER_AGENT).unwrap(),
            "opencode/1.0 test"
        );
        // safe additional opencode header forwarded
        assert_eq!(
            request.headers().get("x-opencode-project").unwrap(),
            "proj_123"
        );
    }

    #[test]
    fn opencode_synthesizes_session_and_user_agent_when_missing() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://opencode.ai".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let conversation_id =
            uuid::Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://opencode.ai/zen/go/v1/responses",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[],
            Some(conversation_id),
        )
        .build()
        .unwrap();

        assert_eq!(
            request
                .headers()
                .get("x-opencode-session")
                .unwrap()
                .to_str()
                .unwrap(),
            format!("ses_{conversation_id}")
        );
        assert_eq!(
            request
                .headers()
                .get(header::USER_AGENT)
                .unwrap()
                .to_str()
                .unwrap(),
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn opencode_empty_header_treated_as_missing_and_synthesized() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://opencode.ai".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let conversation_id =
            uuid::Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://opencode.ai/v1/responses",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[
                ("x-opencode-session".to_string(), "   ".to_string()),
                ("user-agent".to_string(), "".to_string()),
            ],
            Some(conversation_id),
        )
        .build()
        .unwrap();

        assert_eq!(
            request
                .headers()
                .get("x-opencode-session")
                .unwrap()
                .to_str()
                .unwrap(),
            format!("ses_{conversation_id}")
        );
        assert_eq!(
            request
                .headers()
                .get(header::USER_AGENT)
                .unwrap()
                .to_str()
                .unwrap(),
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn opencode_no_identity_omits_session_but_sets_user_agent() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://sub.opencode.ai".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://sub.opencode.ai/v1/chat/completions",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[],
            None,
        )
        .build()
        .unwrap();

        assert!(request.headers().get("x-opencode-session").is_none());
        assert_eq!(
            request
                .headers()
                .get(header::USER_AGENT)
                .unwrap()
                .to_str()
                .unwrap(),
            format!("prompt-ferry/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn non_opencode_does_not_forward_or_synthesize() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://api.openai.com".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let conversation_id =
            uuid::Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://api.openai.com/v1/chat/completions",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[
                (
                    "x-opencode-session".to_string(),
                    "ses_should_not_forward".to_string(),
                ),
                ("User-Agent".to_string(), "custom-agent/1.0".to_string()),
                (
                    "x-opencode-project".to_string(),
                    "proj_should_not_forward".to_string(),
                ),
            ],
            Some(conversation_id),
        )
        .build()
        .unwrap();

        // No opencode headers for non-opencode host, and User-Agent not forwarded/synthesized.
        assert!(request.headers().get("x-opencode-session").is_none());
        assert!(request.headers().get("x-opencode-project").is_none());
        assert!(request.headers().get(header::USER_AGENT).is_none());
        // Bearer auth still set
        assert!(request.headers().get(header::AUTHORIZATION).is_some());
    }

    #[test]
    fn opencode_detection_does_not_use_body_model_metadata() {
        // Ensure that even if caller body would contain model "opencode" string,
        // detection is host-only. Here we use non-opencode host with model hint in headers
        // but ensure no synthesis.
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://api.example.com".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: Some("opencode-model".to_string()),
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let conversation_id =
            uuid::Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap();
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://api.example.com/v1/chat/completions",
            &route,
            &PreparedRequestBody::BufferedBytes(br#"{"model":"opencode"}"#.to_vec()),
            &[],
            Some(conversation_id),
        )
        .build()
        .unwrap();
        assert!(request.headers().get("x-opencode-session").is_none());
        assert!(request.headers().get(header::USER_AGENT).is_none());
    }

    #[test]
    fn opencode_host_case_insensitive_and_trims_values() {
        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://API.OpEnCoDe.AI".to_string(),
            api_key: "upstream-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
        };
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://API.OpEnCoDe.AI/v1/responses",
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
            &[
                (
                    "X-OPENCODE-SESSION".to_string(),
                    "  ses_case_insensitive  ".to_string(),
                ),
                ("UsEr-AgEnT".to_string(), "  custom-ua/2.0  ".to_string()),
            ],
            None,
        )
        .build()
        .unwrap();
        assert_eq!(
            request.headers().get("x-opencode-session").unwrap(),
            "ses_case_insensitive"
        );
        assert_eq!(
            request.headers().get(header::USER_AGENT).unwrap(),
            "custom-ua/2.0"
        );
    }

    fn minimax_route(tier: crate::db::MinimaxServiceTier) -> RouteConfig {
        RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "https://api.minimaxi.com".to_string(),
            api_key: "minimax-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: RouteSelectionReason::Default,
            provider: crate::db::EndpointProvider::Minimax,
            service_tier: tier,
        }
    }

    fn generic_route() -> RouteConfig {
        RouteConfig {
            provider: crate::db::EndpointProvider::Generic,
            service_tier: crate::db::MinimaxServiceTier::Standard,
            ..minimax_route(crate::db::MinimaxServiceTier::Standard)
        }
    }

    #[test]
    fn minimax_injects_configured_service_tier_over_body_value() {
        let route = minimax_route(crate::db::MinimaxServiceTier::Priority);
        let body = br#"{"model":"MiniMax-M2","service_tier":"standard"}"#;
        let injected = apply_minimax_service_tier(&route, body);
        let value: serde_json::Value = serde_json::from_slice(&injected).unwrap();
        assert_eq!(value["service_tier"], "priority");
        assert_eq!(value["model"], "MiniMax-M2");
    }

    #[test]
    fn minimax_defaults_to_standard_when_body_omits_tier() {
        let route = minimax_route(crate::db::MinimaxServiceTier::Standard);
        let injected = apply_minimax_service_tier(&route, br#"{"model":"MiniMax-M2"}"#);
        let value: serde_json::Value = serde_json::from_slice(&injected).unwrap();
        assert_eq!(value["service_tier"], "standard");
    }

    #[test]
    fn generic_endpoints_leave_body_unchanged() {
        let route = generic_route();
        let body = br#"{"model":"gpt-5","service_tier":"priority"}"#;
        assert_eq!(apply_minimax_service_tier(&route, body), body);
    }

    #[test]
    fn non_json_bodies_pass_through_unchanged() {
        let route = minimax_route(crate::db::MinimaxServiceTier::Priority);
        let body = b"not-json";
        assert_eq!(apply_minimax_service_tier(&route, body), body);
        let array_body = b"[1,2,3]";
        assert_eq!(apply_minimax_service_tier(&route, array_body), array_body);
    }

    #[test]
    fn build_upstream_request_injects_tier_for_buffered_and_passthrough() {
        let route = minimax_route(crate::db::MinimaxServiceTier::Priority);
        for body in [
            PreparedRequestBody::BufferedBytes(br#"{"model":"m"}"#.to_vec()),
            PreparedRequestBody::PassthroughStream(br#"{"model":"m"}"#.to_vec()),
        ] {
            let request = build_upstream_request(
                &Client::new(),
                &Method::POST,
                "https://api.minimaxi.com/v1/chat/completions",
                &route,
                &body,
                &[],
                None,
            )
            .build()
            .unwrap();
            let bytes = request
                .body()
                .and_then(|body| body.as_bytes())
                .expect("upstream body bytes");
            let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(value["service_tier"], "priority");
        }
        let generic = generic_route();
        let request = build_upstream_request(
            &Client::new(),
            &Method::POST,
            "https://api.minimaxi.com/v1/chat/completions",
            &generic,
            &PreparedRequestBody::BufferedBytes(
                br#"{"model":"m","service_tier":"priority"}"#.to_vec(),
            ),
            &[],
            None,
        )
        .build()
        .unwrap();
        let bytes = request
            .body()
            .and_then(|body| body.as_bytes())
            .expect("generic body bytes");
        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(value["service_tier"], "priority");
    }

    fn minimax_anthropic_route(base_url: &str) -> RouteConfig {
        RouteConfig {
            base_url: base_url.to_string(),
            native_api: NativeApi::AnthropicMessages,
            provider: crate::db::EndpointProvider::Minimax,
            ..minimax_route(crate::db::MinimaxServiceTier::Standard)
        }
    }

    fn route_with_base(route: &RouteConfig, base_url: &str) -> RouteConfig {
        RouteConfig {
            base_url: base_url.to_string(),
            ..route.clone()
        }
    }

    #[test]
    fn minimax_official_roots_gain_anthropic_prefix() {
        for base in ["https://api.minimaxi.com", "https://api.minimaxi.com/"] {
            let route = minimax_anthropic_route(base);
            assert_eq!(
                upstream_url_for_route(&route, "/v1/messages"),
                "https://api.minimaxi.com/anthropic/v1/messages",
                "base {base} should map to the MiniMax Anthropic path"
            );
        }
        let global = minimax_anthropic_route("https://api.minimax.io");
        assert_eq!(
            upstream_url_for_route(&global, "/v1/messages"),
            "https://api.minimax.io/anthropic/v1/messages"
        );
    }

    #[test]
    fn minimax_existing_anthropic_prefix_is_not_duplicated() {
        for base in [
            "https://api.minimaxi.com/anthropic",
            "https://api.minimaxi.com/anthropic/",
            "https://api.minimax.io/anthropic",
        ] {
            let route = minimax_anthropic_route(base);
            let url = upstream_url_for_route(&route, "/v1/messages");
            assert_eq!(
                url.matches("/anthropic").count(),
                1,
                "base {base} mapped to {url} without duplicating the prefix"
            );
            assert!(url.ends_with("/anthropic/v1/messages"));
        }
        // Custom MiniMax base with an explicit prefix is compatible.
        let custom = minimax_anthropic_route("https://proxy.example.test/anthropic");
        assert_eq!(
            upstream_url_for_route(&custom, "/v1/messages"),
            "https://proxy.example.test/anthropic/v1/messages"
        );
    }

    #[test]
    fn minimax_chat_and_responses_keep_plain_v1_paths() {
        let chat = RouteConfig {
            native_api: NativeApi::Chat,
            ..minimax_route(crate::db::MinimaxServiceTier::Standard)
        };
        assert_eq!(
            upstream_url_for_route(&chat, "/v1/chat/completions"),
            "https://api.minimaxi.com/v1/chat/completions"
        );
        let responses = RouteConfig {
            native_api: NativeApi::Responses,
            ..minimax_route(crate::db::MinimaxServiceTier::Standard)
        };
        assert_eq!(
            upstream_url_for_route(&responses, "/v1/responses"),
            "https://api.minimaxi.com/v1/responses"
        );
        // Even an Anthropic-native MiniMax route keeps non-messages paths plain.
        let anthropic = minimax_anthropic_route("https://api.minimaxi.com");
        assert_eq!(
            upstream_url_for_route(&anthropic, "/v1/responses"),
            "https://api.minimaxi.com/v1/responses"
        );
    }

    #[test]
    fn generic_provider_never_gains_anthropic_prefix() {
        let generic = generic_route();
        assert_eq!(
            upstream_url_for_route(&generic, "/v1/messages"),
            "https://api.minimaxi.com/v1/messages"
        );
        let generic_prefixed = route_with_base(&generic, "https://api.minimaxi.com/anthropic");
        assert_eq!(
            upstream_url_for_route(&generic_prefixed, "/v1/messages"),
            "https://api.minimaxi.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn custom_minimax_root_without_prefix_is_not_rewritten() {
        let custom = minimax_anthropic_route("https://proxy.example.test");
        assert_eq!(
            upstream_url_for_route(&custom, "/v1/messages"),
            "https://proxy.example.test/v1/messages",
            "custom MiniMax bases must not be blindly rewritten"
        );
        let evil = minimax_anthropic_route("https://api.minimaxi.com.evil.test");
        assert_eq!(
            upstream_url_for_route(&evil, "/v1/messages"),
            "https://api.minimaxi.com.evil.test/v1/messages"
        );
    }

    #[test]
    fn anthropic_url_mapping_rejects_non_allowlisted_paths() {
        let route = minimax_anthropic_route("https://api.minimaxi.com");
        for path in [
            "/v1/messages/extra",
            "/v1/messages/",
            "/V1/MESSAGES",
            "/anthropic/v1/messages",
            "/v1/chat/completions",
            "/v1/responses",
            "/v1/models",
        ] {
            let url = upstream_url_for_route(&route, path);
            assert_eq!(
                url,
                format!("https://api.minimaxi.com{path}"),
                "path {path} must not gain an Anthropic prefix"
            );
            assert_eq!(
                url.matches("/anthropic").count(),
                u32::from(path.contains("/anthropic")) as usize
            );
        }
    }
}
