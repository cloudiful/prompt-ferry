use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

use super::presentation::{failure_family_label, metrics_from_row, ranking_row};
use super::*;

#[derive(Debug, FromRow, Clone, Copy)]
pub(super) struct MetricsRow {
    pub(super) request_count: i64,
    pub(super) success_count: i64,
    pub(super) empty_success_count: i64,
    pub(super) rate_limit_count: i64,
    pub(super) auth_error_count: i64,
    pub(super) upstream_5xx_count: i64,
    pub(super) cache_hit_count: i64,
    pub(super) method_coverage_count: i64,
    pub(super) p95_total_ms: Option<f64>,
    pub(super) p95_first_token_ms: Option<f64>,
}

#[derive(Debug, FromRow)]
struct TrendRow {
    bucket_at: DateTime<Utc>,
    request_count: i64,
    success_count: i64,
    empty_success_count: i64,
    rate_limit_count: i64,
    auth_error_count: i64,
    upstream_5xx_count: i64,
    cache_hit_count: i64,
    method_coverage_count: i64,
    p95_total_ms: Option<f64>,
    p95_first_token_ms: Option<f64>,
}

#[derive(Debug, FromRow)]
struct ErrorRow {
    key: String,
    count: i64,
}

#[derive(Debug, FromRow)]
pub(super) struct RankingSqlRow {
    pub(super) label: Option<String>,
    pub(super) secondary_label: Option<String>,
    pub(super) endpoint_id: Option<Uuid>,
    pub(super) model: Option<String>,
    pub(super) mcp_server_id: Option<Uuid>,
    pub(super) mcp_bearer_token_slot: Option<i16>,
    pub(super) request_count: i64,
    pub(super) success_count: i64,
    pub(super) empty_success_count: i64,
    pub(super) rate_limit_count: i64,
    pub(super) auth_error_count: i64,
    pub(super) upstream_5xx_count: i64,
    pub(super) cache_hit_count: i64,
    pub(super) method_coverage_count: i64,
    pub(super) p95_total_ms: Option<f64>,
    pub(super) p95_first_token_ms: Option<f64>,
}

pub(super) async fn query_metrics(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<AggregateMetrics> {
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
    Ok(metrics_from_row(row))
}

pub(super) async fn query_trend(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<Vec<OverviewTrendBucket>> {
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
        .map(|row| {
            let metrics = metrics_from_row(MetricsRow {
                request_count: row.request_count,
                success_count: row.success_count,
                empty_success_count: row.empty_success_count,
                rate_limit_count: row.rate_limit_count,
                auth_error_count: row.auth_error_count,
                upstream_5xx_count: row.upstream_5xx_count,
                cache_hit_count: row.cache_hit_count,
                method_coverage_count: row.method_coverage_count,
                p95_total_ms: row.p95_total_ms,
                p95_first_token_ms: row.p95_first_token_ms,
            });
            OverviewTrendBucket {
                bucket_at: row.bucket_at,
                request_count: metrics.request_count,
                success_rate: metrics.success_rate(),
                quality_score: quality_score(request_category, metrics),
                p95_total_ms: metrics.p95_total_ms,
                p95_first_token_ms: metrics.p95_first_token_ms,
            }
        })
        .collect())
}

pub(super) async fn query_rankings(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<Vec<OverviewRankingGroup>> {
    let mut output = Vec::with_capacity(3);
    match request_category {
        RequestRecordCategory::Ai => {
            let groups = [
                (
                    "by_endpoint",
                    "按 Endpoint",
                    sqlx::query_file_as!(
                        RankingSqlRow,
                        "src/sql/usage/overview/ranking_ai_endpoint.sql",
                        visible_user_id,
                        request_category.as_str(),
                        window.start,
                        window.end,
                        user,
                    )
                    .fetch_all(pool)
                    .await?,
                ),
                (
                    "by_model",
                    "按模型",
                    sqlx::query_file_as!(
                        RankingSqlRow,
                        "src/sql/usage/overview/ranking_ai_model.sql",
                        visible_user_id,
                        request_category.as_str(),
                        window.start,
                        window.end,
                        user,
                    )
                    .fetch_all(pool)
                    .await?,
                ),
                (
                    "by_endpoint_model",
                    "按 Endpoint × 模型",
                    sqlx::query_file_as!(
                        RankingSqlRow,
                        "src/sql/usage/overview/ranking_ai_endpoint_model.sql",
                        visible_user_id,
                        request_category.as_str(),
                        window.start,
                        window.end,
                        user,
                    )
                    .fetch_all(pool)
                    .await?,
                ),
            ];
            for (key, title, rows) in groups {
                output.push(OverviewRankingGroup {
                    key: key.to_string(),
                    title: title.to_string(),
                    rows: rows
                        .into_iter()
                        .map(|row| ranking_row(request_category, row))
                        .collect(),
                });
            }
        }
        RequestRecordCategory::Mcp => {
            let groups = [
                (
                    "by_server",
                    "按 MCP Server",
                    sqlx::query_file_as!(
                        RankingSqlRow,
                        "src/sql/usage/overview/ranking_mcp_server.sql",
                        visible_user_id,
                        request_category.as_str(),
                        window.start,
                        window.end,
                        user,
                    )
                    .fetch_all(pool)
                    .await?,
                ),
                (
                    "by_token_slot",
                    "按 Token 槽位",
                    sqlx::query_file_as!(
                        RankingSqlRow,
                        "src/sql/usage/overview/ranking_mcp_token_slot.sql",
                        visible_user_id,
                        request_category.as_str(),
                        window.start,
                        window.end,
                        user,
                    )
                    .fetch_all(pool)
                    .await?,
                ),
                (
                    "by_server_token_slot",
                    "按 Server × Token 槽位",
                    sqlx::query_file_as!(
                        RankingSqlRow,
                        "src/sql/usage/overview/ranking_mcp_server_token_slot.sql",
                        visible_user_id,
                        request_category.as_str(),
                        window.start,
                        window.end,
                        user,
                    )
                    .fetch_all(pool)
                    .await?,
                ),
            ];
            for (key, title, rows) in groups {
                output.push(OverviewRankingGroup {
                    key: key.to_string(),
                    title: title.to_string(),
                    rows: rows
                        .into_iter()
                        .map(|row| ranking_row(request_category, row))
                        .collect(),
                });
            }
        }
    }
    Ok(output)
}

pub(super) async fn query_error_breakdown(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
) -> Result<Vec<OverviewErrorBreakdownRow>> {
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
        .map(|row| OverviewErrorBreakdownRow {
            label: failure_family_label(&row.key).to_string(),
            key: row.key,
            count: row.count,
            rate: ratio(row.count, total),
        })
        .collect())
}
