use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

use crate::db::{
    OverviewErrorBreakdownRow, OverviewHeatmap, OverviewHeatmapCell, OverviewMetricCard,
    OverviewRankingGroup, OverviewRankingRow, OverviewTrendBucket, RequestRecordCategory,
    RequestRecordOverviewResponse,
};

use super::quality::{AggregateMetrics, quality_formula, quality_score, ratio};

mod presentation;
mod queries;

use self::presentation::{build_heatmap, summary_cards};
use self::queries::{query_error_breakdown, query_metrics, query_rankings, query_trend};
pub(super) const UNKNOWN_LABEL: &str = "(unknown)";

#[derive(Debug, Clone, Copy)]
pub struct OverviewWindow {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub bucket: OverviewBucket,
}

#[derive(Debug, Clone, Copy)]
pub enum OverviewBucket {
    Hour,
    Day,
}

impl OverviewBucket {
    pub(super) fn sql_expr(self) -> &'static str {
        match self {
            Self::Hour => "date_trunc('hour', rr.created_at)",
            Self::Day => "date_trunc('day', rr.created_at)",
        }
    }
}

pub async fn request_records_overview(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<RequestRecordOverviewResponse> {
    let summary = query_metrics(pool, visible_user_id, request_category, window, user).await?;
    let trend = query_trend(pool, visible_user_id, request_category, window, user).await?;
    let rankings = query_rankings(pool, visible_user_id, request_category, window, user).await?;
    let heatmap = build_heatmap(request_category, &rankings);
    let errors =
        query_error_breakdown(pool, visible_user_id, request_category, window, user).await?;

    Ok(RequestRecordOverviewResponse {
        summary_cards: summary_cards(request_category, summary),
        trend,
        quality_formula: quality_formula(request_category),
        top_rankings: rankings,
        heatmap,
        error_breakdown: errors,
    })
}

pub(super) fn base_where() -> &'static str {
    " FROM request_records rr \
      LEFT JOIN users u ON u.user_id = rr.user_id \
      LEFT JOIN provider_endpoints pe ON pe.endpoint_id = rr.endpoint_id \
      LEFT JOIN mcp_servers ms ON ms.server_id = rr.mcp_server_id \
      WHERE rr.event_kind = 'request' \
      AND rr.request_category = $2 \
      AND ($1::BIGINT IS NULL OR rr.user_id = $1) \
      AND ($3::TIMESTAMPTZ IS NULL OR rr.created_at >= $3) \
      AND ($4::TIMESTAMPTZ IS NULL OR rr.created_at < $4) \
      AND ($5::TEXT IS NULL OR COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') = $5) "
}
