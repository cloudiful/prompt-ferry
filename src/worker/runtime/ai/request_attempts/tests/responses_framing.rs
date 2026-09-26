//! Body-prefix framing regression coverage for the Responses forward dispatch
//! (issue #599 R2f.3). The fixture and upstream helper stay `pub(super)` so the
//! sibling `responses_usage` module drives the same sniffed stream.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use super::super::{ForwardOutcome, RouteForwardRequest, forward_route_request};
use super::{
    RESPONSES_JSON_BODY, forward_test_request, spawn_bridge_log, spawn_fixed_body_upstream,
    test_request, test_request_ctx, test_route, test_services, wait_for_bridge, wait_for_count,
    write_chunk,
};
use crate::{
    config::NativeApi,
    db::RouteConfig,
    protocol::BridgeMessage,
    worker::runtime::{context::RuntimeServices, request_assembly::BufferedBridgeRequest},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A split Responses event stream under a generic (or absent) content type —
/// the ChatGPT Codex subscription backend shape, with a multibyte character so
/// the sniff and replay must survive a split codepoint (issue #599 R2f.3).
pub(super) const SNIFFED_EVENT_STREAM_BODY: &str = "event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"note\":\"你\"}}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":120,\"output_tokens\":20,\"total_tokens\":140}}}\n\n";

/// Serve the split event stream chunked, splitting a marker and a multibyte character.
pub(super) async fn spawn_split_event_stream_upstream(
    content_type: Option<&'static str>,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 8192];
                let _ = sock.read(&mut buf).await;
                let bytes = SNIFFED_EVENT_STREAM_BODY.as_bytes();
                let marker = b"data:".len() - 1;
                let multibyte = bytes
                    .windows("你".len())
                    .position(|window| window == "你".as_bytes())
                    .map_or(bytes.len(), |index| index + 1);
                let first = marker.min(multibyte);
                let second = multibyte.max(first);
                let content_type_header = content_type
                    .map(|value| format!("Content-Type: {value}\r\n"))
                    .unwrap_or_default();
                sock.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\n{content_type_header}transfer-encoding: chunked\r\nconnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
                for piece in [&bytes[..first], &bytes[first..second], &bytes[second..]]
                    .into_iter()
                    .filter(|piece| !piece.is_empty())
                {
                    write_chunk(&mut sock, piece).await;
                }
                sock.write_all(b"0\r\n\r\n").await.unwrap();
                let _ = sock.shutdown().await;
            });
        }
    });
    (addr, count)
}

fn streaming_responses_request() -> BufferedBridgeRequest {
    BufferedBridgeRequest {
        body: br#"{"model":"gpt-test","input":"hello","stream":true}"#.to_vec(),
        ..test_request()
    }
}

async fn forward_streaming_responses_request(
    services: &RuntimeServices,
    route: &RouteConfig,
) -> anyhow::Result<ForwardOutcome> {
    let request = streaming_responses_request();
    let mut request_ctx = test_request_ctx(&services.runtime_state);
    Box::pin(forward_route_request(RouteForwardRequest {
        services,
        request: &request,
        request_ctx: &mut request_ctx,
        route,
        method: &http::Method::POST,
        redact_content: false,
        content_logging_enabled: false,
        raw_content_logging_enabled: false,
    }))
    .await
}

#[tokio::test]
async fn sniffs_event_framing_from_the_actual_body_prefix() {
    // Issue #599 R2f.3: the framing comes from the real body, never from the
    // caller's `stream` intent or a misleading header. An event stream under a
    // generic or absent content type must be relayed as SSE — even though the
    // caller's request was not a streaming request — and every byte of the
    // sniffed prefix and the untouched tail must arrive in order.
    for content_type in [None, Some("application/octet-stream")] {
        let (addr, count) = spawn_split_event_stream_upstream(content_type).await;
        let (out_tx, bridge_log) = spawn_bridge_log();
        let services = test_services(out_tx);
        let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

        let outcome = forward_test_request(&services, &route)
            .await
            .expect("forward");
        assert!(matches!(outcome, ForwardOutcome::Handled));
        wait_for_count(&count, 1).await;
        wait_for_bridge(&bridge_log, 3).await;

        let messages = bridge_log.lock().await;
        let start = messages
            .iter()
            .find_map(|message| match message {
                BridgeMessage::ResponseStart(start) => Some(start),
                _ => None,
            })
            .expect("bridge must receive a response start");
        assert_eq!(
            start.content_type.as_deref(),
            Some("text/event-stream"),
            "a sniffed event stream must be relayed to the client as SSE (content_type={content_type:?})",
        );
        let body: Vec<u8> = messages
            .iter()
            .filter_map(|message| match message {
                BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(
            body,
            SNIFFED_EVENT_STREAM_BODY.as_bytes(),
            "the sniffed prefix and the tail must be replayed byte-for-byte (content_type={content_type:?})",
        );
    }
}

#[tokio::test]
async fn does_not_misclassify_a_json_body_with_stream_intent() {
    // A JSON body must stay JSON even when the caller asked for `stream: true`;
    // classification reads the body prefix, not the request intent.
    let (addr, count) = spawn_fixed_body_upstream(RESPONSES_JSON_BODY.as_bytes()).await;
    let (out_tx, bridge_log) = spawn_bridge_log();
    let services = test_services(out_tx);
    let route = test_route(&format!("http://{addr}"), NativeApi::Responses);

    let outcome = forward_streaming_responses_request(&services, &route)
        .await
        .expect("forward");
    assert!(matches!(outcome, ForwardOutcome::Handled));
    wait_for_count(&count, 1).await;
    wait_for_bridge(&bridge_log, 3).await;

    let messages = bridge_log.lock().await;
    let start = messages
        .iter()
        .find_map(|message| match message {
            BridgeMessage::ResponseStart(start) => Some(start),
            _ => None,
        })
        .expect("bridge must receive a response start");
    assert_eq!(
        start.content_type.as_deref(),
        Some("application/json"),
        "a JSON body must keep its JSON framing even for a streaming request",
    );
    let chunks: Vec<Vec<u8>> = messages
        .iter()
        .filter_map(|message| match message {
            BridgeMessage::ResponseChunk(chunk) => Some(chunk.data.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        chunks,
        vec![RESPONSES_JSON_BODY.as_bytes().to_vec()],
        "a JSON body must be forwarded as one buffered JSON chunk",
    );
}
