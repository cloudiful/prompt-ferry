use std::time::Duration;

use anyhow::{Result, anyhow};
use chrono::{DateTime, TimeZone, Utc};
use futures::{StreamExt, stream};
use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    db::{EndpointProvider, EndpointRegion, ProviderEndpoint},
    worker_admin_types::{
        TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse, TokenPlanWindowUsage,
    },
};

const MINIMAX_CN_USAGE_URL: &str = "https://www.minimaxi.com/v1/token_plan/remains";
const MINIMAX_GLOBAL_USAGE_URL: &str = "https://www.minimax.io/v1/token_plan/remains";
const MAX_CONCURRENT_KEY_REQUESTS: usize = 4;

pub async fn fetch_endpoint_usage(endpoint: &ProviderEndpoint) -> Result<TokenPlanUsageResponse> {
    let region = match endpoint.provider {
        EndpointProvider::Minimax => endpoint
            .provider_region
            .ok_or_else(|| anyhow!("MiniMax endpoint has no provider region"))?,
        EndpointProvider::Generic => {
            return Err(anyhow!("endpoint provider has no token plan API"));
        }
    };

    let keys = endpoint
        .api_keys
        .iter()
        .filter(|key| key.enabled && !key.api_key.trim().is_empty())
        .map(|key| (key.key_id, key.key_label.clone(), key.api_key.clone()))
        .collect::<Vec<_>>();
    let keys = if keys.is_empty() && !endpoint.api_key.trim().is_empty() {
        vec![(Uuid::nil(), endpoint.name.clone(), endpoint.api_key.clone())]
    } else {
        keys
    };
    if keys.is_empty() {
        return Err(anyhow!("endpoint has no enabled API key"));
    }

    let client = Client::builder().timeout(Duration::from_secs(8)).build()?;
    let url = usage_url(region);
    let key_results = stream::iter(keys.into_iter().map(|(key_id, key_label, secret)| {
        fetch_key_usage(client.clone(), url, key_id, key_label, secret)
    }))
    .buffer_unordered(MAX_CONCURRENT_KEY_REQUESTS)
    .collect::<Vec<_>>()
    .await;

    Ok(TokenPlanUsageResponse {
        provider: EndpointProvider::Minimax,
        provider_region: region,
        keys: key_results,
    })
}

fn usage_url(region: EndpointRegion) -> &'static str {
    match region {
        EndpointRegion::Cn => MINIMAX_CN_USAGE_URL,
        EndpointRegion::Global => MINIMAX_GLOBAL_USAGE_URL,
    }
}

async fn fetch_key_usage(
    client: Client,
    url: &'static str,
    key_id: Uuid,
    key_label: String,
    secret: String,
) -> TokenPlanKeyUsage {
    let response = client
        .get(url)
        .bearer_auth(secret)
        .header("Content-Type", "application/json")
        .send()
        .await;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return failed_key(
                key_id,
                key_label,
                None,
                None,
                truncate_message(error.to_string()),
            );
        }
    };
    let status = response.status().as_u16();
    let body = match response.json::<Value>().await {
        Ok(body) => body,
        Err(error) => {
            return failed_key(
                key_id,
                key_label,
                Some(status),
                None,
                truncate_message(error.to_string()),
            );
        }
    };

    if !((200..300).contains(&status)) {
        let (error_code, error_message) = response_error(&body);
        return failed_key(
            key_id,
            key_label,
            Some(status),
            error_code,
            error_message.unwrap_or_else(|| format!("MiniMax returned HTTP {status}")),
        );
    }

    match parse_usage_body(&body) {
        Ok(model_remains) => TokenPlanKeyUsage {
            key_id,
            key_label,
            ok: true,
            status: Some(status),
            error_code: None,
            error_message: None,
            model_remains,
        },
        Err((error_code, error_message)) => {
            failed_key(key_id, key_label, Some(status), error_code, error_message)
        }
    }
}

fn parse_usage_body(
    body: &Value,
) -> std::result::Result<Vec<TokenPlanModelUsage>, (Option<String>, String)> {
    let base_resp = body
        .get("base_resp")
        .or_else(|| body.get("data").and_then(|data| data.get("base_resp")));
    if let Some(base_resp) = base_resp
        && let Some(status_code) = base_resp.get("status_code").and_then(value_as_i64)
        && status_code != 0
    {
        let message = base_resp
            .get("status_msg")
            .and_then(value_as_string)
            .unwrap_or_else(|| "MiniMax rejected the credential".to_string());
        return Err((Some(status_code.to_string()), truncate_message(message)));
    }

    let remains = body
        .get("model_remains")
        .or_else(|| body.get("data").and_then(|data| data.get("model_remains")))
        .and_then(Value::as_array)
        .ok_or_else(|| (None, "MiniMax response has no model_remains".to_string()))?;
    if remains.is_empty() {
        return Err((None, "MiniMax response has no model_remains".to_string()));
    }

    let models = remains
        .iter()
        .filter_map(parse_model_usage)
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err((
            None,
            "MiniMax response has no valid model remains".to_string(),
        ));
    }
    Ok(models)
}

fn parse_model_usage(value: &Value) -> Option<TokenPlanModelUsage> {
    let object = value.as_object()?;
    let model_name = object
        .get("model_name")
        .and_then(value_as_string)
        .unwrap_or_else(|| "unknown".to_string());
    Some(TokenPlanModelUsage {
        model_name,
        interval: parse_window(object, WindowKind::Interval),
        weekly: parse_window(object, WindowKind::Weekly),
    })
}

