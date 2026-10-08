//! DeepSeek balance defensive parsers (issue #287 P0, multi-currency #712 P1).
//!
//! `GET /user/balance` returns
//! `{is_available, balance_infos: [{currency, total_balance, granted_balance,
//! topped_up_balance}]}`. The amount fields are decimal strings, so they are
//! coerced through the shared scalar helper; every parseable currency entry is
//! preserved (upper-cased, ordered by currency then amount) and a
//! missing/unparseable amount is `null` (unknown), never a fabricated zero.
//! Errors use `{error: {code, message, type, param}}`. A 402 `Insufficient
//! Balance` has no quota keyword of its own, so the business-error message
//! self-contains a `(quota)` marker for the shared quota-family classifier.

use std::cmp::Ordering;

use serde_json::Value;

use super::json_scalars::{truncate_message, value_as_f64, value_as_string};
use crate::worker_admin_types::{DeepSeekBalance, DeepSeekCurrencyBalance};

/// Parsed `/user/balance` payload.
#[derive(Debug, Clone)]
pub(crate) struct DeepSeekUsage {
    pub(crate) balance: DeepSeekBalance,
}

// HTTP-status business errors: 401 = missing/invalid key, 402 = insufficient
// balance (maps to the `quota` error code so quota-family logging and the
// routing failover path match without touching the shared whitelist), 429 =
// rate limited and passed through with retry semantics.
pub(crate) fn deepseek_http_business_error(
    status: u16,
    body: &Value,
) -> Option<(Option<String>, String)> {
    match status {
        401 => Some((
            Some("401".to_string()),
            "DeepSeek key is missing or invalid".to_string(),
        )),
        402 => Some((
            Some("quota".to_string()),
            format!(
                "DeepSeek insufficient balance (quota): {}",
                deepseek_error_message(body).unwrap_or_else(|| "Insufficient Balance".to_string())
            ),
        )),
        429 => Some((
            Some("429".to_string()),
            deepseek_error_message(body)
                .unwrap_or_else(|| "DeepSeek rate limited (retry later)".to_string()),
        )),
        _ => None,
    }
}

// Server error detail: `{error: {message}}` with a top-level `message`
// fallback. Blank messages are ignored so callers fall back to defaults.
pub(crate) fn deepseek_error_message(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| body.get("message"))
        .and_then(value_as_string)
        .map(|message| message.trim().to_string())
        .filter(|message| !message.is_empty())
        .map(truncate_message)
}

// One `balance_infos` entry: `currency` must be a non-empty string and is
// normalized to upper case so the same currency under different casings sorts
// and merges deterministically. Missing/non-numeric amounts stay `None`
// (unknown) — never a fabricated zero.
fn parse_balance_info(value: &Value) -> Option<DeepSeekCurrencyBalance> {
    let object = value.as_object()?;
    let currency = object
        .get("currency")
        .and_then(value_as_string)
        .map(|currency| currency.trim().to_uppercase())
        .filter(|currency| !currency.is_empty())?;
    Some(DeepSeekCurrencyBalance {
        currency,
        total_balance: parse_amount(object.get("total_balance")),
        granted_balance: parse_amount(object.get("granted_balance")),
        topped_up_balance: parse_amount(object.get("topped_up_balance")),
    })
}

fn parse_amount(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(value_as_f64)
        .filter(|amount| amount.is_finite())
}

