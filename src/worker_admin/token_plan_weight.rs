//! Pure quota-weighting helpers for the unified key pool (issue #281 P1).
//!
//! A key's weight is its tightest (lowest) remaining quota window, lifted by
//! each window's reset urgency, then damped by outstanding reservations so a
//! concurrent burst cannot overshoot the same key. Windows that expose no
//! reset signal keep their raw remaining, so a snapshot without reset data
//! degrades to the plain min-bottleneck behavior.
//!
//! The helpers here are pure functions over parsed usage; the cache methods
//! in [`super::token_plan_cache`] supply the snapshot and reservation state.

use super::command_code_usage::command_code_remaining_percent;
use super::glm_parsing::glm_remaining_percent;
use super::opencode_go_usage::opencode_go_remaining_percent;
use super::quota_urgency::clamp_percent;
use super::quota_window_weight::{
    command_code_weight_percent, effective_weight_percent, glm_weight_percent,
    opencode_go_weight_percent,
};
use crate::worker_admin_types::{TokenPlanKeyUsage, TokenPlanModelUsage};

/// Fraction of a key's window weight charged for each outstanding
/// reservation. Percent-only windows carry no absolute token capacity to
/// convert a token estimate, so concurrent load is bounded by draw count:
/// every in-flight request lowers the key's next weight by this fraction.
pub(crate) const RESERVATION_BACKPRESSURE_PER_DRAW: f64 = 0.1;

/// Cap on the draw count folded into the damping factor, so a sustained
/// burst always leaves a strictly positive weight instead of underflowing
/// to zero and starving a key that still has quota.
const MAX_BACKPRESSURE_DRAWS: u64 = 10_000;

/// Tightest remaining percent for a key that only exposes percent windows
/// (CommandCode, OpencodeGo, OpenRouter, GLM). `None` when no provider
/// window carries a signal.
pub(crate) fn provider_remaining_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    command_code_remaining_percent(key)
        .or_else(|| opencode_go_remaining_percent(key))
        .or_else(|| openrouter_remaining_percent(key))
        .or_else(|| glm_remaining_percent(key))
        .map(clamp_percent)
}

/// Urgency-aware counterpart of [`provider_remaining_percent`] used for pool
/// weighting.
pub(crate) fn provider_weight_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    command_code_weight_percent(key)
        .or_else(|| opencode_go_weight_percent(key))
        .or_else(|| openrouter_remaining_percent(key))
        .or_else(|| glm_weight_percent(key))
        .map(clamp_percent)
}

/// Damp a weight by the number of outstanding reservations on the key.
/// Multiplicative so the result stays strictly positive for any positive
/// weight, and capped so a long burst cannot drive it to zero.
pub(crate) fn apply_reservation_backpressure(remaining: f64, draws: u64) -> f64 {
    let remaining = clamp_percent(remaining);
    if draws == 0 {
        return remaining;
    }
    let draws = draws.min(MAX_BACKPRESSURE_DRAWS);
    clamp_percent(remaining / (1.0 + draws as f64 * RESERVATION_BACKPRESSURE_PER_DRAW))
}

pub(crate) fn model_usage<'a>(
    key: &'a TokenPlanKeyUsage,
    model: Option<&str>,
) -> Option<&'a TokenPlanModelUsage> {
    let model = model?.trim();
    key.model_remains
        .iter()
        .find(|usage| usage.model_name.eq_ignore_ascii_case(model))
        .or_else(|| {
            key.model_remains
                .iter()
                .find(|usage| usage.model_name.eq_ignore_ascii_case("general"))
        })
        .or_else(|| (key.model_remains.len() == 1).then(|| &key.model_remains[0]))
}

// OpenRouter remaining percent (issue #203 P3): a finite key-level cap
// (`limit`) weights by `limit_remaining/limit`; an unlimited key (`limit`
// null) has no cap, so a present spend snapshot means full weight (ratio
// capped at 1). Missing numbers degrade to `None` (no quota signal).
pub(crate) fn openrouter_remaining_percent(key: &TokenPlanKeyUsage) -> Option<f64> {
    let balance = key.openrouter_balance.as_ref()?;
    match balance.limit {
        Some(limit) if limit.is_finite() && limit > 0.0 => {
            let remaining = balance.limit_remaining?;
            Some((remaining / limit * 100.0).clamp(0.0, 100.0))
        }
        // A zero (or negative) cap cannot spend: fully exhausted.
        Some(limit) if limit.is_finite() => Some(0.0),
        // Unlimited (or non-finite) cap: full weight when the key shows a
        // spend snapshot, otherwise no signal.
        _ => key.openrouter_spend.as_ref().map(|_| 100.0),
    }
}

fn effective_remaining_percent(usage: &TokenPlanModelUsage) -> Option<f64> {
    let interval = usage
        .interval
        .as_ref()
        .and_then(|window| window.remaining_percent);
    let weekly = usage
        .weekly
        .as_ref()
        .and_then(|window| window.remaining_percent);
    match (interval, weekly) {
        (Some(interval), Some(weekly)) => Some(interval.min(weekly).clamp(0.0, 100.0)),
        (Some(remaining), None) | (None, Some(remaining)) => Some(remaining.clamp(0.0, 100.0)),
        (None, None) => None,
    }
}

