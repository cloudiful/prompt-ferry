use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;

/// Official Firecrawl account credit-usage endpoint (issue #296 Phase 3).
pub const FIRECRAWL_CREDIT_USAGE_URL: &str = "https://api.firecrawl.dev/v2/team/credit-usage";

/// Firecrawl balance lookups must never stall the refresh loop.
pub const FIRECRAWL_BALANCE_TIMEOUT: Duration = Duration::from_secs(10);

/// Sanitized provider-balance failure. Deliberately carries no token, request
/// URL, or raw response body so a failure can be logged or persisted as
/// `last_error` without leaking credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderBalanceError {
    Timeout,
    Http {
        status: Option<u16>,
    },
    UnsuccessfulResponse,
    MissingRemainingCredits,
    MalformedBody,
    /// The provider responded successfully but the durable balance could not
    /// be persisted. Kept distinct so callers can surface an internal error
    /// instead of a misleading provider failure.
    Persistence,
}

impl std::fmt::Display for ProviderBalanceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(formatter, "provider balance request timed out"),
            Self::Http {
                status: Some(status),
            } => {
                write!(
                    formatter,
                    "provider balance request failed with http {status}"
                )
            }
            Self::Http { status: None } => write!(formatter, "provider balance request failed"),
            Self::UnsuccessfulResponse => {
                write!(
                    formatter,
                    "provider balance response reported success=false"
                )
            }
            Self::MissingRemainingCredits => {
                write!(
                    formatter,
                    "provider balance response is missing remainingCredits"
                )
            }
            Self::MalformedBody => {
                write!(formatter, "provider balance response could not be parsed")
            }
            Self::Persistence => write!(formatter, "provider balance could not be persisted"),
        }
    }
}

impl std::error::Error for ProviderBalanceError {}

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderBalance {
    pub remaining: f64,
    pub plan_credits: Option<f64>,
    pub reset_at: Option<DateTime<Utc>>,
}

/// Parse the documented Firecrawl credit-usage payload. A missing or
/// non-success body is an error so the caller keeps the previous durable
/// balance instead of overwriting it with a fabricated zero.
pub fn parse_firecrawl_credit_usage(body: &str) -> Result<ProviderBalance, ProviderBalanceError> {
    let value: Value =
        serde_json::from_str(body).map_err(|_| ProviderBalanceError::MalformedBody)?;
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(ProviderBalanceError::UnsuccessfulResponse);
    }
    let data = value
        .get("data")
        .ok_or(ProviderBalanceError::MissingRemainingCredits)?;
    let remaining = data
        .get("remainingCredits")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or(ProviderBalanceError::MissingRemainingCredits)?;
    let plan_credits = data
        .get("planCredits")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0);
    let reset_at = data.get("billingPeriodEnd").and_then(parse_reset_at);
    Ok(ProviderBalance {
        remaining,
        plan_credits,
        reset_at,
    })
}

/// `billingPeriodEnd` is documented as an RFC3339 timestamp, but tolerate the
/// epoch (seconds or milliseconds) shape some deployments return.
fn parse_reset_at(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|value| value.with_timezone(&Utc));
    }
    let raw = value.as_i64()?;
    let seconds = if raw > 1_000_000_000_000 {
        raw / 1000
    } else {
        raw
    };
    DateTime::from_timestamp(seconds, 0)
}

/// Issue #375 Phase G: per-`(proxy_url, host)` pool for the Firecrawl
/// balance endpoint. The balance host is fixed
/// (`api.firecrawl.dev`), so the pool key is `(proxy, fixed host)`.
/// Direct stays the same 10s-timeout shape; proxy reuses a pooled 10s
/// client. Errors are redacted; invalid proxy fails closed.
static FIRECRAWL_BALANCE_POOL: OnceLock<Mutex<HashMap<(String, String), reqwest::Client>>> =
    OnceLock::new();

fn pool() -> &'static Mutex<HashMap<(String, String), reqwest::Client>> {
    FIRECRAWL_BALANCE_POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_pool() -> std::sync::MutexGuard<'static, HashMap<(String, String), reqwest::Client>> {
    pool()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn build_pooled_client(validated_proxy: &str) -> Result<reqwest::Client, String> {
    let redacted = crate::db::redact_proxy_url_for_log(validated_proxy);
    let proxy = reqwest::Proxy::all(validated_proxy)
        .map_err(|_| format!("invalid proxy {redacted}: unsupported proxy URL"))?;
    reqwest::Client::builder()
        .timeout(FIRECRAWL_BALANCE_TIMEOUT)
        .proxy(proxy)
        .build()
        .map_err(|_| format!("invalid proxy {redacted}: failed to build proxy client"))
}

