use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::db::{
    RequestRecordCategory, RequestRecordOverviewBreakdownRow, RequestRecordOverviewErrorRow,
    RequestRecordOverviewSummary, RequestRecordOverviewTrendBucket,
};

use super::OverviewWindow;
use super::presentation::{failure_family_label, ratio, summary_from_metrics, token_usage};

#[derive(Debug, Clone, Copy)]
pub enum OverviewBucket {
    Hour,
    Day,
}

#[derive(Debug, FromRow, Clone, Copy)]
pub(super) struct MetricsRow {
    pub(super) request_count: i64,
    pub(super) success_count: i64,
    pub(super) error_count: i64,
    pub(super) cache_hit_count: i64,
    pub(super) method_count: i64,
    pub(super) input_tokens: i64,
    pub(super) cache_read_tokens: i64,
    pub(super) cache_write_tokens: i64,
    pub(super) output_tokens: i64,
    pub(super) total_tokens: i64,
    pub(super) p95_total_ms: Option<f64>,
    pub(super) p95_first_token_ms: Option<f64>,
}

#[derive(Debug, FromRow, Clone, Copy)]
struct TrendRow {
    bucket_at: DateTime<Utc>,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    cache_hit_count: i64,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    p95_total_ms: Option<f64>,
    p95_first_token_ms: Option<f64>,
}

#[derive(Debug, FromRow)]
struct BreakdownRow {
    label: String,
    model: Option<String>,
    mcp_server_id: Option<uuid::Uuid>,
    request_count: i64,
    request_share: f64,
    success_count: i64,
    token_share: Option<f64>,
    cache_hit_count: i64,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

#[derive(Debug, FromRow)]
struct ErrorRow {
    key: String,
    count: i64,
}

pub async fn query_summary(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<RequestRecordOverviewSummary> {
    let row = sqlx::query_file_as!(
        MetricsRow,
        "src/sql/usage/overview/metrics.sql",
        visible_user_id,
        request_category.as_str(),
        window.start,
        window.end,
        user,
    )
    .fetch_one(pool)
    .await?;

    Ok(summary_from_metrics(row))
}

pub async fn query_trend(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<Vec<RequestRecordOverviewTrendBucket>> {
    let rows = match window.bucket {
        OverviewBucket::Hour => {
            sqlx::query_file_as!(
                TrendRow,
                "src/sql/usage/overview/trend_hour.sql",
                visible_user_id,
                request_category.as_str(),
                window.start,
                window.end,
                user,
            )
            .fetch_all(pool)
            .await?
        }
        OverviewBucket::Day => {
            sqlx::query_file_as!(
                TrendRow,
                "src/sql/usage/overview/trend_day.sql",
                visible_user_id,
                request_category.as_str(),
                window.start,
                window.end,
                user,
            )
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(|row| RequestRecordOverviewTrendBucket {
            bucket_at: row.bucket_at,
            request_count: row.request_count,
            success_count: row.success_count,
            error_count: row.error_count,
            success_rate: ratio(row.success_count, row.request_count),
            error_rate: ratio(row.error_count, row.request_count),
            p95_total_ms: row.p95_total_ms,
            p95_first_token_ms: row.p95_first_token_ms,
            tokens: token_usage(
                row.input_tokens,
                row.cache_read_tokens,
                row.cache_write_tokens,
                row.output_tokens,
                row.total_tokens,
                row.cache_hit_count,
                row.request_count,
            ),
        })
        .collect())
}

pub async fn query_breakdown(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<Vec<RequestRecordOverviewBreakdownRow>> {
    let rows = match request_category {
        RequestRecordCategory::Ai => {
            sqlx::query_file_as!(
                BreakdownRow,
                "src/sql/usage/overview/breakdown_ai_model.sql",
                visible_user_id,
                request_category.as_str(),
                window.start,
                window.end,
                user,
            )
            .fetch_all(pool)
            .await?
        }
        RequestRecordCategory::Mcp => {
            sqlx::query_file_as!(
                BreakdownRow,
                "src/sql/usage/overview/breakdown_mcp_server.sql",
                visible_user_id,
                request_category.as_str(),
                window.start,
                window.end,
                user,
            )
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(|row| RequestRecordOverviewBreakdownRow {
            label: row.label,
            request_count: row.request_count,
            request_share: row.request_share,
            success_count: row.success_count,
            success_rate: ratio(row.success_count, row.request_count),
            token_share: row.token_share,
            tokens: token_usage(
                row.input_tokens,
                row.cache_read_tokens,
                row.cache_write_tokens,
                row.output_tokens,
                row.total_tokens,
                row.cache_hit_count,
                row.request_count,
            ),
            model: row.model,
            mcp_server_id: row.mcp_server_id,
        })
        .collect())
}

pub async fn query_error_breakdown(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<Vec<RequestRecordOverviewErrorRow>> {
    let rows = sqlx::query_file_as!(
        ErrorRow,
        "src/sql/usage/overview/error_breakdown.sql",
        visible_user_id,
        request_category.as_str(),
        window.start,
        window.end,
        user,
    )
    .fetch_all(pool)
    .await?;
    let total = rows.iter().map(|row| row.count).sum::<i64>();

    Ok(rows
        .into_iter()
        .map(|row| RequestRecordOverviewErrorRow {
            label: failure_family_label(&row.key).to_string(),
            key: row.key,
            count: row.count,
            rate: ratio(row.count, total),
        })
        .collect())
}
