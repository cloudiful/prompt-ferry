use crate::db::{RequestRecordOverviewSummary, RequestRecordOverviewTokenUsage};

use super::queries::MetricsRow;

pub(super) fn summary_from_metrics(row: MetricsRow) -> RequestRecordOverviewSummary {
    RequestRecordOverviewSummary {
        request_count: row.request_count,
        success_count: row.success_count,
        error_count: row.error_count,
        method_count: row.method_count,
        success_rate: ratio(row.success_count, row.request_count),
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
    }
}

pub(super) fn token_usage(
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cache_hit_count: i64,
    request_count: i64,
) -> RequestRecordOverviewTokenUsage {
    let cache_input_tokens = input_tokens + cache_read_tokens + cache_write_tokens;
    RequestRecordOverviewTokenUsage {
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        total_tokens,
        cache_rate: ratio_option(cache_read_tokens, cache_input_tokens),
        cache_hit_rate: ratio_option(cache_hit_count, request_count),
    }
}

pub(super) fn ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator <= 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn ratio_option(numerator: i64, denominator: i64) -> Option<f64> {
    (denominator > 0).then(|| ratio(numerator, denominator))
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

#[cfg(test)]
mod tests {
    use super::token_usage;

    #[test]
    fn cache_rate_uses_normalized_input_and_stays_bounded() {
        let usage = token_usage(0, 80_000, 0, 64, 80_064, 1, 1);

        assert_eq!(usage.cache_rate, Some(1.0));
        assert_eq!(usage.cache_hit_rate, Some(1.0));
    }

    #[test]
    fn cache_rate_is_not_available_without_input_tokens() {
        let usage = token_usage(0, 0, 0, 64, 64, 0, 1);

        assert_eq!(usage.cache_rate, None);
        assert_eq!(usage.cache_hit_rate, Some(0.0));
    }
}
