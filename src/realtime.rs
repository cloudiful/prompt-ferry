use anyhow::{Context, anyhow};
use async_openai::types::realtime::{RealtimeClientEvent, RealtimeServerEvent};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    naming::REALTIME_CLIENT_SECRET_PREFIX,
    relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager},
};

const REALTIME_SECRET_VERSION: u8 = 1;
const DEFAULT_EXPIRES_SECONDS: u32 = 600;
const MIN_EXPIRES_SECONDS: u32 = 10;
const MAX_EXPIRES_SECONDS: u32 = 7_200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeSessionClaims {
    pub version: u8,
    pub user_id: Option<i64>,
    pub route_id: Option<String>,
    pub client_key_hash: Option<String>,
    pub model: Option<String>,
    pub session: serde_json::Value,
    pub expires_at: u64,
}

#[derive(Debug, Clone)]
pub struct RelayRealtimeClientSecret {
    pub value: String,
    pub expires_at: u64,
    pub session: serde_json::Value,
}

pub fn parse_client_event(text: &str) -> anyhow::Result<RealtimeClientEvent> {
    serde_json::from_str(text).context("failed to parse Realtime client event")
}

pub fn parse_server_event(text: &str) -> anyhow::Result<RealtimeServerEvent> {
    serde_json::from_str(text).context("failed to parse Realtime server event")
}

pub fn event_json<T: Serialize>(value: &T) -> anyhow::Result<String> {
    serde_json::to_string(value).context("failed to serialize Realtime event")
}

pub fn create_relay_client_secret(
    manager: &RelaySecretManager,
    mut claims: RealtimeSessionClaims,
    requested_seconds: Option<u32>,
) -> anyhow::Result<RelayRealtimeClientSecret> {
    claims.version = REALTIME_SECRET_VERSION;
    let expires_seconds = requested_seconds
        .unwrap_or(DEFAULT_EXPIRES_SECONDS)
        .clamp(MIN_EXPIRES_SECONDS, MAX_EXPIRES_SECONDS);
    let expires_at = (Utc::now() + Duration::seconds(i64::from(expires_seconds))).timestamp();
    claims.expires_at = expires_at.max(0) as u64;
    let plaintext = serde_json::to_string(&claims).context("failed to encode Realtime claims")?;
    let envelope = manager.encrypt(&plaintext)?;
    let value = encode_secret_envelope(&envelope);
    Ok(RelayRealtimeClientSecret {
        value,
        expires_at: claims.expires_at,
        session: claims.session,
    })
}

pub fn verify_relay_client_secret(
    manager: &RelaySecretManager,
    secret: &str,
) -> anyhow::Result<RealtimeSessionClaims> {
    let encoded = secret
        .strip_prefix(REALTIME_CLIENT_SECRET_PREFIX)
        .ok_or_else(|| anyhow!("invalid Realtime client secret prefix"))?;
    let envelope = decode_secret_envelope(encoded)?;
    let plaintext = manager.decrypt(&envelope)?;
    let claims: RealtimeSessionClaims =
        serde_json::from_str(&plaintext).context("failed to decode Realtime claims")?;
    if claims.version != REALTIME_SECRET_VERSION {
        return Err(anyhow!(
            "unsupported Realtime secret version {}",
            claims.version
        ));
    }
    let now = Utc::now().timestamp().max(0) as u64;
    if claims.expires_at < now {
        return Err(anyhow!("Realtime client secret has expired"));
    }
    Ok(claims)
}

fn encode_secret_envelope(envelope: &EncryptedSecretEnvelope) -> String {
    let payload = serde_json::json!({
        "v": envelope.key_version,
        "n": URL_SAFE_NO_PAD.encode(&envelope.nonce),
        "c": URL_SAFE_NO_PAD.encode(&envelope.ciphertext),
    });
    format!(
        "{REALTIME_CLIENT_SECRET_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(payload.to_string()),
    )
}

fn decode_secret_envelope(value: &str) -> anyhow::Result<EncryptedSecretEnvelope> {
    let payload = URL_SAFE_NO_PAD
        .decode(value)
        .context("failed to decode Realtime secret envelope")?;
    let json: serde_json::Value =
        serde_json::from_slice(&payload).context("failed to parse Realtime secret envelope")?;
    let key_version = json
        .get("v")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| anyhow!("Realtime secret envelope is missing key version"))?
        as i16;
    let nonce = json
        .get("n")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("Realtime secret envelope is missing nonce"))?;
    let ciphertext = json
        .get("c")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("Realtime secret envelope is missing ciphertext"))?;
    Ok(EncryptedSecretEnvelope {
        ciphertext: URL_SAFE_NO_PAD
            .decode(ciphertext)
            .context("failed to decode Realtime secret ciphertext")?,
        nonce: URL_SAFE_NO_PAD
            .decode(nonce)
            .context("failed to decode Realtime secret nonce")?,
        key_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;

    fn test_manager() -> RelaySecretManager {
        RelaySecretManager::from_base64(&STANDARD.encode([5_u8; 32])).expect("manager")
    }

    #[test]
    fn realtime_secret_round_trip() {
        let manager = test_manager();
        let secret = create_relay_client_secret(
            &manager,
            RealtimeSessionClaims {
                version: 0,
                user_id: Some(1),
                route_id: Some("route".to_string()),
                client_key_hash: Some("hash".to_string()),
                model: Some("gpt-realtime".to_string()),
                session: serde_json::json!({"type": "realtime"}),
                expires_at: 0,
            },
            Some(60),
        )
        .expect("create");

        let claims = verify_relay_client_secret(&manager, &secret.value).expect("verify");
        assert_eq!(claims.user_id, Some(1));
        assert_eq!(claims.model.as_deref(), Some("gpt-realtime"));
    }
}