/// Pooled balance client for an optional endpoint proxy. `None`/empty means
/// direct with the same timeout. The host is always the fixed Firecrawl
/// balance host so callers only pass the proxy.
pub fn pooled_client_for_proxy(proxy_url: Option<&str>) -> Result<reqwest::Client, String> {
    let Some(raw) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(reqwest::Client::builder()
            .timeout(FIRECRAWL_BALANCE_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new()));
    };
    let validated =
        crate::db::validate_outbound_proxy_url(raw).map_err(|message| message.to_string())?;
    let key = (
        validated.clone(),
        crate::db::proxy_base_host(FIRECRAWL_CREDIT_USAGE_URL),
    );
    {
        let guard = lock_pool();
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }
    let client = build_pooled_client(&validated)?;
    lock_pool().insert(key, client.clone());
    Ok(client)
}

/// Minimal reqwest client for the Firecrawl balance endpoint. The token is
/// only ever sent in the Authorization header and never formatted into an
/// error.
///
/// Issue #375 Phase G: `new`/`with_timeout` stay direct by design — balance
/// refreshes (scheduled loop, admin refresh) carry no endpoint proxy context
/// (Firecrawl is a hosted MCP provider, not an LLM endpoint), so they use
/// direct plus this comment. Endpoint-aware callers must build via
/// `pooled_client_for_proxy` + `from_client` so the endpoint proxy applies.
#[derive(Clone)]
pub struct FirecrawlBalanceClient {
    client: reqwest::Client,
}

impl FirecrawlBalanceClient {
    pub fn new() -> Self {
        Self::with_timeout(FIRECRAWL_BALANCE_TIMEOUT)
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }

