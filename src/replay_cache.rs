use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

use crate::{
    config::WorkerConfig, db, usage_prompt::PromptMessageRef, worker_admin_types::SessionUser,
};

pub const REPLAY_VALKEY_KEY_PREFIX: &str = "pfy:replay:snapshot:";
pub const SESSION_VALKEY_KEY_PREFIX: &str = "pfy:session:";
pub const REQUEST_LEASE_VALKEY_KEY_PREFIX: &str = "pfy:req-lease:";
pub const REPLAY_PG_TURN_THRESHOLD: i32 = 16;
pub const REPLAY_PG_BYTES_THRESHOLD: usize = 64 * 1024;

#[derive(Clone)]
pub struct ReplayCache {
    backend: ReplayCacheBackend,
}

#[derive(Clone)]
enum ReplayCacheBackend {
    Disabled,
    Redis(Arc<RedisBackend>),
    Local(Arc<LocalBackend>),
}

struct RedisBackend {
    manager: ConnectionManager,
    replay_ttl_seconds: u64,
    session_ttl_seconds: u64,
}

struct LocalBackend {
    sessions: Mutex<HashMap<String, LocalSessionEntry>>,
    session_ttl: Duration,
    max_session_entries: usize,
    replay_snapshots: Mutex<HashMap<Uuid, ReplaySnapshotValue>>,
}

struct LocalSessionEntry {
    user: SessionUser,
    expires_at: Instant,
    last_access: u64,
}

