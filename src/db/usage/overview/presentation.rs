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
            row.full_input_tokens,
        ),
    }
}

/// Build the aggregate token-usage presentation from a metrics row. The
/// `full_input_tokens` is the fold-aware denominator (`SUM(normalized_full_input_tokens)`)
/// carried through from SQL, used only for `cache_rate`; it cannot be derived here
/// because still-folded rows must fall back to `max(input, read+write)` per-row.
#[allow(clippy::too_many_arguments)]
pub(super) fn token_usage(
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cache_hit_count: i64,
    request_count: i64,
    full_input_tokens: i64,
) -> RequestRecordOverviewTokenUsage {
    RequestRecordOverviewTokenUsage {
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        total_tokens,
        cache_rate: overview_cache_rate(full_input_tokens, cache_read_tokens),
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

/// P1 (issue #207): model-breakdown error rate helper.
///
/// Mirrors `success_rate = ratio(success_count, request_count)` so the new
/// 错误率 column stays consistent with the trend `error_rate`. Returns `0.0`
/// for empty rows instead of `NaN`, matching `ratio` semantics.
pub(super) fn error_rate(error_count: i64, request_count: i64) -> f64 {
    ratio(error_count, request_count)
}

/// Nullable variant for `Option` breakdown columns: `None` propagates so MCP
/// rows (which never report `error_count`) stay `None` instead of `0.0`.
pub(super) fn opt_error_rate(error_count: Option<i64>, request_count: i64) -> Option<f64> {
    error_count.map(|count| error_rate(count, request_count))
}

/// Compute the overview cache-read rate from the per-row `full_input` sum.
///
/// P1 (issue #226): the aggregate denominator must be `SUM(normalized_full_input_tokens)`,
/// not a guard re-derived on the raw `input_tokens` SUM. The per-row `CASE` guard
/// (0072/0073 `cache>0 AND total>=output AND input>=total-output`) is applied row-by-row
/// in SQL so still-folded rows fall back to `max(input, read+write)`; re-deriving that
/// guard on the aggregate SUM was always false and double-counted (49% instead of 98.58%).
/// Here we only divide `cache_read / full_input`, clamped to `[0, 1]`. Returns `None`
/// when the denominator is non-positive, matching the SQL `NULL` semantics.
pub(super) fn overview_cache_rate(full_input_tokens: i64, cache_read_tokens: i64) -> Option<f64> {
    let full_input = full_input_tokens.max(0);
    let read = cache_read_tokens.max(0);
    if full_input <= 0 {
        None
    } else {
        Some((read as f64 / full_input as f64).clamp(0.0, 1.0))
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
    use super::{
        error_rate, opt_error_rate, overview_cache_rate, summary_from_metrics, token_usage,
    };
    use crate::db::usage::overview::queries::MetricsRow;

    #[test]
    fn cache_rate_uses_normalized_input_and_stays_bounded() {
        // Old row shape: ordinary=0, cache_read=80_000, write=0, output=64.
        // full_input = ordinary + read + write = 0 + 80_000 + 0 = 80_000.
        let usage = token_usage(0, 80_000, 0, 64, 80_064, 1, 1, 80_000);

        assert_eq!(usage.cache_rate, Some(1.0));
        assert_eq!(usage.cache_hit_rate, Some(1.0));
    }

    #[test]
    fn cache_rate_is_not_available_without_input_tokens() {
        let usage = token_usage(0, 0, 0, 64, 64, 0, 1, 0);

        assert_eq!(usage.cache_rate, None);
        assert_eq!(usage.cache_hit_rate, Some(0.0));
    }

    #[test]
    fn overview_cache_rate_caps_legacy_rows_at_full_when_input_is_ordinary_only() {
        // Legacy ordinary-only row (0 after clamp + 82_793 read): NOT still-folded
        // (0 >= 82_793 total-output is false), so full_input = ordinary + read = 82_793
        // and the rate caps at 1.0.
        let rate = overview_cache_rate(82_793, 82_793);

        assert_eq!(rate, Some(1.0));
    }

    #[test]
    fn overview_cache_rate_uses_full_canonical_input_after_phase_3_backfill() {
        // P2 (issue #205): denominator is `ordinary+read+write` — 真 0.49 — so
        // 176+82793+7=82976. Still-folded rows fall back to `max` via the guard.
        let rate = overview_cache_rate(82_976, 82_793);

        let value = rate.expect("rate must be present when denominator is positive");
        assert!((value - (82_793.0 / 82_976.0)).abs() < 1e-9);
        assert!((0.0..=1.0).contains(&value));
    }

    #[test]
    fn overview_cache_rate_clamps_negative_meters_without_panicking() {
        // Defensive: negative cache_read from a future schema must clamp to 0 and
        // not invert the sign, yielding a bounded non-negative ratio.
        let rate = overview_cache_rate(120, -30);

        assert_eq!(rate, Some(0.0));
    }

    #[test]
    fn overview_cache_rate_returns_none_when_denominator_is_zero() {
        assert_eq!(overview_cache_rate(0, 0), None);
        assert_eq!(overview_cache_rate(-5, 0), None);
    }

    #[test]
    fn overview_cache_rate_matches_old_behavior_for_new_openai_rows() {
        // P2 (issue #205): OpenAI Responses ordinary=83, read=30, write=7 uses
        // full_input = 83+37 = 120, so rate is 30/120, not 30/83.
        let rate = overview_cache_rate(120, 30);

        let value = rate.expect("rate must be present");
        assert!((value - (30.0 / 120.0)).abs() < 1e-9);
    }

    #[test]
    fn overview_cache_rate_falls_back_to_max_for_still_folded_rows() {
        // Still-folded Anthropic: input already holds the cache (82976 == 83018-42),
        // so full_input = max(82976, 82793+7=82800) = 82976, not the double-counted
        // 82976+82800.
        let rate = overview_cache_rate(82_976, 82_793);

        let value = rate.expect("rate must be present");
        assert!((value - (82_793.0 / 82_976.0)).abs() < 1e-9);
    }

    #[test]
    fn overview_cache_rate_reports_half_for_balanced_ordinary_rows() {
        // P1 真 0.49 case: ordinary≈cache (8416 vs 9728) must report ~0.53,
        // not the old `max` 1.0. full_input = 8416+9728 = 18144.
        let rate = overview_cache_rate(18_144, 9_728);

        let value = rate.expect("rate must be present");
        assert!((value - (9_728.0 / 18_144.0)).abs() < 1e-9);
    }

    #[test]
    fn overview_cache_rate_aggregate_sum_matches_fixture_gold() {
        // P1 (issue #226): aggregate SUM(full_input) over the still-folded fixture
        // yields ≈ 0.9858 (fixture GOLD 0.985037 ± 0.005), not the 0.49 double-count.
        let full_input = 102_608_716;
        let cache_read = 101_153_152;
        let rate = overview_cache_rate(full_input, cache_read);

        let value = rate.expect("rate must be present");
        assert!((value - (101_153_152.0 / 102_608_716.0)).abs() < 1e-9);
        assert!((value - 0.985).abs() < 0.005);
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
            full_input_tokens: 100,
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

    #[test]
    fn breakdown_error_rate_matches_ratio_and_handles_empty_rows() {
        // P1 (issue #207): 1/4 -> 0.25, empty denominator stays 0.0.
        assert!((error_rate(1, 4) - 0.25).abs() < 1e-12);
        assert_eq!(error_rate(0, 4), 0.0);
        assert_eq!(error_rate(3, 0), 0.0);
    }

    #[test]
    fn breakdown_opt_error_rate_keeps_mcp_rows_null() {
        assert_eq!(opt_error_rate(None, 4), None);
        let value = opt_error_rate(Some(1), 4).expect("must be present");
        assert!((value - 0.25).abs() < 1e-12);
    }
}
