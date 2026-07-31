use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

pub(crate) const RESPONSE_AFFINITY_VALKEY_KEY_PREFIX: &str = "pfy:responses-affinity:";

const BIND_IF_ABSENT_SCRIPT: &str = r#"
if redis.call('EXISTS', KEYS[1]) == 1 then
    return 0
end
redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
return 1
"#;

const GET_AND_REFRESH_SCRIPT: &str = r#"
local payload = redis.call('GET', KEYS[1])
if not payload then
    return nil
end
redis.call('EXPIRE', KEYS[1], ARGV[1])
return payload
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ResponseAffinityBinding {
    pub(crate) endpoint_id: Uuid,
    pub(crate) endpoint_key_id: Option<Uuid>,
    pub(crate) endpoint_key_fingerprint: String,
}

#[derive(Clone)]
pub(crate) struct ResponseAffinityStore {
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
    pub(crate) fn unavailable() -> Self {
        Self {
            backend: ResponseAffinityBackend::Unavailable,
        }
    }

    pub(crate) fn for_tests() -> Self {
        Self {
            backend: ResponseAffinityBackend::Local(Arc::new(LocalAffinityBackend {
                bindings: Mutex::new(HashMap::new()),
                ttl: Duration::from_secs(7 * 24 * 60 * 60),
            })),
        }
    }

    pub(crate) fn from_connection_manager(manager: ConnectionManager, ttl_seconds: u64) -> Self {
        Self {
            backend: ResponseAffinityBackend::Redis(Arc::new(RedisAffinityBackend {
                manager,
                ttl_seconds: ttl_seconds.max(1),
            })),
        }
    }

    pub(crate) fn cache_key(user_id: i64, rule_id: Uuid, stable_identity: &str) -> String {
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

    pub(crate) async fn get(&self, key: &str) -> Result<Option<ResponseAffinityBinding>> {
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

    pub(crate) async fn get_or_create(
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
                if let Some(binding) = self.get(key).await? {
                    return Ok(binding);
                }
                let payload = serde_json::to_string(candidate)?;
                let mut manager = inner.manager.clone();
                let created: i64 = redis::Script::new(BIND_IF_ABSENT_SCRIPT)
                    .key(key)
                    .arg(payload)
                    .arg(inner.ttl_seconds)
                    .invoke_async(&mut manager)
                    .await
                    .context("failed to create response affinity binding")?;
                if created == 1 {
                    return Ok(candidate.clone());
                }
                self.get(key).await?.ok_or_else(|| {
                    anyhow!("response affinity binding disappeared after creation race")
                })
            }
        }
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn api_key_fingerprint(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hex_digest(&hasher.finalize())
}

pub(crate) fn log_unavailable(error: &anyhow::Error) {
    warn!(error = %error, "response affinity backend unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(endpoint_id: Uuid) -> ResponseAffinityBinding {
        ResponseAffinityBinding {
            endpoint_id,
            endpoint_key_id: None,
            endpoint_key_fingerprint: "fingerprint".to_string(),
        }
    }

    #[tokio::test]
    async fn local_store_keeps_first_binding_for_concurrent_creators() {
        let store = ResponseAffinityStore::for_tests();
        let key = "affinity-key";
        let first = binding(Uuid::new_v4());
        let second = binding(Uuid::new_v4());
        let (left, right) = tokio::join!(
            store.get_or_create(key, &first),
            store.get_or_create(key, &second)
        );
        let left = left.unwrap();
        let right = right.unwrap();
        assert_eq!(left, right);
        assert!(left == first || left == second);
    }

    #[test]
    fn cache_key_is_hashed_and_scope_sensitive() {
        let rule_id = Uuid::new_v4();
        let first = ResponseAffinityStore::cache_key(1, rule_id, "session-a");
        let second = ResponseAffinityStore::cache_key(1, rule_id, "session-b");
        assert!(first.starts_with(RESPONSE_AFFINITY_VALKEY_KEY_PREFIX));
        assert_ne!(first, second);
        assert!(!first.contains("session-a"));
    }
}
