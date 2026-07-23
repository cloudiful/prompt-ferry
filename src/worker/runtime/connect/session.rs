use anyhow::{Context, anyhow};
use futures::{SinkExt, StreamExt};
use http::{HeaderValue, header};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config, connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};
use tracing::{error, info, warn};

use super::{
    RelayConnectionConfig,
    backoff::{relay_reconnect_base_delay, relay_reconnect_delay_with_jitter},
    handshake::negotiate_bridge_encryption,
    is_expected_relay_disconnect,
    support::{format_error_chain, ws_connect_error_detail},
};
use crate::{
    bridge_wire,
    config::WorkerConfig,
    protocol::BridgeMessage,
    tls,
    worker::runtime::{
        RELAY_RECONNECT_DELAY_SECONDS, SHUTDOWN_DRAIN_TIMEOUT_SECONDS, WorkerRuntimeState,
        handle_relay_bridge_message,
    },
    worker_admin,
};

pub(super) async fn run_relay_loop(
    relay: RelayConnectionConfig,
    config: WorkerConfig,
    client: Client,
    admin_state: Option<worker_admin::AdminState>,
    runtime_state: WorkerRuntimeState,
) {
    let mut consecutive_failures = 0_u32;
    loop {
        if runtime_state.is_shutting_down() {
            break;
        }
        let reconnect_delay = match connect_once(
            &relay,
            config.clone(),
            client.clone(),
            admin_state.clone(),
            runtime_state.clone(),
        )
        .await
        {
            Ok(()) => {
                consecutive_failures = 0;
                if let Some(state) = admin_state.as_ref() {
                    mark_relay_disconnected(state, &relay, None).await;
                }
                warn!(
                    relay_url = %relay.relay_url,
                    delay_seconds = RELAY_RECONNECT_DELAY_SECONDS,
                    "relay websocket closed; reconnecting"
                );
                relay_reconnect_base_delay(1)
            }
            Err(err) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let reconnect_delay = relay_reconnect_delay_with_jitter(consecutive_failures);
                let error_detail = format_error_chain(&err);
                if let Some(state) = admin_state.as_ref() {
                    mark_relay_disconnected(state, &relay, Some(error_detail.clone())).await;
                }
                if is_expected_relay_disconnect(&err) {
                    warn!(
                        relay_url = %relay.relay_url,
                        consecutive_failures,
                        delay_ms = reconnect_delay.as_millis() as u64,
                        error = %error_detail,
                        "relay connection ended; reconnecting"
                    );
                } else {
                    error!(
                        relay_url = %relay.relay_url,
                        consecutive_failures,
                        delay_ms = reconnect_delay.as_millis() as u64,
                        error = %error_detail,
                        "relay connection failed; reconnecting"
                    );
                }
                reconnect_delay
            }
        };

        tokio::select! {
            _ = runtime_state.wait_for_shutdown() => break,
            _ = tokio::time::sleep(reconnect_delay) => {}
        }
    }
}

