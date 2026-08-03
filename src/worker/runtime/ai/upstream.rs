use crate::{db, upstream_adapter::PreparedRequestBody};
use bytes::Bytes;
use futures::StreamExt;
use http::header;
use reqwest::{Client, Method};

pub(super) async fn send_upstream_request(
    client: &Client,
    method: &Method,
    url: &str,
    route: &db::RouteConfig,
    body: &PreparedRequestBody,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_builder = client
        .request(method.clone(), url)
        .header(header::CONTENT_TYPE, "application/json");
    let request_builder = match route.native_api {
        crate::config::NativeApi::AnthropicMessages => request_builder
            .header("x-api-key", &route.api_key)
            .header("anthropic-version", "2023-06-01"),
        _ => request_builder.bearer_auth(&route.api_key),
    };
    match body {
        PreparedRequestBody::PassthroughStream(bytes) => {
            request_builder.body(bytes.clone()).send().await
        }
        PreparedRequestBody::BufferedBytes(bytes) => {
            request_builder.body(bytes.clone()).send().await
        }
    }
}

pub(super) async fn read_response_sample(response: reqwest::Response, max_bytes: usize) -> Vec<u8> {
    let mut collected = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            break;
        };
        append_sample(&mut collected, &chunk, max_bytes);
        if collected.len() >= max_bytes {
            break;
        }
    }
    collected
}

fn append_sample(collected: &mut Vec<u8>, chunk: &Bytes, max_bytes: usize) {
    if collected.len() >= max_bytes {
        return;
    }
    let remaining = max_bytes - collected.len();
    collected.extend(chunk.iter().copied().take(remaining));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::NativeApi,
        db::{ResponsesContinuationPolicy, RouteConfig, RouteSelectionReason},
    };
    use axum::{Router, http::StatusCode, routing::post};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn bad_gateway_is_returned_without_retry() {
        let request_count = Arc::new(AtomicUsize::new(0));
        let counter = request_count.clone();
        let app = Router::new().route(
            "/v1/responses",
            post(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    StatusCode::BAD_GATEWAY
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let address = listener.local_addr().expect("mock upstream address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve mock upstream");
        });

        let route = RouteConfig {
            route_id: uuid::Uuid::new_v4(),
            user_id: 1,
            model_route_rule_id: None,
            base_url: format!("http://{address}"),
            api_key: "test-key".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Responses,
            upstream_model: None,
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
            chat_reasoning_replay_policy: crate::db::ChatReasoningReplayPolicy::Auto,
            route_selection_reason: RouteSelectionReason::Default,
        };
        let response = send_upstream_request(
            &Client::new(),
            &Method::POST,
            &format!("http://{address}/v1/responses"),
            &route,
            &PreparedRequestBody::BufferedBytes(b"{}".to_vec()),
        )
        .await
        .expect("upstream response");

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(request_count.load(Ordering::SeqCst), 1);
    }
}
