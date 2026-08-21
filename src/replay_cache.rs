use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, SqlitePool};
use tokio::sync::Mutex;
use tracing::warn;
use uuid::Uuid;

use crate::{
    config::WorkerConfig, db, response_affinity::ResponseAffinityStore,
    usage_prompt::PromptMessageRef, worker_admin_types::SessionUser,
};

pub const REPLAY_VALKEY_KEY_PREFIX: &str = "pfy:replay:snapshot:";
pub const SESSION_VALKEY_KEY_PREFIX: &str = "pfy:session:";
pub const REQUEST_LEASE_VALKEY_KEY_PREFIX: &str = "pfy:req-lease:";
pub const REPLAY_PG_TURN_THRESHOLD: i32 = 16;
pub const REPLAY_PG_BYTES_THRESHOLD: usize = 64 * 1024;

const REPLAY_SNAPSHOT_CAS_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[1])
if current then
    local current_value = cjson.decode(current)
    local next_value = cjson.decode(ARGV[1])
    if tonumber(current_value.conversation_seq) > tonumber(next_value.conversation_seq)
        or (tonumber(current_value.conversation_seq) == tonumber(next_value.conversation_seq)
            and tonumber(current_value.base_event_id) >= tonumber(next_value.base_event_id)) then
        return 0
    end
end
redis.call('SET', KEYS[1], ARGV[1], 'EX', ARGV[2])
return 1
"#;

#[derive(Clone)]
pub struct ReplayCache {
    backend: ReplayCacheBackend,
    response_affinity: ResponseAffinityStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStateBackend {
    Valkey,
    Sqlite,
    Memory,
    Unavailable,
}

#[derive(Clone)]
enum ReplayCacheBackend {
    Disabled,
    Redis(Arc<RedisBackend>),
    Sqlite(Arc<SqliteBackend>),
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
    replay_snapshot_ttl: Duration,
    replay_snapshots: Mutex<HashMap<Uuid, LocalReplaySnapshotEntry>>,
}

struct SqliteBackend {
    coordinator: crate::standalone_config::StandaloneCoordinatorStore,
    replay_ttl_seconds: u64,
    session_ttl_seconds: u64,
}

struct LocalSessionEntry {
    user: SessionUser,
    expires_at: Instant,
    last_access: u64,
}

struct LocalReplaySnapshotEntry {
    snapshot: ReplaySnapshotValue,
    expires_at: Instant,
}

impl LocalBackend {
    fn new(session_ttl_seconds: u64, replay_ttl_seconds: u64, max_session_entries: usize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            session_ttl: Duration::from_secs(session_ttl_seconds.max(1)),
            max_session_entries: max_session_entries.max(1),
            replay_snapshot_ttl: Duration::from_secs(replay_ttl_seconds.max(1)),
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
            response_affinity: ResponseAffinityStore::unavailable(),
        }
    }
}

impl ReplayCache {
    fn unavailable_sessions_only(config: &WorkerConfig, reason: &str) -> Self {
        warn!(
            backend = "unavailable",
            reason,
            capability = "session_persistence",
            "session state is unavailable; process-local authentication fallback is disabled"
        );
        Self {
            backend: ReplayCacheBackend::Disabled,
            response_affinity: ResponseAffinityStore::local_with_ttl_and_capacity(
                Duration::from_secs(config.session_ttl_seconds.max(1)),
                config.local_session_max_entries,
            ),
        }
    }

    pub async fn from_config(config: &WorkerConfig) -> Self {
        Self::from_config_with_sqlite(config, None).await
    }

    pub async fn from_config_with_sqlite(
        config: &WorkerConfig,
        sqlite_pool: Option<SqlitePool>,
    ) -> Self {
        let url = config.valkey_url.trim();
        if url.is_empty() {
            if let Some(pool) = sqlite_pool {
                return Self::sqlite(config, pool, "valkey_not_configured");
            }
            return Self::unavailable_sessions_only(
                config,
                "valkey_not_configured_without_durable_store",
            );
        }
        let client = match redis::Client::open(url) {
            Ok(client) => client,
            Err(err) => {
                warn!(error = %err, "failed to open valkey client");
                if let Some(pool) = sqlite_pool {
                    return Self::sqlite(config, pool, "valkey_client_open_failed");
                }
                return Self::unavailable_sessions_only(
                    config,
                    "valkey_client_open_failed_without_durable_store",
                );
            }
        };
        let manager = match client.get_connection_manager().await {
            Ok(manager) => manager,
            Err(err) => {
                warn!(error = %err, "failed to connect valkey");
                if let Some(pool) = sqlite_pool {
                    return Self::sqlite(config, pool, "valkey_connection_failed");
                }
                return Self::unavailable_sessions_only(
                    config,
                    "valkey_connection_failed_without_durable_store",
                );
            }
        };
        Self {
            backend: ReplayCacheBackend::Redis(Arc::new(RedisBackend {
                manager: manager.clone(),
                replay_ttl_seconds: config.valkey_ttl_seconds.max(1),
                session_ttl_seconds: config.session_ttl_seconds.max(1),
            })),
            response_affinity: ResponseAffinityStore::from_connection_manager(
                manager,
                config.session_ttl_seconds,
            ),
        }
    }

