//! Additive canonical quota observations (issue #844 P1).
//!
//! Every successful provider fetch exposes one row per provider
//! slot/model/currency in `TokenPlanKeyUsage.quota_windows` while keeping
//! all legacy fields byte-compatible. Presence is truthful: `Some(0.0)` is
//! a real zero, `None` is absent/non-finite. Percentages are valid finite
//! reported shares (or finite derived shares from a present current plus a
//! positive finite cap) only; `balance`/`unlimited` rows never carry a
//! synthetic percentage. Failed keys stay `None`; a successful fetch with
//! no rows is `Some([])` (explicit assessed empty, never legacy fallback).

use chrono::{DateTime, Utc};

use crate::db::EndpointProvider;
use crate::worker_admin_types::{
    CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
    TokenPlanKeyUsage,
};

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

fn remaining_from_used(used_percent: Option<f64>) -> Option<f64> {
    let used = finite(used_percent)?;
    Some((100.0 - used).clamp(0.0, 100.0))
}

fn known(remaining_percent: Option<f64>) -> SubscriptionWindowAvailability {
    if remaining_percent.is_some() {
        SubscriptionWindowAvailability::Known
    } else {
        SubscriptionWindowAvailability::Unknown
    }
}

fn window_row(
    source_id: &str,
    kind: CanonicalQuotaWindowKind,
    source_model: Option<String>,
    unit: Option<String>,
    duration_seconds: Option<i64>,
    is_default_scope: bool,
    remaining_percent: Option<f64>,
    current: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    reset_at: Option<DateTime<Utc>>,
) -> CanonicalQuotaWindow {
    let remaining_percent = finite(remaining_percent);
    let current = finite(current);
    let limit = finite(limit);
    let remaining = finite(remaining);
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind,
        source_model,
        unit,
        duration_seconds,
        is_default_scope,
        availability: if remaining_percent.is_some() {
            SubscriptionWindowAvailability::Known
        } else {
            known_balance(current.as_ref(), limit.as_ref(), remaining.as_ref())
        },
        remaining_percent,
        current,
        limit,
        remaining,
        reset_at,
    }
}

fn known_balance(
    current: Option<&f64>,
    limit: Option<&f64>,
    remaining: Option<&f64>,
) -> SubscriptionWindowAvailability {
    if current.is_some() || limit.is_some() || remaining.is_some() {
        SubscriptionWindowAvailability::Known
    } else {
        SubscriptionWindowAvailability::Unknown
    }
}

fn balance_row(
    source_id: &str,
    unit: Option<String>,
    current: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
) -> CanonicalQuotaWindow {
    let current = finite(current);
    let limit = finite(limit);
    let remaining = finite(remaining);
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Balance,
        source_model: None,
        unit,
        duration_seconds: None,
        is_default_scope: true,
        availability: known_balance(current.as_ref(), limit.as_ref(), remaining.as_ref()),
        remaining_percent: None,
        current,
        limit,
        remaining,
        reset_at: None,
    }
}

/// GLM fallback from typed windows (presence already lost to `0.0`
/// defaults): keep an honest percent when known, leave quantities unknown
/// rather than pretending a default `0` was observed. Fresh fetches set a
/// presence-aware vector in `glm_usage` directly and this fallback never
/// overwrites it.
pub(crate) fn glm_fallback_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let mut rows = Vec::new();
    for (slot, kind, window) in [
        (
            "glm:five_hour",
            CanonicalQuotaWindowKind::FiveHour,
            &key.glm_five_hour,
        ),
        (
            "glm:weekly",
            CanonicalQuotaWindowKind::Weekly,
            &key.glm_weekly,
        ),
    ] {
        if let Some(window) = window {
            let remaining_percent =
                finite(window.percentage).map(|used| (100.0 - used).clamp(0.0, 100.0));
            rows.push(CanonicalQuotaWindow {
                source_id: slot.to_string(),
                kind,
                source_model: None,
                unit: None,
                duration_seconds: None,
                is_default_scope: true,
                availability: known(remaining_percent),
                remaining_percent,
                current: None,
                limit: None,
                remaining: None,
                reset_at: window.next_reset_at,
            });
        }
    }
    rows
}

