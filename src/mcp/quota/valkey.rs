use chrono::{DateTime, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};
use tracing::warn;

use crate::config::WorkerConfig;

pub const MCP_QUOTA_VALKEY_KEY_PREFIX: &str = "pfy:mcp-quota:";
const COOLDOWN_SUFFIX: &str = ":cooldown";
const REMAINING_SUFFIX: &str = ":remaining";

/// Valkey-backed hot state for MCP credential quota. This is an acceleration
/// layer only: PostgreSQL remains the authoritative budget ledger, so a
/// Valkey outage or restart never changes what is spent.
#[derive(Clone, Default)]
pub struct McpQuotaValkey {
    manager: Option<ConnectionManager>,
}

impl McpQuotaValkey {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn from_config(config: &WorkerConfig) -> Self {
        let url = config.valkey_url.trim();
        if url.is_empty() {
            return Self::new();
        }
        let client = match redis::Client::open(url) {
            Ok(client) => client,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to open valkey client for MCP quota");
                return Self::new();
            }
        };
        let manager = match client.get_connection_manager().await {
            Ok(manager) => manager,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to connect valkey for MCP quota");
                return Self::new();
            }
        };
        Self {
            manager: Some(manager),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.manager.is_some()
    }

    /// Returns the cached cooldown deadline, or `None` when Valkey is not
    /// configured or the key is absent.
    pub async fn cooldown_until(&self, credential_id: uuid::Uuid) -> Option<DateTime<Utc>> {
        let mut manager = self.manager.clone()?;
        let key = cooldown_key(credential_id);
        let value: Option<String> = match manager.get(&key).await {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, credential_id = %credential_id, "failed to read MCP quota cooldown from valkey");
                return None;
            }
        };
        let millis: i64 = value?.parse().ok()?;
        DateTime::from_timestamp_millis(millis)
    }

    pub async fn set_cooldown(&self, credential_id: uuid::Uuid, until: DateTime<Utc>) {
        let Some(manager) = self.manager.as_ref() else {
            return;
        };
        let mut manager = manager.clone();
        let key = cooldown_key(credential_id);
        let ttl = (until - Utc::now()).num_seconds().max(1) as u64;
        if let Err(err) = manager
            .set_ex::<_, _, ()>(key, until.timestamp_millis(), ttl)
            .await
        {
            warn!(error = %err, credential_id = %credential_id, "failed to write MCP quota cooldown to valkey");
        }
    }

    pub async fn set_provider_remaining(
        &self,
        credential_id: uuid::Uuid,
        remaining: f64,
        reset_at: Option<DateTime<Utc>>,
    ) {
        let Some(manager) = self.manager.as_ref() else {
            return;
        };
        let mut manager = manager.clone();
        let key = remaining_key(credential_id);
        let ttl = reset_at
            .map(|reset| (reset - Utc::now()).num_seconds())
            .unwrap_or(3600)
            .max(60) as u64;
        let payload = format!(
            "{}|{}",
            remaining,
            reset_at
                .map(|value| value.timestamp_millis())
                .unwrap_or_default()
        );
        if let Err(err) = manager.set_ex::<_, _, ()>(key, payload, ttl).await {
            warn!(error = %err, credential_id = %credential_id, "failed to write MCP quota remaining to valkey");
        }
    }
}

fn cooldown_key(credential_id: uuid::Uuid) -> String {
    format!("{MCP_QUOTA_VALKEY_KEY_PREFIX}{credential_id}{COOLDOWN_SUFFIX}")
}

fn remaining_key(credential_id: uuid::Uuid) -> String {
    format!("{MCP_QUOTA_VALKEY_KEY_PREFIX}{credential_id}{REMAINING_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prefixes_are_namespaced() {
        let id = uuid::Uuid::new_v4();
        assert!(cooldown_key(id).starts_with(MCP_QUOTA_VALKEY_KEY_PREFIX));
        assert!(cooldown_key(id).ends_with(COOLDOWN_SUFFIX));
        assert!(remaining_key(id).ends_with(REMAINING_SUFFIX));
    }
}
