use std::time::Duration;

use anyhow::{Context, anyhow};
use axum::http::StatusCode;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tracing::info;

use crate::{
    config::{NativeApi, WorkerConfig},
    protocol::{
        BridgeMessage, RealtimeServerEventMessage, RealtimeSessionClose, RealtimeSessionStart,
        ResponseError,
    },
    realtime::{event_json, parse_client_event, parse_server_event},
    worker::runtime::{RealtimeInboundMessage, context::RuntimeServices},
};

use super::{request_init::InitializedRequest, request_routes::resolve_route};

pub(in crate::worker::runtime) async fn start_realtime_session(
    request: RealtimeSessionStart,
    mut inbound_rx: mpsc::Receiver<RealtimeInboundMessage>,
    config: &WorkerConfig,
    services: &RuntimeServices,
) {
    if let Err(err) = run_realtime_session(&request, &mut inbound_rx, config, services).await {
        let _ = services
            .out_tx
            .send(BridgeMessage::ResponseError(ResponseError {
                request_id: request.request_id.clone(),
                status: StatusCode::BAD_GATEWAY.as_u16(),
                code: "realtime_error".to_string(),
                message: err.to_string(),
            }));
    }
    services
        .runtime_state
        .pending_realtime_sessions
        .lock()
        .await
        .remove(&request.request_id);
}

async fn run_realtime_session(
    request: &RealtimeSessionStart,
    inbound_rx: &mut mpsc::Receiver<RealtimeInboundMessage>,
    config: &WorkerConfig,
    services: &RuntimeServices,
) -> anyhow::Result<()> {
    let (route, _endpoint_load_guard) = resolve_realtime_route(request, config, services).await?;
    let effective_model = route.upstream_model.as_deref().unwrap_or(&request.model);
    let upstream = format!(
        "{}?model={}",
        crate::worker::runtime::routing::upstream_url(&route.base_url, NativeApi::Realtime.path())
            .replace("https://", "wss://")
            .replace("http://", "ws://"),
        urlencoding::encode(effective_model)
    );

    let mut ws_request = upstream
        .into_client_request()
        .context("failed to build Realtime websocket request")?;
    ws_request.headers_mut().insert(
        http::header::AUTHORIZATION,
        http::HeaderValue::from_str(&format!("Bearer {}", route.api_key))
            .context("invalid upstream api key header")?,
    );

    let ws_config = Some(
        WebSocketConfig::default()
            .max_message_size(Some(crate::bridge_wire::BRIDGE_WS_MAX_MESSAGE_BYTES))
            .max_frame_size(Some(crate::bridge_wire::BRIDGE_WS_MAX_FRAME_BYTES)),
    );
    let (upstream_socket, _) = connect_async_with_config(ws_request, ws_config, false)
        .await
        .context("failed to connect upstream Realtime websocket")?;
    info!(request_id = %request.request_id, endpoint_id = %route.route_id, model = %effective_model, requested_model = %request.model, "connected upstream Realtime websocket");

    let (mut upstream_tx, mut upstream_rx) = upstream_socket.split();

    loop {
        tokio::select! {
            inbound = inbound_rx.recv() => {
                match inbound {
                    Some(RealtimeInboundMessage::Event(event_json_text)) => {
                        reject_unsupported_client_event(&event_json_text)?;
                        let parsed = parse_client_event(&event_json_text)?;
                        let serialized = event_json(&parsed)?;
                        upstream_tx.send(Message::Text(serialized.into())).await.context("failed to send client Realtime event upstream")?;
                    }
                    Some(RealtimeInboundMessage::Close { code, reason }) => {
                        let close_reason = reason.clone().unwrap_or_default();
                        upstream_tx.send(Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: code.map(tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from).unwrap_or(tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal),
                            reason: close_reason.into(),
                        }))).await.ok();
                        let _ = services.out_tx.send(BridgeMessage::RealtimeSessionClose(RealtimeSessionClose {
                            request_id: request.request_id.clone(),
                            code,
                            reason,
                        }));
                        break;
                    }
                    None => break,
                }
            }
            upstream = upstream_rx.next() => {
                match upstream {
                    Some(Ok(Message::Text(text))) => {
                        let parsed = parse_server_event(&text)?;
                        let serialized = event_json(&parsed)?;
                        let _ = services.out_tx.send(BridgeMessage::RealtimeServerEvent(RealtimeServerEventMessage {
                            request_id: request.request_id.clone(),
                            event_json: serialized,
                        }));
                    }
                    Some(Ok(Message::Binary(_))) => {
                        return Err(anyhow!("upstream returned unsupported binary Realtime frame"));
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let _ = services.out_tx.send(BridgeMessage::RealtimeSessionClose(RealtimeSessionClose {
                            request_id: request.request_id.clone(),
                            code: frame.as_ref().map(|frame| u16::from(frame.code)),
                            reason: frame.map(|frame| frame.reason.to_string()),
                        }));
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        upstream_tx.send(Message::Pong(payload)).await.ok();
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Frame(_))) => {}
                    Some(Err(err)) => return Err(err).context("failed to read upstream Realtime websocket"),
                    None => {
                        let _ = services.out_tx.send(BridgeMessage::RealtimeSessionClose(RealtimeSessionClose {
                            request_id: request.request_id.clone(),
                            code: None,
                            reason: Some("upstream websocket closed".to_string()),
                        }));
                        break;
                    }
                }
            }
        }
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
    Ok(())
}

async fn resolve_realtime_route(
    request: &RealtimeSessionStart,
    config: &WorkerConfig,
    services: &RuntimeServices,
) -> anyhow::Result<(
    crate::db::RouteConfig,
    Option<crate::worker::runtime::EndpointLoadGuard>,
)> {
    let fake_request = crate::worker::runtime::request_assembly::BufferedBridgeRequest {
        request_id: request.request_id.clone(),
        method: "GET".to_string(),
        path: request.path.clone(),
        headers: Vec::new(),
        body: serde_json::to_vec(&serde_json::json!({"model": request.model})).unwrap_or_default(),
        request_deadline_unix_ms: 0,
        user_id: request.user_id,
        client_key_hash: request.client_key_hash.clone(),
        request_user_agent: request.request_user_agent.clone(),
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
    };
    let InitializedRequest { request_ctx, .. } =
        super::request_init::initialize_request(&fake_request, services).await?;
    let (route, load_guard) = match resolve_route(&fake_request, config, services, &request_ctx)
        .await?
    {
        super::request_routes::RouteResolution::Ready { route, load_guard } => (*route, load_guard),
        super::request_routes::RouteResolution::Responded => {
            return Err(anyhow!("realtime route was rejected by budget gate"));
        }
    };
    (route.native_api == NativeApi::Realtime)
        .then_some((route, load_guard))
        .ok_or_else(|| {
            anyhow!(
                "selected endpoint does not support realtime for model {}",
                request.model
            )
        })
}

fn reject_unsupported_client_event(event_json_text: &str) -> anyhow::Result<()> {
    let value: serde_json::Value =
        serde_json::from_str(event_json_text).context("failed to inspect Realtime client event")?;
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if event_type.starts_with("input_audio_buffer.")
        || event_type.starts_with("output_audio_buffer.")
    {
        return Err(anyhow!(
            "Realtime audio events are not implemented in this relay"
        ));
    }
    Ok(())
}
