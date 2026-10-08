//! DeepSeek balance fetcher (issue #287 P0): Bearer `GET /user/balance`
//! against the endpoint's normalized base. The stored base is normalized to
//! the official `https://api.deepseek.com` root by the preset helper, so the
//! `/user/balance` path is appended directly (DeepSeek has no `/v1` segment on
//! this endpoint).

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use super::deepseek_parsing::{
    deepseek_error_message, deepseek_http_business_error, parse_deepseek_balance,
};
use super::json_scalars::{failed_key, truncate_message};
use crate::worker_admin_types::TokenPlanKeyUsage;

// The endpoint stores the official root (no `/v1` suffix). A stale `/v1` suffix
// is stripped defensively so the URL never carries a version segment.
fn balance_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = base
        .strip_suffix("/v1")
        .unwrap_or(base)
        .trim_end_matches('/');
    format!("{base}/user/balance")
}

async fn get_json(
    client: &Client,
    url: String,
    secret: &str,
) -> std::result::Result<(u16, Value), (Option<u16>, String)> {
    let response = client
        .get(&url)
        .bearer_auth(secret)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|error| (None, truncate_message(error.to_string())))?;
    let status = response.status().as_u16();
    let body: Value = response
        .json()
        .await
        .map_err(|error| (Some(status), truncate_message(error.to_string())))?;
    Ok((status, body))
}

// One key through `/user/balance`. `base` is owned because it comes from the
// per-endpoint normalized value, unlike the fixed CommandCode/OpencodeGo
// hosts. No balance arithmetic here: `is_available` weighting lives in the
// quota cache.
pub(crate) async fn fetch_deepseek_key_usage(
    client: Client,
    base: String,
    key_id: Uuid,
    key_label: String,
    secret: String,
) -> TokenPlanKeyUsage {
    let (status, body) = match get_json(&client, balance_url(&base), &secret).await {
        Ok(ok) => ok,
        Err((status, message)) => {
            return failed_key(key_id, key_label, status, None, message);
        }
    };
    if !(200..300).contains(&status) {
        let (code, message) = match deepseek_http_business_error(status, &body) {
            Some((code, message)) => (code, message),
            None => (
                None,
                deepseek_error_message(&body)
                    .unwrap_or_else(|| format!("DeepSeek returned HTTP {status}")),
            ),
        };
        return failed_key(key_id, key_label, Some(status), code, message);
    }
    let parsed = match parse_deepseek_balance(&body) {
        Some(parsed) => parsed,
        None => {
            let message = deepseek_error_message(&body).unwrap_or_else(|| {
                "DeepSeek returned an unrecognized balance response".to_string()
            });
            return failed_key(key_id, key_label, Some(status), None, message);
        }
    };
    TokenPlanKeyUsage {
        key_id,
        key_label,
        ok: true,
        status: Some(status),
        error_code: None,
        error_message: None,
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
        deepseek_balance: Some(parsed.balance),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn balance_url_appends_path_once() {
        assert_eq!(
            balance_url("https://api.deepseek.com"),
            "https://api.deepseek.com/user/balance"
        );
        assert_eq!(
            balance_url("https://api.deepseek.com/"),
            "https://api.deepseek.com/user/balance"
        );
        // A stale version suffix is stripped defensively.
        assert_eq!(
            balance_url("https://api.deepseek.com/v1"),
            "https://api.deepseek.com/user/balance"
        );
    }

    #[test]
    fn multi_currency_body_carries_every_entry_in_order() {
        // Contract fixture for the #712 shape: two currencies in non-sorted,
        // mixed-case input normalize to a sorted, upper-cased list, and the
        // fetcher forwards the whole parsed balance untouched (no arithmetic,
        // no FX folding).
        let body = json!({
            "is_available": true,
            "balance_infos": [
                {"currency": "usd", "total_balance": "7.25"},
                {"currency": "CNY", "total_balance": "110.00",
                 "granted_balance": "10.00", "topped_up_balance": "100.00"}
            ]
        });
        let parsed = parse_deepseek_balance(&body).expect("balance");
        let currencies = parsed
            .balance
            .balances
            .iter()
            .map(|entry| entry.currency.as_str())
            .collect::<Vec<_>>();
        assert_eq!(currencies, vec!["CNY", "USD"]);
        assert!(parsed.balance.is_available);
        assert_eq!(parsed.balance.balances[1].total_balance, Some(7.25));
    }

    #[test]
    fn availability_signal_survives_without_currency_entries() {
        let parsed =
            parse_deepseek_balance(&json!({"is_available": false})).expect("flag-only balance");
        assert!(!parsed.balance.is_available);
        assert!(parsed.balance.balances.is_empty());
    }
}
