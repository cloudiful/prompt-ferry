//! OpenRouter balance fetcher (issue #203 P3): Bearer `GET /v1/key` first,
//! then `GET /v1/credits` for account totals. The base reuses the endpoint
//! normalized value (`https://openrouter.ai/api`, `/v1` appended here);
//! `/credits` needs a management key, so 403 (or any other failure)
//! silently degrades to `None` totals instead of failing the key.

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use super::json_scalars::{failed_key, truncate_message};
use super::openrouter_parsing::{
    openrouter_error_message, openrouter_http_business_error, parse_openrouter_credits,
    parse_openrouter_key,
};
use crate::worker_admin_types::TokenPlanKeyUsage;

// The endpoint stores the normalized `/api` base (no `/v1` suffix); the
// version path is appended here. A stale `/v1`-suffixed base is stripped
// defensively so the URL never doubles the segment.
fn key_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = base
        .strip_suffix("/v1")
        .unwrap_or(base)
        .trim_end_matches('/');
    format!("{base}/v1/key")
}

fn credits_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = base
        .strip_suffix("/v1")
        .unwrap_or(base)
        .trim_end_matches('/');
    format!("{base}/v1/credits")
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

// One key through `/key` then `/credits`. Only `/key` is fatal; `/credits`
// degrades to `None` totals on any failure (403 included). No balance
// arithmetic here: remaining-percent weighting lives in the quota cache.
// `base` is owned because it comes from the per-endpoint normalized value,
// unlike the fixed CommandCode/OpencodeGo hosts.
pub(crate) async fn fetch_openrouter_key_usage(
    client: Client,
    base: String,
    key_id: Uuid,
    key_label: String,
    secret: String,
) -> TokenPlanKeyUsage {
    let (status, body) = match get_json(&client, key_url(&base), &secret).await {
        Ok(ok) => ok,
        Err((status, message)) => {
            return failed_key(key_id, key_label, status, None, message);
        }
    };
    if !(200..300).contains(&status) {
        let (code, message) = match openrouter_http_business_error(status, &body) {
            Some((code, message)) => (code, message),
            None => (
                None,
                openrouter_error_message(&body)
                    .unwrap_or_else(|| format!("OpenRouter returned HTTP {status}")),
            ),
        };
        return failed_key(key_id, key_label, Some(status), code, message);
    }
    let mut parsed = match parse_openrouter_key(&body) {
        Some(parsed) => parsed,
        None => {
            let message = openrouter_error_message(&body)
                .unwrap_or_else(|| "OpenRouter returned an unrecognized key response".to_string());
            return failed_key(key_id, key_label, Some(status), None, message);
        }
    };
    if let Ok((credits_status, credits_body)) = get_json(&client, credits_url(&base), &secret).await
        && (200..300).contains(&credits_status)
        && let Some((total_credits, total_usage)) = parse_openrouter_credits(&credits_body)
    {
        parsed.balance.total_credits = Some(total_credits);
        parsed.balance.total_usage = Some(total_usage);
    }
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
        openrouter_balance: Some(parsed.balance),
        openrouter_spend: Some(parsed.spend),
        glm_five_hour: None,
        glm_weekly: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_credits_urls_append_v1_once() {
        assert_eq!(
            key_url("https://openrouter.ai/api"),
            "https://openrouter.ai/api/v1/key"
        );
        assert_eq!(
            key_url("https://openrouter.ai/api/"),
            "https://openrouter.ai/api/v1/key"
        );
        assert_eq!(
            key_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/key"
        );
        assert_eq!(
            credits_url("https://openrouter.ai/api"),
            "https://openrouter.ai/api/v1/credits"
        );
        assert_eq!(
            credits_url("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai/api/v1/credits"
        );
    }
}
