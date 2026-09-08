//! Shared JSON scalar helpers for token-plan fetchers (issue #184 P3).

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::worker_admin_types::TokenPlanKeyUsage;

pub(crate) fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))
        .or_else(|| value.as_f64().map(|v| v as i64))
        .or_else(|| value.as_str()?.parse().ok())
}

pub(crate) fn value_as_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_i64().map(|v| v as f64))
        .or_else(|| value.as_str()?.parse().ok())
}

pub(crate) fn value_as_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value_as_i64(value).map(|v| v.to_string()))
}

pub(crate) fn epoch_millis(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value).single()
}

pub(crate) fn truncate_message(message: String) -> String {
    const MAX: usize = 300;
    if message.chars().count() <= MAX {
        return message;
    }
    message.chars().take(MAX).collect::<String>() + "..."
}

pub(crate) fn failed_key(
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
        balances: None,
        five_hour: None,
        weekly: None,
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
        openrouter_balance: None,
        openrouter_spend: None,
    }
}