    fn sqlite(config: &WorkerConfig, pool: SqlitePool, reason: &str) -> Self {
        warn!(
            backend = "sqlite",
            reason,
            scope = "single-host",
            network_filesystem_safe = false,
            "using WAL-backed SQLite coordinator for replay and sessions"
        );
        let coordinator = crate::standalone_config::StandaloneCoordinatorStore::new(pool);
        Self {
            backend: ReplayCacheBackend::Sqlite(Arc::new(SqliteBackend {
                coordinator: coordinator.clone(),
                replay_ttl_seconds: config.valkey_ttl_seconds.max(1),
                session_ttl_seconds: config.session_ttl_seconds.max(1),
            })),
            response_affinity: ResponseAffinityStore::sqlite_with_ttl(
                coordinator,
                config.session_ttl_seconds,
            ),
        }
    }

    pub fn enabled(&self) -> bool {
        matches!(self.backend, ReplayCacheBackend::Redis(_))
    }

    pub fn state_backend(&self) -> ReplayStateBackend {
        match &self.backend {
            ReplayCacheBackend::Redis(_) => ReplayStateBackend::Valkey,
            ReplayCacheBackend::Sqlite(_) => ReplayStateBackend::Sqlite,
            ReplayCacheBackend::Local(_) => ReplayStateBackend::Memory,
            ReplayCacheBackend::Disabled => ReplayStateBackend::Unavailable,
        }
    }

    pub fn session_available(&self) -> bool {
        !matches!(self.backend, ReplayCacheBackend::Disabled)
    }

    pub fn response_affinity(&self) -> ResponseAffinityStore {
        self.response_affinity.clone()
    }

    pub(crate) fn replay_snapshots_available(&self) -> bool {
        !matches!(self.backend, ReplayCacheBackend::Disabled)
    }

    pub fn for_tests() -> Self {
        Self {
            backend: ReplayCacheBackend::Local(Arc::new(LocalBackend::new(
                7 * 24 * 60 * 60,
                24 * 60 * 60,
                10_000,
            ))),
            response_affinity: ResponseAffinityStore::for_tests(),
        }
    }

    pub fn for_tests_without_affinity() -> Self {
        Self {
            backend: ReplayCacheBackend::Local(Arc::new(LocalBackend::new(
                7 * 24 * 60 * 60,
                24 * 60 * 60,
                10_000,
            ))),
            response_affinity: ResponseAffinityStore::unavailable(),
        }
    }

