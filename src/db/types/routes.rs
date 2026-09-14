use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

use crate::config::NativeApi;
use crate::db::RouteSelectionReason;
use crate::db::types::EndpointApiKey;
use crate::db::types::endpoints::{EndpointProvider, MinimaxServiceTier};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelRouteRoutingStrategy {
    #[default]
    ClientKeyRendezvous,
    ResponsesSessionAffinity,
}

impl ModelRouteRoutingStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientKeyRendezvous => "client_key_rendezvous",
            Self::ResponsesSessionAffinity => "responses_session_affinity",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct StreamDeltaBatchingSettings {
    pub enabled: bool,
    pub flush_window_ms: u64,
    pub max_buffer_chars: usize,
    pub max_buffer_bytes: usize,
    pub flush_on_line_break: bool,
    pub flush_on_sentence_end: bool,
}

impl Default for StreamDeltaBatchingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            flush_window_ms: 50,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct RouteConfig {
    pub route_id: uuid::Uuid,
    pub user_id: i64,
    pub model_route_rule_id: Option<uuid::Uuid>,
    pub base_url: String,
    pub api_key: String,
    pub endpoint_key_id: Option<uuid::Uuid>,
    pub endpoint_key_label: Option<String>,
    pub api_keys: Vec<EndpointApiKey>,
    pub key_lb_enabled: bool,
    pub native_api: NativeApi,
    pub upstream_model: Option<String>,
    pub route_selection_reason: RouteSelectionReason,
    pub provider: EndpointProvider,
    pub service_tier: MinimaxServiceTier,
    // Issue #368 Phase A+D: resolved outbound proxy for this route
    // (`proxy_url_override` ?? endpoint `proxy_url`). `None` means direct.
    // Pooled client selection lives in `worker::runtime::ai::proxy`.
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct ModelRouteTarget {
    pub target_id: uuid::Uuid,
    pub rule_id: uuid::Uuid,
    pub endpoint_id: uuid::Uuid,
    pub endpoint_name: Option<String>,
    pub endpoint_enabled: bool,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    // Issue #368 Phase A: per-target proxy override (PG plaintext).
    // `None` falls back to the endpoint default; never echoed without
    // the Phase B `has_*` contract, so skip serializing for now.
    #[serde(skip_serializing)]
    pub proxy_url_override: Option<String>,
    /// Issue #368 Phase C (P2): response-side saved-override indicator.
    /// `true` when an override is stored; the secret itself is never echoed.
    #[serde(default)]
    #[sqlx(default)]
    pub has_proxy_url_override: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelEndpointRule {
    pub rule_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub targets: Vec<ModelRouteTarget>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ModelEndpointRuleRow {
    pub rule_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: String,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ModelRouteTargetCreate {
    pub endpoint_id: uuid::Uuid,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    // Issue #368 Phase A: plaintext override for the PG write path
    // (SQLite encrypts via the 0018 envelope). `None`/empty means inherit.
    #[serde(default)]
    pub proxy_url_override: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ModelEndpointRuleCreate {
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub targets: Vec<ModelRouteTargetCreate>,
}

#[derive(Debug, Clone)]
pub struct ModelRouteCandidate {
    pub rule_id: uuid::Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: ModelRouteRoutingStrategy,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub updated_at: DateTime<Utc>,
    pub targets: Vec<ModelRouteCandidateTarget>,
}

#[derive(Debug, Clone)]
pub struct ModelRouteCandidateTarget {
    pub target_id: uuid::Uuid,
    pub endpoint_id: uuid::Uuid,
    pub endpoint_name: String,
    pub base_url: String,
    pub api_key: String,
    pub api_keys: Vec<EndpointApiKey>,
    pub key_lb_enabled: bool,
    pub native_api: NativeApi,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub provider: EndpointProvider,
    pub service_tier: MinimaxServiceTier,
    // Issue #368 Phase A+D: endpoint default plus per-target override.
    // Resolution (`override ?? endpoint`) via `resolve_proxy_url`; both are
    // carried here so the selector can pick without extra lookups.
    pub proxy_url: Option<String>,
    pub proxy_url_override: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RouteTestEndpoint {
    pub endpoint_id: uuid::Uuid,
    pub name: String,
}

// Issue #368 Phase D: resolved outbound proxy for LLM upstream.
// `proxy_url_override` (per-target) wins over the endpoint default;
// empty/whitespace means direct. Both inputs are trimmed.
pub fn resolve_proxy_url(
    endpoint_proxy: Option<&str>,
    proxy_override: Option<&str>,
) -> Option<String> {
    proxy_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            endpoint_proxy
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

/// Issue #368 Phase D: validate a non-empty outbound proxy URL.
/// Accepts only `http/https/socks5/socks5h` with a non-empty host.
/// Errors never echo userinfo.
pub fn validate_outbound_proxy_url(trimmed: &str) -> Result<String, &'static str> {
    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|_| "proxy_url scheme must be one of http, https, socks5, socks5h")?;
    match parsed.scheme().to_ascii_lowercase().as_str() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => return Err("proxy_url scheme must be one of http, https, socks5, socks5h"),
    }
    if parsed.host_str().is_none_or(|host| host.trim().is_empty()) {
        return Err("proxy_url must include a host");
    }
    Ok(trimmed.to_string())
}

/// Issue #368 Phase D: scrub userinfo before logging.
/// URLs without credentials are returned unchanged so clean URLs are
/// never mangled; unparseable input maps to a static placeholder.
pub fn redact_proxy_url_for_log(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return "[invalid-proxy-url]".to_string();
    };
    if parsed.username().is_empty() && parsed.password().is_none() {
        return url.to_string();
    }
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.to_string()
}

/// Issue #368 Phase D: lowercased host for the `(proxy, host)` pool key.
pub fn proxy_base_host(base_url: &str) -> String {
    if let Ok(parsed) = reqwest::Url::parse(base_url)
        && let Some(host) = parsed.host_str()
    {
        return host.to_ascii_lowercase();
    }
    base_url.trim().to_ascii_lowercase()
}

/// Issue #368 Phase D: pool key for `(proxy_url, base_host)`.
/// Empty/whitespace proxy means direct (`None`).
pub fn proxy_pool_key(proxy_url: &str, base_url: &str) -> Option<(String, String)> {
    let trimmed = proxy_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some((trimmed.to_string(), proxy_base_host(base_url)))
}

#[derive(Debug, Clone, FromRow)]
pub struct SnapshotKey {
    pub key_hash: String,
    pub key_prefix: String,
    pub user_id: i64,
    pub route_id: uuid::Uuid,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ModelRoutePage {
    pub total: i64,
    pub routes: Vec<ModelEndpointRule>,
    pub first: i64,
    pub rows: i64,
}
