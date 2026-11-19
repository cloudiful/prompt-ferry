//! The framed transport of a peer link.
//!
//! A peer link speaks two frame kinds: the JSON frames of the handshake
//! phase and the binary frames of the existing bridge codec. The transport
//! is abstracted so the handshake and the session are transport independent
//! and can be driven by in-memory streams.

use anyhow::anyhow;
use axum::extract::ws::{Message as AxumMessage, WebSocket};
use futures::{SinkExt, StreamExt};
use std::future::Future;
use tokio_tungstenite::{WebSocketStream, tungstenite::Message as WsMessage};

/// One frame of a peer link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerFrame {
    /// A text frame. The handshake frames are text frames.
    Text(String),
    /// A binary frame. The bridge messages are binary frames.
    Binary(Vec<u8>),
}

/// The reading half of a peer link.
pub(crate) trait PeerReader {
    /// The next frame of the peer, or `None` when the link ended.
    fn recv(&mut self) -> impl Future<Output = anyhow::Result<Option<PeerFrame>>> + Send;
}

/// The writing half of a peer link.
pub(crate) trait PeerWriter {
    /// Write a binary frame.
    fn send_binary(&mut self, bytes: Vec<u8>) -> impl Future<Output = anyhow::Result<()>> + Send;
    /// Flush the written frames to the peer.
    fn flush(&mut self) -> impl Future<Output = anyhow::Result<()>> + Send;
}

/// The reading half of an accepted peer link.
pub(crate) struct AxumPeerReader {
    reader: futures::stream::SplitStream<WebSocket>,
}

/// The writing half of an accepted peer link.
pub(crate) struct AxumPeerWriter {
    writer: futures::stream::SplitSink<WebSocket, AxumMessage>,
}

/// Split an accepted websocket into its two halves.
pub(crate) fn axum_peer_transport(socket: WebSocket) -> (AxumPeerReader, AxumPeerWriter) {
    let (writer, reader) = socket.split();
    (AxumPeerReader { reader }, AxumPeerWriter { writer })
}

/// The websocket limits of an authenticated peer link.
///
/// The link is authenticated before the upgrade, so the frames of the link
/// may be as large as the bridge contract allows. The limits are the ones of
/// the bridge, so a legitimate frame is never refused by a smaller default.
pub(crate) fn peer_socket_config() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    let mut config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    config.max_frame_size = Some(crate::bridge::wire::BRIDGE_WS_MAX_FRAME_BYTES);
    config.max_message_size = Some(crate::bridge::wire::BRIDGE_WS_MAX_MESSAGE_BYTES);
    config
}

impl PeerReader for AxumPeerReader {
    async fn recv(&mut self) -> anyhow::Result<Option<PeerFrame>> {
        loop {
            let Some(next) = self.reader.next().await else {
                return Ok(None);
            };
            let message = next?;
            match message {
                AxumMessage::Text(text) => {
                    return Ok(Some(PeerFrame::Text(text.as_str().to_string())));
                }
                AxumMessage::Binary(bytes) => return Ok(Some(PeerFrame::Binary(bytes.to_vec()))),
                AxumMessage::Ping(_) | AxumMessage::Pong(_) => continue,
                AxumMessage::Close(_) => return Ok(None),
            }
        }
    }
}

impl PeerWriter for AxumPeerWriter {
    async fn send_binary(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.writer
            .send(AxumMessage::Binary(bytes.into()))
            .await
            .map_err(|err| anyhow!("failed to write the peer bridge frame: {err}"))
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        self.writer
            .flush()
            .await
            .map_err(|err| anyhow!("failed to flush the peer link: {err}"))
    }
}

/// The reading half of a dialed peer link.
pub(crate) struct TungstenitePeerReader<S> {
    reader: futures::stream::SplitStream<WebSocketStream<S>>,
}

/// The writing half of a dialed peer link.
pub(crate) struct TungstenitePeerWriter<S> {
    writer: futures::stream::SplitSink<WebSocketStream<S>, WsMessage>,
}

