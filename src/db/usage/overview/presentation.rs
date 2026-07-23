use super::queries::{MetricsRow, RankingSqlRow};
use super::*;

pub(super) fn build_heatmap(
    request_category: RequestRecordCategory,
    rankings: &[OverviewRankingGroup],
) -> OverviewHeatmap {
    let key = if request_category == RequestRecordCategory::Ai {
        "endpoint_model"
    } else {
        "server_token_slot"
    };
    let source_key = if request_category == RequestRecordCategory::Ai {
        "by_endpoint_model"
    } else {
        "by_server_token_slot"
    };
    let rows = rankings
        .iter()
        .find(|group| group.key == source_key)
        .map(|group| group.rows.clone())
        .unwrap_or_default();
    let mut xs = BTreeMap::<String, i32>::new();
    let mut ys = BTreeMap::<String, i32>::new();
    for row in &rows {
        let x = row
            .secondary_label
            .clone()
            .unwrap_or_else(|| UNKNOWN_LABEL.to_string());
        let y = row.label.clone();
        let next_x = xs.len() as i32;
        xs.entry(x).or_insert(next_x);
        let next_y = ys.len() as i32;
        ys.entry(y).or_insert(next_y);
    }
    let x_labels = xs.keys().cloned().collect::<Vec<_>>();
    let y_labels = ys.keys().cloned().collect::<Vec<_>>();
    let cells = rows
        .into_iter()
        .map(|row| OverviewHeatmapCell {
            x_index: *xs
                .get(
                    &row.secondary_label
                        .clone()
                        .unwrap_or_else(|| UNKNOWN_LABEL.to_string()),
                )
                .unwrap_or(&0),
            y_index: *ys.get(&row.label).unwrap_or(&0),
            request_count: row.request_count,
            success_rate: row.success_rate,
            quality_score: row.quality_score,
            p95_total_ms: row.p95_total_ms,
            p95_first_token_ms: row.p95_first_token_ms,
            error_rate: 1.0 - row.success_rate,
            endpoint_id: row.endpoint_id,
            model: row.model,
            mcp_server_id: row.mcp_server_id,
            mcp_bearer_token_slot: row.mcp_bearer_token_slot,
        })
        .collect();
    OverviewHeatmap {
        key: key.to_string(),
        x_labels,
        y_labels,
        cells,
    }
}

pub(super) fn metrics_from_row(row: MetricsRow) -> AggregateMetrics {
    AggregateMetrics {
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
    }
}

pub(super) fn ranking_row(
    category: RequestRecordCategory,
    row: RankingSqlRow,
) -> OverviewRankingRow {
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
    OverviewRankingRow {
        label: row.label.unwrap_or_else(|| UNKNOWN_LABEL.to_string()),
        secondary_label: row.secondary_label.filter(|value| value != "NULL"),
        request_count: metrics.request_count,
        success_rate: metrics.success_rate(),
        quality_score: quality_score(category, metrics),
        p95_total_ms: metrics.p95_total_ms,
        p95_first_token_ms: metrics.p95_first_token_ms,
        rate_limit_rate: Some(metrics.rate_limit_rate()),
        auth_error_rate: Some(metrics.auth_error_rate()),
        upstream_5xx_rate: Some(metrics.upstream_5xx_rate()),
        empty_success_rate: Some(metrics.empty_success_rate()),
        cache_hit_rate: Some(metrics.cache_hit_rate()),
        endpoint_id: row.endpoint_id,
        model: row.model,
        mcp_server_id: row.mcp_server_id,
        mcp_bearer_token_slot: row.mcp_bearer_token_slot,
    }
}

pub(super) fn summary_cards(
    category: RequestRecordCategory,
    metrics: AggregateMetrics,
) -> Vec<OverviewMetricCard> {
    let mut cards = vec![
        card(
            "request_count",
            "请求数",
            metrics.request_count as f64,
            "count",
        ),
        card("success_rate", "成功率", metrics.success_rate(), "ratio"),
        card(
            "quality_score",
            "质量分",
            quality_score(category, metrics),
            "score",
        ),
        card(
            "p95_total_ms",
            "P95 总时延",
            metrics.p95_total_ms.unwrap_or(0.0),
            "ms",
        ),
    ];
    match category {
        RequestRecordCategory::Ai => {
            cards.push(card(
                "p95_first_token_ms",
                "P95 首 token",
                metrics.p95_first_token_ms.unwrap_or(0.0),
                "ms",
            ));
            cards.push(card(
                "rate_limit_rate",
                "429 占比",
                metrics.rate_limit_rate(),
                "ratio",
            ));
            cards.push(card(
                "upstream_5xx_rate",
                "5xx 占比",
                metrics.upstream_5xx_rate(),
                "ratio",
            ));
            cards.push(card(
                "cache_hit_rate",
                "缓存命中率",
                metrics.cache_hit_rate(),
                "ratio",
            ));
        }
        RequestRecordCategory::Mcp => {
            cards.push(card(
                "auth_error_rate",
                "401/403 占比",
                metrics.auth_error_rate(),
                "ratio",
            ));
            cards.push(card(
                "rate_limit_rate",
                "429 占比",
                metrics.rate_limit_rate(),
                "ratio",
            ));
            cards.push(card(
                "method_coverage_count",
                "方法覆盖数",
                metrics.method_coverage_count as f64,
                "count",
            ));
        }
    }
    cards
}

fn card(key: &str, label: &str, value: f64, unit: &str) -> OverviewMetricCard {
    OverviewMetricCard {
        key: key.to_string(),
        label: label.to_string(),
        value,
        unit: unit.to_string(),
    }
}

pub(super) fn failure_family_label(key: &str) -> &'static str {
    match key {
        "auth" => "鉴权失败",
        "rate_limit" => "限流",
        "quota" => "配额",
        "timeout" => "超时",
        "upstream_4xx" => "上游 4xx",
        "upstream_5xx" => "上游 5xx",
        "network" => "网络/传输",
        "empty_success" => "空成功",
        "policy" => "策略拦截",
        _ => "未知",
    }
}
