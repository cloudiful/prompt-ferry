mod client;
mod token_selection;
mod tool_headers;

use std::{cell::RefCell, sync::LazyLock, time::Duration};

use reqwest::StatusCode;
use rmcp::transport::streamable_http_client::StreamableHttpError;
use serde_json::Value;
use tracing::warn;

use crate::db::McpServer;

use self::token_selection::{McpBearerTokenBalancer, SelectedToken};

static TOKEN_BALANCER: LazyLock<McpBearerTokenBalancer> =
    LazyLock::new(McpBearerTokenBalancer::new);
tokio::task_local! {
    static TOKEN_SLOT_TRACKER: RefCell<Option<i16>>;
    static CREDITS_USED_TRACKER: RefCell<Option<f64>>;
    static UPSTREAM_FAILURE_TRACKER: RefCell<Option<(i16, u16)>>;
}

/// Run `future` inside the token slot and credits-used tracking scopes. The
/// worker reads the tracked values after the MCP request completes.
pub(crate) async fn with_tracked_credits<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    with_tracked_token_slot(async {
        UPSTREAM_FAILURE_TRACKER
            .scope(RefCell::new(None), async {
                CREDITS_USED_TRACKER.scope(RefCell::new(None), future).await
            })
            .await
    })
    .await
}

pub(crate) fn tracked_credits_used() -> Option<f64> {
    CREDITS_USED_TRACKER
        .try_with(|tracker| *tracker.borrow())
        .ok()
        .flatten()
}

/// The (one-based token slot, HTTP status) of the last retryable upstream
/// failure, when the upstream itself rejected the request. `None` when the
/// upstream call succeeded or failed for non-retryable reasons.
pub(crate) fn tracked_upstream_failure() -> Option<(i16, u16)> {
    UPSTREAM_FAILURE_TRACKER
        .try_with(|tracker| *tracker.borrow())
        .ok()
        .flatten()
}

fn record_credits_used(credits: f64) {
    let _ = CREDITS_USED_TRACKER.try_with(|tracker| {
        let current = tracker.borrow().unwrap_or(0.0);
        *tracker.borrow_mut() = Some(current.max(credits));
    });
}

fn record_upstream_failure(slot: Option<i16>, status: u16) {
    let Some(slot) = slot else {
        return;
    };
    let _ = UPSTREAM_FAILURE_TRACKER.try_with(|tracker| {
        *tracker.borrow_mut() = Some((slot, status));
    });
}

/// Recursively find a positive `creditsUsed` number in an upstream MCP
/// response. Firecrawl includes the real credit cost on search-style results.
/// Only the `creditsUsed` key is accepted; arbitrary numbers elsewhere in the
/// response (ids, counts, sizes) must never be treated as credit costs.
fn scan_credits_used(value: &Value) -> Option<f64> {
    match value {
        Value::Object(object) => object
            .get("creditsUsed")
            .and_then(Value::as_f64)
            .filter(|value| *value > 0.0)
            .or_else(|| {
                object
                    .values()
                    .filter_map(scan_credits_used)
                    .max_by(|left, right| left.total_cmp(right))
            }),
        Value::Array(items) => items
            .iter()
            .filter_map(scan_credits_used)
            .max_by(|left, right| left.total_cmp(right)),
        _ => None,
    }
}

/// Test-only convenience wrapper: same as [`call_with_storage`] without a
/// database pool, so connections never persist learned lifecycle state.
#[cfg(test)]
pub(super) async fn call(
    server: &McpServer,
    request: Value,
    conversation_id: Option<&str>,
    forced: Option<&crate::db::McpCredential>,
) -> anyhow::Result<Value> {
    call_with_storage(None, server, request, conversation_id, forced).await
}

