mod client;
mod token_selection;

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
}

pub(super) async fn call(
    server: &McpServer,
    request: Value,
    conversation_id: Option<&str>,
) -> anyhow::Result<Value> {
    let timeout = Duration::from_millis(server.timeout_ms.max(100) as u64);
    tokio::time::timeout(timeout, async {
        let tokens = server.bearer_tokens();
        let enabled_token_count = tokens.iter().filter(|token| token.enabled).count();
        let mut attempted = Vec::new();
        let mut attempts = 0usize;

        loop {
            let selected = TOKEN_BALANCER
                .select_token(server.server_id, &tokens, &attempted, conversation_id)
                .await;
            record_token_slot(&selected);
            attempts += 1;
            let result = client::call_once(server, selected.clone(), request.clone()).await;
            match result {
                Ok(value) => return Ok(value),
                Err(err) => {
                    let retry_status = retryable_status(&err);
                    if let Some(index) = selected.index {
                        attempted.push(index);
                    }
                    if retry_status.is_some()
                        && attempts == 1
                        && attempted.len() < enabled_token_count
                    {
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
    server: &McpServer,
    conversation_id: Option<&str>,
) -> anyhow::Result<rmcp::service::RunningService<rmcp::RoleClient, rmcp::model::ClientInfo>> {
    let tokens = server.bearer_tokens();
    let selected = TOKEN_BALANCER
        .select_token(server.server_id, &tokens, &[], conversation_id)
        .await;
    record_token_slot(&selected);
    client::connect_with_selected(server, selected).await
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
    let service_err = err.downcast_ref::<rmcp::service::ServiceError>()?;
    let rmcp::service::ServiceError::TransportSend(transport_err) = service_err else {
        return None;
    };
    let stream_err = transport_err
        .error
        .downcast_ref::<StreamableHttpError<reqwest::Error>>()?;
    match stream_err {
        StreamableHttpError::Client(reqwest_err) => reqwest_err.status(),
        StreamableHttpError::AuthRequired(_) => Some(StatusCode::UNAUTHORIZED),
        StreamableHttpError::InsufficientScope(_) => Some(StatusCode::FORBIDDEN),
        StreamableHttpError::UnexpectedServerResponse(message) => {
            parse_status_from_message(message.as_ref())
        }
        _ => None,
    }
    .filter(|status| {
        matches!(
            *status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        )
    })
}

fn parse_status_from_message(message: &str) -> Option<StatusCode> {
    let code = message.strip_prefix("HTTP ")?.split(':').next()?.trim();
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
}

#[cfg(test)]
mod v2_tests;