    pub async fn get_snapshot(&self, conversation_id: Uuid) -> Result<Option<ReplaySnapshotValue>> {
        match &self.backend {
            ReplayCacheBackend::Disabled => Ok(None),
            ReplayCacheBackend::Local(inner) => {
                let mut snapshots = inner.replay_snapshots.lock().await;
                let now = Instant::now();
                let expired = snapshots
                    .get(&conversation_id)
                    .is_some_and(|entry| entry.expires_at <= now);
                if expired {
                    snapshots.remove(&conversation_id);
                    return Ok(None);
                }
                Ok(snapshots
                    .get(&conversation_id)
                    .map(|entry| entry.snapshot.clone()))
            }
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
            ReplayCacheBackend::Sqlite(inner) => {
                let Some(payload) = inner
                    .coordinator
                    .get("replay", &conversation_id.to_string())
                    .await?
                else {
                    return Ok(None);
                };
                Ok(Some(
                    serde_json::from_str(&payload)
                        .context("invalid SQLite replay snapshot json")?,
                ))
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
                let now = Instant::now();
                snapshots.retain(|_, entry| entry.expires_at > now);
                if let Some(current) = snapshots.get(&update.conversation_id)
                    && (current.snapshot.conversation_seq > next.conversation_seq
                        || (current.snapshot.conversation_seq == next.conversation_seq
                            && current.snapshot.base_event_id >= next.base_event_id))
                {
                    return Ok(false);
                }
                if snapshots.len() >= inner.max_session_entries
                    && !snapshots.contains_key(&update.conversation_id)
                    && let Some(oldest) = snapshots
                        .iter()
                        .min_by_key(|(_, entry)| entry.snapshot.updated_at)
                        .map(|(id, _)| *id)
                {
                    snapshots.remove(&oldest);
                }
                snapshots.insert(
                    update.conversation_id,
                    LocalReplaySnapshotEntry {
                        snapshot: next,
                        expires_at: now + inner.replay_snapshot_ttl,
                    },
                );
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
                let payload = serde_json::to_string(&next)?;
                let mut manager = inner.manager.clone();
                let updated: i64 = redis::Script::new(REPLAY_SNAPSHOT_CAS_SCRIPT)
                    .key(key)
                    .arg(payload)
                    .arg(inner.replay_ttl_seconds)
                    .invoke_async(&mut manager)
                    .await
                    .context("failed to compare-and-set replay valkey snapshot")?;
                Ok(updated == 1)
            }
            ReplayCacheBackend::Sqlite(inner) => {
                let next = ReplaySnapshotValue {
                    conversation_id: update.conversation_id,
                    base_event_id: update.event_id,
                    conversation_seq: update.conversation_seq,
                    prompt_refs: update.prompt_refs.clone(),
                    ref_count: i32::try_from(update.prompt_refs.len()).unwrap_or(i32::MAX),
                    byte_size: update.byte_size()?,
                    updated_at: Utc::now(),
                };
                let key = update.conversation_id.to_string();
                let payload = serde_json::to_string(&next)?;
                let current = inner.coordinator.get("replay", &key).await?;
                if let Some(current) = current {
                    let current: ReplaySnapshotValue = serde_json::from_str(&current)
                        .context("invalid SQLite replay snapshot json")?;
                    if current.conversation_seq > next.conversation_seq
                        || (current.conversation_seq == next.conversation_seq
                            && current.base_event_id >= next.base_event_id)
                    {
                        return Ok(false);
                    }
                }
                if let Some(current) = inner.coordinator.get("replay", &key).await? {
                    return Ok(inner
                        .coordinator
                        .replace_if_current(
                            "replay",
                            &key,
                            &current,
                            &payload,
                            inner.replay_ttl_seconds,
                        )
                        .await?);
                }
                let stored = inner
                    .coordinator
                    .get_or_insert("replay", &key, &payload, inner.replay_ttl_seconds)
                    .await?;
                Ok(stored == payload)
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
            ReplayCacheBackend::Sqlite(inner) => {
                let payload = serde_json::to_string(user)?;
                inner
                    .coordinator
                    .put("session", session_id, &payload, inner.session_ttl_seconds)
                    .await?;
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
            ReplayCacheBackend::Sqlite(inner) => {
                let Some(value) = inner.coordinator.get("session", session_id).await? else {
                    return Ok(None);
                };
                let user = serde_json::from_str(&value).context("invalid SQLite session json")?;
                inner
                    .coordinator
                    .put("session", session_id, &value, inner.session_ttl_seconds)
                    .await?;
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
            ReplayCacheBackend::Sqlite(inner) => {
                inner.coordinator.delete("session", session_id).await?;
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
            ReplayCacheBackend::Disabled
            | ReplayCacheBackend::Local(_)
            | ReplayCacheBackend::Sqlite(_) => Ok(false),
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
            ReplayCacheBackend::Disabled
            | ReplayCacheBackend::Local(_)
            | ReplayCacheBackend::Sqlite(_) => Ok(false),
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
            ReplayCacheBackend::Disabled
            | ReplayCacheBackend::Local(_)
            | ReplayCacheBackend::Sqlite(_) => Ok(false),
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
            ReplayCacheBackend::Disabled
            | ReplayCacheBackend::Local(_)
            | ReplayCacheBackend::Sqlite(_) => Ok(None),
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
                inner.replay_snapshots.lock().await.insert(
                    snapshot.conversation_id,
                    LocalReplaySnapshotEntry {
                        snapshot,
                        expires_at: Instant::now() + inner.replay_snapshot_ttl,
                    },
                );
                Ok(())
            }
            ReplayCacheBackend::Disabled
            | ReplayCacheBackend::Redis(_)
            | ReplayCacheBackend::Sqlite(_) => Err(anyhow!(
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
    if replay_cache.replay_snapshots_available()
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

    #[tokio::test]
    async fn local_snapshot_cache_keeps_newest_out_of_order_update() {
        let cache = ReplayCache::for_tests();
        let conversation_id = Uuid::new_v4();
        let older = ReplaySnapshotUpdate {
            event_id: 10,
            conversation_id,
            conversation_seq: 2,
            prompt_refs: Vec::new(),
        };
        let newer = ReplaySnapshotUpdate {
            event_id: 12,
            conversation_id,
            conversation_seq: 3,
            prompt_refs: Vec::new(),
        };

        let _ = tokio::join!(
            cache.write_snapshot_if_newer(&older),
            cache.write_snapshot_if_newer(&newer)
        );

        let snapshot = cache
            .get_snapshot(conversation_id)
            .await
            .unwrap()
            .expect("newest local snapshot should be retained");
        assert_eq!(snapshot.conversation_seq, newer.conversation_seq);
        assert_eq!(snapshot.base_event_id, newer.event_id);
    }

    #[tokio::test]
    async fn from_config_without_valkey_uses_local_response_affinity() {
        let mut config = WorkerConfig::default();
        config.session_ttl_seconds = 60;
        config.local_session_max_entries = 2;

        let cache = ReplayCache::from_config(&config).await;
        assert!(!cache.enabled());
        assert!(!cache.session_available());

        let key = "local-response-affinity";
        let binding = crate::response_affinity::ResponseAffinityBinding {
            endpoint_id: Uuid::new_v4(),
            endpoint_key_id: None,
            endpoint_key_fingerprint: "fingerprint".to_string(),
        };
        assert_eq!(
            cache
                .response_affinity()
                .get_or_create(key, &binding)
                .await
                .unwrap(),
            binding
        );
    }

    #[tokio::test]
    async fn from_config_with_invalid_valkey_url_uses_local_response_affinity() {
        let mut config = WorkerConfig::default();
        config.valkey_url = "not-a-valkey-url".to_string();

        let cache = ReplayCache::from_config(&config).await;
        let key = "invalid-valkey-response-affinity";
        let binding = crate::response_affinity::ResponseAffinityBinding {
            endpoint_id: Uuid::new_v4(),
            endpoint_key_id: None,
            endpoint_key_fingerprint: "fingerprint".to_string(),
        };
        assert_eq!(
            cache.response_affinity().get(key).await.unwrap(),
            None,
            "invalid Valkey configuration must fall back to a usable local store"
        );
        cache
            .response_affinity()
            .get_or_create(key, &binding)
            .await
            .unwrap();
        assert_eq!(
            cache.response_affinity().peek(key).await.unwrap(),
            Some(binding)
        );
    }

    #[tokio::test]
    async fn sqlite_backend_persists_sessions_and_orders_replay_snapshots() {
        let path =
            std::env::temp_dir().join(format!("prompt-ferry-replay-{}.sqlite", Uuid::new_v4()));
        let pool = crate::db::connect_sqlite(&path).await.unwrap();
        crate::db::migrate_standalone(&pool).await.unwrap();
        let mut config = WorkerConfig::default();
        config.session_ttl_seconds = 60;
        config.valkey_ttl_seconds = 60;
        let cache = ReplayCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
        let user = SessionUser {
            user_id: 7,
            login_name: "operator".to_string(),
            display_name: "Operator".to_string(),
            is_admin: false,
        };
        cache.write_session("sqlite-session", &user).await.unwrap();
        let loaded = cache
            .read_session_refresh("sqlite-session")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.user_id, user.user_id);
        assert_eq!(loaded.login_name, user.login_name);
        assert_eq!(loaded.is_admin, user.is_admin);

        let conversation_id = Uuid::new_v4();
        let newer = ReplaySnapshotUpdate {
            event_id: 12,
            conversation_id,
            conversation_seq: 3,
            prompt_refs: Vec::new(),
        };
        let older = ReplaySnapshotUpdate {
            event_id: 11,
            conversation_id,
            conversation_seq: 2,
            prompt_refs: Vec::new(),
        };
        assert!(cache.write_snapshot_if_newer(&newer).await.unwrap());
        assert!(!cache.write_snapshot_if_newer(&older).await.unwrap());
        assert_eq!(
            cache
                .get_snapshot(conversation_id)
                .await
                .unwrap()
                .unwrap()
                .conversation_seq,
            3
        );
        pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
