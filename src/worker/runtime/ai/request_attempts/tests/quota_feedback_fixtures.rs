use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::{
    config::NativeApi,
    db::RouteConfig,
    upstream_adapter::ResponseAdapter,
    worker::runtime::{
        ai::{
            forward::{ResponseForwardContext, ResponseLoggingContext, forward_upstream_response},
            quota_feedback::AttemptFeedback,
        },
        context::RouteExecutionContext,
    },
};

use super::super::test_route;

pub(super) async fn forward_with_feedback(
    address: SocketAddr,
    route: &RouteConfig,
    request: &crate::worker::runtime::request_assembly::BufferedBridgeRequest,
    request_ctx: &crate::worker::runtime::RequestExecutionContext,
    services: &crate::worker::runtime::context::RuntimeServices,
    feedback: AttemptFeedback,
) -> anyhow::Result<()> {
    let route_ctx = RouteExecutionContext::new(route).with_attempt_feedback(feedback.clone());
    let response = services
        .client
        .get(format!("http://{address}/v1/responses"))
        .send()
        .await?;
    forward_upstream_response(
        response,
        ResponseForwardContext {
            route_ctx: &route_ctx,
            request,
            request_ctx,
            upstream_redacted_request_json: None,
            upstream_restore_session: None,
            logging: ResponseLoggingContext {
                redact_content: false,
                content_logging_enabled: false,
                raw_content_logging_enabled: false,
            },
            response_adapter: ResponseAdapter::Passthrough,
            attempt_feedback: Some(feedback),
            services,
            quota_failover_enabled: false,
        },
    )
    .await
}

pub(super) fn keyed_route(address: &str) -> RouteConfig {
    let mut route = test_route(address, NativeApi::Responses);
    route.endpoint_key_id = Some(uuid::Uuid::new_v4());
    route
}

pub(super) async fn spawn_http_response(
    status: u16,
    reason: &'static str,
    headers: &'static str,
    body: &'static [u8],
) -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut request = [0u8; 8192];
                let _ = socket.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (addr, count)
}

pub(super) async fn spawn_responses_failed_stream() -> (SocketAddr, Arc<AtomicUsize>) {
    const BODY: &[u8] = b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}\n\nevent: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"server_error\",\"message\":\"upstream failed\"}}}\n\n";
    spawn_http_response(200, "OK", "Content-Type: text/event-stream\r\n", BODY).await
}
