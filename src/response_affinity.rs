use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

pub(crate) const RESPONSE_AFFINITY_VALKEY_KEY_PREFIX: &str = "pfy:responses-affinity:";

const GET_OR_CREATE_SCRIPT: &str = r#"
local payload = redis.call('GET', KEYS[1])
if payload then
    redis.call('EXPIRE', KEYS[1], ARGV[2])
    return payload
end
redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
return ARGV[1]
"#;

const GET_AND_REFRESH_SCRIPT: &str = r#"
local payload = redis.call('GET', KEYS[1])
if not payload then
    return nil
end
redis.call('EXPIRE', KEYS[1], ARGV[1])
return payload
"#;

const REPLACE_IF_MATCH_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[1])
if not current or current ~= ARGV[1] then
    return 0
end
redis.call('SET', KEYS[1], ARGV[2], 'EX', ARGV[3])
return 1
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseAffinityBinding {
    pub endpoint_id: Uuid,
    pub endpoint_key_id: Option<Uuid>,
    pub endpoint_key_fingerprint: String,
}

#[derive(Clone)]
pub struct ResponseAffinityStore {
    backend: ResponseAffinityBackend,
}

#[derive(Clone)]
enum ResponseAffinityBackend {
    Unavailable,
    Redis(Arc<RedisAffinityBackend>),
    Local(Arc<LocalAffinityBackend>),
}

struct RedisAffinityBackend {
    manager: ConnectionManager,
    ttl_seconds: u64,
}

struct LocalAffinityBackend {
    bindings: Mutex<HashMap<String, LocalAffinityEntry>>,
    ttl: Duration,
}

struct LocalAffinityEntry {
    binding: ResponseAffinityBinding,
    expires_at: Instant,
}

impl Default for ResponseAffinityStore {
    fn default() -> Self {
        Self::unavailable()
    }
}

impl ResponseAffinityStore {
    pub fn unavailable() -> Self {
        Self {
            backend: ResponseAffinityBackend::Unavailable,
        }
    }

    pub fn for_tests() -> Self {
        Self {
            backend: ResponseAffinityBackend::Local(Arc::new(LocalAffinityBackend {
                bindings: Mutex::new(HashMap::new()),
                ttl: Duration::from_secs(7 * 24 * 60 * 60),
            })),
        }
    }

    pub fn from_connection_manager(manager: ConnectionManager, ttl_seconds: u64) -> Self {
        Self {
            backend: ResponseAffinityBackend::Redis(Arc::new(RedisAffinityBackend {
                manager,
                ttl_seconds: ttl_seconds.max(1),
            })),
        }
    }

    pub fn cache_key(user_id: i64, rule_id: Uuid, stable_identity: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(user_id.to_string().as_bytes());
        hasher.update([0]);
        hasher.update(rule_id.as_bytes());
        hasher.update([0]);
        hasher.update(stable_identity.as_bytes());
        format!(
            "{RESPONSE_AFFINITY_VALKEY_KEY_PREFIX}{}",
            hex_digest(&hasher.finalize())
        )
    }

    pub async fn get(&self, key: &str) -> Result<Option<ResponseAffinityBinding>> {
        match &self.backend {
            ResponseAffinityBackend::Unavailable => {
                Err(anyhow!("response affinity backend unavailable"))
            }
            ResponseAffinityBackend::Local(inner) => {
                let mut bindings = inner.bindings.lock().await;
                let Some(entry) = bindings.get_mut(key) else {
                    return Ok(None);
                };
                if entry.expires_at <= Instant::now() {
                    bindings.remove(key);
                    return Ok(None);
                }
                entry.expires_at = Instant::now() + inner.ttl;
                Ok(Some(entry.binding.clone()))
            }
            ResponseAffinityBackend::Redis(inner) => {
                let mut manager = inner.manager.clone();
                let payload: Option<String> = redis::Script::new(GET_AND_REFRESH_SCRIPT)
                    .key(key)
                    .arg(inner.ttl_seconds)
                    .invoke_async(&mut manager)
                    .await
                    .context("failed to read response affinity binding")?;
                let Some(payload) = payload else {
                    return Ok(None);
                };
                let binding =
                    serde_json::from_str(&payload).context("invalid response affinity binding")?;
                Ok(Some(binding))
            }
        }
    }