/// GLM presence-aware observation for one winning row (built at parse time
/// before `0.0` defaults erase absence). `row_type` selects the documented
/// unit (`TOKENS_LIMIT` → `tokens`, `CREDIT_LIMIT` → `credits`).
pub(crate) fn glm_observation(
    slot: &str,
    kind: CanonicalQuotaWindowKind,
    row_type: Option<&str>,
    current: Option<f64>,
    limit: Option<f64>,
    remaining: Option<f64>,
    percentage: Option<f64>,
    reset_at: Option<DateTime<Utc>>,
) -> CanonicalQuotaWindow {
    let unit = match row_type {
        Some("TOKENS_LIMIT") => Some("tokens".to_string()),
        Some("CREDIT_LIMIT") => Some("credits".to_string()),
        _ => None,
    };
    window_row(
        slot,
        kind,
        None,
        unit,
        None,
        true,
        remaining_from_used(percentage),
        current,
        limit,
        remaining,
        reset_at,
    )
}

fn command_code_window_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let mut rows = Vec::new();
    for (slot, kind, window) in [
        (
            "command_code:five_hour",
            CanonicalQuotaWindowKind::FiveHour,
            &key.five_hour,
        ),
        (
            "command_code:weekly",
            CanonicalQuotaWindowKind::Weekly,
            &key.weekly,
        ),
    ] {
        if let Some(window) = window {
            let current = finite(Some(window.used));
            let cap = finite(Some(window.cap));
            let derived_remaining = match (current, cap) {
                (Some(used), Some(cap)) => Some(cap - used),
                _ => None,
            };
            rows.push(window_row(
                slot,
                kind,
                None,
                Some("currency".to_string()),
                None,
                true,
                window.remaining_percent,
                current,
                cap,
                derived_remaining,
                window.reset_at,
            ));
        }
    }
    if let Some(balances) = key.balances.as_ref() {
        let current = finite(Some(balances.remaining_credits));
        let limit = finite(Some(
            balances.monthly_credits + balances.purchased_credits + balances.free_credits,
        ));
        rows.push(balance_row(
            "command_code:balances",
            Some("credits".to_string()),
            current,
            limit,
            current,
        ));
    }
    rows
}

fn opencode_go_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let mut rows = Vec::new();
    for (slot, kind, window) in [
        (
            "opencode_go:rolling",
            CanonicalQuotaWindowKind::Rolling,
            &key.opencodego_rolling,
        ),
        (
            "opencode_go:weekly",
            CanonicalQuotaWindowKind::Weekly,
            &key.opencodego_weekly,
        ),
        (
            "opencode_go:monthly",
            CanonicalQuotaWindowKind::Monthly,
            &key.opencodego_monthly,
        ),
    ] {
        if let Some(window) = window {
            rows.push(window_row(
                slot,
                kind,
                None,
                None,
                None,
                true,
                window.percent.map(|used| 100.0 - used),
                None,
                None,
                None,
                window.resets_at,
            ));
        }
    }
    rows
}

fn openrouter_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let Some(balance) = key.openrouter_balance.as_ref() else {
        return Vec::new();
    };
    let limit = finite(balance.limit);
    let remaining = finite(balance.limit_remaining);
    match limit {
        Some(cap) if cap > 0.0 => {
            vec![balance_row(
                "openrouter:balance",
                Some("credits".to_string()),
                remaining,
                limit,
                remaining,
            )]
        }
        Some(_) => {
            // Zero/negative cap cannot spend: known exhausted balance with a
            // zero amount, never a synthetic percentage.
            vec![balance_row(
                "openrouter:balance",
                Some("credits".to_string()),
                Some(0.0),
                limit,
                Some(0.0),
            )]
        }
        None => {
            // Unlimited cap: known only with a spend snapshot, otherwise no
            // valid signal (empty, assessed at the caller).
            if key.openrouter_spend.is_some() {
                vec![CanonicalQuotaWindow {
                    source_id: "openrouter:unlimited".to_string(),
                    kind: CanonicalQuotaWindowKind::Unlimited,
                    source_model: None,
                    unit: Some("credits".to_string()),
                    duration_seconds: None,
                    is_default_scope: true,
                    availability: SubscriptionWindowAvailability::Known,
                    remaining_percent: None,
                    current: None,
                    limit: None,
                    remaining: None,
                    reset_at: None,
                }]
            } else {
                Vec::new()
            }
        }
    }
}