/// Split a dialed websocket into its two halves.
pub(crate) fn tungstenite_peer_transport<S>(
    socket: WebSocketStream<S>,
) -> (TungstenitePeerReader<S>, TungstenitePeerWriter<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    let (writer, reader) = socket.split();
    (
        TungstenitePeerReader { reader },
        TungstenitePeerWriter { writer },
    )
}

impl<S> PeerReader for TungstenitePeerReader<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    async fn recv(&mut self) -> anyhow::Result<Option<PeerFrame>> {
        loop {
            let Some(next) = self.reader.next().await else {
                return Ok(None);
            };
            let message = next?;
            match message {
                WsMessage::Text(text) => {
                    return Ok(Some(PeerFrame::Text(text.as_str().to_string())));
                }
                WsMessage::Binary(bytes) => return Ok(Some(PeerFrame::Binary(bytes.to_vec()))),
                WsMessage::Ping(_) | WsMessage::Pong(_) | WsMessage::Frame(_) => continue,
                WsMessage::Close(_) => return Ok(None),
            }
        }
    }
}

impl<S> PeerWriter for TungstenitePeerWriter<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    async fn send_binary(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.writer
            .send(WsMessage::Binary(bytes.into()))
            .await
            .map_err(|err| anyhow!("failed to write the peer bridge frame: {err}"))
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        self.writer
            .flush()
            .await
            .map_err(|err| anyhow!("failed to flush the peer link: {err}"))
    }
}

/// An in-memory peer link.
///
/// The two halves exchange frames over bounded channels, so the handshake
/// and the session can be exercised without a socket and without a server.
#[cfg(test)]
pub(crate) struct MemoryPeer {
    /// The frames this half writes.
    outbound: tokio::sync::mpsc::Sender<PeerFrame>,
    /// The frames this half reads.
    inbound: tokio::sync::mpsc::Receiver<PeerFrame>,
}

/// The reading half of an in-memory peer link.
#[cfg(test)]
pub(crate) struct MemoryPeerReader {
    inbound: tokio::sync::mpsc::Receiver<PeerFrame>,
}

/// The writing half of an in-memory peer link.
#[cfg(test)]
pub(crate) struct MemoryPeerWriter {
    outbound: tokio::sync::mpsc::Sender<PeerFrame>,
}

#[cfg(test)]
impl MemoryPeer {
    /// Build the two halves of an in-memory link.
    ///
    /// The frames of one half reach the other half: the link is a pipe
    /// between the two sides, not two pipes into themselves.
    pub(crate) fn pair(buffer: usize) -> (MemoryPeer, MemoryPeer) {
        let (left_tx, right_rx) = tokio::sync::mpsc::channel(buffer);
        let (right_tx, left_rx) = tokio::sync::mpsc::channel(buffer);
        (
            MemoryPeer {
                outbound: left_tx,
                inbound: left_rx,
            },
            MemoryPeer {
                outbound: right_tx,
                inbound: right_rx,
            },
        )
    }

    /// Split the half into its reading and its writing part.
    pub(crate) fn into_halves(self) -> (MemoryPeerReader, MemoryPeerWriter) {
        (
            MemoryPeerReader {
                inbound: self.inbound,
            },
            MemoryPeerWriter {
                outbound: self.outbound,
            },
        )
    }
}

#[cfg(test)]
impl PeerReader for MemoryPeerReader {
    async fn recv(&mut self) -> anyhow::Result<Option<PeerFrame>> {
        Ok(self.inbound.recv().await)
    }
}

#[cfg(test)]
impl PeerWriter for MemoryPeerWriter {
    async fn send_binary(&mut self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.outbound
            .send(PeerFrame::Binary(bytes))
            .await
            .map_err(|_| anyhow!("the peer link is closed"))
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::peer_socket_config;
    use crate::bridge::wire::{BRIDGE_WS_MAX_FRAME_BYTES, BRIDGE_WS_MAX_MESSAGE_BYTES};

    #[test]
    fn the_peer_link_uses_the_limits_of_the_bridge_contract() {
        let config = peer_socket_config();
        assert_eq!(config.max_frame_size, Some(BRIDGE_WS_MAX_FRAME_BYTES));
        assert_eq!(config.max_message_size, Some(BRIDGE_WS_MAX_MESSAGE_BYTES));
    }
}
