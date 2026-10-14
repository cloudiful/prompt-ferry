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

/// Finite-only scalar for quota parsers (issue #844 P1). Accepts the same
/// numeric/string shapes as [`value_as_f64`] but rejects `inf`, `-inf`,
/// and `NaN` (including overflow strings such as `"1e400"` that parse to
/// infinity) so malformed totals stay unknown instead of manufacturing a
/// percentage. The global [`value_as_f64`] keeps its existing non-quota
/// clients unchanged.
pub(crate) fn finite_value_as_f64(value: &Value) -> Option<f64> {
    value_as_f64(value).filter(|v| v.is_finite())
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
        quota_status: None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finite_helper_rejects_nonfinite_quota_scalars() {
        assert_eq!(finite_value_as_f64(&json!(10.0)), Some(10.0));
        assert_eq!(finite_value_as_f64(&json!("80")), Some(80.0));
        assert_eq!(finite_value_as_f64(&json!(0.0)), Some(0.0));
        for raw in [
            json!("inf"),
            json!("-inf"),
            json!("NaN"),
            json!("infinity"),
            json!("1e400"),
        ] {
            assert_eq!(
                finite_value_as_f64(&raw),
                None,
                "non-finite quota scalar {raw} stays unknown"
            );
        }
        // The global helper keeps its legacy shape for non-quota clients.
        assert!(value_as_f64(&json!("inf")).is_some());
    }
}
