//! OpenRouter balance defensive parsers (issue #203 P3).
//!
//! `GET /api/v1/key` returns `{data: {limit, limit_remaining, limit_reset,
//! usage, usage_daily/weekly/monthly, is_free_tier}}` (`limit` null means the
//! key is unlimited). Errors use `{error: {code, message}}`. `GET
//! /api/v1/credits` returns `{data: {total_credits, total_usage}}` but only
//! for management keys; plain keys get 403 and degrade to `None` totals.

use serde_json::Value;

use super::json_scalars::{truncate_message, value_as_f64, value_as_string};
use crate::worker_admin_types::{OpenRouterBalance, OpenRouterSpend};

/// Parsed `/key` payload: balance fields plus the spend snapshot. Credits
/// totals are filled in later by the `/credits` call (or left `None`).
#[derive(Debug, Clone)]
pub(crate) struct OpenRouterKeyUsage {
    pub(crate) balance: OpenRouterBalance,
    pub(crate) spend: OpenRouterSpend,
}

// HTTP-status business errors: 401 = missing/invalid key, 402 = credit limit
// exhausted (maps to the `quota` error code so quota-family logging matches
// without touching the shared whitelist), 429 = rate limited and passed
// through with retry semantics.
pub(crate) fn openrouter_http_business_error(
    status: u16,
    body: &Value,
) -> Option<(Option<String>, String)> {
    match status {
        401 => Some((
            Some("401".to_string()),
            "OpenRouter key is missing or invalid".to_string(),
        )),
        402 => Some((
            Some("quota".to_string()),
            "OpenRouter credit limit exhausted (quota)".to_string(),
        )),
        429 => Some((
            Some("429".to_string()),
            openrouter_error_message(body)
                .unwrap_or_else(|| "OpenRouter rate limited (retry later)".to_string()),
        )),
        _ => None,
    }
}

// Server error detail: `{error: {message}}` with a top-level `message`
// fallback. Blank messages are ignored so callers fall back to defaults.
pub(crate) fn openrouter_error_message(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| body.get("message"))
        .and_then(value_as_string)
        .map(|message| message.trim().to_string())
        .filter(|message| !message.is_empty())
        .map(truncate_message)
}

// The `/key` (and `/credits`) data object. `None` for missing/non-object
// payloads so callers can fail or degrade explicitly.
fn key_data(body: &Value) -> Option<&serde_json::Map<String, Value>> {
    body.get("data")?.as_object()
}

// `/key` balance plus spend. `limit` null degrades to `None` (unlimited);
// `is_free_tier` passes through (missing defaults to false). Returns `None`
// when the body carries no recognizable key signal (never padded).
pub(crate) fn parse_openrouter_key(body: &Value) -> Option<OpenRouterKeyUsage> {
    let data = key_data(body)?;
    let limit = data.get("limit").and_then(value_as_f64);
    let limit_remaining = data.get("limit_remaining").and_then(value_as_f64);
    let usage = data.get("usage").and_then(value_as_f64);
    if limit.is_none() && limit_remaining.is_none() && usage.is_none() {
        return None;
    }
    Some(OpenRouterKeyUsage {
        balance: OpenRouterBalance {
            limit,
            limit_remaining,
            limit_reset: data.get("limit_reset").and_then(value_as_string),
            is_free_tier: data
                .get("is_free_tier")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            total_credits: None,
            total_usage: None,
        },
        spend: OpenRouterSpend {
            usage: usage.unwrap_or(0.0),
            daily: data
                .get("usage_daily")
                .and_then(value_as_f64)
                .unwrap_or(0.0),
            weekly: data
                .get("usage_weekly")
                .and_then(value_as_f64)
                .unwrap_or(0.0),
            monthly: data
                .get("usage_monthly")
                .and_then(value_as_f64)
                .unwrap_or(0.0),
        },
    })
}

