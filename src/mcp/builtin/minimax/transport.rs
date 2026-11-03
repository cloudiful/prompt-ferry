//! The outbound seam of the builtin MiniMax tools (issue #1179 P1).
//!
//! The transport is the test seam of the builtin path: production uses the
//! proxied HTTP client, and the tests drive an in-process stub, so the
//! selection, the failover and the feedback are exercised without an
//! upstream call. It adds no public base URL or environment knob.

use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

use super::SOURCE_HEADER;
use super::diagnostic::Diagnostic;

/// One upstream request.
pub(crate) struct MiniMaxRequest<'a> {
    pub path: &'a str,
    pub body: &'a Value,
    /// The bearer secret. It never reaches a log line or an error message.
    pub secret: &'a str,
}

/// One upstream response.
pub(crate) struct MiniMaxResponse {
    pub status: StatusCode,
    pub payload: Value,
    pub retry_after: Option<Duration>,
}

/// The outbound transport of the builtin tools.
#[async_trait::async_trait]
pub(crate) trait MiniMaxTransport: Send + Sync {
    async fn send(&self, request: MiniMaxRequest<'_>) -> anyhow::Result<MiniMaxResponse>;
}

/// The production transport: the endpoint proxied HTTP client.
pub(crate) struct HttpTransport {
    client: reqwest::Client,
    host: String,
}

impl HttpTransport {
    pub(crate) fn new(client: reqwest::Client, host: String) -> Self {
        Self { client, host }
    }
}

#[async_trait::async_trait]
impl MiniMaxTransport for HttpTransport {
    async fn send(&self, request: MiniMaxRequest<'_>) -> anyhow::Result<MiniMaxResponse> {
        let response = self
            .client
            .post(format!("{}{}", self.host, request.path))
            .bearer_auth(request.secret)
            .header("MM-API-Source", SOURCE_HEADER)
            .json(request.body)
            .send()
            .await
            .map_err(|_error| {
                // The request error may repeat the endpoint and the input, so
                // only the fixed diagnostic leaves this transport.
                Diagnostic::TransportFailed { status: None }.into_error()
            })?;
        let status = response.status();
        let retry_after = parse_retry_after(response.headers());
        // Issue #1179 P2 repair: a received non-success status and its
        // `Retry-After` survive a body that is unreadable or truncated, so a
        // rate limit is never downgraded to a generic transport failure. A
        // success status with an unreadable body fails the attempt instead of
        // counting as an empty success. The raw body and the read error never
        // reach the error text.
        let body = match response.bytes().await {
            Ok(body) => body,
            Err(_) if status.is_success() => {
                return Err(Diagnostic::TransportFailed {
                    status: Some(status.as_u16()),
                }
                .into_error());
            }
            Err(_) => {
                return Ok(MiniMaxResponse {
                    status,
                    payload: Value::Null,
                    retry_after,
                });
            }
        };
        let payload = match serde_json::from_slice::<Value>(&body) {
            Ok(payload) => payload,
            Err(_) if status.is_success() => {
                return Err(Diagnostic::TransportFailed {
                    status: Some(status.as_u16()),
                }
                .into_error());
            }
            // A non-success status is classified by the status itself, so an
            // unreadable error body must not hide it.
            Err(_) => json!({}),
        };
        Ok(MiniMaxResponse {
            status,
            payload,
            retry_after,
        })
    }
}

/// The `Retry-After` delay of a rate-limited response.
///
/// Only a single, well-formed value is honoured, and it is bounded to one
/// hour so a provider hint can never park a call for longer than the
/// feedback cooldown it feeds.
pub(super) fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    const MAX_RETRY_AFTER: Duration = Duration::from_secs(3600);
    let mut values = headers.get_all(reqwest::header::RETRY_AFTER).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let value = value.to_str().ok()?.trim();
    if value.is_empty() {
        return None;
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(Duration::from_secs(
            value
                .parse::<u64>()
                .unwrap_or(u64::MAX)
                .min(MAX_RETRY_AFTER.as_secs()),
        ));
    }
    if !value.ends_with(" GMT") {
        return None;
    }
    let deadline = chrono::DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&chrono::Utc);
    let remaining = deadline.signed_duration_since(chrono::Utc::now());
    if remaining <= chrono::Duration::zero() {
        return Some(Duration::ZERO);
    }
    Some(
        remaining
            .to_std()
            .unwrap_or(MAX_RETRY_AFTER)
            .min(MAX_RETRY_AFTER),
    )
}
