use anyhow::{Context, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::config::BridgeEncryptionMode;

use super::{ALG, HANDSHAKE_NONCE_BYTES, KEY_BYTES, VERSION};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptionHandshake {
    pub version: u8,
    pub alg: String,
    pub nonce: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum BridgeWireMessage {
    EncryptionHello(EncryptionHandshake),
    EncryptionReady(EncryptionHandshake),
}

pub fn validate_settings(
    component: &'static str,
    mode: BridgeEncryptionMode,
    key: &str,
) -> anyhow::Result<()> {
    if mode == BridgeEncryptionMode::Off {
        return Ok(());
    }
    parse_key(key).with_context(|| format!("{component} bridge_encryption_key is invalid"))?;
    Ok(())
}

pub fn encode_hello(nonce: &[u8; HANDSHAKE_NONCE_BYTES]) -> anyhow::Result<String> {
    encode_wire(BridgeWireMessage::EncryptionHello(handshake(nonce)))
}

pub fn encode_ready(nonce: &[u8; HANDSHAKE_NONCE_BYTES]) -> anyhow::Result<String> {
    encode_wire(BridgeWireMessage::EncryptionReady(handshake(nonce)))
}

pub fn decode_hello(text: &str) -> anyhow::Result<[u8; HANDSHAKE_NONCE_BYTES]> {
    match decode_wire(text)? {
        BridgeWireMessage::EncryptionHello(handshake) => decode_handshake(handshake),
        _ => Err(anyhow!("expected encryption hello")),
    }
}

pub fn decode_ready(text: &str) -> anyhow::Result<[u8; HANDSHAKE_NONCE_BYTES]> {
    match decode_wire(text)? {
        BridgeWireMessage::EncryptionReady(handshake) => decode_handshake(handshake),
        _ => Err(anyhow!("expected encryption ready")),
    }
}

pub fn random_handshake_nonce() -> [u8; HANDSHAKE_NONCE_BYTES] {
    let mut nonce = [0_u8; HANDSHAKE_NONCE_BYTES];
    rand::rng().fill_bytes(&mut nonce);
    nonce
}

pub(super) fn parse_key(key: &str) -> anyhow::Result<[u8; KEY_BYTES]> {
    if key.trim().is_empty() {
        return Err(anyhow!("bridge encryption key is required"));
    }
    decode_fixed(key.trim(), "bridge encryption key")
}

fn handshake(nonce: &[u8; HANDSHAKE_NONCE_BYTES]) -> EncryptionHandshake {
    EncryptionHandshake {
        version: VERSION,
        alg: ALG.to_string(),
        nonce: STANDARD.encode(nonce),
    }
}

fn decode_handshake(handshake: EncryptionHandshake) -> anyhow::Result<[u8; HANDSHAKE_NONCE_BYTES]> {
    if handshake.version != VERSION {
        return Err(anyhow!(
            "unsupported bridge encryption version {}",
            handshake.version
        ));
    }
    if handshake.alg != ALG {
        return Err(anyhow!(
            "unsupported bridge encryption algorithm {}",
            handshake.alg
        ));
    }
    decode_fixed(&handshake.nonce, "handshake nonce")
}

fn encode_wire(message: BridgeWireMessage) -> anyhow::Result<String> {
    serde_json::to_string(&message).context("failed to encode bridge wire message")
}

fn decode_wire(text: &str) -> anyhow::Result<BridgeWireMessage> {
    serde_json::from_str(text).context("failed to decode bridge wire message")
}

fn decode_fixed<const N: usize>(value: &str, label: &'static str) -> anyhow::Result<[u8; N]> {
    let bytes = STANDARD
        .decode(value.as_bytes())
        .with_context(|| format!("invalid base64 {label}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow!("{label} must be {N} bytes, got {}", bytes.len()))
}
