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
    let sql = format!(
        "SELECT COUNT(*)::BIGINT AS request_count, \
            COUNT(*) FILTER (WHERE rr.ok IS TRUE)::BIGINT AS success_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'empty_success')::BIGINT AS empty_success_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'rate_limit' OR rr.failure_family = 'quota')::BIGINT AS rate_limit_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'auth')::BIGINT AS auth_error_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'upstream_5xx' OR rr.failure_family = 'network')::BIGINT AS upstream_5xx_count, \
            COUNT(*) FILTER (WHERE COALESCE(rr.cached_tokens, 0) > 0 OR COALESCE(rr.cache_read_tokens, 0) > 0)::BIGINT AS cache_hit_count, \
            COUNT(DISTINCT rr.mcp_protocol_method)::BIGINT AS method_coverage_count, \
            percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.duration_ms) FILTER (WHERE rr.duration_ms IS NOT NULL) AS p95_total_ms, \
            percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.first_chunk_ms) FILTER (WHERE rr.first_chunk_ms IS NOT NULL) AS p95_first_token_ms{}",
        base_where()
    );
    let row = sqlx::query_as::<_, MetricsRow>(sqlx::AssertSqlSafe(sql))
        .bind(visible_user_id)
        .bind(request_category.as_str())
        .bind(window.start)
        .bind(window.end)
        .bind(user)
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
    let sql = format!(
        "SELECT {} AS bucket_at, \
            COUNT(*)::BIGINT AS request_count, \
            COUNT(*) FILTER (WHERE rr.ok IS TRUE)::BIGINT AS success_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'empty_success')::BIGINT AS empty_success_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'rate_limit' OR rr.failure_family = 'quota')::BIGINT AS rate_limit_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'auth')::BIGINT AS auth_error_count, \
            COUNT(*) FILTER (WHERE rr.failure_family = 'upstream_5xx' OR rr.failure_family = 'network')::BIGINT AS upstream_5xx_count, \
            COUNT(*) FILTER (WHERE COALESCE(rr.cached_tokens, 0) > 0 OR COALESCE(rr.cache_read_tokens, 0) > 0)::BIGINT AS cache_hit_count, \
            COUNT(DISTINCT rr.mcp_protocol_method)::BIGINT AS method_coverage_count, \
            percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.duration_ms) FILTER (WHERE rr.duration_ms IS NOT NULL) AS p95_total_ms, \
            percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.first_chunk_ms) FILTER (WHERE rr.first_chunk_ms IS NOT NULL) AS p95_first_token_ms \
         {} GROUP BY 1 ORDER BY 1 ASC",
        window.bucket.sql_expr(),
        base_where()
    );
    let rows = sqlx::query_as::<_, TrendRow>(sqlx::AssertSqlSafe(sql))
        .bind(visible_user_id)
        .bind(request_category.as_str())
        .bind(window.start)
        .bind(window.end)
        .bind(user)
        .fetch_all(pool)
        .await?;
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
    let groups = match request_category {
        RequestRecordCategory::Ai => vec![
            (
                "by_endpoint",
                "按 Endpoint",
                "COALESCE(pe.name, rr.endpoint_id::TEXT, '(unknown)')",
                "NULL::TEXT",
                "rr.endpoint_id, pe.name",
            ),
            (
                "by_model",
                "按模型",
                "COALESCE(rr.model, '(unknown)')",
                "NULL::TEXT",
                "rr.model",
            ),
            (
                "by_endpoint_model",
                "按 Endpoint × 模型",
                "COALESCE(pe.name, rr.endpoint_id::TEXT, '(unknown)')",
                "COALESCE(rr.model, '(unknown)')",
                "rr.endpoint_id, pe.name, rr.model",
            ),
        ],
        RequestRecordCategory::Mcp => vec![
            (
                "by_server",
                "按 MCP Server",
                "COALESCE(rr.mcp_server_name, ms.name, '(unknown)')",
                "NULL::TEXT",
                "rr.mcp_server_id, rr.mcp_server_name, ms.name",
            ),
            (
                "by_token_slot",
                "按 Token 槽位",
                "COALESCE('Token #' || rr.mcp_bearer_token_slot::TEXT, '(unknown)')",
                "NULL::TEXT",
                "rr.mcp_bearer_token_slot",
            ),
            (
                "by_server_token_slot",
                "按 Server × Token 槽位",
                "COALESCE(rr.mcp_server_name, ms.name, '(unknown)')",
                "COALESCE('Token #' || rr.mcp_bearer_token_slot::TEXT, '(unknown)')",
                "rr.mcp_server_id, rr.mcp_server_name, ms.name, rr.mcp_bearer_token_slot",
            ),
        ],
    };
    let mut output = Vec::with_capacity(groups.len());
    for (key, title, label_expr, secondary_expr, group_expr) in groups {
        let sql = format!(
            "SELECT {label_expr} AS label, \
                NULLIF({secondary_expr}, 'NULL') AS secondary_label, \
                rr.endpoint_id, rr.model, rr.mcp_server_id, rr.mcp_bearer_token_slot, \
                COUNT(*)::BIGINT AS request_count, \
                COUNT(*) FILTER (WHERE rr.ok IS TRUE)::BIGINT AS success_count, \
                COUNT(*) FILTER (WHERE rr.failure_family = 'empty_success')::BIGINT AS empty_success_count, \
                COUNT(*) FILTER (WHERE rr.failure_family = 'rate_limit' OR rr.failure_family = 'quota')::BIGINT AS rate_limit_count, \
                COUNT(*) FILTER (WHERE rr.failure_family = 'auth')::BIGINT AS auth_error_count, \
                COUNT(*) FILTER (WHERE rr.failure_family = 'upstream_5xx' OR rr.failure_family = 'network')::BIGINT AS upstream_5xx_count, \
                COUNT(*) FILTER (WHERE COALESCE(rr.cached_tokens, 0) > 0 OR COALESCE(rr.cache_read_tokens, 0) > 0)::BIGINT AS cache_hit_count, \
                COUNT(DISTINCT rr.mcp_protocol_method)::BIGINT AS method_coverage_count, \
                percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.duration_ms) FILTER (WHERE rr.duration_ms IS NOT NULL) AS p95_total_ms, \
                percentile_cont(0.95) WITHIN GROUP (ORDER BY rr.first_chunk_ms) FILTER (WHERE rr.first_chunk_ms IS NOT NULL) AS p95_first_token_ms \
             {} GROUP BY {group_expr}, rr.endpoint_id, rr.model, rr.mcp_server_id, rr.mcp_bearer_token_slot \
             ORDER BY request_count DESC, label ASC LIMIT 12",
            base_where()
        );
        let rows = sqlx::query_as::<_, RankingSqlRow>(sqlx::AssertSqlSafe(sql))
            .bind(visible_user_id)
            .bind(request_category.as_str())
            .bind(window.start)
            .bind(window.end)
            .bind(user)
            .fetch_all(pool)
            .await?;
        output.push(OverviewRankingGroup {
            key: key.to_string(),
            title: title.to_string(),
            rows: rows
                .into_iter()
                .map(|row| ranking_row(request_category, row))
                .collect(),
        });
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
    let sql = format!(
        "SELECT COALESCE(rr.failure_family, 'unknown') AS key, COUNT(*)::BIGINT AS count \
         {} AND rr.failure_family IS NOT NULL \
         GROUP BY 1 ORDER BY count DESC, key ASC",
        base_where()
    );
    let rows = sqlx::query_as::<_, ErrorRow>(sqlx::AssertSqlSafe(sql))
        .bind(visible_user_id)
        .bind(request_category.as_str())
        .bind(window.start)
        .bind(window.end)
        .bind(user)
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
