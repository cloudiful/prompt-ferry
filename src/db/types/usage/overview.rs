use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewTokenUsage {
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cache_rate: Option<f64>,
    pub cache_hit_rate: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewSummary {
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub method_count: i64,
    pub success_rate: f64,
    pub p95_total_ms: Option<f64>,
    pub p95_first_token_ms: Option<f64>,
    /// Average output tokens per second for completed AI requests with
    /// positive output tokens and positive duration. `None` when no such
    /// rows match (for example, when the overview filters to MCP requests,
    /// or when there are no completed AI requests in the window).
    pub avg_output_tokens_per_second: Option<f64>,
    pub tokens: RequestRecordOverviewTokenUsage,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewTrendBucket {
    pub bucket_at: DateTime<Utc>,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: f64,
    pub error_rate: f64,
    pub p95_total_ms: Option<f64>,
    pub p95_first_token_ms: Option<f64>,
    pub tokens: RequestRecordOverviewTokenUsage,
}

/// Top-level grouping for the AI distribution table. `Model` keeps the
/// historical model-first rows with an upstream hover; `Upstream` groups the
/// full filtered window by endpoint identity and shows a per-model hover.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestRecordOverviewPerspective {
    #[default]
    Model,
    Upstream,
}

impl RequestRecordOverviewPerspective {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Upstream => "upstream",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RequestRecordOverviewUpstreamBreakdown {
    pub endpoint_id: Option<Uuid>,
    pub endpoint_name: Option<String>,
    /// Model actually sent upstream when a route target override applies.
    /// `None` when the request used the requested model unchanged.
    pub upstream_model: Option<String>,
    pub request_count: i64,
    /// Share of the window's requests, matching the main model table.
    pub request_share: f64,
    pub error_count: i64,
    pub error_rate: f64,
    pub total_tokens: i64,
    /// Share of the window's total tokens, matching the main model table.
    /// `None` when the window has zero total tokens.
    pub token_share: Option<f64>,
    pub cache_rate: Option<f64>,
    pub avg_output_tokens_per_second: Option<f64>,
}

/// Per-effective-model metrics inside an upstream row. The effective model is
/// the route-target `upstream_model` override when present, otherwise the
/// originally requested model.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RequestRecordOverviewModelBreakdown {
    pub model: String,
    /// Share of the window's requests, matching the main model table.
    pub request_share: f64,
    pub request_count: i64,
    pub error_count: i64,
    pub error_rate: f64,
    pub total_tokens: i64,
    /// Share of the window's total tokens, matching the main model table.
    /// `None` when the window has zero total tokens.
    pub token_share: Option<f64>,
    pub cache_rate: Option<f64>,
    pub avg_output_tokens_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewBreakdownRow {
    pub label: String,
    pub request_count: i64,
    pub request_share: f64,
    pub success_count: i64,
    pub success_rate: f64,
    /// Number of failed requests (`ok IS FALSE` or terminal failure state).
    /// `None` for rows that do not report errors (e.g. MCP breakdown).
    pub error_count: Option<i64>,
    /// `error_count / request_count`, `None` when the row does not report errors.
    pub error_rate: Option<f64>,
    /// Number of distinct upstreams (`endpoint_id`) observed for this row.
    /// `None` for rows without upstream breakdown (e.g. MCP breakdown).
    pub upstream_count: Option<i64>,
    /// Per-upstream metrics for AI model rows, ordered by `total_tokens` desc.
    /// `None` or empty when the row has a single upstream.
    pub upstream_breakdown: Option<Vec<RequestRecordOverviewUpstreamBreakdown>>,
    /// Per-effective-model metrics for upstream rows in the upstream
    /// perspective, ordered by `total_tokens` desc. `None` for model rows.
    pub model_breakdown: Option<Vec<RequestRecordOverviewModelBreakdown>>,
    pub token_share: Option<f64>,
    pub tokens: RequestRecordOverviewTokenUsage,
    pub model: Option<String>,
    /// Endpoint identity for upstream rows so the client can filter records by
    /// the clicked upstream. `None` for model rows, MCP rows, and `(direct)`.
    pub endpoint_id: Option<Uuid>,
    pub mcp_server_id: Option<Uuid>,
    /// Canonical MCP provider preset id (`context7`/`firecrawl`/`minimax`)
    /// resolved from `mcp_servers.provider_kind`. `None` for AI rows and for
    /// generic/legacy MCP servers.
    pub server_provider_kind: Option<String>,
    /// Usage unit for the provider (`requests`/`credits`), derived from the
    /// server-side registry so the client never re-derives it. `None` when the
    /// provider is unknown or the row is not an MCP server row.
    pub usage_unit: Option<String>,
    /// Average output tokens per second for completed AI requests with
    /// positive output tokens and positive duration, averaged per request
    /// within the breakdown row. `None` when no valid samples exist or
    /// when the row is not an AI model row.
    pub avg_output_tokens_per_second: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewResponse {
    pub summary: RequestRecordOverviewSummary,
    pub trend: Vec<RequestRecordOverviewTrendBucket>,
    pub breakdown: Vec<RequestRecordOverviewBreakdownRow>,
}
