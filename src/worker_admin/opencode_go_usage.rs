//! OpencodeGo Zen usage fetcher (issue #193 P3), mirroring the CommandCode
//! fetcher. One Bearer GET to `/zen/go/v1/usage`; no balance arithmetic.

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use super::{json_scalars::truncate_message, opencode_go_parsing::*};
use crate::worker_admin_types::TokenPlanKeyUsage;

// Keep the cache call path stable after the parsing split.
pub(crate) use super::opencode_go_parsing::opencode_go_remaining_percent;

/// OpencodeGo Zen Go-usage base. The `/usage` path is appended below; the
/// endpoint carries this same base (see the admin UI) and we pin it here just
/// as CommandCode pins its production host. Revisit if a staging host appears.
pub(crate) const OPENCODE_GO_BASE: &str = "https://opencode.ai/zen/go/v1";

fn usage_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/usage")
}

/// One key through the single `/usage` call. Missing windows degrade to None;
/// only transport failure and the 401/403 business errors are fatal.
pub(crate) async fn fetch_opencode_go_key_usage(
    client: Client,
    base: &str,
    key_id: Uuid,
    key_label: String,
    secret: String,
) -> TokenPlanKeyUsage {
    let response = client
        .get(usage_url(base))
        .bearer_auth(secret)
        .header("Content-Type", "application/json")
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return failed_key(key_id, key_label, None, None, truncate_message(error.to_string()));
        }
    };
    let status = response.status().as_u16();
    let body: Value = match response.json().await {
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

    if !(200..300).contains(&status) {
        let (code, message) = match opencode_go_http_business_error(status) {
            Some((code, message)) => (code, message),
            None => {
                let message = opencode_go_business_error(&body)
                    .map(|(_, message)| message)
                    .unwrap_or_else(|| format!("OpencodeGo returned HTTP {status}"));
                (None, message)
            }
        };
        return failed_key(key_id, key_label, Some(status), code, message);
    }

    match parse_opencode_go_usage(&body) {
        Some(usage) => TokenPlanKeyUsage {
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
            opencodego_rolling: usage.rolling,
            opencodego_weekly: usage.weekly,
            opencodego_monthly: usage.monthly,
        },
        None => failed_key(
            key_id,
            key_label,
            Some(status),
            None,
            "OpencodeGo returned no recognized usage data".to_string(),
        ),
    }
}

fn failed_key(
    key_id: Uuid,
    key_label: String,
    status: Option<u16>,
    error_code: Option<String>,
    error_message: String,
) -> TokenPlanKeyUsage {
    super::json_scalars::failed_key(key_id, key_label, status, error_code, error_message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_url_appends_once_and_trims_trailing_slash() {
        assert_eq!(usage_url("https://opencode.ai/zen/go/v1"), "https://opencode.ai/zen/go/v1/usage");
        assert_eq!(
            usage_url("https://opencode.ai/zen/go/v1/"),
            "https://opencode.ai/zen/go/v1/usage"
        );
    }

    #[test]
    fn business_error_message_is_human_readable() {
        assert_eq!(
            opencode_go_http_business_error(401).unwrap().1,
            "OpencodeGo key is missing or invalid"
        );
        assert_eq!(
            opencode_go_http_business_error(403).unwrap().1,
            "OpencodeGo subscription is not available"
        );
        // Non-401/403 statuses fall through to no-coded business error.
        assert!(opencode_go_http_business_error(500).is_none());
    }

    #[test]
    fn compat_shape_is_accepted_by_parser() {
        let body = json!({
            "usage": {
                "rolling": {"usage_percent": 65, "resets_in_seconds": 2520},
                "weekly": {"usage_percent": 30, "resets_in_seconds": 259200}
            }
        });
        let usage = parse_opencode_go_usage(&body).expect("usage");
        assert_eq!(usage.rolling.as_ref().unwrap().percent, Some(65.0));
        assert!(usage.monthly.is_none());
    }
}
