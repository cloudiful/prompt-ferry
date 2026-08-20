use chrono::{DateTime, Utc};
use serde::Serialize;
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

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewBreakdownRow {
    pub label: String,
    pub request_count: i64,
    pub request_share: f64,
    pub success_count: i64,
    pub success_rate: f64,
    pub token_share: Option<f64>,
    pub tokens: RequestRecordOverviewTokenUsage,
    pub model: Option<String>,
    pub mcp_server_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewErrorRow {
    pub key: String,
    pub label: String,
    pub count: i64,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewResponse {
    pub summary: RequestRecordOverviewSummary,
    pub trend: Vec<RequestRecordOverviewTrendBucket>,
    pub breakdown: Vec<RequestRecordOverviewBreakdownRow>,
    pub error_breakdown: Vec<RequestRecordOverviewErrorRow>,
}
