use super::super::{REQUEST_RECORD_LEASE_SECONDS, elapsed_ms};
use crate::{
    db,
    protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
    session_affinity::{apply_opencode_headers, opencode_headers_apply, resolve_opencode_session},
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
    /// Issue #277 Phase P7: the admission record already carries this instant;
    /// the models outcome must reuse it so both writes land on one row.
    pub(super) created_at: chrono::DateTime<chrono::Utc>,
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
        created_at,
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

    let catalog_requests = routes
        .into_iter()
        .flat_map(|route| {
            // Issue #1032: an Auto OpenRouter endpoint serves chat and
            // embeddings, so its listing is the deduplicated union of both
            // catalogs; every other route has one protocol-specific catalog.
            crate::endpoint_models::models_urls(&route.base_url, route.provider, route.native_api)
                .into_iter()
                .map(move |url| (route.clone(), url))
        })
        .collect::<Vec<_>>();

    let mut requests = futures::stream::iter(catalog_requests.into_iter().map(|(route, url)| {
        let client = client.clone();
        // Issue #1052 P1: an OpenCode endpoint answers the catalog read only
        // with a routing session, so the client model listing carries the
        // caller's own session or the request's stable one.
        let session = resolve_opencode_session(request_headers, None, request_id);
        async move {
            let request = catalog_request(&client, &route, &url, request_headers, &session);
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
                    created_at: Some(created_at),
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
                created_at: Some(created_at),
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

/// The catalog request for one route.
///
/// The request keeps the route's own protocol authentication and adds the
/// OpenCode routing session when the route is an OpenCode endpoint.
fn catalog_request(
    client: &Client,
    route: &db::RouteConfig,
    url: &str,
    request_headers: &[(String, String)],
    session: &str,
) -> reqwest::RequestBuilder {
    // GLM (issue #230 P2) lists models at `{base}/models`; every other
    // provider keeps the plain `/v1/models` join. An embeddings protocol
    // lists the provider's embeddings catalog.
    let opencode = opencode_headers_apply(Some(route.provider), url)
        || opencode_headers_apply(Some(route.provider), &route.base_url);
    let request = client.get(url);
    let request = match route.native_api {
        crate::config::NativeApi::AnthropicMessages => super::upstream::with_anthropic_headers(
            request.header("x-api-key", &route.api_key),
            request_headers,
        ),
        _ => request.bearer_auth(&route.api_key),
    };
    if opencode {
        apply_opencode_headers(request, request_headers, session)
    } else {
        request
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{RouteConfig, RouteSelectionReason};

    fn route(provider: db::EndpointProvider) -> RouteConfig {
        RouteConfig {
            embedding_dimensions_override: None,
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: "http://127.0.0.1:9".to_string(),
            api_key: "sk-client".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: crate::config::NativeApi::Chat,
            upstream_model: None,
            route_selection_reason: RouteSelectionReason::Default,
            provider,
            service_tier: None,
            proxy_url: None,
            dev_system_normalize: false,
            thinking_effort_override: None,
            compact_mode: crate::db::CompactMode::Passthrough,
            thinking_downgrade_enabled: false,
            has_oauth_token: false,
        }
    }

    #[test]
    fn an_opencode_route_reads_its_catalog_with_a_routing_session() {
        // Issue #1052 P1: the OpenCode gateway rejects a catalog read without a
        // routing session, so the client model listing sends one.
        let route = route(db::EndpointProvider::OpencodeGo);
        let request = catalog_request(
            &Client::new(),
            &route,
            "http://127.0.0.1:9/v1/models",
            &[],
            "ses_client",
        )
        .build()
        .expect("catalog request");

        assert_eq!(
            request
                .headers()
                .get("x-opencode-session")
                .unwrap()
                .to_str()
                .unwrap(),
            "ses_client"
        );
        assert_eq!(
            request
                .headers()
                .get("authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer sk-client",
            "the catalog read keeps the route's own authentication"
        );
    }

    #[test]
    fn a_generic_route_reads_its_catalog_without_an_opencode_session() {
        let route = route(db::EndpointProvider::Generic);
        let request = catalog_request(
            &Client::new(),
            &route,
            "http://127.0.0.1:9/v1/models",
            &[],
            "ses_client",
        )
        .build()
        .expect("catalog request");

        assert!(request.headers().get("x-opencode-session").is_none());
    }
}