impl LocalBackend {
    fn new(session_ttl_seconds: u64, max_session_entries: usize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            session_ttl: Duration::from_secs(session_ttl_seconds.max(1)),
            max_session_entries: max_session_entries.max(1),
            replay_snapshots: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplaySnapshotValue {
    pub conversation_id: Uuid,
    pub base_event_id: i64,
    pub conversation_seq: i32,
    pub prompt_refs: Vec<PromptMessageRef>,
    pub ref_count: i32,
    pub byte_size: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ReplaySnapshotUpdate {
    pub event_id: i64,
    pub conversation_id: Uuid,
    pub conversation_seq: i32,
    pub prompt_refs: Vec<PromptMessageRef>,
}

impl ReplaySnapshotUpdate {
    pub fn byte_size(&self) -> Result<i32> {
        let bytes = serde_json::to_vec(&self.prompt_refs)?;
        Ok(i32::try_from(bytes.len()).unwrap_or(i32::MAX))
    }
}

impl Default for ReplayCache {
    fn default() -> Self {
        Self {
            backend: ReplayCacheBackend::Disabled,
        }
    }
}

impl ReplayCache {
    fn local_sessions_only(config: &WorkerConfig, reason: &str) -> Self {
        warn!(
            backend = "local",
            reason,
            max_entries = config.local_session_max_entries,
            ttl_seconds = config.session_ttl_seconds,
            "using local session fallback; backend will not switch at runtime"
        );
        Self {
            backend: ReplayCacheBackend::Local(Arc::new(LocalBackend::new(
                config.session_ttl_seconds,
                config.local_session_max_entries,
            ))),
        }
    }

    pub async fn from_config(config: &WorkerConfig) -> Self {
        let url = config.valkey_url.trim();
        if url.is_empty() {
            return Self::local_sessions_only(config, "valkey_not_configured");
        }
        let client = match redis::Client::open(url) {
            Ok(client) => client,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to open valkey client; falling back to local in-memory sessions");
                return Self::local_sessions_only(config, "valkey_client_open_failed");
            }
        };
        let manager = match client.get_connection_manager().await {
            Ok(manager) => manager,
            Err(err) => {
                warn!(error = %err, valkey_url = url, "failed to connect valkey; falling back to local in-memory sessions");
                return Self::local_sessions_only(config, "valkey_connection_failed");
            }
        };
        Self {
            backend: ReplayCacheBackend::Redis(Arc::new(RedisBackend {
                manager,
                replay_ttl_seconds: config.valkey_ttl_seconds,
                session_ttl_seconds: config.session_ttl_seconds,
            })),
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self.backend, ReplayCacheBackend::Redis(_))
    }

    pub fn session_available(&self) -> bool {
        !matches!(self.backend, ReplayCacheBackend::Disabled)
    }

    pub fn for_tests() -> Self {
        Self {
            backend: ReplayCacheBackend::Local(Arc::new(LocalBackend::new(
                7 * 24 * 60 * 60,
                10_000,
            ))),
        }
    }

    pub async fn get_snapshot(&self, conversation_id: Uuid) -> Result<Option<ReplaySnapshotValue>> {
        match &self.backend {
            ReplayCacheBackend::Disabled => Ok(None),
            ReplayCacheBackend::Local(inner) => Ok(inner
                .replay_snapshots
                .lock()
                .await
                .get(&conversation_id)
                .cloned()),
            ReplayCacheBackend::Redis(inner) => {
                let key = replay_cache_key(conversation_id);
                let mut manager = inner.manager.clone();
                let value: Option<String> = manager.get(key).await?;
                value
                    .map(|text| {
                        serde_json::from_str(&text).context("invalid replay valkey snapshot json")
                    })
                    .transpose()
            }
        }
    }

    pub async fn write_snapshot_if_newer(&self, update: &ReplaySnapshotUpdate) -> Result<bool> {
        match &self.backend {
            ReplayCacheBackend::Disabled => Ok(false),
            ReplayCacheBackend::Local(inner) => {
                let next = ReplaySnapshotValue {
                    conversation_id: update.conversation_id,
                    base_event_id: update.event_id,
                    conversation_seq: update.conversation_seq,
                    prompt_refs: update.prompt_refs.clone(),
                    ref_count: i32::try_from(update.prompt_refs.len()).unwrap_or(i32::MAX),
                    byte_size: update.byte_size()?,
                    updated_at: Utc::now(),
                };
                let mut snapshots = inner.replay_snapshots.lock().await;
                if let Some(current) = snapshots.get(&update.conversation_id)
                    && (current.conversation_seq > next.conversation_seq
                        || (current.conversation_seq == next.conversation_seq
                            && current.base_event_id >= next.base_event_id))
                {
                    return Ok(false);
                }
                snapshots.insert(update.conversation_id, next);
                Ok(true)
            }
            ReplayCacheBackend::Redis(inner) => {
                let next = ReplaySnapshotValue {
                    conversation_id: update.conversation_id,
                    base_event_id: update.event_id,
                    conversation_seq: update.conversation_seq,
                    prompt_refs: update.prompt_refs.clone(),
                    ref_count: i32::try_from(update.prompt_refs.len()).unwrap_or(i32::MAX),
                    byte_size: update.byte_size()?,
                    updated_at: Utc::now(),
                };
                let key = replay_cache_key(update.conversation_id);
                let mut manager = inner.manager.clone();
                let current: Option<String> = manager.get(&key).await?;
                if let Some(current) = current {
                    let parsed: ReplaySnapshotValue = serde_json::from_str(&current)
                        .context("invalid replay valkey snapshot json")?;
                    if parsed.conversation_seq > next.conversation_seq
                        || (parsed.conversation_seq == next.conversation_seq
                            && parsed.base_event_id >= next.base_event_id)
                    {
                        return Ok(false);
                    }
                }
                let payload = serde_json::to_string(&next)?;
                let _: () = manager
                    .set_ex(key, payload, inner.replay_ttl_seconds)
                    .await
                    .context("failed to write replay valkey snapshot")?;
                Ok(true)
            }
        }
    }

    pub async fn write_session(&self, session_id: &str, user: &SessionUser) -> Result<()> {
        match &self.backend {
            ReplayCacheBackend::Disabled => Err(anyhow!("session backend unavailable")),
            ReplayCacheBackend::Local(inner) => {
                let mut sessions = inner.sessions.lock().await;
                let now = Instant::now();
                let before_cleanup = sessions.len();
                sessions.retain(|_, entry| entry.expires_at > now);
                let expired_count = before_cleanup.saturating_sub(sessions.len());
                let access = sessions
                    .values()
                    .map(|entry| entry.last_access)
                    .max()
                    .unwrap_or_default()
                    .saturating_add(1);
                if sessions.len() >= inner.max_session_entries
                    && !sessions.contains_key(session_id)
                    && let Some(oldest) = sessions
                        .iter()
                        .min_by_key(|(_, entry)| entry.last_access)
                        .map(|(id, _)| id.clone())
                {
                    sessions.remove(&oldest);
                    warn!(evicted = 1, "local session capacity eviction");
                }
                sessions.insert(
                    session_id.to_string(),
                    LocalSessionEntry {
                        user: user.clone(),
                        expires_at: now + inner.session_ttl,
                        last_access: access,
                    },
                );
                if expired_count != 0 {
                    tracing::debug!(expired = expired_count, "local session expiry cleanup");
                }
                Ok(())
            }
            ReplayCacheBackend::Redis(inner) => {
                let key = session_cache_key(session_id);
                let payload = serde_json::to_string(user)?;
                let mut manager = inner.manager.clone();
                let _: () = manager
                    .set_ex(key, payload, inner.session_ttl_seconds)
                    .await
                    .context("failed to write valkey session")?;
                Ok(())
            }
        }
    }

    pub async fn read_session_refresh(&self, session_id: &str) -> Result<Option<SessionUser>> {
        match &self.backend {
            ReplayCacheBackend::Disabled => Err(anyhow!("session backend unavailable")),
            ReplayCacheBackend::Local(inner) => {
                let mut sessions = inner.sessions.lock().await;
                let now = Instant::now();
                let expired_ids = sessions
                    .iter()
                    .filter(|(_, entry)| entry.expires_at <= now)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                for id in &expired_ids {
                    sessions.remove(id);
                }
                let next_access = sessions
                    .values()
                    .map(|value| value.last_access)
                    .max()
                    .unwrap_or_default()
                    .saturating_add(1);
                let Some(entry) = sessions.get_mut(session_id) else {
                    if !expired_ids.is_empty() {
                        tracing::debug!(
                            expired = expired_ids.len(),
                            "local session expiry cleanup"
                        );
                    }
                    return Ok(None);
                };
                entry.last_access = next_access;
                entry.expires_at = now + inner.session_ttl;
                if !expired_ids.is_empty() {
                    tracing::debug!(expired = expired_ids.len(), "local session expiry cleanup");
                }
                Ok(Some(entry.user.clone()))
            }
            ReplayCacheBackend::Redis(inner) => {
                let key = session_cache_key(session_id);
                let mut manager = inner.manager.clone();
                let value: Option<String> = manager.get(&key).await?;
                let Some(value) = value else {
                    return Ok(None);
                };
                let user: SessionUser =
                    serde_json::from_str(&value).context("invalid valkey session json")?;
                let _: bool = manager
                    .expire(
                        &key,
                        i64::try_from(inner.session_ttl_seconds).unwrap_or(i64::MAX),
                    )
                    .await
                    .context("failed to refresh valkey session ttl")?;
                Ok(Some(user))
            }
        }
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        match &self.backend {
            ReplayCacheBackend::Disabled => Err(anyhow!("session backend unavailable")),
            ReplayCacheBackend::Local(inner) => {
                inner.sessions.lock().await.remove(session_id);
                Ok(())
            }
            ReplayCacheBackend::Redis(inner) => {
                let key = session_cache_key(session_id);
                let mut manager = inner.manager.clone();
                let _: usize = manager
                    .del(key)
                    .await
                    .context("failed to delete valkey session")?;
                Ok(())
            }
        }
    }

    pub async fn write_request_lease(
        &self,
        request_id: Uuid,
        worker_id: Uuid,
        ttl_seconds: u64,
    ) -> Result<bool> {
        match &self.backend {
            ReplayCacheBackend::Disabled | ReplayCacheBackend::Local(_) => Ok(false),
            ReplayCacheBackend::Redis(inner) => {
                let key = request_lease_cache_key(request_id);
                let mut manager = inner.manager.clone();
                let _: () = manager
                    .set_ex(key, worker_id.to_string(), ttl_seconds)
                    .await
                    .context("failed to write valkey request lease")?;
                Ok(true)
            }
        }
    }

    pub async fn refresh_request_lease(&self, request_id: Uuid, ttl_seconds: u64) -> Result<bool> {
        match &self.backend {
            ReplayCacheBackend::Disabled | ReplayCacheBackend::Local(_) => Ok(false),
            ReplayCacheBackend::Redis(inner) => {
                let key = request_lease_cache_key(request_id);
                let mut manager = inner.manager.clone();
                let exists: bool = manager.exists(&key).await?;
                if !exists {
                    return Ok(false);
                }
                let _: bool = manager
                    .expire(&key, i64::try_from(ttl_seconds).unwrap_or(i64::MAX))
                    .await
                    .context("failed to refresh valkey request lease ttl")?;
                Ok(true)
            }
        }
    }

    pub async fn delete_request_lease(&self, request_id: Uuid) -> Result<bool> {
        match &self.backend {
            ReplayCacheBackend::Disabled | ReplayCacheBackend::Local(_) => Ok(false),
            ReplayCacheBackend::Redis(inner) => {
                let key = request_lease_cache_key(request_id);
                let mut manager = inner.manager.clone();
                let deleted: usize = manager
                    .del(key)
                    .await
                    .context("failed to delete valkey request lease")?;
                Ok(deleted > 0)
            }
        }
    }

    pub async fn request_lease_exists(&self, request_id: Uuid) -> Result<Option<bool>> {
        match &self.backend {
            ReplayCacheBackend::Disabled | ReplayCacheBackend::Local(_) => Ok(None),
            ReplayCacheBackend::Redis(inner) => {
                let key = request_lease_cache_key(request_id);
                let mut manager = inner.manager.clone();
                let exists: bool = manager
                    .exists(key)
                    .await
                    .context("failed to check valkey request lease")?;
                Ok(Some(exists))
            }
        }
    }

    pub async fn replace_snapshot_for_tests(&self, snapshot: ReplaySnapshotValue) -> Result<()> {
        match &self.backend {
            ReplayCacheBackend::Local(inner) => {
                inner
                    .replay_snapshots
                    .lock()
                    .await
                    .insert(snapshot.conversation_id, snapshot);
                Ok(())
            }
            ReplayCacheBackend::Disabled | ReplayCacheBackend::Redis(_) => Err(anyhow!(
                "test snapshot injection requires local replay cache backend"
            )),
        }
    }
}

pub async fn update_replay_state(
    pool: &PgPool,
    replay_cache: &ReplayCache,
    update: ReplaySnapshotUpdate,
) {
    if replay_cache.enabled()
        && let Err(err) = replay_cache.write_snapshot_if_newer(&update).await
    {
        warn!(error = %err, conversation_id = %update.conversation_id, "failed to update replay valkey snapshot");
    }

    match db::latest_replay_snapshot(pool, update.conversation_id).await {
        Ok(latest) => {
            let latest_seq = latest.as_ref().map(|row| row.conversation_seq).unwrap_or(0);
            let turn_gap = update.conversation_seq - latest_seq;
            let byte_size = match update.byte_size() {
                Ok(value) => value,
                Err(err) => {
                    warn!(error = %err, conversation_id = %update.conversation_id, "failed to measure replay snapshot bytes");
                    return;
                }
            };
            let should_persist = latest.is_none()
                || turn_gap >= REPLAY_PG_TURN_THRESHOLD
                || usize::try_from(byte_size).unwrap_or(usize::MAX) >= REPLAY_PG_BYTES_THRESHOLD;
            if !should_persist {
                return;
            }
            if let Err(err) = db::insert_replay_snapshot(
                pool,
                db::ReplaySnapshotCreate {
                    event_id: update.event_id,
                    conversation_id: update.conversation_id,
                    conversation_seq: update.conversation_seq,
                    base_event_id: update.event_id,
                    prompt_refs_json: serde_json::to_value(&update.prompt_refs).unwrap_or_default(),
                    ref_count: i32::try_from(update.prompt_refs.len()).unwrap_or(i32::MAX),
                    byte_size,
                },
            )
            .await
            {
                warn!(error = %err, conversation_id = %update.conversation_id, "failed to persist replay snapshot");
            }
        }
        Err(err) => {
            warn!(error = %err, conversation_id = %update.conversation_id, "failed to load latest replay snapshot")
        }
    }
}

fn replay_cache_key(conversation_id: Uuid) -> String {
    format!("{REPLAY_VALKEY_KEY_PREFIX}{conversation_id}")
}

fn session_cache_key(session_id: &str) -> String {
    format!("{SESSION_VALKEY_KEY_PREFIX}{session_id}")
}

fn request_lease_cache_key(request_id: Uuid) -> String {
    format!("{REQUEST_LEASE_VALKEY_KEY_PREFIX}{request_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_snapshot_wins_by_seq_then_event_id() {
        let conversation_id = Uuid::new_v4();
        let current = ReplaySnapshotValue {
            conversation_id,
            base_event_id: 10,
            conversation_seq: 3,
            prompt_refs: Vec::new(),
            ref_count: 0,
            byte_size: 0,
            updated_at: Utc::now(),
        };
        let older_seq = ReplaySnapshotUpdate {
            event_id: 11,
            conversation_id,
            conversation_seq: 2,
            prompt_refs: Vec::new(),
        };
        let same_seq_lower_event = ReplaySnapshotUpdate {
            event_id: 9,
            conversation_id,
            conversation_seq: 3,
            prompt_refs: Vec::new(),
        };
        let newer = ReplaySnapshotUpdate {
            event_id: 12,
            conversation_id,
            conversation_seq: 4,
            prompt_refs: Vec::new(),
        };

        assert!(current.conversation_seq > older_seq.conversation_seq);
        assert!(
            current.conversation_seq == same_seq_lower_event.conversation_seq
                && current.base_event_id > same_seq_lower_event.event_id
        );
        assert!(newer.conversation_seq > current.conversation_seq);
    }
}