// Total order over one amount: unknown (`None`) sorts before every known
// value, and `total_cmp` keeps the comparison total for the two-sided case.
fn compare_amounts(left: &Option<f64>, right: &Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.total_cmp(right),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

// Entry order: currency first, then each amount. Same-currency entries are
// therefore ordered by their reported values rather than by the position the
// provider listed them in, so a repeated currency keeps every parseable entry
// in one deterministic order.
fn compare_entries(left: &DeepSeekCurrencyBalance, right: &DeepSeekCurrencyBalance) -> Ordering {
    left.currency
        .cmp(&right.currency)
        .then_with(|| compare_amounts(&left.total_balance, &right.total_balance))
        .then_with(|| compare_amounts(&left.granted_balance, &right.granted_balance))
        .then_with(|| compare_amounts(&left.topped_up_balance, &right.topped_up_balance))
}

// `/user/balance`: `is_available` plus every parseable currency entry, ordered
// by currency and then by amount so repeated currencies stay complete and
// deterministic. Returns `None` when the body carries no recognizable balance
// signal (never padded). `is_available` present without any balance entry
// still yields a signal because the flag alone drives routing weight.
pub(crate) fn parse_deepseek_balance(body: &Value) -> Option<DeepSeekUsage> {
    let is_available = body.get("is_available").and_then(Value::as_bool);
    let mut balances: Vec<DeepSeekCurrencyBalance> = body
        .get("balance_infos")
        .and_then(Value::as_array)
        .map(|infos| {
            infos
                .iter()
                .filter_map(parse_balance_info)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    balances.sort_by(compare_entries);
    if is_available.is_none() && balances.is_empty() {
        return None;
    }
    Some(DeepSeekUsage {
        balance: DeepSeekBalance {
            is_available: is_available.unwrap_or(false),
            balances,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn balance_fixture() -> Value {
        json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "CNY",
                "total_balance": "110.00",
                "granted_balance": "10.00",
                "topped_up_balance": "100.00"
            }]
        })
    }

    #[test]
    fn balance_shape_parses_string_amounts() {
        let parsed = parse_deepseek_balance(&balance_fixture()).expect("balance");
        assert!(parsed.balance.is_available);
        assert_eq!(parsed.balance.balances.len(), 1);
        let entry = &parsed.balance.balances[0];
        assert_eq!(entry.currency, "CNY");
        assert_eq!(entry.total_balance, Some(110.0));
        assert_eq!(entry.granted_balance, Some(10.0));
        assert_eq!(entry.topped_up_balance, Some(100.0));
    }

    #[test]
    fn unavailable_flag_is_preserved() {
        let body = json!({
            "is_available": false,
            "balance_infos": [{
                "currency": "USD",
                "total_balance": 0,
                "granted_balance": 0,
                "topped_up_balance": 0
            }]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        assert!(!parsed.balance.is_available);
        assert_eq!(parsed.balance.balances.len(), 1);
        // A reported zero stays a real zero, distinct from unknown.
        assert_eq!(parsed.balance.balances[0].total_balance, Some(0.0));
    }

    #[test]
    fn numeric_amounts_parse_and_missing_fields_stay_unknown() {
        let body = json!({
            "is_available": true,
            "balance_infos": [{"currency": "CNY", "total_balance": 12.5}]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        assert_eq!(parsed.balance.balances.len(), 1);
        assert_eq!(parsed.balance.balances[0].total_balance, Some(12.5));
        assert_eq!(parsed.balance.balances[0].granted_balance, None);
        assert_eq!(parsed.balance.balances[0].topped_up_balance, None);
    }

    #[test]
    fn flag_without_entries_still_yields_signal() {
        let parsed =
            parse_deepseek_balance(&json!({"is_available": true})).expect("flag-only balance");
        assert!(parsed.balance.is_available);
        assert!(parsed.balance.balances.is_empty());
    }

    #[test]
    fn garbage_bodies_are_rejected() {
        assert!(parse_deepseek_balance(&json!({})).is_none());
        assert!(parse_deepseek_balance(&json!({"balance_infos": []})).is_none());
        assert!(
            parse_deepseek_balance(&json!({"balance_infos": [{"total_balance": "1"}]})).is_none()
        );
        assert!(parse_deepseek_balance(&json!({"is_available": "yes"})).is_none());
        assert!(parse_deepseek_balance(&json!([])).is_none());
    }

    #[test]
    fn all_parseable_currencies_are_preserved_and_sorted() {
        let body = json!({
            "is_available": true,
            "balance_infos": [
                {"currency": "cny", "total_balance": "110.00"},
                {"currency": "USD", "total_balance": "7.25"},
                {"currency": "  ", "total_balance": "1"},
                {"total_balance": "1"}
            ]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        let currencies = parsed
            .balance
            .balances
            .iter()
            .map(|entry| entry.currency.as_str())
            .collect::<Vec<_>>();
        // Unparseable entries are dropped, identifiers are upper-cased, and
        // the order is deterministic.
        assert_eq!(currencies, vec!["CNY", "USD"]);
        assert!(parsed.balance.is_available);
    }

    #[test]
    fn repeated_currency_keeps_every_entry_in_a_deterministic_order() {
        // Every parseable entry of a repeated currency survives, ordered by the
        // reported amounts rather than by the position the provider listed them
        // in, and the same set in a different input order yields one order.
        let entries = |infos: Value| {
            let body = json!({"is_available": true, "balance_infos": infos});
            parse_deepseek_balance(&body)
                .expect("balance")
                .balance
                .balances
        };
        let totals = |infos: Value| {
            entries(infos)
                .iter()
                .map(|entry| entry.total_balance)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            totals(json!([
                {"currency": "usd", "total_balance": "2"},
                {"currency": "USD", "total_balance": "1"},
                {"currency": "usd", "total_balance": "3"}
            ])),
            vec![Some(1.0), Some(2.0), Some(3.0)]
        );
        // Unknown amounts sort before reported ones, so a real zero is never
        // pushed out by a `null` sibling.
        assert_eq!(
            totals(json!([
                {"currency": "CNY", "total_balance": "1"},
                {"currency": "CNY", "total_balance": "n/a"},
                {"currency": "CNY", "granted_balance": "3"}
            ])),
            vec![None, None, Some(1.0)]
        );
        assert_eq!(
            entries(json!([
                {"currency": "CNY", "total_balance": "1"},
                {"currency": "CNY", "total_balance": "2"},
                {"currency": "usd", "total_balance": "5"}
            ])),
            entries(json!([
                {"currency": "usd", "total_balance": "5"},
                {"currency": "CNY", "total_balance": "2"},
                {"currency": "CNY", "total_balance": "1"}
            ]))
        );
    }

    #[test]
    fn unparseable_amounts_are_unknown_not_zero() {
        let body = json!({
            "is_available": true,
            "balance_infos": [{
                "currency": "CNY",
                "total_balance": "n/a",
                "granted_balance": null
            }]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        assert_eq!(parsed.balance.balances.len(), 1);
        assert_eq!(parsed.balance.balances[0].total_balance, None);
        assert_eq!(parsed.balance.balances[0].granted_balance, None);
    }

    #[test]
    fn currency_entry_normalizes_identifier() {
        let entry =
            parse_balance_info(&json!({"currency": " cny ", "total_balance": "1"})).expect("entry");
        assert_eq!(
            entry,
            DeepSeekCurrencyBalance {
                currency: "CNY".to_string(),
                total_balance: Some(1.0),
                granted_balance: None,
                topped_up_balance: None,
            }
        );
    }

    #[test]
    fn business_errors_distinguish_status_codes() {
        let body = json!({"error": {"message": "Authentication Fails"}});
        assert_eq!(
            deepseek_http_business_error(401, &body)
                .unwrap()
                .0
                .as_deref(),
            Some("401")
        );
        let insufficient = json!({
            "error": {
                "message": "Insufficient Balance",
                "type": "unknown_error",
                "code": "invalid_request_error"
            }
        });
        let (code, message) = deepseek_http_business_error(402, &insufficient).expect("402");
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
        let limited = json!({"error": {"message": "Rate limit reached"}});
        let (code, message) = deepseek_http_business_error(429, &limited).expect("429");
        assert_eq!(code.as_deref(), Some("429"));
        assert_eq!(message, "Rate limit reached");
        let (code, message) = deepseek_http_business_error(429, &json!({})).expect("429 default");
        assert_eq!(code.as_deref(), Some("429"));
        assert!(message.contains("retry"));
        assert!(deepseek_http_business_error(500, &body).is_none());
    }

    #[test]
    fn error_message_branches_hold() {
        assert_eq!(
            deepseek_error_message(&json!({"error": {"message": "boom"}})).as_deref(),
            Some("boom")
        );
        assert_eq!(
            deepseek_error_message(&json!({"message": "top"})).as_deref(),
            Some("top")
        );
        assert!(deepseek_error_message(&json!({"error": {"message": "  "}})).is_none());
        assert!(deepseek_error_message(&json!({"data": {}})).is_none());
    }
}