pub(super) async fn call_with_storage(
    storage: Option<&super::McpRuntimeStorage>,
    server: &McpServer,
    request: Value,
    conversation_id: Option<&str>,
    forced: Option<&crate::db::McpCredential>,
) -> anyhow::Result<Value> {
    let timeout = Duration::from_millis(server.timeout_ms.max(100) as u64);
    tokio::time::timeout(timeout, async {
        let tokens = server.bearer_tokens();
        let enabled_token_count = tokens.iter().filter(|token| token.enabled).count();
        let mut attempted = Vec::new();
        let mut attempts = 0usize;
        let mut forced_pending = forced.map(|credential| SelectedToken {
            value: Some(credential.secret.clone()),
            index: Some(credential.position as usize),
        });

        loop {
            let selected = match forced_pending.take() {
                Some(selected) => selected,
                None => {
                    TOKEN_BALANCER
                        .select_token(server.server_id, &tokens, &attempted, conversation_id)
                        .await
                }
            };
            record_token_slot(&selected);
            attempts += 1;
            let result = client::call_once(
                storage,
                server,
                selected.clone(),
                request.clone(),
                conversation_id,
            )
            .await;
            match result {
                Ok(value) => {
                    if let Some(credits) = scan_credits_used(&value) {
                        record_credits_used(credits);
                    }
                    return Ok(value);
                }
                Err(err) => {
                    let retry_status = retryable_status(&err);
                    if let Some(status) = retry_status {
                        let slot = selected
                            .index
                            .and_then(|index| i16::try_from(index + 1).ok());
                        record_upstream_failure(slot, status.as_u16());
                    }
                    if let Some(index) = selected.index {
                        attempted.push(index);
                    }
                    if retry_status.is_some() && attempted.len() < enabled_token_count {
                        warn!(
                            server_name = %server.name,
                            attempts,
                            status = retry_status.map(|status| status.as_u16()),
                            "retrying mcp request with next bearer token"
                        );
                        continue;
                    }
                    if let Some(status) = retry_status {
                        warn!(
                            server_name = %server.name,
                            attempts,
                            status = status.as_u16(),
                            "mcp request failed after bearer token attempts"
                        );
                    }
                    return Err(err);
                }
            }
        }
    })
    .await?
}

pub(super) async fn connect(
    storage: Option<&super::McpRuntimeStorage>,
    server: &McpServer,
    conversation_id: Option<&str>,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>> {
    let tokens = server.bearer_tokens();
    let selected = TOKEN_BALANCER
        .select_token(server.server_id, &tokens, &[], conversation_id)
        .await;
    record_token_slot(&selected);
    client::connect_with_selected(storage, server, selected).await
}

pub(super) async fn with_tracked_token_slot<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    TOKEN_SLOT_TRACKER.scope(RefCell::new(None), future).await
}

pub(super) fn tracked_token_slot() -> Option<i16> {
    TOKEN_SLOT_TRACKER
        .try_with(|slot| *slot.borrow())
        .ok()
        .flatten()
}

pub(super) fn peer_list_or_empty<T: serde::Serialize>(
    result: Result<Vec<T>, rmcp::ServiceError>,
    result_key: &str,
) -> anyhow::Result<Value> {
    client::peer_list_or_empty(result, result_key)
}

fn retryable_status(err: &anyhow::Error) -> Option<StatusCode> {
    if let Some(service_err) = err.downcast_ref::<rmcp::service::ServiceError>() {
        return match service_err {
            rmcp::service::ServiceError::TransportSend(transport_err) => transport_err
                .error
                .downcast_ref::<StreamableHttpError<reqwest::Error>>()
                .and_then(request_time_status)
                .filter(is_retryable_status),
            _ => None,
        };
    }
    handshake_status(err)
}

fn is_retryable_status(status: &StatusCode) -> bool {
    matches!(
        *status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    )
}

fn request_time_status(stream_err: &StreamableHttpError<reqwest::Error>) -> Option<StatusCode> {
    match stream_err {
        StreamableHttpError::Client(reqwest_err) => reqwest_err.status(),
        StreamableHttpError::AuthRequired(_) => Some(StatusCode::UNAUTHORIZED),
        StreamableHttpError::InsufficientScope(_) => Some(StatusCode::FORBIDDEN),
        StreamableHttpError::UnexpectedServerResponse(message) => {
            parse_status_from_message(message.as_ref())
        }
        _ => None,
    }
}

