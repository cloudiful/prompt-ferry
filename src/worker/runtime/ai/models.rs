use super::super::{REQUEST_RECORD_LEASE_SECONDS, elapsed_ms, upstream_url};
use crate::{
    db,
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
    usage::truncate_chars,
    worker::runtime::context::BridgeSender,
    worker::runtime::request_assembly::BufferedBridgeRequest,
    worker_admin::AdminState,
    worker_usage::{UsageLog, UsageRequestMetadata, record_usage_event},
};
use anyhow::{Context, anyhow};
use chrono::Utc;
use futures::StreamExt;
use reqwest::{Client, StatusCode};
use std::{collections::HashSet, time::Instant};
use tracing::warn;

pub(super) struct ModelsRequestContext<'a> {
    pub(super) state: &'a AdminState,
    pub(super) client: &'a Client,
    pub(super) out_tx: &'a BridgeSender,
    pub(super) request: &'a BufferedBridgeRequest,
    pub(super) request_id: uuid::Uuid,
    pub(super) started: Instant,
    pub(super) user_id: i64,
    pub(super) owner_worker_id: uuid::Uuid,
    pub(super) anthropic: bool,
    pub(super) request_headers: &'a [(String, String)],
}

pub(super) async fn process_models_request(
    context: ModelsRequestContext<'_>,
) -> anyhow::Result<()> {
    let ModelsRequestContext {
        state,
        client,
        out_tx,
        request,
        request_id,
        started,
        user_id,
        owner_worker_id,
        anthropic,
        request_headers,
    } = context;
    let routes = db::list_visible_endpoints(&state.pool, user_id)
        .await?
        .into_iter()
        .filter(|route| {
            !anthropic || route.native_api == crate::config::NativeApi::AnthropicMessages
        })
        .collect::<Vec<_>>();
    if routes.is_empty() {
        return Err(anyhow!("route not found"));
    }

    let mut merged = Vec::<serde_json::Value>::new();
    let mut seen_ids = HashSet::<String>::new();
    let mut success = false;
    let mut last_error_status = StatusCode::BAD_GATEWAY;
    let mut last_error_body = String::new();

    let mut requests = futures::stream::iter(routes.into_iter().map(|route| {
        let client = client.clone();
        async move {
            let request = client.get(upstream_url(&route.base_url, "/v1/models"));
            let request = match route.native_api {
                crate::config::NativeApi::AnthropicMessages => {
                    super::upstream::with_anthropic_headers(
                        request.header("x-api-key", &route.api_key),
                        request_headers,
                    )
                }
                _ => request.bearer_auth(&route.api_key),
            };
            let response = request.send().await;
            (route, response)
        }
    }))
    .buffer_unordered(8);

    while let Some((route, response)) = requests.next().await {
        let response = match response {
            Ok(response) => response,
            Err(err) => {
                warn!(
                    endpoint_id = %route.route_id,
                    error = %err,
                    "failed to fetch models from route target"
                );
                continue;
            }
        };
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            last_error_status = status;
            last_error_body = body;
            continue;
        }
        success = true;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body)
            && let Some(items) = value.get("data").and_then(serde_json::Value::as_array)
        {
            for item in items {
                let Some(id) = item.get("id").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if seen_ids.insert(id.to_string()) {
                    merged.push(item.clone());
                }
            }
        }
    }

    if !success {
        let body = if anthropic {
            serde_json::json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": if last_error_body.trim().is_empty() {
                        "failed to fetch models from every visible Anthropic endpoint"
                    } else {
                        last_error_body.trim()
                    }
                }
            })
            .to_string()
        } else if last_error_body.trim().is_empty() {
            serde_json::json!({
                "error": {
                    "code": "models_unavailable",
                    "message": "failed to fetch models from every visible endpoint"
                }
            })
            .to_string()
        } else {
            last_error_body
        };
        out_tx
            .send(BridgeMessage::ResponseStart(ResponseStart {
                request_id: request.request_id.clone(),
                status: last_error_status.as_u16(),
                content_type: Some("application/json".to_string()),
                headers: Vec::new(),
            }))
            .await
            .context("relay response channel closed")?;
        out_tx
            .send(BridgeMessage::ResponseChunk(ResponseChunk {
                request_id: request.request_id.clone(),
                data: body.clone().into_bytes(),
            }))
            .await
            .context("relay response channel closed")?;
        out_tx
            .send(BridgeMessage::ResponseEnd(ResponseEnd {
                request_id: request.request_id.clone(),
            }))
            .await
            .context("relay response channel closed")?;
        record_usage_event(
            Some(state),
            UsageLog::ai_request(
                request_id,
                UsageRequestMetadata {
                    user_id: Some(user_id).filter(|id| *id > 0),
                    request_user_agent: request.request_user_agent.clone(),
                    path: request.path.clone(),
                    ..Default::default()
                },
                None,
            )
            .with_state(db::UsageEventKind::Request, db::RequestRecordState::Failed)
            .with_worker_lease(
                Some(owner_worker_id),
                Some(Utc::now() + chrono::Duration::seconds(REQUEST_RECORD_LEASE_SECONDS)),
                Some(Utc::now()),
            )
            .with_status(
                Some(last_error_status.as_u16() as i32),
                Some(false),
                Some(elapsed_ms(started)),
                None,
            )
            .with_error(
                Some("http_error".to_string()),
                Some(crate::worker::runtime::error_handling::http_error_message(
                    last_error_status.as_u16(),
                    Some(&body),
                )),
                Some(truncate_chars(&body, 512)),
            ),
        )
        .await;
        return Ok(());
    }

    merged.sort_by(|left, right| {
        left.get("id")
            .and_then(serde_json::Value::as_str)
            .cmp(&right.get("id").and_then(serde_json::Value::as_str))
    });
    let body = if anthropic {
        let data = merged
            .into_iter()
            .map(anthropic_model_item)
            .collect::<Vec<_>>();
        serde_json::json!({
            "data": data,
            "first_id": data.first().and_then(|item| item.get("id")),
            "has_more": false,
            "last_id": data.last().and_then(|item| item.get("id")),
        })
    } else {
        serde_json::json!({
            "object": "list",
            "data": merged,
        })
    }
    .to_string();
    out_tx
        .send(BridgeMessage::ResponseStart(ResponseStart {
            request_id: request.request_id.clone(),
            status: StatusCode::OK.as_u16(),
            content_type: Some("application/json".to_string()),
            headers: Vec::new(),
        }))
        .await
        .context("relay response channel closed")?;
    out_tx
        .send(BridgeMessage::ResponseChunk(ResponseChunk {
            request_id: request.request_id.clone(),
            data: body.clone().into_bytes(),
        }))
        .await
        .context("relay response channel closed")?;
    out_tx
        .send(BridgeMessage::ResponseEnd(ResponseEnd {
            request_id: request.request_id.clone(),
        }))
        .await
        .context("relay response channel closed")?;
    record_usage_event(
        Some(state),
        UsageLog::ai_request(
            request_id,
            UsageRequestMetadata {
                user_id: Some(user_id).filter(|id| *id > 0),
                request_user_agent: request.request_user_agent.clone(),
                path: request.path.clone(),
                ..Default::default()
            },
            None,
        )
        .with_state(
            db::UsageEventKind::Request,
            db::RequestRecordState::Completed,
        )
        .with_worker_lease(
            Some(owner_worker_id),
            Some(Utc::now() + chrono::Duration::seconds(REQUEST_RECORD_LEASE_SECONDS)),
            Some(Utc::now()),
        )
        .with_status(
            Some(StatusCode::OK.as_u16() as i32),
            Some(true),
            Some(elapsed_ms(started)),
            None,
        ),
    )
    .await;
    Ok(())
}

fn anthropic_model_item(item: serde_json::Value) -> serde_json::Value {
    let id = item
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let created_at = item
        .get("created_at")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("1970-01-01T00:00:00Z");
    serde_json::json!({
        "type": "model",
        "id": id,
        "display_name": item
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| item.get("id").and_then(serde_json::Value::as_str).unwrap_or("unknown")),
        "created_at": created_at,
    })
}