    pub async fn get_or_create(
        &self,
        key: &str,
        candidate: &ResponseAffinityBinding,
    ) -> Result<ResponseAffinityBinding> {
        match &self.backend {
            ResponseAffinityBackend::Unavailable => {
                Err(anyhow!("response affinity backend unavailable"))
            }
            ResponseAffinityBackend::Local(inner) => {
                let mut bindings = inner.bindings.lock().await;
                let now = Instant::now();
                if let Some(entry) = bindings.get_mut(key) {
                    if entry.expires_at > now {
                        entry.expires_at = now + inner.ttl;
                        return Ok(entry.binding.clone());
                    }
                }
                bindings.insert(
                    key.to_string(),
                    LocalAffinityEntry {
                        binding: candidate.clone(),
                        expires_at: now + inner.ttl,
                    },
                );
                Ok(candidate.clone())
            }
            ResponseAffinityBackend::Redis(inner) => {
                let payload = serde_json::to_string(candidate)?;
                let mut manager = inner.manager.clone();
                let payload: String = redis::Script::new(GET_OR_CREATE_SCRIPT)
                    .key(key)
                    .arg(payload)
                    .arg(inner.ttl_seconds)
                    .invoke_async(&mut manager)
                    .await
                    .context("failed to get or create response affinity binding")?;
                serde_json::from_str(&payload)
                    .context("invalid response affinity binding returned by Valkey")
            }
        }
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        match &self.backend {
            ResponseAffinityBackend::Unavailable => {
                Err(anyhow!("response affinity backend unavailable"))
            }
            ResponseAffinityBackend::Local(inner) => {
                inner.bindings.lock().await.remove(key);
                Ok(())
            }
            ResponseAffinityBackend::Redis(inner) => {
                let mut manager = inner.manager.clone();
                let _: usize = manager
                    .del(key)
                    .await
                    .context("failed to delete response affinity binding")?;
                Ok(())
            }
        }
    }

    pub async fn replace_if_current(
        &self,
        key: &str,
        expected: &ResponseAffinityBinding,
        replacement: &ResponseAffinityBinding,
    ) -> Result<bool> {
        match &self.backend {
            ResponseAffinityBackend::Unavailable => {
                Err(anyhow!("response affinity backend unavailable"))
            }
            ResponseAffinityBackend::Local(inner) => {
                let mut bindings = inner.bindings.lock().await;
                let now = Instant::now();
                let Some(entry) = bindings.get_mut(key) else {
                    return Ok(false);
                };
                if entry.expires_at <= now {
                    bindings.remove(key);
                    return Ok(false);
                }
                if entry.binding != *expected {
                    return Ok(false);
                }
                entry.binding = replacement.clone();
                entry.expires_at = now + inner.ttl;
                Ok(true)
            }
            ResponseAffinityBackend::Redis(inner) => {
                let expected_payload = serde_json::to_string(expected)?;
                let replacement_payload = serde_json::to_string(replacement)?;
                let mut manager = inner.manager.clone();
                let replaced: i64 = redis::Script::new(REPLACE_IF_MATCH_SCRIPT)
                    .key(key)
                    .arg(expected_payload)
                    .arg(replacement_payload)
                    .arg(inner.ttl_seconds)
                    .invoke_async(&mut manager)
                    .await
                    .context("failed to replace response affinity binding")?;
                Ok(replaced == 1)
            }
        }
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn api_key_fingerprint(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hex_digest(&hasher.finalize())
}

pub(crate) fn log_unavailable(error: &anyhow::Error) {
    warn!(error = %error, "response affinity backend unavailable");
}

#[cfg(test)]
mod tests;
