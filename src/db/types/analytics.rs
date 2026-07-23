use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Type;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RequestFailureFamily {
    Auth,
    RateLimit,
    Quota,
    Timeout,
    #[serde(rename = "upstream_4xx")]
    #[sqlx(rename = "upstream_4xx")]
    Upstream4xx,
    #[serde(rename = "upstream_5xx")]
    #[sqlx(rename = "upstream_5xx")]
    Upstream5xx,
    Network,
    EmptySuccess,
    Policy,
    Unknown,
}

impl RequestFailureFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::RateLimit => "rate_limit",
            Self::Quota => "quota",
            Self::Timeout => "timeout",
            Self::Upstream4xx => "upstream_4xx",
            Self::Upstream5xx => "upstream_5xx",
            Self::Network => "network",
            Self::EmptySuccess => "empty_success",
            Self::Policy => "policy",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RequestFailureFamily;

    #[test]
    fn request_failure_family_uses_legacy_db_wire_names() {
        let four_xx: RequestFailureFamily = serde_json::from_str("\"upstream_4xx\"").unwrap();
        let five_xx: RequestFailureFamily = serde_json::from_str("\"upstream_5xx\"").unwrap();

        assert_eq!(four_xx, RequestFailureFamily::Upstream4xx);
        assert_eq!(five_xx, RequestFailureFamily::Upstream5xx);
        assert_eq!(
            serde_json::to_string(&RequestFailureFamily::Upstream4xx).unwrap(),
            "\"upstream_4xx\""
        );
        assert_eq!(
            serde_json::to_string(&RequestFailureFamily::Upstream5xx).unwrap(),
            "\"upstream_5xx\""
        );
        assert_eq!(RequestFailureFamily::Upstream4xx.as_str(), "upstream_4xx");
        assert_eq!(RequestFailureFamily::Upstream5xx.as_str(), "upstream_5xx");
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RouteSelectionReason {
    #[default]
    Default,
    SessionAffinity,
    SessionLoadBalance,
    ConversationOverride,
}

impl RouteSelectionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::SessionAffinity => "session_affinity",
            Self::SessionLoadBalance => "session_load_balance",
            Self::ConversationOverride => "conversation_override",
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewMetricCard {
    pub key: String,
    pub label: String,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewTrendBucket {
    pub bucket_at: DateTime<Utc>,
    pub request_count: i64,
    pub success_rate: f64,
    pub quality_score: f64,
    pub p95_total_ms: Option<f64>,
    pub p95_first_token_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewQualityComponent {
    pub key: String,
    pub label: String,
    pub weight: f64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewQualityFormula {
    pub score_kind: String,
    pub components: Vec<OverviewQualityComponent>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewRankingRow {
    pub label: String,
    pub secondary_label: Option<String>,
    pub request_count: i64,
    pub success_rate: f64,
    pub quality_score: f64,
    pub p95_total_ms: Option<f64>,
    pub p95_first_token_ms: Option<f64>,
    pub rate_limit_rate: Option<f64>,
    pub auth_error_rate: Option<f64>,
    pub upstream_5xx_rate: Option<f64>,
    pub empty_success_rate: Option<f64>,
    pub cache_hit_rate: Option<f64>,
    pub endpoint_id: Option<Uuid>,
    pub model: Option<String>,
    pub mcp_server_id: Option<Uuid>,
    pub mcp_bearer_token_slot: Option<i16>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewRankingGroup {
    pub key: String,
    pub title: String,
    pub rows: Vec<OverviewRankingRow>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewHeatmapCell {
    pub x_index: i32,
    pub y_index: i32,
    pub request_count: i64,
    pub success_rate: f64,
    pub quality_score: f64,
    pub p95_total_ms: Option<f64>,
    pub p95_first_token_ms: Option<f64>,
    pub error_rate: f64,
    pub endpoint_id: Option<Uuid>,
    pub model: Option<String>,
    pub mcp_server_id: Option<Uuid>,
    pub mcp_bearer_token_slot: Option<i16>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewHeatmap {
    pub key: String,
    pub x_labels: Vec<String>,
    pub y_labels: Vec<String>,
    pub cells: Vec<OverviewHeatmapCell>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct OverviewErrorBreakdownRow {
    pub key: String,
    pub label: String,
    pub count: i64,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RequestRecordOverviewResponse {
    pub summary_cards: Vec<OverviewMetricCard>,
    pub trend: Vec<OverviewTrendBucket>,
    pub quality_formula: OverviewQualityFormula,
    pub top_rankings: Vec<OverviewRankingGroup>,
    pub heatmap: OverviewHeatmap,
    pub error_breakdown: Vec<OverviewErrorBreakdownRow>,
}
