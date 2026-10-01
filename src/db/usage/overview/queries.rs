use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::db::{
    RequestRecordCategory, RequestRecordOverviewBreakdownRow, RequestRecordOverviewPerspective,
    RequestRecordOverviewSummary, RequestRecordOverviewTrendBucket,
    RequestRecordOverviewUpstreamBreakdown,
};

use super::OverviewWindow;
use super::presentation::{error_rate, opt_error_rate, ratio, summary_from_metrics, token_usage};

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
    /// Ordinary cache-miss input (`SUM(normalized_input_tokens)`): still-folded
    /// rows are expanded to `input - cache_read - cache_write` in SQL, so this
    /// never re-includes the cache meters that `full_input_tokens` carries.
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
    /// Per-row-summed fold-aware full-input denominator from `trend_*.sql`,
    /// so the trend `cache_rate` is `SUM(cache_read) / SUM(full_input)` and
    /// still-folded rows are not double-counted on the aggregate SUM.
    full_input_tokens: i64,
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
    /// Per-row-summed fold-aware full-input denominator from
    /// `breakdown_mcp_server.sql`, so the MCP breakdown `cache_rate` is
    /// `SUM(cache_read) / SUM(full_input)`.
    full_input_tokens: i64,
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

#[derive(Debug, FromRow)]
struct AiUpstreamBreakdownRow {
    label: String,
    endpoint_id: Option<uuid::Uuid>,
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
    /// Per-row-summed full-input denominator carried from
    /// `breakdown_ai_upstream.sql` so `cache_rate` is
    /// `SUM(cache_read) / SUM(full_input)`.
    full_input_tokens: i64,
    avg_output_tokens_per_second: Option<f64>,
    model_breakdown: Option<serde_json::Value>,
}

/// Decode a `json_agg` breakdown payload; SQL `NULL` and malformed payloads
/// both collapse to `None` so a bad row never fabricates entries.
fn parse_breakdown<T: serde::de::DeserializeOwned>(
    value: Option<serde_json::Value>,
) -> Option<Vec<T>> {
    serde_json::from_value(value?).ok()
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
                row.full_input_tokens,
            ),
        })
        .collect())
}

/// Map an upstream-perspective row into the shared breakdown shape. Endpoint
/// rows carry `model_breakdown` for the hover and `endpoint_id` for drilldown.
fn ai_upstream_totals(row: AiUpstreamBreakdownRow) -> RequestRecordOverviewBreakdownRow {
    RequestRecordOverviewBreakdownRow {
        label: row.label,
        request_count: row.request_count,
        request_share: row.request_share,
        success_count: row.success_count,
        success_rate: ratio(row.success_count, row.request_count),
        error_count: Some(row.error_count),
        error_rate: Some(error_rate(row.error_count, row.request_count)),
        upstream_count: None,
        upstream_breakdown: None,
        model_breakdown: parse_breakdown(row.model_breakdown),
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
        model: None,
        endpoint_id: row.endpoint_id,
        mcp_server_id: None,
        server_provider_kind: None,
        usage_unit: None,
        avg_output_tokens_per_second: row.avg_output_tokens_per_second,
    }
}

pub async fn query_breakdown(
    pool: &sqlx::PgPool,
    visible_user_id: Option<i64>,
    request_category: RequestRecordCategory,
    window: OverviewWindow,
    user: Option<&str>,
    perspective: RequestRecordOverviewPerspective,
) -> Result<Vec<RequestRecordOverviewBreakdownRow>> {
    match request_category {
        RequestRecordCategory::Ai => match perspective {
            RequestRecordOverviewPerspective::Upstream => {
                let rows = sqlx::query_file_as!(
                    AiUpstreamBreakdownRow,
                    "src/sql/usage/overview/breakdown_ai_upstream.sql",
                    visible_user_id,
                    request_category.as_str(),
                    window.start,
                    window.end,
                    user,
                )
                .fetch_all(pool)
                .await?;
                Ok(rows.into_iter().map(ai_upstream_totals).collect())
            }
            RequestRecordOverviewPerspective::Model => {
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
                        let upstream_breakdown: Option<
                            Vec<RequestRecordOverviewUpstreamBreakdown>,
                        > = parse_breakdown(row.upstream_breakdown);
                        RequestRecordOverviewBreakdownRow {
                            label: row.label,
                            request_count: row.request_count,
                            request_share: row.request_share,
                            success_count: row.success_count,
                            success_rate: ratio(row.success_count, row.request_count),
                            error_count: Some(row.error_count),
                            error_rate: Some(row_error_rate),
                            upstream_count: Some(row.upstream_count),
                            upstream_breakdown,
                            model_breakdown: None,
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
                            endpoint_id: None,
                            mcp_server_id: row.mcp_server_id,
                            server_provider_kind: None,
                            usage_unit: None,
                            avg_output_tokens_per_second: row.avg_output_tokens_per_second,
                        }
                    })
                    .collect())
            }
        },
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
                        model_breakdown: None,
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
                        endpoint_id: None,
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

    fn upstream_row(model_breakdown: Option<serde_json::Value>) -> AiUpstreamBreakdownRow {
        AiUpstreamBreakdownRow {
            label: "endpoint-a".to_string(),
            endpoint_id: Some(uuid::Uuid::nil()),
            request_count: 4,
            request_share: 0.5,
            success_count: 3,
            error_count: 1,
            token_share: Some(0.25),
            cache_hit_count: 2,
            input_tokens: 100,
            cache_read_tokens: 50,
            cache_write_tokens: 0,
            output_tokens: 200,
            total_tokens: 350,
            full_input_tokens: 150,
            avg_output_tokens_per_second: Some(12.5),
            model_breakdown,
        }
    }

    #[test]
    fn ai_upstream_totals_carries_endpoint_and_model_detail() {
        let value = serde_json::json!([{
            "model": "gpt-upstream",
            "request_share": 0.5,
            "request_count": 4,
            "error_count": 1,
            "error_rate": 0.25,
            "total_tokens": 350,
            "token_share": 0.25,
            "cache_rate": 0.5,
            "avg_output_tokens_per_second": 12.5
        }]);
        let row = ai_upstream_totals(upstream_row(Some(value)));

        assert_eq!(row.label, "endpoint-a");
        assert_eq!(row.endpoint_id, Some(uuid::Uuid::nil()));
        assert!(row.model.is_none());
        assert!(row.upstream_breakdown.is_none());
        assert_eq!(row.error_rate, Some(0.25));
        assert_eq!(
            row.model_breakdown.as_deref().map(|entries| entries.len()),
            Some(1)
        );
        assert_eq!(
            row.model_breakdown
                .as_deref()
                .and_then(|entries| entries.first())
                .map(|entry| entry.model.as_str()),
            Some("gpt-upstream")
        );
    }

    #[test]
    fn ai_upstream_totals_tolerates_missing_model_detail() {
        let row = ai_upstream_totals(upstream_row(None));

        assert!(row.model_breakdown.is_none());
        assert_eq!(row.endpoint_id, Some(uuid::Uuid::nil()));
    }
}
