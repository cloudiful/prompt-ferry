//! OpenAI Platform organization usage fetcher (issue #589 P2b).
//!
//! Reads the official Admin API endpoints `GET /v1/organization/usage/completions`
//! and `GET /v1/organization/costs` with the endpoint's dedicated OpenAI Admin
//! API key, and aggregates UTC month-to-date input/output tokens and USD cost.
//! Both reads are pinned to the official `https://api.openai.com` origin: the
//! endpoint's stored inference base is never used, so the privileged Admin key
//! cannot follow a custom or OpenAI-compatible host.
//!
//! The result is display-only. It is never read by routing weights, token-plan
//! quota accounting, or remaining-credit calculations; `costs` is spend, not a
//! usable balance. ChatGPT subscription quota stays separate (issue #599).
//!
//! [`aggregate`] holds the pure response parsing/summing, [`fetch`] the HTTP
//! client, cursor pagination, and URL construction.

mod aggregate;
mod fetch;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

use crate::db::EndpointProvider;
use crate::upstream_presets::OPENAI_BASE_URL;

const ORGANIZATION_USAGE_PATH: &str = "/v1/organization/usage/completions";
const ORGANIZATION_COSTS_PATH: &str = "/v1/organization/costs";
const DEFAULT_CURRENCY: &str = "usd";
/// One bucket per UTC day; a month-to-date window spans at most 31 buckets.
const BUCKET_WIDTH: &str = "1d";
/// Per-page bucket cap, sized so a full month usually fits in one request.
const PAGE_LIMIT: i64 = 31;
/// Hard cap on `next_page` follow-ups so a buggy or hostile cursor can never
/// loop forever; exceeding it marks the result truncated.
const MAX_PAGES: usize = 4;

/// Issue #589 P2b: OpenAI Platform organization usage for the endpoint's Admin
/// API key. UTC month-to-date input/output tokens and USD cost from the
/// official `usage/completions` and `costs` endpoints. Display-only: it never
/// feeds routing weights, token-plan quota, or remaining-credit calculations.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OpenAiOrganizationUsageResponse {
    pub provider: EndpointProvider,
    /// UTC first instant of the current month.
    pub period_start: DateTime<Utc>,
    /// UTC instant the window was read (`now`).
    pub period_end: DateTime<Utc>,
    pub currency: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
    /// The upstream reported more pages than the fetch cap read; totals are a
    /// lower bound.
    pub truncated: bool,
    /// Served from the 60s display cache instead of a live upstream read.
    pub cached: bool,
    pub fetched_at: DateTime<Utc>,
}

/// Official OpenAI Platform origin for the two organization Admin API reads.
///
/// The endpoint's stored inference `base_url` is deliberately ignored: the
/// `usage/completions` and `costs` paths exist only on the official Platform
/// origin, and the privileged Admin API key must never follow a custom or
/// OpenAI-compatible inference host. Only proxy selection stays
/// endpoint-derived.
pub(crate) fn organization_base_url(_stored_base_url: &str) -> &'static str {
    OPENAI_BASE_URL
}

/// Read both official organization endpoints concurrently (at most two
/// in-flight requests) and flatten them into one month-to-date snapshot.
///
/// `stored_base_url` is accepted so the caller never has to decide the host:
/// it is pinned to the official origin by [`organization_base_url`].
pub(crate) async fn fetch_organization_usage(
    stored_base_url: &str,
    admin_api_key: &str,
    proxy_url: Option<&str>,
    now: DateTime<Utc>,
) -> Result<OpenAiOrganizationUsageResponse> {
    fetch::fetch_organization_usage_at(
        organization_base_url(stored_base_url),
        admin_api_key,
        proxy_url,
        now,
    )
    .await
}