// `/credits` totals. `None` when the payload carries no credit totals, so
// the fetcher can silently degrade (403 for non-management keys never
// reaches here, but an unparseable 200 degrades the same way).
pub(crate) fn parse_openrouter_credits(body: &Value) -> Option<(f64, f64)> {
    let data = key_data(body)?;
    Some((
        data.get("total_credits").and_then(value_as_f64)?,
        data.get("total_usage").and_then(value_as_f64)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key_fixture() -> Value {
        json!({
            "data": {
                "label": "sk-or-v1-au7...890",
                "limit": 100,
                "limit_remaining": 74.5,
                "limit_reset": "monthly",
                "usage": 25.5,
                "usage_daily": 1.5,
                "usage_weekly": 5.25,
                "usage_monthly": 12.0,
                "is_free_tier": false
            }
        })
    }

    #[test]
    fn key_shape_parses_balance_and_spend() {
        let parsed = parse_openrouter_key(&key_fixture()).expect("key");
        assert_eq!(parsed.balance.limit, Some(100.0));
        assert_eq!(parsed.balance.limit_remaining, Some(74.5));
        assert_eq!(parsed.balance.limit_reset.as_deref(), Some("monthly"));
        assert!(!parsed.balance.is_free_tier);
        assert_eq!(parsed.spend.usage, 25.5);
        assert_eq!(parsed.spend.daily, 1.5);
        assert_eq!(parsed.spend.weekly, 5.25);
        assert_eq!(parsed.spend.monthly, 12.0);
    }

    #[test]
    fn null_limit_is_unlimited_and_free_tier_passes_through() {
        let body = json!({
            "data": {
                "limit": null,
                "limit_remaining": null,
                "limit_reset": null,
                "usage": 3.0,
                "is_free_tier": true
            }
        });
        let parsed = parse_openrouter_key(&body).expect("key");
        assert_eq!(parsed.balance.limit, None);
        assert_eq!(parsed.balance.limit_remaining, None);
        assert_eq!(parsed.balance.limit_reset, None);
        assert!(parsed.balance.is_free_tier);
        // Missing usage windows degrade to zero, never to padding elsewhere.
        assert_eq!(parsed.spend.daily, 0.0);
        assert_eq!(parsed.spend.weekly, 0.0);
        assert_eq!(parsed.spend.monthly, 0.0);
    }

    #[test]
    fn stringy_numbers_coerce_and_garbage_bodies_rejected() {
        let body = json!({
            "data": {"limit": "50", "limit_remaining": "10", "usage": "5"}
        });
        let parsed = parse_openrouter_key(&body).expect("key");
        assert_eq!(parsed.balance.limit, Some(50.0));
        assert_eq!(parsed.spend.usage, 5.0);
        assert!(parse_openrouter_key(&json!({"data": {}})).is_none());
        assert!(parse_openrouter_key(&json!({"data": null})).is_none());
        assert!(parse_openrouter_key(&json!({"error": {"code": 401}})).is_none());
        assert!(parse_openrouter_key(&json!([])).is_none());
    }

    #[test]
    fn credits_shape_parses_totals_and_missing_degrades() {
        let body = json!({"data": {"total_credits": 100.5, "total_usage": 25.75}});
        assert_eq!(parse_openrouter_credits(&body), Some((100.5, 25.75)));
        assert!(parse_openrouter_credits(&json!({"data": {}})).is_none());
        assert!(parse_openrouter_credits(&json!({"data": {"total_credits": 1}})).is_none());
        assert!(
            parse_openrouter_credits(&json!({"error": {"code": 403, "message": "Forbidden"}}))
                .is_none()
        );
    }

    #[test]
    fn business_errors_distinguish_status_codes() {
        let body = json!({"error": {"code": 401, "message": "Missing Authentication header"}});
        assert_eq!(
            openrouter_http_business_error(401, &body)
                .unwrap()
                .0
                .as_deref(),
            Some("401")
        );
        let (code, message) = openrouter_http_business_error(402, &body).expect("402");
        assert_eq!(code.as_deref(), Some("quota"));
        // The 402 message self-contains a quota marker so quota-family
        // logging matches without touching the shared whitelist.
        assert!(crate::upstream_error::is_quota_exhaustion(&message));
        assert!(crate::upstream_error::is_quota_exhaustion(&format!(
            "{} {}",
            code.unwrap_or_default(),
            message
        )));
        // 429 passes the server message through (retry semantics preserved).
        let limited = json!({"error": {"code": 429, "message": "Rate limit exceeded"}});
        let (code, message) = openrouter_http_business_error(429, &limited).expect("429");
        assert_eq!(code.as_deref(), Some("429"));
        assert_eq!(message, "Rate limit exceeded");
        let (code, message) = openrouter_http_business_error(429, &json!({})).expect("429 default");
        assert_eq!(code.as_deref(), Some("429"));
        assert!(message.contains("retry"));
        assert!(openrouter_http_business_error(500, &body).is_none());
    }

    #[test]
    fn error_message_branches_hold() {
        assert_eq!(
            openrouter_error_message(&json!({"error": {"message": "boom"}})).as_deref(),
            Some("boom")
        );
        assert_eq!(
            openrouter_error_message(&json!({"message": "top"})).as_deref(),
            Some("top")
        );
        assert!(openrouter_error_message(&json!({"error": {"message": "  "}})).is_none());
        assert!(openrouter_error_message(&json!({"data": {}})).is_none());
    }

    #[test]
    fn zero_limit_preserved_for_cache_exhaustion_mapping() {
        // Parsing keeps a zero cap verbatim; the quota cache maps it to
        // exhausted (no normalization here).
        let body = json!({"data": {"limit": 0, "limit_remaining": 0, "usage": 0}});
        let parsed = parse_openrouter_key(&body).expect("key");
        assert_eq!(parsed.balance.limit, Some(0.0));
        assert_eq!(parsed.balance.limit_remaining, Some(0.0));
    }

    #[test]
    fn usage_only_key_parses_as_unlimited_with_spend() {
        // No limit keys but a usage signal still yields an unlimited shape
        // (limit None) with the spend snapshot attached.
        let body = json!({"data": {"usage": 3.5, "usage_daily": 0.5}});
        let parsed = parse_openrouter_key(&body).expect("key");
        assert_eq!(parsed.balance.limit, None);
        assert_eq!(parsed.spend.usage, 3.5);
        assert_eq!(parsed.spend.daily, 0.5);
        // A lone limit_reset carries no numeric signal and is rejected.
        assert!(parse_openrouter_key(&json!({"data": {"limit_reset": "monthly"}})).is_none());
    }

    #[test]
    fn credits_stringy_totals_coerce_and_limit_reset_numeric_coerces() {
        let credits = json!({"data": {"total_credits": "100.5", "total_usage": "25"}});
        assert_eq!(parse_openrouter_credits(&credits), Some((100.5, 25.0)));
        let key =
            json!({"data": {"limit": 10, "limit_remaining": 4, "limit_reset": 7, "usage": 1}});
        let parsed = parse_openrouter_key(&key).expect("key");
        assert_eq!(parsed.balance.limit_reset.as_deref(), Some("7"));
    }
}
