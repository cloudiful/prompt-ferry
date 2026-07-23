use crate::db::{OverviewQualityComponent, OverviewQualityFormula, RequestRecordCategory};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AggregateMetrics {
    pub request_count: i64,
    pub success_count: i64,
    pub empty_success_count: i64,
    pub rate_limit_count: i64,
    pub auth_error_count: i64,
    pub upstream_5xx_count: i64,
    pub cache_hit_count: i64,
    pub method_coverage_count: i64,
    pub p95_total_ms: Option<f64>,
    pub p95_first_token_ms: Option<f64>,
}

impl AggregateMetrics {
    pub(crate) fn success_rate(self) -> f64 {
        ratio(self.success_count, self.request_count)
    }
    pub(crate) fn empty_success_rate(self) -> f64 {
        ratio(self.empty_success_count, self.request_count)
    }

    pub(crate) fn rate_limit_rate(self) -> f64 {
        ratio(self.rate_limit_count, self.request_count)
    }

    pub(crate) fn auth_error_rate(self) -> f64 {
        ratio(self.auth_error_count, self.request_count)
    }

    pub(crate) fn upstream_5xx_rate(self) -> f64 {
        ratio(self.upstream_5xx_count, self.request_count)
    }

    pub(crate) fn cache_hit_rate(self) -> f64 {
        ratio(self.cache_hit_count, self.request_count)
    }
}

pub(crate) fn ratio(numerator: i64, denominator: i64) -> f64 {
    if denominator <= 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub(crate) fn quality_formula(category: RequestRecordCategory) -> OverviewQualityFormula {
    match category {
        RequestRecordCategory::Ai => OverviewQualityFormula {
            score_kind: "rule_based".to_string(),
            components: vec![
                component("success_rate", "成功率", 0.45, "成功率越高越好。"),
                component("p95_total_ms", "P95 总时延", 0.20, "8 秒满分，60 秒归零。"),
                component(
                    "p95_first_token_ms",
                    "P95 首 token",
                    0.15,
                    "1.5 秒满分，15 秒归零。",
                ),
                component("rate_limit_or_quota", "429/配额错误率", 0.10, "越低越好。"),
                component("upstream_5xx", "上游 5xx 错误率", 0.05, "越低越好。"),
                component("empty_success", "空成功响应率", 0.05, "越低越好。"),
            ],
        },
        RequestRecordCategory::Mcp => OverviewQualityFormula {
            score_kind: "rule_based".to_string(),
            components: vec![
                component("success_rate", "成功率", 0.55, "成功率越高越好。"),
                component("p95_total_ms", "P95 总时延", 0.25, "3 秒满分，30 秒归零。"),
                component(
                    "auth_or_rate_limit",
                    "401/403/429 错误率",
                    0.10,
                    "越低越好。",
                ),
                component(
                    "upstream_or_transport",
                    "上游 5xx/传输失败率",
                    0.10,
                    "越低越好。",
                ),
            ],
        },
    }
}

pub(crate) fn quality_score(category: RequestRecordCategory, metrics: AggregateMetrics) -> f64 {
    let score = match category {
        RequestRecordCategory::Ai => {
            100.0
                * (0.45 * metrics.success_rate()
                    + 0.20 * linear_latency_score(metrics.p95_total_ms, 8_000.0, 60_000.0)
                    + 0.15 * linear_latency_score(metrics.p95_first_token_ms, 1_500.0, 15_000.0)
                    + 0.10 * (1.0 - metrics.rate_limit_rate())
                    + 0.05 * (1.0 - metrics.upstream_5xx_rate())
                    + 0.05 * (1.0 - metrics.empty_success_rate()))
        }
        RequestRecordCategory::Mcp => {
            100.0
                * (0.55 * metrics.success_rate()
                    + 0.25 * linear_latency_score(metrics.p95_total_ms, 3_000.0, 30_000.0)
                    + 0.10
                        * (1.0 - (metrics.auth_error_rate() + metrics.rate_limit_rate()).min(1.0))
                    + 0.10 * (1.0 - metrics.upstream_5xx_rate()))
        }
    };
    score.clamp(0.0, 100.0)
}

fn component(key: &str, label: &str, weight: f64, description: &str) -> OverviewQualityComponent {
    OverviewQualityComponent {
        key: key.to_string(),
        label: label.to_string(),
        weight,
        description: description.to_string(),
    }
}

fn linear_latency_score(value: Option<f64>, best_ms: f64, worst_ms: f64) -> f64 {
    let Some(value) = value else {
        return 0.0;
    };
    if value <= best_ms {
        return 1.0;
    }
    if value >= worst_ms {
        return 0.0;
    }
    1.0 - ((value - best_ms) / (worst_ms - best_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_quality_score_prefers_success_and_fast_latency() {
        let strong = AggregateMetrics {
            request_count: 100,
            success_count: 99,
            p95_total_ms: Some(2_000.0),
            p95_first_token_ms: Some(500.0),
            ..AggregateMetrics::default()
        };
        let weak = AggregateMetrics {
            request_count: 100,
            success_count: 70,
            rate_limit_count: 20,
            upstream_5xx_count: 10,
            empty_success_count: 10,
            p95_total_ms: Some(65_000.0),
            p95_first_token_ms: Some(20_000.0),
            ..AggregateMetrics::default()
        };

        let strong_score = quality_score(RequestRecordCategory::Ai, strong);
        let weak_score = quality_score(RequestRecordCategory::Ai, weak);

        assert!(strong_score > 90.0);
        assert!(weak_score < 60.0);
        assert!(strong_score > weak_score);
    }

    #[test]
    fn mcp_quality_score_penalizes_auth_and_rate_limit_failures() {
        let baseline = AggregateMetrics {
            request_count: 100,
            success_count: 98,
            p95_total_ms: Some(1_000.0),
            ..AggregateMetrics::default()
        };
        let degraded = AggregateMetrics {
            request_count: 100,
            success_count: 75,
            auth_error_count: 10,
            rate_limit_count: 10,
            upstream_5xx_count: 5,
            p95_total_ms: Some(20_000.0),
            ..AggregateMetrics::default()
        };

        let baseline_score = quality_score(RequestRecordCategory::Mcp, baseline);
        let degraded_score = quality_score(RequestRecordCategory::Mcp, degraded);

        assert!(baseline_score > 90.0);
        assert!(degraded_score < 75.0);
        assert!(baseline_score > degraded_score);
    }
}
