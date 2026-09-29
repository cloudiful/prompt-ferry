//! Display-only cache for OpenAI Platform organization usage (issue #589 P2b).
//!
//! A 60s TTL bounds live Admin API reads during UI refreshes. The cached value
//! is never consumed by routing weights, token-plan quota, or remaining-credit
//! calculations, and setting or clearing the endpoint's Admin API key drops the
//! entry so a changed credential is never served from the old snapshot.

use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use super::openai_org_usage::OpenAiOrganizationUsageResponse;

const DISPLAY_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Default)]
pub(crate) struct OpenAiOrgUsageCache {
    entries: Arc<RwLock<HashMap<Uuid, CachedOrgUsage>>>,
    refresh_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
}

#[derive(Clone)]
struct CachedOrgUsage {
    usage: OpenAiOrganizationUsageResponse,
    fetched_at: Instant,
}

impl OpenAiOrgUsageCache {
    /// Serve a fresh snapshot, otherwise run `fetch` once under a per-endpoint
    /// lock so concurrent refreshes collapse into a single upstream read. Only
    /// successful reads are cached.
    pub(crate) async fn get_or_refresh<F, Fut>(
        &self,
        endpoint_id: Uuid,
        fetch: F,
    ) -> Result<OpenAiOrganizationUsageResponse>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<OpenAiOrganizationUsageResponse>>,
    {
        if let Some(usage) = self.fresh(endpoint_id).await {
            return Ok(usage);
        }
        let lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks
                .entry(endpoint_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;
        if let Some(usage) = self.fresh(endpoint_id).await {
            return Ok(usage);
        }
        let usage = fetch().await?;
        self.store(endpoint_id, usage.clone()).await;
        Ok(usage)
    }

    /// Drop the endpoint's entry so the next read refetches. Called whenever
    /// the Admin API key is written or cleared, and after endpoint deletion.
    pub(crate) async fn invalidate(&self, endpoint_id: Uuid) {
        self.entries.write().await.remove(&endpoint_id);
    }

    async fn fresh(&self, endpoint_id: Uuid) -> Option<OpenAiOrganizationUsageResponse> {
        let entries = self.entries.read().await;
        let entry = entries.get(&endpoint_id)?;
        if entry.fetched_at.elapsed() >= DISPLAY_CACHE_TTL {
            return None;
        }
        let mut usage = entry.usage.clone();
        usage.cached = true;
        Some(usage)
    }

    async fn store(&self, endpoint_id: Uuid, mut usage: OpenAiOrganizationUsageResponse) {
        usage.cached = false;
        self.entries.write().await.insert(
            endpoint_id,
            CachedOrgUsage {
                usage,
                fetched_at: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::EndpointProvider;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn sample() -> OpenAiOrganizationUsageResponse {
        let now = Utc::now();
        OpenAiOrganizationUsageResponse {
            provider: EndpointProvider::OpenAi,
            period_start: now,
            period_end: now,
            currency: "usd".to_string(),
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cost_usd: 0.25,
            truncated: false,
            cached: false,
            fetched_at: now,
        }
    }

    #[tokio::test]
    async fn fresh_entry_is_served_without_refetching() {
        let cache = OpenAiOrgUsageCache::default();
        let endpoint_id = Uuid::new_v4();
        let calls = Arc::new(AtomicUsize::new(0));

        let first = cache
            .get_or_refresh(endpoint_id, || {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(sample())
                }
            })
            .await
            .expect("first fetch");
        assert!(!first.cached);

        let second = cache
            .get_or_refresh(endpoint_id, || {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(sample())
                }
            })
            .await
            .expect("cached read");
        assert!(second.cached);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn invalidate_forces_a_refetch() {
        let cache = OpenAiOrgUsageCache::default();
        let endpoint_id = Uuid::new_v4();
        let calls = Arc::new(AtomicUsize::new(0));
        cache
            .get_or_refresh(endpoint_id, || {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(sample())
                }
            })
            .await
            .expect("first fetch");

        cache.invalidate(endpoint_id).await;

        cache
            .get_or_refresh(endpoint_id, || {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(sample())
                }
            })
            .await
            .expect("refetch");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn expired_entry_refetches() {
        let cache = OpenAiOrgUsageCache::default();
        let endpoint_id = Uuid::new_v4();
        let calls = Arc::new(AtomicUsize::new(0));
        let Some(aged) = Instant::now().checked_sub(DISPLAY_CACHE_TTL + Duration::from_secs(1))
        else {
            // Host uptime is shorter than the TTL; there is no past instant to
            // store. The TTL boundary is covered by `invalidate` above.
            return;
        };
        cache.entries.write().await.insert(
            endpoint_id,
            CachedOrgUsage {
                usage: sample(),
                fetched_at: aged,
            },
        );

        cache
            .get_or_refresh(endpoint_id, || {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(sample())
                }
            })
            .await
            .expect("refetch after expiry");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