fn reserved_percent(usage: &TokenPlanModelUsage, reserved_tokens: u64) -> f64 {
    let total_count = [
        usage
            .interval
            .as_ref()
            .and_then(|window| window.total_count),
        usage.weekly.as_ref().and_then(|window| window.total_count),
    ]
    .into_iter()
    .flatten()
    .filter(|total| *total > 0)
    .min()
    .unwrap_or_default();
    if total_count <= 0 {
        return 0.0;
    }
    (reserved_tokens as f64 / total_count as f64 * 100.0).min(100.0)
}

/// Raw MiniMax window remaining minus the token reservation share.
pub(crate) fn model_remaining_percent(
    usage: &TokenPlanModelUsage,
    reserved_tokens: u64,
) -> Option<f64> {
    let remaining = effective_remaining_percent(usage)?;
    Some((remaining - reserved_percent(usage, reserved_tokens)).max(0.0))
}

/// Urgency-lifted MiniMax window remaining minus the token reservation share.
pub(crate) fn model_weight_percent(
    usage: &TokenPlanModelUsage,
    reserved_tokens: u64,
) -> Option<f64> {
    let remaining = effective_weight_percent(usage)?;
    Some((remaining - reserved_percent(usage, reserved_tokens)).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin_types::{OpenRouterBalance, OpenRouterSpend, TokenPlanWindowUsage};

    fn usage(interval: Option<f64>, weekly: Option<f64>) -> TokenPlanModelUsage {
        TokenPlanModelUsage {
            model_name: "general".to_string(),
            interval: interval.map(window),
            weekly: weekly.map(window),
        }
    }

    fn window(remaining_percent: f64) -> TokenPlanWindowUsage {
        TokenPlanWindowUsage {
            status: Some(1),
            remaining_percent: Some(remaining_percent),
            total_count: None,
            usage_count: None,
            boost_permille: None,
            start_at: None,
            end_at: None,
            remains_time_ms: None,
        }
    }

    #[test]
    fn effective_remaining_uses_the_most_constrained_window() {
        assert_eq!(
            effective_remaining_percent(&usage(Some(0.0), Some(69.0))),
            Some(0.0)
        );
        assert_eq!(
            effective_remaining_percent(&usage(Some(42.0), Some(69.0))),
            Some(42.0)
        );
        assert_eq!(
            effective_remaining_percent(&usage(Some(42.0), None)),
            Some(42.0)
        );
        assert_eq!(effective_remaining_percent(&usage(None, None)), None);
    }

    #[test]
    fn reservation_is_converted_to_a_quota_percentage_when_total_is_known() {
        let model = TokenPlanModelUsage {
            model_name: "general".to_string(),
            interval: Some(TokenPlanWindowUsage {
                status: Some(1),
                remaining_percent: Some(100.0),
                total_count: Some(1_000),
                usage_count: None,
                boost_permille: None,
                start_at: None,
                end_at: None,
                remains_time_ms: None,
            }),
            weekly: None,
        };
        assert_eq!(reserved_percent(&model, 100), 10.0);
        assert_eq!(reserved_percent(&model, 2_000), 100.0);
    }

    #[test]
    fn reservation_backpressure_damps_but_never_zeroes_a_key() {
        assert_eq!(apply_reservation_backpressure(80.0, 0), 80.0);
        let damped = apply_reservation_backpressure(80.0, 5);
        assert!(damped < 80.0 && damped > 0.0, "damped={damped}");
        let floor = apply_reservation_backpressure(80.0, u64::MAX);
        assert!(floor.is_finite() && floor > 0.0, "floor={floor}");
    }

    fn openrouter_key(
        limit: Option<f64>,
        limit_remaining: Option<f64>,
        spend: bool,
    ) -> TokenPlanKeyUsage {
        TokenPlanKeyUsage {
            key_id: uuid::Uuid::nil(),
            key_label: "k".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            model_remains: Vec::new(),
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: Some(OpenRouterBalance {
                limit,
                limit_remaining,
                limit_reset: None,
                is_free_tier: false,
                total_credits: None,
                total_usage: None,
            }),
            openrouter_spend: spend.then_some(OpenRouterSpend {
                usage: 1.0,
                daily: 0.5,
                weekly: 0.75,
                monthly: 1.0,
            }),
            glm_five_hour: None,
            glm_weekly: None,
        }
    }

    #[test]
    fn openrouter_remaining_shares_limit_or_falls_back_to_spend() {
        let pct = |limit, remaining, spend| {
            openrouter_remaining_percent(&openrouter_key(limit, remaining, spend))
        };
        assert_eq!(pct(Some(100.0), Some(74.5), true), Some(74.5));
        // Over-full remaining clamps at 100; zero caps are exhausted.
        assert_eq!(pct(Some(100.0), Some(120.0), true), Some(100.0));
        assert_eq!(pct(Some(100.0), Some(0.0), true), Some(0.0));
        assert_eq!(pct(Some(0.0), Some(0.0), true), Some(0.0));
        // Unlimited keys carry full weight once a spend snapshot exists.
        assert_eq!(pct(None, None, true), Some(100.0));
        // Missing numbers degrade to no quota signal.
        assert_eq!(pct(Some(100.0), None, true), None);
        assert_eq!(pct(None, None, false), None);
        let mut key = openrouter_key(None, None, true);
        key.openrouter_balance = None;
        assert_eq!(openrouter_remaining_percent(&key), None);
    }
}