    pub fn from_client(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Issue #375 Phase G: endpoint-proxy-aware constructor. `None`/empty
    /// stays direct; non-empty validates (fail-closed).
    pub fn from_endpoint_proxy(proxy_url: Option<&str>) -> Result<Self, String> {
        pooled_client_for_proxy(proxy_url).map(Self::from_client)
    }

    pub async fn fetch_balance(
        &self,
        token: &str,
    ) -> Result<ProviderBalance, ProviderBalanceError> {
        let response = self
            .client
            .get(FIRECRAWL_CREDIT_USAGE_URL)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|err| {
                if err.is_timeout() {
                    ProviderBalanceError::Timeout
                } else {
                    ProviderBalanceError::Http {
                        status: err.status().map(|status| status.as_u16()),
                    }
                }
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(ProviderBalanceError::Http {
                status: Some(status.as_u16()),
            });
        }
        let body = response
            .text()
            .await
            .map_err(|_| ProviderBalanceError::MalformedBody)?;
        parse_firecrawl_credit_usage(&body)
    }
}

impl Default for FirecrawlBalanceClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_successful_balance_with_optional_fields() {
        let balance = parse_firecrawl_credit_usage(
            r#"{
                "success": true,
                "data": {
                    "remainingCredits": 123.5,
                    "planCredits": 5000,
                    "billingPeriodEnd": "2026-10-01T00:00:00Z"
                }
            }"#,
        )
        .expect("valid balance");
        assert_eq!(balance.remaining, 123.5);
        assert_eq!(balance.plan_credits, Some(5000.0));
        assert_eq!(
            balance.reset_at,
            Some(
                DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc)
            )
        );
    }

    #[test]
    fn parses_successful_balance_without_optional_fields() {
        let balance =
            parse_firecrawl_credit_usage(r#"{"success":true,"data":{"remainingCredits":0}}"#)
                .expect("zero is a real balance, not an error");
        assert_eq!(balance.remaining, 0.0);
        assert_eq!(balance.plan_credits, None);
        assert_eq!(balance.reset_at, None);
    }

    #[test]
    fn accepts_epoch_seconds_and_millis_for_reset() {
        let seconds = parse_firecrawl_credit_usage(
            r#"{"success":true,"data":{"remainingCredits":1,"billingPeriodEnd":1798761600}}"#,
        )
        .unwrap()
        .reset_at
        .unwrap();
        assert_eq!(seconds.timestamp(), 1798761600);
        let millis = parse_firecrawl_credit_usage(
            r#"{"success":true,"data":{"remainingCredits":1,"billingPeriodEnd":1798761600000}}"#,
        )
        .unwrap()
        .reset_at
        .unwrap();
        assert_eq!(millis.timestamp(), 1798761600);
    }

    #[test]
    fn missing_remaining_credits_is_an_error() {
        assert_eq!(
            parse_firecrawl_credit_usage(r#"{"success":true,"data":{"planCredits":10}}"#),
            Err(ProviderBalanceError::MissingRemainingCredits)
        );
        assert_eq!(
            parse_firecrawl_credit_usage(r#"{"success":true,"data":{}}"#),
            Err(ProviderBalanceError::MissingRemainingCredits)
        );
        assert_eq!(
            parse_firecrawl_credit_usage(r#"{"success":true}"#),
            Err(ProviderBalanceError::MissingRemainingCredits)
        );
    }

    #[test]
    fn non_success_payload_is_an_error() {
        assert_eq!(
            parse_firecrawl_credit_usage(
                r#"{"success":false,"error":"invalid token","data":{"remainingCredits":0}}"#
            ),
            Err(ProviderBalanceError::UnsuccessfulResponse)
        );
        assert_eq!(
            parse_firecrawl_credit_usage(r#"{"data":{"remainingCredits":1}}"#),
            Err(ProviderBalanceError::UnsuccessfulResponse)
        );
    }

    #[test]
    fn malformed_body_is_an_error() {
        assert_eq!(
            parse_firecrawl_credit_usage("<html>gateway error</html>"),
            Err(ProviderBalanceError::MalformedBody)
        );
    }

    #[test]
    fn invalid_optional_reset_is_ignored() {
        let balance = parse_firecrawl_credit_usage(
            r#"{"success":true,"data":{"remainingCredits":5,"billingPeriodEnd":"not-a-date"}}"#,
        )
        .expect("invalid optional reset must not drop the balance");
        assert_eq!(balance.remaining, 5.0);
        assert_eq!(balance.reset_at, None);
    }

    #[test]
    fn empty_proxy_means_direct_balance_client() {
        for proxy in [None, Some(""), Some("   ")] {
            pooled_client_for_proxy(proxy).expect("direct must succeed");
            FirecrawlBalanceClient::from_endpoint_proxy(proxy).expect("direct must succeed");
        }
        assert!(crate::db::proxy_pool_key("", FIRECRAWL_CREDIT_USAGE_URL).is_none());
    }

    #[test]
    fn proxy_selection_reuses_pooled_balance_client() {
        let proxy = "http://proxy-firecrawl-reuse.test:8080";
        let via_a = pooled_client_for_proxy(Some(proxy)).expect("proxy must build");
        let via_a_again = pooled_client_for_proxy(Some(proxy)).expect("proxy must reuse");
        let _ = (via_a, via_a_again);
        let key = crate::db::proxy_pool_key(proxy, FIRECRAWL_CREDIT_USAGE_URL).unwrap();
        assert!(lock_pool().contains_key(&key));
        FirecrawlBalanceClient::from_endpoint_proxy(Some(proxy)).expect("ctor must reuse");
    }

    #[test]
    fn invalid_balance_proxy_is_rejected_without_userinfo_leak() {
        let err =
            pooled_client_for_proxy(Some("ftp://user:secret@proxy-firecrawl-invalid.test:21"))
                .expect_err("ftp must be rejected");
        assert!(err.contains("scheme"));
        assert!(!err.contains("secret"));
        assert!(!err.contains("user"));
    }

    #[test]
    fn error_display_never_contains_the_token() {
        let secret = "fc-super-secret-token";
        let errors = [
            ProviderBalanceError::Timeout,
            ProviderBalanceError::Http { status: Some(401) },
            ProviderBalanceError::Http { status: None },
            ProviderBalanceError::UnsuccessfulResponse,
            ProviderBalanceError::MissingRemainingCredits,
            ProviderBalanceError::MalformedBody,
            ProviderBalanceError::Persistence,
        ];
        for error in errors {
            let text = error.to_string();
            assert!(!text.contains(secret), "{text} must not leak the token");
        }
        // A response body containing a token must not echo it either.
        let leaky =
            parse_firecrawl_credit_usage(&format!(r#"{{"success":false,"error":"{secret}"}}"#))
                .unwrap_err()
                .to_string();
        assert!(!leaky.contains(secret), "{leaky} must not leak the token");
    }
}
