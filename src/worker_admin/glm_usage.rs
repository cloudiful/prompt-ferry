//! GLM/Zhipu Coding Plan usage fetcher (issue #230 P2).
//!
//! One Bearer `GET {base_origin}/api/monitor/usage/quota/limit` per key.
//! The configured `base_url` (e.g. `https://open.bigmodel.cn/api/coding/paas/v4`
//! or `https://api.z.ai/api/coding/paas/v4`) is reduced to its origin
//! (scheme + host + port) before the monitor path is appended, so the
//! quota endpoint is never accidentally re-rooted under the Coding Plan
//! path. Non-Coding-Plan keys get a bare `Unauthorized` body from the
//! upstream; the fetcher surfaces that as a transport-shaped failure so
//! the cache never silently fabricates an empty success.

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use super::glm_parsing::{glm_envelope_error, glm_http_business_error, parse_glm_quota};
use super::json_scalars::{failed_key, truncate_message};
use crate::worker_admin_types::TokenPlanKeyUsage;

const QUOTA_PATH: &str = "/api/monitor/usage/quota/limit";

/// Reduce the configured `base_url` to its origin (scheme + host + port)
/// only. A bare path is left empty; a missing scheme is treated as
/// unparseable so the caller can surface a configuration error.
pub(crate) fn glm_quota_origin(base_url: &str) -> Option<String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(parsed) = reqwest::Url::parse(trimmed) {
        let host = parsed.host_str()?;
        let port = parsed.port();
        let scheme = parsed.scheme();
        if port.is_some() {
            return Some(format!("{scheme}://{host}:{}", port.unwrap()));
        }
        return Some(format!("{scheme}://{host}"));
    }
    // Fallback manual parse: `<scheme>://<host>[:port]<path?>`. The
    // path/query/fragment are dropped, leaving only the origin.
    let without_scheme = trimmed.find("://").map(|idx| &trimmed[idx + 3..])?;
    let host_part = without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    let (host, port) = match host_part.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        _ => (host_part, None),
    };
    if host.is_empty() {
        return None;
    }
    let scheme_end = trimmed.find("://").unwrap_or(0);
    let scheme = &trimmed[..scheme_end];
    if let Some(port) = port {
        Some(format!("{scheme}://{host}:{port}"))
    } else {
        Some(format!("{scheme}://{host}"))
    }
}

fn quota_url(base_url: &str) -> Option<String> {
    let origin = glm_quota_origin(base_url)?;
    Some(format!("{origin}{QUOTA_PATH}"))
}

async fn get_json(
    client: &Client,
    url: String,
    secret: &str,
) -> std::result::Result<(u16, Value), (Option<u16>, Option<String>, String)> {
    let response = client
        .get(&url)
        .bearer_auth(secret)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|error| (None, None, truncate_message(error.to_string())))?;
    let status = response.status().as_u16();
    let body: Value = response
        .json()
        .await
        .map_err(|error| (Some(status), None, truncate_message(error.to_string())))?;
    Ok((status, body))
}

