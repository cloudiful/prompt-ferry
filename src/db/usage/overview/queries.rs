use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::db::{
    RequestRecordCategory, RequestRecordOverviewBreakdownRow, RequestRecordOverviewErrorRow,
    RequestRecordOverviewSummary, RequestRecordOverviewTrendBucket,
    RequestRecordOverviewUpstreamBreakdown,
};

use super::OverviewWindow;
use super::presentation::{
    error_rate, failure_family_label, opt_error_rate, ratio, summary_from_metrics, token_usage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Per-row-summed full-input denominator `ordinary+read+write` (or the
    /// still-folded `max` fallback), carried through from `metrics.sql` so the
    /// overview `cache_rate` is `SUM(cache_read) / SUM(full_input)` and never
    /// re-derives the fold guard on the aggregate SUM.
    pub(super) full_input_tokens: i64,
    pub(super) avg_output_tokens_per_second: Option<f64>,
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
    server_provider_kind: Option<String>,
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
    avg_output_tokens_per_second: Option<f64>,
}

#[derive(Debug, FromRow)]
struct AiBreakdownRow {
    label: String,
    model: Option<String>,
    mcp_server_id: Option<uuid::Uuid>,
    request_count: i64,
    request_share: f64,
    success_count: i64,
    error_count: i64,
    token_share: Option<f64>,
    cache_hit_count: i64,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    /// Per-row-summed full-input denominator carried from `breakdown_ai_model.sql`
    /// so the breakdown `cache_rate` is `SUM(cache_read) / SUM(full_input)`.
    full_input_tokens: i64,
    avg_output_tokens_per_second: Option<f64>,
    upstream_count: i64,
    upstream_breakdown: Option<serde_json::Value>,
}

fn parse_upstream_breakdown(
    value: Option<serde_json::Value>,
) -> Option<Vec<RequestRecordOverviewUpstreamBreakdown>> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    serde_json::from_value(value).ok()
}

/// Resolve the canonical provider preset and its usage unit for an MCP
/// breakdown row. Generic/legacy/unknown stored values collapse to `(None,
/// None)` so the client never renders a fabricated provider or unit.
fn mcp_provider_dimension(raw: Option<&str>) -> (Option<String>, Option<String>) {
    let provider_kind = crate::db::canonical_mcp_provider_kind(raw);
    let usage_unit =
        crate::db::mcp_provider_info(provider_kind).map(|info| info.unit.as_str().to_string());
    (provider_kind.map(str::to_string), usage_unit)
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
                row.input_tokens
                    .saturating_add(row.cache_read_tokens)
                    .saturating_add(row.cache_write_tokens),
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
    match request_category {
        RequestRecordCategory::Ai => {
            let rows = sqlx::query_file_as!(
                AiBreakdownRow,
                "src/sql/usage/overview/breakdown_ai_model.sql",
                visible_user_id,
                request_category.as_str(),
                window.start,
                window.end,
                user,
            )
            .fetch_all(pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let row_error_rate = error_rate(row.error_count, row.request_count);
                    RequestRecordOverviewBreakdownRow {
                        label: row.label,
                        request_count: row.request_count,
                        request_share: row.request_share,
                        success_count: row.success_count,
                        success_rate: ratio(row.success_count, row.request_count),
                        error_count: Some(row.error_count),
                        error_rate: Some(row_error_rate),
                        upstream_count: Some(row.upstream_count),
                        upstream_breakdown: parse_upstream_breakdown(row.upstream_breakdown),
                        token_share: row.token_share,
                        tokens: token_usage(
                            row.input_tokens,
                            row.cache_read_tokens,
                            row.cache_write_tokens,
                            row.output_tokens,
                            row.total_tokens,
                            row.cache_hit_count,
                            row.request_count,
                            row.full_input_tokens,
                        ),
                        model: row.model,
                        mcp_server_id: row.mcp_server_id,
                        server_provider_kind: None,
                        usage_unit: None,
                        avg_output_tokens_per_second: row.avg_output_tokens_per_second,
                    }
                })
                .collect())
        }
        RequestRecordCategory::Mcp => {
            let rows = sqlx::query_file_as!(
                BreakdownRow,
                "src/sql/usage/overview/breakdown_mcp_server.sql",
                visible_user_id,
                request_category.as_str(),
                window.start,
                window.end,
                user,
            )
            .fetch_all(pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(|row| {
                    let (server_provider_kind, usage_unit) =
                        mcp_provider_dimension(row.server_provider_kind.as_deref());
                    RequestRecordOverviewBreakdownRow {
                        label: row.label,
                        request_count: row.request_count,
                        request_share: row.request_share,
                        success_count: row.success_count,
                        success_rate: ratio(row.success_count, row.request_count),
                        error_count: None,
                        error_rate: opt_error_rate(None, row.request_count),
                        upstream_count: None,
                        upstream_breakdown: None,
                        token_share: row.token_share,
                        tokens: token_usage(
                            row.input_tokens,
                            row.cache_read_tokens,
                            row.cache_write_tokens,
                            row.output_tokens,
                            row.total_tokens,
                            row.cache_hit_count,
                            row.request_count,
                            row.input_tokens
                                .saturating_add(row.cache_read_tokens)
                                .saturating_add(row.cache_write_tokens),
                        ),
                        model: row.model,
                        mcp_server_id: row.mcp_server_id,
                        server_provider_kind,
                        usage_unit,
                        avg_output_tokens_per_second: row.avg_output_tokens_per_second,
                    }
                })
                .collect())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_provider_dimension_uses_the_registry_unit() {
        assert_eq!(
            mcp_provider_dimension(Some("firecrawl")),
            (Some("firecrawl".to_string()), Some("credits".to_string()))
        );
        assert_eq!(
            mcp_provider_dimension(Some("context7")),
            (Some("context7".to_string()), Some("requests".to_string()))
        );
        assert_eq!(
            mcp_provider_dimension(Some("minimax")),
            (Some("minimax".to_string()), Some("requests".to_string()))
        );
        // Generic/legacy/unknown stored values must not fabricate a unit.
        assert_eq!(mcp_provider_dimension(Some("generic")), (None, None));
        assert_eq!(mcp_provider_dimension(Some("legacy-unknown")), (None, None));
        assert_eq!(mcp_provider_dimension(Some("")), (None, None));
        assert_eq!(mcp_provider_dimension(None), (None, None));
    }
}