fn deepseek_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let Some(balance) = key.deepseek_balance.as_ref() else {
        return Vec::new();
    };
    balance
        .balances
        .iter()
        .map(|entry| {
            let current = finite(entry.total_balance);
            let remaining = finite(entry.total_balance);
            // Known amount observations never change the authoritative typed
            // status: availability only records that an amount was observed.
            let availability = known_balance(current.as_ref(), None, remaining.as_ref());
            CanonicalQuotaWindow {
                source_id: format!("deepseek:{}", entry.currency.to_uppercase()),
                kind: CanonicalQuotaWindowKind::Balance,
                source_model: None,
                unit: Some(entry.currency.to_uppercase()),
                duration_seconds: None,
                is_default_scope: true,
                availability,
                remaining_percent: None,
                current,
                limit: None,
                remaining,
                reset_at: None,
            }
        })
        .collect()
}

fn is_default_minimax_model(model_name: &str) -> bool {
    model_name.trim().eq_ignore_ascii_case("general")
}

fn minimax_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let mut rows = Vec::new();
    for model in &key.model_remains {
        // Subscription-shaped models carry real durations and are projected
        // by the subscription adapter, not the minimax count adapter.
        if model.windows.is_some() {
            continue;
        }
        let default_scope = is_default_minimax_model(&model.model_name);
        for (slot, kind, window) in [
            (
                "interval",
                CanonicalQuotaWindowKind::FiveHour,
                &model.interval,
            ),
            ("weekly", CanonicalQuotaWindowKind::Weekly, &model.weekly),
        ] {
            if let Some(window) = window {
                let limit = window.total_count.map(|v| v as f64);
                let current = window.usage_count.map(|v| v as f64);
                let derived_remaining = match (finite(limit), finite(current)) {
                    (Some(cap), Some(used)) => Some(cap - used),
                    _ => None,
                };
                rows.push(window_row(
                    &format!("minimax:{}:{slot}", model.model_name),
                    kind,
                    Some(model.model_name.clone()),
                    Some("tokens".to_string()),
                    None,
                    default_scope,
                    window.remaining_percent,
                    finite(current),
                    finite(limit),
                    finite(derived_remaining),
                    window.end_at,
                ));
            }
        }
    }
    rows
}

fn subscription_observations(key: &TokenPlanKeyUsage) -> Vec<CanonicalQuotaWindow> {
    let mut rows = Vec::new();
    for model in &key.model_remains {
        let Some(windows) = model.windows.as_deref() else {
            continue;
        };
        for window in windows {
            rows.push(window_row(
                &format!("subscription:{}", window.source_window),
                CanonicalQuotaWindowKind::Custom,
                Some(model.model_name.clone()),
                None,
                window.window_seconds,
                true,
                window.remaining_percent,
                None,
                None,
                None,
                window.reset_at,
            ));
        }
    }
    rows
}

/// Build the canonical vector for one key. Failed keys yield `None`
/// (unknown, never available); successful keys yield `Some(vec)` where an
/// empty vec is an explicit assessed empty. Existing `Some` (including
/// parser-captured presence-aware vectors) is never overwritten by the
/// caller — use [`populate_key_observations`] for that guard.
pub(crate) fn observations_for_key(
    provider: EndpointProvider,
    key: &TokenPlanKeyUsage,
) -> Option<Vec<CanonicalQuotaWindow>> {
    if !key.ok {
        return None;
    }
    let rows = match provider {
        EndpointProvider::Minimax => minimax_observations(key),
        EndpointProvider::CommandCode => command_code_window_observations(key),
        EndpointProvider::OpencodeGo => opencode_go_observations(key),
        EndpointProvider::OpenRouter => openrouter_observations(key),
        EndpointProvider::Glm => glm_fallback_observations(key),
        EndpointProvider::DeepSeek => deepseek_observations(key),
        EndpointProvider::OpenAi | EndpointProvider::Generic => subscription_observations(key),
    };
    Some(rows)
}

/// Fill `quota_windows` for successful keys that do not carry one yet.
/// Preserves parser-captured `Some` (including `Some([])`) and forces
/// failed keys to `None` so an unknown failure never reads as available.
pub(crate) fn populate_key_observations(provider: EndpointProvider, key: &mut TokenPlanKeyUsage) {
    if !key.ok {
        key.quota_windows = None;
        return;
    }
    if key.quota_windows.is_some() {
        return;
    }
    key.quota_windows = observations_for_key(provider, key);
}

