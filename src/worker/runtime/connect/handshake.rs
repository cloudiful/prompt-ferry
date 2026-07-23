use anyhow::{Context, anyhow};
use futures::{SinkExt, StreamExt, stream::SplitSink, stream::SplitStream};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

use crate::{
    bridge_crypto::{self, BridgeCipher, Direction},
    protocol::BridgeMessage,
};

use super::RelayConnectionConfig;

type RelaySocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(super) async fn negotiate_bridge_encryption(
    ws_tx: &mut SplitSink<RelaySocket, Message>,
    ws_rx: &mut SplitStream<RelaySocket>,
    relay: &RelayConnectionConfig,
) -> anyhow::Result<(Option<BridgeCipher>, Option<BridgeCipher>)> {
    if !relay.bridge_encryption_mode.required() {
        return Ok((None, None));
    }

    let worker_nonce = bridge_crypto::random_handshake_nonce();
    let hello = bridge_crypto::encode_hello(&worker_nonce)?;
    ws_tx
        .send(Message::Text(hello.into()))
        .await
        .context("failed to send encryption hello")?;

    let text = match tokio::time::timeout(
        Duration::from_secs(relay.connect_timeout_seconds),
        ws_rx.next(),
    )
    .await
    {
        Ok(Some(Ok(Message::Text(text)))) => text.to_string(),
        Ok(Some(Ok(Message::Binary(bytes)))) => {
            String::from_utf8(bytes.to_vec()).context("invalid encryption ready utf8")?
        }
        Ok(Some(Ok(_))) => return Err(anyhow!("relay did not send encryption ready")),
        Ok(Some(Err(err))) => return Err(err).context("websocket read failed"),
        Ok(None) => return Err(anyhow!("relay websocket closed before encryption ready")),
        Err(_) => return Err(anyhow!("timed out waiting for relay encryption ready")),
    };
    let relay_nonce = bridge_crypto::decode_ready(&text)?;

    let mut write_cipher = BridgeCipher::new(
        &relay.bridge_encryption_key,
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )?;
    let mut read_cipher = BridgeCipher::new(
        &relay.bridge_encryption_key,
        &worker_nonce,
        &relay_nonce,
        Direction::WorkerToRelay,
        Direction::RelayToWorker,
    )?;

    let ping = write_cipher.encrypt_message(&BridgeMessage::Ping)?;
    ws_tx
        .send(Message::Binary(ping.into()))
        .await
        .context("failed to send encrypted verification ping")?;

    let bytes = match tokio::time::timeout(
        Duration::from_secs(relay.connect_timeout_seconds),
        ws_rx.next(),
    )
    .await
    {
        Ok(Some(Ok(Message::Binary(bytes)))) => bytes.to_vec(),
        Ok(Some(Ok(Message::Text(_)))) => {
            return Err(anyhow!(
                "relay sent unexpected text encrypted verification pong"
            ));
        }
        Ok(Some(Ok(_))) => {
            return Err(anyhow!("relay did not send encrypted verification pong"));
        }
        Ok(Some(Err(err))) => return Err(err).context("websocket read failed"),
        Ok(None) => {
            return Err(anyhow!(
                "relay websocket closed before encrypted verification"
            ));
        }
        Err(_) => {
            return Err(anyhow!(
                "timed out waiting for relay encrypted verification"
            ));
        }
    };

    match read_cipher.decrypt_message(&bytes)? {
        BridgeMessage::Pong => {}
        _ => {
            return Err(anyhow!(
                "relay sent unexpected encrypted verification message"
            ));
        }
    }

    Ok((Some(write_cipher), Some(read_cipher)))
}
