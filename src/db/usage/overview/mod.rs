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
