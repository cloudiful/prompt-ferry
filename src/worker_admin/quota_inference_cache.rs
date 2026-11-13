use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use tokio::sync::{Mutex, OwnedMutexGuard};
use uuid::Uuid;

use super::quota_inference::response::InferenceResponse;
use crate::response_affinity::api_key_fingerprint;

const TTL: Duration = Duration::from_secs(600);
const MAX_ENTRIES: usize = 256;

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AccountIdentity {
    Account(String),
    Credential(String),
}

impl AccountIdentity {
    pub(crate) fn from_credentials(account: Option<&str>, access: &str, refresh: &str) -> Self {
        match account {
            Some(account) => Self::Account(api_key_fingerprint(account)),
            None => Self::Credential(api_key_fingerprint(&format!(
                "{}:{}:{}",
                access.len(),
                access,
                refresh
            ))),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Identity {
    pub account: AccountIdentity,
    pub plan: Option<String>,
    pub reset: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    pub remaining: f64,
    pub retention_days: i32,
}

impl Identity {
    fn matches(&self, other: &Self) -> bool {
        self.account == other.account
            && self.plan == other.plan
            && self.retention_days == other.retention_days
            && (self.reset - other.reset).num_seconds().abs() <= 120
            && other.observed_at >= self.observed_at
            && other.remaining <= self.remaining
    }
}

struct Entry {
    identity: Identity,
    result: InferenceResponse,
    stored_at: Instant,
}

#[derive(Clone, Default)]
pub(crate) struct QuotaInferenceCache {
    entries: Arc<Mutex<HashMap<Uuid, Entry>>>,
    locks: Arc<Mutex<HashMap<Uuid, Weak<Mutex<()>>>>>,
}

impl QuotaInferenceCache {
    pub(crate) async fn lock(&self, endpoint: Uuid) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.locks.lock().await;
            locks.retain(|_, lock| lock.strong_count() > 0);
            match locks.get(&endpoint).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(Mutex::new(()));
                    locks.insert(endpoint, Arc::downgrade(&lock));
                    lock
                }
            }
        };
        lock.lock_owned().await
    }

    pub(crate) async fn get(
        &self,
        endpoint: Uuid,
        identity: &Identity,
    ) -> Option<InferenceResponse> {
        let mut entries = self.entries.lock().await;
        entries.retain(|_, entry| entry.stored_at.elapsed() < TTL);
        let entry = entries.get(&endpoint)?;
        if !entry.identity.matches(identity) {
            entries.remove(&endpoint);
            return None;
        }
        let mut result = entry.result.clone();
        result.cached = true;
        Some(result)
    }

    pub(crate) async fn store(
        &self,
        endpoint: Uuid,
        identity: Identity,
        result: InferenceResponse,
    ) {
        let mut entries = self.entries.lock().await;
        entries.retain(|_, entry| entry.stored_at.elapsed() < TTL);
        if entries.len() >= MAX_ENTRIES
            && !entries.contains_key(&endpoint)
            && let Some(oldest) = entries
                .iter()
                .min_by_key(|(_, e)| e.stored_at)
                .map(|(id, _)| *id)
        {
            entries.remove(&oldest);
        }
        entries.insert(
            endpoint,
            Entry {
                identity,
                result,
                stored_at: Instant::now(),
            },
        );
    }

    pub(crate) async fn invalidate(&self, endpoint: Uuid) {
        self.entries.lock().await.remove(&endpoint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin::quota_inference::response::Estimate;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn identity() -> Identity {
        Identity {
            account: AccountIdentity::from_credentials(Some("account"), "access", "refresh"),
            plan: Some("plus".into()),
            reset: Utc::now(),
            observed_at: Utc::now(),
            remaining: 75.0,
            retention_days: 8,
        }
    }

    fn response() -> InferenceResponse {
        InferenceResponse {
            method_version: "observed-v1",
            status: "insufficient_signal",
            as_of: None,
            latest_snapshot_at: None,
            calculated_at: Utc::now(),
            span_start: None,
            consumed_pp: 0.0,
            usable_blocks: 0,
            account: Estimate::unavailable("insufficient_signal"),
            models: Vec::new(),
            models_truncated: false,
            unknown_tokens: 0,
            overflow_tokens: 0,
            omitted_rows: 0,
            observation_provenance: "success_only",
            account_history_assumed_stable: true,
            current_refresh_failed: false,
            account_identity_known: false,
            cached: false,
        }
    }

    #[tokio::test]
    async fn concurrent_endpoint_reads_share_one_computation() {
        let cache = QuotaInferenceCache::default();
        let endpoint = Uuid::new_v4();
        let calls = Arc::new(AtomicUsize::new(0));
        let identity = identity();
        let mut tasks = Vec::new();
        for _ in 0..20 {
            let cache = cache.clone();
            let identity = identity.clone();
            let calls = calls.clone();
            tasks.push(tokio::spawn(async move {
                let _guard = cache.lock(endpoint).await;
                if cache.get(endpoint, &identity).await.is_none() {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::task::yield_now().await;
                    cache.store(endpoint, identity, response()).await;
                }
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn account_identity_survives_rotation_and_unknown_identity_is_conservative() {
        use crate::worker_admin::handlers::chatgpt_backend::codex_account_id_from_access_token;
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        let jwt = |account: &str, nonce: &str| {
            let claims = serde_json::json!({"chatgpt_account_id": account, "nonce": nonce});
            format!(
                "header.{}.signature",
                URL_SAFE_NO_PAD.encode(claims.to_string())
            )
        };
        let key = |access: &str, refresh: &str| {
            let account = codex_account_id_from_access_token(access);
            AccountIdentity::from_credentials(account.as_deref(), access, refresh)
        };
        let first = jwt("account-one", "first");
        let rotated = jwt("account-one", "second");
        let different = jwt("account-two", "third");
        let cache = QuotaInferenceCache::default();
        let endpoint = Uuid::new_v4();
        let mut initial = identity();
        initial.account = key(&first, "refresh-one");
        cache.store(endpoint, initial.clone(), response()).await;
        let mut candidate = initial.clone();
        candidate.account = key(&rotated, "refresh-two");
        assert!(cache.get(endpoint, &candidate).await.unwrap().cached);
        candidate.account = key(&different, "refresh-two");
        assert!(cache.get(endpoint, &candidate).await.is_none());

        initial.account = key("opaque-one", "refresh-one");
        cache.store(endpoint, initial.clone(), response()).await;
        candidate.account = key("opaque-one", "refresh-one");
        assert!(cache.get(endpoint, &candidate).await.unwrap().cached);
        candidate.account = key("opaque-one", "refresh-two");
        assert!(cache.get(endpoint, &candidate).await.is_none());
        cache.store(endpoint, initial.clone(), response()).await;
        candidate.account = key("opaque-two", "refresh-one");
        assert!(cache.get(endpoint, &candidate).await.is_none());
        cache.store(endpoint, initial.clone(), response()).await;
        candidate.account = key(&first, "refresh-one");
        assert!(cache.get(endpoint, &candidate).await.is_none());
        cache.store(endpoint, candidate, response()).await;
        assert!(cache.get(endpoint, &initial).await.is_none());
    }

    #[tokio::test]
    async fn identity_changes_and_expiry_invalidate() {
        let cache = QuotaInferenceCache::default();
        let endpoint = Uuid::new_v4();
        let original = identity();
        cache.store(endpoint, original.clone(), response()).await;
        let mut changed = original.clone();
        changed.reset += chrono::Duration::seconds(120);
        assert!(cache.get(endpoint, &changed).await.unwrap().cached);
        changed.reset += chrono::Duration::seconds(1);
        assert!(cache.get(endpoint, &changed).await.is_none());
        for change in 0..4 {
            cache.store(endpoint, original.clone(), response()).await;
            let mut changed = original.clone();
            match change {
                0 => {
                    changed.account =
                        AccountIdentity::from_credentials(Some("other"), "access", "refresh")
                }
                1 => changed.retention_days += 1,
                2 => changed.plan = Some("other".into()),
                _ => changed.remaining += 1.0,
            }
            assert!(cache.get(endpoint, &changed).await.is_none());
        }
        cache.store(endpoint, original.clone(), response()).await;
        cache
            .entries
            .lock()
            .await
            .get_mut(&endpoint)
            .unwrap()
            .stored_at -= TTL;
        assert!(cache.get(endpoint, &original).await.is_none());
    }
}