/// One key through the quota endpoint. The transport-level failure and
/// the 401/403/429 HTTP statuses are fatal; envelope-level errors
/// (non-zero `code`, success:false, plain string `error`) on a 200
/// response are also fatal. Empty / unparseable payloads degrade to a
/// `failed_key` so the cache never pads a success.
pub(crate) async fn fetch_glm_key_usage(
    client: Client,
    base: String,
    key_id: Uuid,
    key_label: String,
    secret: String,
) -> TokenPlanKeyUsage {
    let Some(url) = quota_url(&base) else {
        return failed_key(
            key_id,
            key_label,
            None,
            None,
            format!(
                "GLM base URL {base:?} is not a valid http(s) origin; expected a Coding Plan base like https://open.bigmodel.cn/api/coding/paas/v4"
            ),
        );
    };
    let (status, body) = match get_json(&client, url, &secret).await {
        Ok(ok) => ok,
        Err((status, code, message)) => {
            return failed_key(key_id, key_label, status, code, message);
        }
    };
    if !(200..300).contains(&status) {
        let (code, message) = match glm_http_business_error(status) {
            Some((code, message)) => (code, message),
            None => (None, format!("GLM returned HTTP {status}")),
        };
        return failed_key(key_id, key_label, Some(status), code, message);
    }
    if let Some((code, message)) = glm_envelope_error(&body) {
        return failed_key(key_id, key_label, Some(status), code, message);
    }
    match parse_glm_quota(&body, chrono::Utc::now()) {
        Some(parsed) => TokenPlanKeyUsage {
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
            glm_five_hour: parsed.five_hour,
            glm_weekly: parsed.weekly,
            deepseek_balance: None,
        },
        None => failed_key(
            key_id,
            key_label,
            Some(status),
            None,
            "GLM returned an unrecognized quota response (empty limits or missing fields)"
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quota_origin_strips_path_query_and_fragment() {
        assert_eq!(
            glm_quota_origin("https://open.bigmodel.cn/api/coding/paas/v4").as_deref(),
            Some("https://open.bigmodel.cn")
        );
        assert_eq!(
            glm_quota_origin("https://api.z.ai/api/coding/paas/v4").as_deref(),
            Some("https://api.z.ai")
        );
        assert_eq!(
            glm_quota_origin("https://open.bigmodel.cn/api/coding/paas/v4/").as_deref(),
            Some("https://open.bigmodel.cn")
        );
        assert_eq!(
            glm_quota_origin("http://127.0.0.1:8080/api/coding/paas/v4?x=1#frag").as_deref(),
            Some("http://127.0.0.1:8080")
        );
        // No scheme: treated as unparseable so the fetcher surfaces the
        // configuration error rather than silently building a bad URL.
        assert!(glm_quota_origin("open.bigmodel.cn/api/coding/paas/v4").is_none());
        assert!(glm_quota_origin("").is_none());
        assert!(glm_quota_origin("   ").is_none());
    }

    #[test]
    fn quota_url_appends_known_path() {
        assert_eq!(
            quota_url("https://open.bigmodel.cn/api/coding/paas/v4").as_deref(),
            Some("https://open.bigmodel.cn/api/monitor/usage/quota/limit")
        );
        assert_eq!(
            quota_url("https://api.z.ai/api/coding/paas/v4/").as_deref(),
            Some("https://api.z.ai/api/monitor/usage/quota/limit")
        );
        assert!(quota_url("not-a-url").is_none());
    }

    #[test]
    fn glm_http_business_error_messages_cover_401_403_429() {
        // 401/403/429 must map to provider-specific messages so the admin
        // UI can surface the credential-plan context (issue #230 P2).
        let (code, message) = glm_http_business_error(401).unwrap();
        assert_eq!(code.as_deref(), Some("401"));
        assert!(message.contains("GLM"));
        let (_, message) = glm_http_business_error(403).unwrap();
        assert!(message.contains("Coding Plan"));
        let (code, message) = glm_http_business_error(429).unwrap();
        assert_eq!(code.as_deref(), Some("429"));
        assert!(message.contains("retry"));
    }

    #[test]
    fn glm_envelope_error_rejects_success_false_and_code_nonzero() {
        let bad_code = json!({"code": 401, "msg": "Invalid token"});
        let (code, message) = glm_envelope_error(&bad_code).unwrap();
        assert_eq!(code.as_deref(), Some("401"));
        assert!(message.contains("Invalid token"));
        // success:false / ok:false surface without a code, so the fetcher
        // can still fail the key.
        assert!(glm_envelope_error(&json!({"success": false, "data": {}})).is_some());
        assert!(glm_envelope_error(&json!({"ok": false})).is_some());
        // Empty error string is not a business error.
        assert!(glm_envelope_error(&json!({"error": "  "})).is_none());
    }
}