pub(super) async fn connect_once(
    relay: &RelayConnectionConfig,
    config: WorkerConfig,
    client: Client,
    admin_state: Option<worker_admin::AdminState>,
    runtime_state: WorkerRuntimeState,
) -> anyhow::Result<()> {
    let mut request = relay
        .relay_url
        .as_str()
        .into_client_request()
        .context("failed to build websocket request")?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", relay.worker_token))
            .context("invalid worker token header")?,
    );

    let connector = if relay.tls_mode.enabled() {
        Some(Connector::Rustls(tls::client_config_from_pem(
            &relay.relay_url,
            relay.tls_mode,
            relay.relay_ca_pem.as_deref(),
            relay.client_cert_pem.as_deref(),
            relay.client_key_pem.as_deref(),
        )?))
    } else {
        None
    };
    let ws_config = Some(
        WebSocketConfig::default()
            .max_message_size(Some(bridge_wire::BRIDGE_WS_MAX_MESSAGE_BYTES))
            .max_frame_size(Some(bridge_wire::BRIDGE_WS_MAX_FRAME_BYTES)),
    );
    let connect = if connector.is_some() {
        connect_async_tls_with_config(request, ws_config, false, connector).await
    } else {
        connect_async_with_config(request, ws_config, false).await
    };

    let (socket, _) = connect.map_err(|err| {
        let error_detail = ws_connect_error_detail(&err);
        error!(
            relay_url = %relay.relay_url,
            error = %error_detail,
            "failed to connect relay websocket"
        );
        anyhow!(err).context("failed to connect relay websocket")
    })?;
    info!(relay_url = %relay.relay_url, "connected to relay");

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (mut write_cipher, mut read_cipher) =
        negotiate_bridge_encryption(&mut ws_tx, &mut ws_rx, relay).await?;
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<BridgeMessage>();
    if let Some(state) = admin_state.as_ref() {
        worker_admin::set_bridge_sender(state, &relay.relay_key, Some(out_tx.clone())).await;
        mark_relay_connected(state, relay).await;
        worker_admin::publish_snapshot(state).await?;
    }

    let write_task = tokio::spawn(async move {
        let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                Some(message) = out_rx.recv() => {
                    let payload = if let Some(cipher) = write_cipher.as_mut() {
                        cipher.encrypt_message(&message)
                    } else {
                        bridge_wire::encode_message(&message)
                    };
                    let payload = match payload {
                        Ok(payload) => payload,
                        Err(err) => {
                            error!(error = %err, "failed to encode bridge message");
                            break;
                        }
                    };
                    if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                        break;
                    }
                }
                _ = heartbeat.tick() => {
                    if ws_tx.send(Message::Ping(Vec::new().into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    loop {
        let next_message = tokio::select! {
            _ = runtime_state.wait_for_shutdown() => None,
            value = ws_rx.next() => value,
        };

        let Some(message) = next_message else {
            break;
        };

        match message {
            Ok(Message::Text(_)) => return Err(anyhow!("unexpected text relay bridge message")),
            Ok(Message::Binary(bytes)) => {
                let decoded = if let Some(cipher) = read_cipher.as_mut() {
                    cipher.decrypt_message(&bytes)
                } else {
                    bridge_wire::decode_message(&bytes)
                };
                match decoded {
                    Ok(message) => {
                        let services = super::super::context::RuntimeServices::new(
                            admin_state.clone(),
                            out_tx.clone(),
                            client.clone(),
                            runtime_state.clone(),
                        );
                        handle_relay_bridge_message(message, &config, &services).await;
                    }
                    Err(err) => return Err(err).context("failed to decode relay message"),
                }
            }
            Ok(Message::Ping(_)) => {
                let _ = out_tx.send(BridgeMessage::Pong);
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => break,
            Err(err) => return Err(err).context("websocket read failed"),
            _ => {}
        }
    }

    if runtime_state.is_shutting_down() {
        runtime_state
            .wait_for_drain(Duration::from_secs(SHUTDOWN_DRAIN_TIMEOUT_SECONDS))
            .await;
    }

    write_task.abort();
    if let Some(state) = admin_state.as_ref() {
        worker_admin::set_bridge_sender(state, &relay.relay_key, None).await;
    }
    Ok(())
}

async fn mark_relay_connected(state: &worker_admin::AdminState, relay: &RelayConnectionConfig) {
    if let Some(relay_id) = relay.relay_id {
        let mut statuses = state.managed_relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = true;
        status.last_error = None;
        status.last_connected_at = Some(chrono::Utc::now());
    }
}

async fn mark_relay_disconnected(
    state: &worker_admin::AdminState,
    relay: &RelayConnectionConfig,
    error_message: Option<String>,
) {
    if let Some(relay_id) = relay.relay_id {
        let mut statuses = state.managed_relay_statuses.write().await;
        let status = statuses.entry(relay_id).or_default();
        status.connected = false;
        status.last_disconnected_at = Some(chrono::Utc::now());
        if let Some(error_message) = error_message {
            status.last_error = Some(error_message);
        }
    }
}
