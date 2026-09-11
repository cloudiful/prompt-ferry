//! DeepSeek balance defensive parsers (issue #287 P0).
//!
//! `GET /user/balance` returns
//! `{is_available, balance_infos: [{currency, total_balance, granted_balance,
//! topped_up_balance}]}`. The amount fields are decimal strings, so they are
//! coerced through the shared scalar helper. Errors use
//! `{error: {code, message, type, param}}`. A 402 `Insufficient Balance` has
//! no quota keyword of its own, so the business-error message self-contains a
//! `(quota)` marker for the shared quota-family classifier.

use serde_json::Value;

use super::json_scalars::{truncate_message, value_as_f64, value_as_string};
use crate::worker_admin_types::DeepSeekBalance;

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

// One `balance_infos` entry: `currency` must be a non-empty string, the three
// amount fields degrade to zero when missing/non-numeric.
fn parse_balance_info(value: &Value) -> Option<DeepSeekBalance> {
    let object = value.as_object()?;
    let currency = object
        .get("currency")
        .and_then(value_as_string)
        .map(|currency| currency.trim().to_string())
        .filter(|currency| !currency.is_empty())?;
    Some(DeepSeekBalance {
        is_available: false,
        currency,
        total_balance: object
            .get("total_balance")
            .and_then(value_as_f64)
            .unwrap_or(0.0),
        granted_balance: object
            .get("granted_balance")
            .and_then(value_as_f64)
            .unwrap_or(0.0),
        topped_up_balance: object
            .get("topped_up_balance")
            .and_then(value_as_f64)
            .unwrap_or(0.0),
    })
}

// `/user/balance`: `is_available` plus the first parseable currency entry.
// Returns `None` when the body carries no recognizable balance signal (never
// padded). `is_available` present without a balance entry still yields a
// signal because the flag alone drives routing weight.
pub(crate) fn parse_deepseek_balance(body: &Value) -> Option<DeepSeekUsage> {
    let is_available = body.get("is_available").and_then(Value::as_bool);
    let info = body
        .get("balance_infos")
        .and_then(Value::as_array)
        .and_then(|infos| infos.iter().find_map(parse_balance_info));
    if is_available.is_none() && info.is_none() {
        return None;
    }
    let mut balance = info.unwrap_or(DeepSeekBalance {
        is_available: false,
        currency: String::new(),
        total_balance: 0.0,
        granted_balance: 0.0,
        topped_up_balance: 0.0,
    });
    balance.is_available = is_available.unwrap_or(false);
    Some(DeepSeekUsage { balance })
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
        assert_eq!(parsed.balance.currency, "CNY");
        assert_eq!(parsed.balance.total_balance, 110.0);
        assert_eq!(parsed.balance.granted_balance, 10.0);
        assert_eq!(parsed.balance.topped_up_balance, 100.0);
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
        assert_eq!(parsed.balance.currency, "USD");
        assert_eq!(parsed.balance.total_balance, 0.0);
    }

    #[test]
    fn numeric_amounts_and_missing_fields_degrade() {
        let body = json!({
            "is_available": true,
            "balance_infos": [{"currency": "CNY", "total_balance": 12.5}]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        assert_eq!(parsed.balance.total_balance, 12.5);
        assert_eq!(parsed.balance.granted_balance, 0.0);
        assert_eq!(parsed.balance.topped_up_balance, 0.0);
    }

    #[test]
    fn flag_without_entries_still_yields_signal() {
        let parsed =
            parse_deepseek_balance(&json!({"is_available": true})).expect("flag-only balance");
        assert!(parsed.balance.is_available);
        assert!(parsed.balance.currency.is_empty());
        assert_eq!(parsed.balance.total_balance, 0.0);
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
    fn first_parseable_currency_entry_wins() {
        let body = json!({
            "is_available": true,
            "balance_infos": [
                {"currency": "  ", "total_balance": "1"},
                {"currency": "USD", "total_balance": "7.25"}
            ]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        assert_eq!(parsed.balance.currency, "USD");
        assert_eq!(parsed.balance.total_balance, 7.25);
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
