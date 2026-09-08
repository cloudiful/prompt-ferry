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
        avg_output_tokens_per_second: row.avg_output_tokens_per_second,
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
    RequestRecordOverviewTokenUsage {
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        total_tokens,
        cache_rate: overview_cache_rate(
            input_tokens,
            cache_read_tokens,
            cache_write_tokens,
            output_tokens,
            total_tokens,
        ),
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

/// Compute the overview cache-read rate from aggregate token sums.
///
/// P2 (issue #205): the denominator is the full input
/// `ordinary + read + write` — 真 0.49, not the old
/// `max(input, read+write)` which dropped the ordinary part and reported
/// 1.0 for ordinary≈cache rows (e.g. 9728/18144≈0.536 报 1.0). A still-folded
/// `input` already contains the cache, so a `CASE` guard (same as
/// 0072/0073 `cache>0 AND total>=output AND input>=total-output`) falls back
/// to `max(input, read+write)` to avoid the ≈1.9x double-count. Clamped to
/// `[0, 1]`. Returns `None` when the denominator is non-positive, matching
/// the SQL `NULL` semantics. Non-negative clamping is applied defensively.
pub(super) fn overview_cache_rate(
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
) -> Option<f64> {
    let ordinary = input_tokens.max(0);
    let read = cache_read_tokens.max(0);
    let write = cache_write_tokens.max(0);
    let output = output_tokens.max(0);
    let total = total_tokens.max(0);
    let cache_sum = read.saturating_add(write);
    let sum = ordinary.saturating_add(cache_sum);
    let is_still_folded =
        cache_sum > 0 && total >= output && ordinary >= total.saturating_sub(output);
    let denominator = if is_still_folded {
        ordinary.max(cache_sum)
    } else {
        sum
    };
    if denominator <= 0 {
        None
    } else {
        let raw = read as f64 / denominator as f64;
        Some(raw.clamp(0.0, 1.0))
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

#[cfg(test)]
mod tests {
    use super::{overview_cache_rate, summary_from_metrics, token_usage};
    use crate::db::usage::overview::queries::MetricsRow;

    #[test]
    fn cache_rate_uses_normalized_input_and_stays_bounded() {
        // Old row shape: ordinary=0, cache_read=80_000, write=0, output=64.
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

    #[test]
    fn overview_cache_rate_caps_legacy_rows_at_full_when_input_is_ordinary_only() {
        // Mirrors the historic bug: ordinary (0 after clamp) + read (80_000)
        // drives the rate to the bounded 1.0 cap.
        let rate = overview_cache_rate(0, 82_793, 0, 64, 82_857);

        assert_eq!(rate, Some(1.0));
    }

    #[test]
    fn overview_cache_rate_uses_full_canonical_input_after_phase_3_backfill() {
        // P2 (issue #205): denominator is `ordinary+read+write` — 真 0.49 —
        // so 176+82793+7=82976 picks the sum, not `max(176, 82800)=82800`.
        // Still-folded rows fall back to `max` via the total guard.
        let rate = overview_cache_rate(176, 82_793, 7, 42, 83_018);

        let value = rate.expect("rate must be present when denominator is positive");
        assert!((value - (82_793.0 / 82_976.0)).abs() < 1e-9);
        assert!((0.0..=1.0).contains(&value));
    }

    #[test]
    fn overview_cache_rate_clamps_negative_meters_without_panicking() {
        // Defensive: negative meters from a future schema must not invert the
        // sign; clamping should yield a bounded non-negative ratio.
        let rate = overview_cache_rate(120, -30, -7, 20, 140);

        assert_eq!(rate, Some(0.0));
    }

    #[test]
    fn overview_cache_rate_returns_none_when_denominator_is_zero() {
        assert_eq!(overview_cache_rate(0, 0, 0, 0, 0), None);
        assert_eq!(overview_cache_rate(-5, 0, 0, 0, 0), None);
    }

    #[test]
    fn overview_cache_rate_matches_old_behavior_for_new_openai_rows() {
        // P2 (issue #205): OpenAI Responses ordinary=83, read=30, write=7,
        // output=20, total=140 uses `83+37=120` — 与 usage_events_page
        // `LEAST(1.0, read/sum)` 一致 — so rate is 30/120, not 30/83.
        let rate = overview_cache_rate(83, 30, 7, 20, 140);

        let value = rate.expect("rate must be present");
        assert!((value - (30.0 / 120.0)).abs() < 1e-9);
    }

    #[test]
    fn overview_cache_rate_falls_back_to_max_for_still_folded_rows() {
        // Still-folded Anthropic: input already holds the cache
        // (82976 == 83018-42), so the CASE guard picks `max`, not the
        // double-counted sum 82976+82800.
        let rate = overview_cache_rate(82_976, 82_793, 7, 42, 83_018);

        let value = rate.expect("rate must be present");
        assert!((value - (82_793.0 / 82_976.0)).abs() < 1e-9);
    }

    #[test]
    fn overview_cache_rate_reports_half_for_balanced_ordinary_rows() {
        // P1 真 0.49 case: ordinary≈cache (8416 vs 9728) must report ~0.53,
        // not the old `max` 1.0.
        let rate = overview_cache_rate(8_416, 9_728, 0, 1_895, 20_039);

        let value = rate.expect("rate must be present");
        assert!((value - (9_728.0 / 18_144.0)).abs() < 1e-9);
    }

    fn fixture_metrics_row(avg_output_tokens_per_second: Option<f64>) -> MetricsRow {
        MetricsRow {
            request_count: 4,
            success_count: 3,
            error_count: 1,
            cache_hit_count: 1,
            method_count: 0,
            input_tokens: 100,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 200,
            total_tokens: 300,
            avg_output_tokens_per_second,
            p95_total_ms: Some(1_500.0),
            p95_first_token_ms: Some(120.0),
        }
    }

    #[test]
    fn summary_from_metrics_passes_through_avg_output_tokens_per_second() {
        let summary = summary_from_metrics(fixture_metrics_row(Some(42.5)));

        assert_eq!(summary.avg_output_tokens_per_second, Some(42.5));
    }

    #[test]
    fn summary_from_metrics_keeps_avg_output_tokens_per_second_null_when_no_valid_rows() {
        // SQL returns NULL when no AI/completed rows had positive output
        // and duration (e.g. MCP-only window or zero-duration failures).
        // The presentation must preserve the NULL rather than collapsing it.
        let summary = summary_from_metrics(fixture_metrics_row(None));

        assert_eq!(summary.avg_output_tokens_per_second, None);
    }

    #[test]
    fn summary_from_metrics_preserves_existing_fields_when_setting_avg_speed() {
        let summary = summary_from_metrics(fixture_metrics_row(Some(7.0)));

        assert_eq!(summary.request_count, 4);
        assert_eq!(summary.success_count, 3);
        assert_eq!(summary.error_count, 1);
        assert_eq!(summary.method_count, 0);
        assert_eq!(summary.p95_total_ms, Some(1_500.0));
        assert_eq!(summary.p95_first_token_ms, Some(120.0));
        assert_eq!(summary.tokens.output_tokens, 200);
        assert_eq!(summary.tokens.total_tokens, 300);
    }
}