/// Auth/throttling failures during the connect/initialize handshake surface as
/// `ClientInitializeError::TransportError` (before any request is dispatched);
/// they are retryable with the next bearer token just like request-time
/// 401/403/429s.
fn handshake_status(err: &anyhow::Error) -> Option<StatusCode> {
    let Some(rmcp::service::ClientInitializeError::TransportError { error, .. }) =
        err.downcast_ref::<rmcp::service::ClientInitializeError>()
    else {
        return None;
    };
    let stream_err = error
        .error
        .downcast_ref::<StreamableHttpError<reqwest::Error>>()?;
    match stream_err {
        StreamableHttpError::Client(reqwest_err) => reqwest_err.status().filter(|status| {
            matches!(
                *status,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
            )
        }),
        StreamableHttpError::AuthRequired(_) => Some(StatusCode::UNAUTHORIZED),
        StreamableHttpError::InsufficientScope(_) => Some(StatusCode::FORBIDDEN),
        StreamableHttpError::UnexpectedServerResponse(message) => {
            parse_status_from_message(message.as_ref()).filter(|status| {
                matches!(
                    *status,
                    StatusCode::UNAUTHORIZED
                        | StatusCode::FORBIDDEN
                        | StatusCode::TOO_MANY_REQUESTS
                )
            })
        }
        _ => None,
    }
}

fn parse_status_from_message(message: &str) -> Option<StatusCode> {
    let code = message
        .strip_prefix("HTTP ")?
        .split_whitespace()
        .next()?
        .trim_end_matches(':');
    code.parse::<u16>()
        .ok()
        .and_then(|value| StatusCode::from_u16(value).ok())
}

fn record_token_slot(selected: &SelectedToken) {
    let slot = selected
        .index
        .and_then(|index| i16::try_from(index + 1).ok());
    let _ = TOKEN_SLOT_TRACKER.try_with(|tracker| {
        *tracker.borrow_mut() = slot;
    });
}

pub(crate) fn record_builtin_token_slot(index: usize) {
    record_token_slot(&SelectedToken {
        value: None,
        index: Some(index),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tracked_token_slot_records_one_based_index() {
        let slot = with_tracked_token_slot(async {
            record_token_slot(&SelectedToken {
                value: Some("b".to_string()),
                index: Some(1),
            });
            tracked_token_slot()
        })
        .await;

        assert_eq!(slot, Some(2));
    }

    #[test]
    fn retryable_status_parses_http_message_status() {
        assert_eq!(
            parse_status_from_message("HTTP 429: quota"),
            Some(StatusCode::TOO_MANY_REQUESTS)
        );
        assert_eq!(parse_status_from_message("boom"), None);
    }

    #[test]
    fn scans_credits_used_from_firecrawl_envelope() {
        let value = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": {
                "success": true,
                "data": { "web": [{ "url": "https://example.com", "title": "t" }] },
                "creditsUsed": 2
            }
        });
        assert_eq!(scan_credits_used(&value), Some(2.0));
    }

    #[test]
    fn scans_credits_used_nested_inside_arrays() {
        let value = serde_json::json!({ "result": { "items": [{ "creditsUsed": 5 }] } });
        assert_eq!(scan_credits_used(&value), Some(5.0));
    }

    #[test]
    fn scan_ignores_unrelated_numbers_and_zeros() {
        assert_eq!(
            scan_credits_used(&serde_json::json!({
                "id": 99,
                "result": { "count": 42, "duration_ms": 123 }
            })),
            None
        );
        assert_eq!(
            scan_credits_used(&serde_json::json!({ "creditsUsed": 0 })),
            None
        );
    }

    #[tokio::test]
    async fn tracked_credits_uses_max_of_records_in_scope() {
        let value = with_tracked_credits(async {
            record_credits_used(3.0);
            record_credits_used(1.0);
            tracked_credits_used()
        })
        .await;
        assert_eq!(value, Some(3.0));
        assert_eq!(tracked_credits_used(), None);
    }

    #[tokio::test]
    async fn tracked_upstream_failure_is_scoped_and_keeps_slot() {
        let failure = with_tracked_credits(async {
            record_upstream_failure(Some(2), 429);
            tracked_upstream_failure()
        })
        .await;
        assert_eq!(failure, Some((2, 429)));
        assert_eq!(tracked_upstream_failure(), None);
    }

    #[tokio::test]
    async fn tracked_upstream_failure_ignores_missing_slot() {
        let failure = with_tracked_credits(async {
            record_upstream_failure(None, 429);
            tracked_upstream_failure()
        })
        .await;
        assert_eq!(failure, None);
    }
}

#[cfg(test)]
mod v2_tests;