/// Fill a whole provider response (fetch envelope + cache/decorate paths).
pub(crate) fn populate_response_observations(
    provider: EndpointProvider,
    response: &mut crate::worker_admin_types::TokenPlanUsageResponse,
) {
    for key in &mut response.keys {
        populate_key_observations(provider, key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin_types::{
        CommandCodeBalances, CommandCodeWindowUsage, DeepSeekBalance, DeepSeekCurrencyBalance,
        GlmWindowUsage, OpenRouterBalance, OpenRouterSpend, OpencodeGoWindowUsage,
        SubscriptionWindowAvailability, SubscriptionWindowUsage, TokenPlanModelUsage,
        TokenPlanWindowUsage,
    };
    use chrono::Duration;
    use uuid::Uuid;

    fn key() -> TokenPlanKeyUsage {
        TokenPlanKeyUsage {
            key_id: Uuid::nil(),
            key_label: "k".into(),
            ok: true,
            status: Some(200),
            error_code: None,
            error_message: None,
            quota_status: None,
            model_remains: Vec::new(),
            balances: None,
            five_hour: None,
            weekly: None,
            opencodego_rolling: None,
            opencodego_weekly: None,
            opencodego_monthly: None,
            openrouter_balance: None,
            openrouter_spend: None,
            glm_five_hour: None,
            glm_weekly: None,
            deepseek_balance: None,
            quota_windows: None,
        }
    }

    fn failed() -> TokenPlanKeyUsage {
        TokenPlanKeyUsage { ok: false, ..key() }
    }

    #[test]
    fn failed_keys_stay_none_and_empty_is_authoritative() {
        let mut bad = failed();
        populate_key_observations(EndpointProvider::Glm, &mut bad);
        assert!(bad.quota_windows.is_none());
        assert!(observations_for_key(EndpointProvider::Glm, &bad).is_none());

        // Successful fetch with no rows is an explicit empty, not fallback.
        let mut generic = key();
        populate_key_observations(EndpointProvider::Generic, &mut generic);
        assert_eq!(generic.quota_windows, Some(Vec::new()));

        // Parser-captured vectors (including empty) are never overwritten.
        let mut captured = key();
        captured.quota_windows = Some(Vec::new());
        captured.glm_five_hour = Some(GlmWindowUsage {
            limit: 100.0,
            current_value: 10.0,
            remaining: 90.0,
            percentage: Some(10.0),
            next_reset_at: None,
        });
        populate_key_observations(EndpointProvider::Glm, &mut captured);
        assert_eq!(captured.quota_windows, Some(Vec::new()));
    }

    #[test]
    fn glm_presence_zero_cap_and_nonfinite_stay_honest() {
        // Real present zero current with a positive cap derives through the
        // presence-aware constructor.
        let row = glm_observation(
            "glm:five_hour",
            CanonicalQuotaWindowKind::FiveHour,
            Some("TOKENS_LIMIT"),
            Some(0.0),
            Some(2000.0),
            Some(2000.0),
            Some(0.0),
            None,
        );
        assert_eq!(row.remaining_percent, Some(100.0));
        assert_eq!(row.current, Some(0.0));
        assert_eq!(row.unit.as_deref(), Some("tokens"));
        assert_eq!(row.availability, SubscriptionWindowAvailability::Known);

        // Zero cap keeps a known zero amount but cannot derive a percent.
        let row = glm_observation(
            "glm:weekly",
            CanonicalQuotaWindowKind::Weekly,
            Some("CREDIT_LIMIT"),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            None,
            None,
        );
        assert_eq!(row.limit, Some(0.0));
        assert!(row.remaining_percent.is_none());
        // Amounts were observed, so the row is known even without a share.
        assert_eq!(row.availability, SubscriptionWindowAvailability::Known);
        assert_eq!(row.unit.as_deref(), Some("credits"));

        // Non-finite totals never manufacture a share.
        let row = glm_observation(
            "glm:five_hour",
            CanonicalQuotaWindowKind::FiveHour,
            Some("TOKENS_LIMIT"),
            Some(10.0),
            Some(f64::INFINITY),
            Some(f64::INFINITY),
            None,
            None,
        );
        assert!(row.limit.is_none());
        assert!(row.remaining_percent.is_none());

        // Missing current with a positive cap stays unknown.
        let row = glm_observation(
            "glm:five_hour",
            CanonicalQuotaWindowKind::FiveHour,
            Some("TOKENS_LIMIT"),
            None,
            Some(100.0),
            None,
            None,
            None,
        );
        assert!(row.remaining_percent.is_none());
    }

    #[test]
    fn command_code_windows_and_balances_keep_units() {
        let mut cc = key();
        cc.balances = Some(CommandCodeBalances {
            monthly_credits: 80.0,
            purchased_credits: 5.0,
            free_credits: 0.0,
            remaining_credits: 85.0,
        });
        cc.five_hour = Some(CommandCodeWindowUsage {
            used: 8.0,
            cap: 16.0,
            used_percent: Some(50.0),
            remaining_percent: Some(50.0),
            reset_at: None,
        });
        populate_key_observations(EndpointProvider::CommandCode, &mut cc);
        let rows = cc.quota_windows.expect("cc rows");
        assert_eq!(rows.len(), 2);
        let window = rows
            .iter()
            .find(|r| r.kind == CanonicalQuotaWindowKind::FiveHour)
            .expect("5h");
        assert_eq!(window.unit.as_deref(), Some("currency"));
        assert_eq!(window.remaining_percent, Some(50.0));
        assert_eq!(window.current, Some(8.0));
        let balance = rows
            .iter()
            .find(|r| r.kind == CanonicalQuotaWindowKind::Balance)
            .expect("balance");
        assert!(balance.remaining_percent.is_none());
        assert_eq!(balance.current, Some(85.0));
    }

    #[test]
    fn opencode_go_nonfinite_percent_is_unknown() {
        let mut go = key();
        go.opencodego_rolling = Some(OpencodeGoWindowUsage {
            status: None,
            percent: Some(f64::INFINITY),
            resets_at: None,
        });
        go.opencodego_weekly = Some(OpencodeGoWindowUsage {
            status: None,
            percent: Some(0.0),
            resets_at: None,
        });
        populate_key_observations(EndpointProvider::OpencodeGo, &mut go);
        let rows = go.quota_windows.expect("go rows");
        assert_eq!(rows.len(), 2);
        // Non-finite input is unknown (never clamped inf -> 100); a real
        // zero used share is a known full remaining.
        let rolling = rows
            .iter()
            .find(|r| r.source_id == "opencode_go:rolling")
            .expect("rolling");
        assert!(rolling.remaining_percent.is_none());
        assert_eq!(
            rolling.availability,
            SubscriptionWindowAvailability::Unknown
        );
        let weekly = rows
            .iter()
            .find(|r| r.source_id == "opencode_go:weekly")
            .expect("weekly");
        assert_eq!(weekly.remaining_percent, Some(100.0));
    }

    #[test]
    fn minimax_default_scope_only_marks_general() {
        let mut mini = key();
        mini.model_remains = vec![
            TokenPlanModelUsage {
                model_name: "general".into(),
                interval: Some(TokenPlanWindowUsage {
                    status: None,
                    remaining_percent: Some(35.0),
                    total_count: Some(100),
                    usage_count: Some(65),
                    boost_permille: None,
                    start_at: None,
                    end_at: None,
                    remains_time_ms: None,
                }),
                weekly: None,
                windows: None,
                observation: None,
            },
            TokenPlanModelUsage {
                model_name: "video".into(),
                interval: Some(TokenPlanWindowUsage {
                    status: None,
                    remaining_percent: Some(90.0),
                    total_count: Some(100),
                    usage_count: Some(10),
                    boost_permille: None,
                    start_at: None,
                    end_at: None,
                    remains_time_ms: None,
                }),
                weekly: None,
                windows: None,
                observation: None,
            },
        ];
        populate_key_observations(EndpointProvider::Minimax, &mut mini);
        let rows = mini.quota_windows.expect("mini rows");
        assert_eq!(rows.len(), 2);
        let general = rows
            .iter()
            .find(|r| r.source_model.as_deref() == Some("general"))
            .expect("general");
        assert!(general.is_default_scope);
        assert_eq!(general.remaining_percent, Some(35.0));
        let video = rows
            .iter()
            .find(|r| r.source_model.as_deref() == Some("video"))
            .expect("video");
        assert!(!video.is_default_scope);
    }

    #[test]
    fn subscription_rows_keep_real_duration_and_availability() {
        let reset = Utc::now() + Duration::hours(5);
        let mut sub = key();
        sub.model_remains = vec![TokenPlanModelUsage {
            model_name: "chatgpt".into(),
            interval: None,
            weekly: None,
            windows: Some(vec![SubscriptionWindowUsage {
                source_window: "primary".into(),
                window_seconds: Some(18_000),
                used_percent: Some(20.0),
                remaining_percent: Some(80.0),
                reset_at: Some(reset),
                reset_after_seconds: None,
                availability: SubscriptionWindowAvailability::Known,
            }]),
            observation: None,
        }];
        populate_key_observations(EndpointProvider::OpenAi, &mut sub);
        let rows = sub.quota_windows.expect("sub rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].duration_seconds, Some(18_000));
        assert_eq!(rows[0].remaining_percent, Some(80.0));
        assert_eq!(rows[0].reset_at, Some(reset));
    }

    #[test]
    fn subscription_account_windows_are_default_scope() {
        // The subscription model IS the account (plan_type/chatgpt), so its
        // windows bind the default pool independently of MiniMax `general`
        // naming; detail rows stay explicitly out of the pool elsewhere.
        let mut sub = key();
        sub.model_remains = vec![TokenPlanModelUsage {
            model_name: "chatgpt".into(),
            interval: None,
            weekly: None,
            windows: Some(vec![SubscriptionWindowUsage {
                source_window: "primary".into(),
                window_seconds: Some(18_000),
                used_percent: Some(20.0),
                remaining_percent: Some(80.0),
                reset_at: None,
                reset_after_seconds: None,
                availability: SubscriptionWindowAvailability::Known,
            }]),
            observation: None,
        }];
        populate_key_observations(EndpointProvider::OpenAi, &mut sub);
        let rows = sub.quota_windows.expect("sub rows");
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_default_scope);
        assert_eq!(rows[0].source_model.as_deref(), Some("chatgpt"));
    }

    #[test]
    fn balance_and_unlimited_never_carry_percentages() {
        let mut router = key();
        router.openrouter_balance = Some(OpenRouterBalance {
            limit: Some(100.0),
            limit_remaining: Some(40.0),
            limit_reset: None,
            is_free_tier: false,
            total_credits: None,
            total_usage: None,
        });
        populate_key_observations(EndpointProvider::OpenRouter, &mut router);
        let rows = router.quota_windows.expect("router rows");
        assert_eq!(rows[0].kind, CanonicalQuotaWindowKind::Balance);
        assert!(rows[0].remaining_percent.is_none());

        let mut unlimited = key();
        unlimited.openrouter_balance = Some(OpenRouterBalance {
            limit: None,
            limit_remaining: None,
            limit_reset: None,
            is_free_tier: true,
            total_credits: None,
            total_usage: None,
        });
        unlimited.openrouter_spend = Some(OpenRouterSpend {
            usage: 1.0,
            daily: 1.0,
            weekly: 1.0,
            monthly: 1.0,
        });
        populate_key_observations(EndpointProvider::OpenRouter, &mut unlimited);
        let rows = unlimited.quota_windows.expect("unlimited rows");
        assert_eq!(rows[0].kind, CanonicalQuotaWindowKind::Unlimited);
        assert!(rows[0].remaining_percent.is_none());

        let mut deepseek = key();
        deepseek.deepseek_balance = Some(DeepSeekBalance {
            is_available: false,
            balances: vec![DeepSeekCurrencyBalance {
                currency: "cny".into(),
                total_balance: Some(5.0),
                granted_balance: None,
                topped_up_balance: None,
            }],
        });
        populate_key_observations(EndpointProvider::DeepSeek, &mut deepseek);
        let rows = deepseek.quota_windows.expect("deepseek rows");
        assert_eq!(rows[0].unit.as_deref(), Some("CNY"));
        assert!(rows[0].remaining_percent.is_none());
        // Known amount observation; typed Exhausted stays authoritative
        // outside this projection.
        assert_eq!(rows[0].availability, SubscriptionWindowAvailability::Known);
    }

    #[test]
    fn canonical_agrees_with_valid_legacy_percent_not_urgency() {
        // A fresh valid GLM typed window (used 25 -> remaining 75) projects
        // to the same remaining share the legacy percent math reports.
        let typed_remaining = 100.0 - 25.0;
        let row = glm_observation(
            "glm:five_hour",
            CanonicalQuotaWindowKind::FiveHour,
            Some("TOKENS_LIMIT"),
            Some(25.0),
            Some(100.0),
            Some(75.0),
            Some(25.0),
            None,
        );
        assert_eq!(row.remaining_percent, Some(typed_remaining));
    }
}