#[derive(Clone, Copy)]
enum WindowKind {
    Interval,
    Weekly,
}

fn parse_window(
    object: &serde_json::Map<String, Value>,
    kind: WindowKind,
) -> Option<TokenPlanWindowUsage> {
    let (
        status_key,
        remaining_key,
        total_key,
        usage_key,
        start_key,
        end_key,
        remains_key,
        boost_keys,
    ) = match kind {
        WindowKind::Interval => (
            "current_interval_status",
            "current_interval_remaining_percent",
            "current_interval_total_count",
            "current_interval_usage_count",
            "start_time",
            "end_time",
            "remains_time",
            [
                "current_interval_boost_permille",
                "interval_boost_permille",
                "interval_boost_permill",
            ],
        ),
        WindowKind::Weekly => (
            "current_weekly_status",
            "current_weekly_remaining_percent",
            "current_weekly_total_count",
            "current_weekly_usage_count",
            "weekly_start_time",
            "weekly_end_time",
            "weekly_remains_time",
            [
                "current_weekly_boost_permille",
                "weekly_boost_permille",
                "weekly_boost_permill",
            ],
        ),
    };

    let has_value = [
        status_key,
        remaining_key,
        total_key,
        usage_key,
        start_key,
        end_key,
        remains_key,
    ]
    .into_iter()
    .any(|key| object.contains_key(key));
    if !has_value {
        return None;
    }

    Some(TokenPlanWindowUsage {
        status: object
            .get(status_key)
            .and_then(value_as_i64)
            .and_then(|value| i32::try_from(value).ok()),
        remaining_percent: object.get(remaining_key).and_then(value_as_f64),
        total_count: object.get(total_key).and_then(value_as_i64),
        usage_count: object.get(usage_key).and_then(value_as_i64),
        boost_permille: boost_keys
            .into_iter()
            .find_map(|key| object.get(key).and_then(value_as_i64)),
        start_at: object
            .get(start_key)
            .and_then(value_as_i64)
            .and_then(epoch_millis),
        end_at: object
            .get(end_key)
            .and_then(value_as_i64)
            .and_then(epoch_millis),
        remains_time_ms: object.get(remains_key).and_then(value_as_i64),
    })
}

fn response_error(body: &Value) -> (Option<String>, Option<String>) {
    let base_resp = body
        .get("base_resp")
        .or_else(|| body.get("data").and_then(|data| data.get("base_resp")));
    let error_code = base_resp
        .and_then(|value| value.get("status_code"))
        .and_then(value_as_string);
    let message = base_resp
        .and_then(|value| value.get("status_msg"))
        .and_then(value_as_string)
        .or_else(|| body.get("message").and_then(value_as_string))
        .or_else(|| body.get("error").and_then(value_as_string));
    (error_code, message.map(truncate_message))
}

fn failed_key(
    key_id: Uuid,
    key_label: String,
    status: Option<u16>,
    error_code: Option<String>,
    error_message: String,
) -> TokenPlanKeyUsage {
    TokenPlanKeyUsage {
        key_id,
        key_label,
        ok: false,
        status,
        error_code,
        error_message: Some(error_message),
        model_remains: Vec::new(),
    }
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value as i64))
        .or_else(|| value.as_str()?.parse().ok())
}

fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|value| value as f64))
        .or_else(|| value.as_str()?.parse().ok())
}

fn value_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value_as_i64(value).map(|value| value.to_string()))
}

fn epoch_millis(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value).single()
}

fn truncate_message(message: String) -> String {
    const MAX_MESSAGE_LENGTH: usize = 300;
    if message.chars().count() <= MAX_MESSAGE_LENGTH {
        return message;
    }
    message.chars().take(MAX_MESSAGE_LENGTH).collect::<String>() + "..."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimax_percent_and_epoch_fields() {
        let body = serde_json::json!({
            "base_resp": { "status_code": "0" },
            "model_remains": [{
                "model_name": "general",
                "current_interval_status": 1,
                "current_interval_remaining_percent": "96",
                "current_interval_total_count": 0,
                "current_interval_usage_count": 0,
                "start_time": 1780279200000_i64,
                "end_time": 1780297200000_i64,
                "current_weekly_status": 1,
                "current_weekly_remaining_percent": 99,
                "weekly_start_time": 1780243200000_i64,
                "weekly_end_time": 1780848000000_i64,
                "weekly_boost_permill": 1500
            }]
        });

        let models = parse_usage_body(&body).expect("valid response");
        assert_eq!(models[0].model_name, "general");
        assert_eq!(
            models[0].interval.as_ref().unwrap().remaining_percent,
            Some(96.0)
        );
        assert_eq!(
            models[0].weekly.as_ref().unwrap().boost_permille,
            Some(1500)
        );
        assert!(models[0].interval.as_ref().unwrap().end_at.is_some());
    }

    #[test]
    fn rejects_minimax_business_error_even_on_http_200() {
        let body = serde_json::json!({
            "base_resp": { "status_code": 1004, "status_msg": "invalid api key" },
            "model_remains": []
        });

        let error = parse_usage_body(&body).expect_err("business error");
        assert_eq!(error.0.as_deref(), Some("1004"));
        assert_eq!(error.1, "invalid api key");
    }

    #[test]
    fn maps_regions_to_fixed_minimax_hosts() {
        assert_eq!(usage_url(EndpointRegion::Cn), MINIMAX_CN_USAGE_URL);
        assert_eq!(usage_url(EndpointRegion::Global), MINIMAX_GLOBAL_USAGE_URL);
    }
}
