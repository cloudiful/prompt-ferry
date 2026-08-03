use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{config::NativeApi, db::RouteConfig};

const DEFAULT_MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointModelSnapshot {
    model_ids: HashSet<String>,
}

impl EndpointModelSnapshot {
    pub fn from_model_ids<I, S>(model_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            model_ids: model_ids.into_iter().map(Into::into).collect(),
        }
    }

    pub fn contains(&self, model: &str) -> bool {
        self.model_ids.contains(model)
    }

    pub fn model_ids(&self) -> impl Iterator<Item = &str> + '_ {
        self.model_ids.iter().map(String::as_str)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheLookup {
    Fresh(EndpointModelSnapshot),
    Stale(EndpointModelSnapshot),
    Missing,
}

#[derive(Clone, Debug)]
struct CacheEntry {
    fetched_at: Instant,
    signature: EndpointSignature,
    snapshot: EndpointModelSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EndpointSignature {
    base_url: String,
    api_key: String,
    native_api: NativeApi,
}

impl From<&RouteConfig> for EndpointSignature {
    fn from(route: &RouteConfig) -> Self {
        Self {
            base_url: route.base_url.clone(),
            api_key: route.api_key.clone(),
            native_api: route.native_api,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EndpointModelCache {
    ttl: Duration,
    max_entries: usize,
    inner: Arc<RwLock<HashMap<Uuid, CacheEntry>>>,
}

impl EndpointModelCache {
    pub fn new(ttl: Duration) -> Self {
        Self::with_max_entries(ttl, DEFAULT_MAX_ENTRIES)
    }

    pub fn with_max_entries(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn lookup(&self, route: &RouteConfig) -> CacheLookup {
        self.lookup_at(route, Instant::now()).await
    }

    pub async fn put(&self, route: &RouteConfig, snapshot: EndpointModelSnapshot) {
        let mut inner = self.inner.write().await;
        inner.insert(
            route.route_id,
            CacheEntry {
                fetched_at: Instant::now(),
                signature: EndpointSignature::from(route),
                snapshot,
            },
        );
        evict_oldest_entries(&mut inner, self.max_entries);
    }

    pub async fn load_or_fetch<F, Fut>(
        &self,
        route: &RouteConfig,
        fetcher: &F,
    ) -> Result<Option<EndpointModelSnapshot>>
    where
        F: Fn(&RouteConfig) -> Fut,
        Fut: Future<Output = Result<EndpointModelSnapshot>>,
    {
        match self.lookup(route).await {
            CacheLookup::Fresh(snapshot) => Ok(Some(snapshot)),
            CacheLookup::Stale(snapshot) => match fetcher(route).await {
                Ok(fresh) => {
                    self.put(route, fresh.clone()).await;
                    Ok(Some(fresh))
                }
                Err(_) => Ok(Some(snapshot)),
            },
            CacheLookup::Missing => {
                let fresh = fetcher(route).await?;
                self.put(route, fresh.clone()).await;
                Ok(Some(fresh))
            }
        }
    }

    async fn lookup_at(&self, route: &RouteConfig, now: Instant) -> CacheLookup {
        let Some(entry) = self.inner.read().await.get(&route.route_id).cloned() else {
            return CacheLookup::Missing;
        };
        if entry.signature != EndpointSignature::from(route) {
            return CacheLookup::Missing;
        }
        if now.duration_since(entry.fetched_at) <= self.ttl {
            return CacheLookup::Fresh(entry.snapshot);
        }
        CacheLookup::Stale(entry.snapshot)
    }
}

fn evict_oldest_entries(inner: &mut HashMap<Uuid, CacheEntry>, max_entries: usize) {
    while inner.len() > max_entries {
        let Some(oldest_id) = inner
            .iter()
            .min_by_key(|(_, entry)| entry.fetched_at)
            .map(|(id, _)| *id)
        else {
            return;
        };
        inner.remove(&oldest_id);
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;
    use crate::db::{RouteConfig, RouteSelectionReason};

    fn route(id: Uuid, base_url: &str) -> RouteConfig {
        RouteConfig {
            route_id: id,
            user_id: 7,
            base_url: base_url.to_string(),
            api_key: "secret".to_string(),
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: false,
            native_api: NativeApi::Chat,
            upstream_model: None,
            responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            chat_reasoning_replay_policy: crate::db::ChatReasoningReplayPolicy::Auto,
            model_route_rule_id: None,
            route_selection_reason: RouteSelectionReason::Default,
        }
    }

    #[tokio::test]
    async fn fresh_cache_hit_returns_snapshot() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let route = route(Uuid::new_v4(), "https://alpha.example.com");
        let snapshot = EndpointModelSnapshot::from_model_ids(["gpt-4.1-mini"]);
        cache.put(&route, snapshot.clone()).await;

        assert_eq!(cache.lookup(&route).await, CacheLookup::Fresh(snapshot));
    }

    #[tokio::test]
    async fn config_change_invalidates_cache_entry() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let route = route(Uuid::new_v4(), "https://alpha.example.com");
        cache
            .put(
                &route,
                EndpointModelSnapshot::from_model_ids(["gpt-4.1-mini"]),
            )
            .await;

        let mut changed = route.clone();
        changed.base_url = "https://beta.example.com".to_string();

        assert_eq!(cache.lookup(&changed).await, CacheLookup::Missing);
    }

    #[tokio::test]
    async fn stale_cache_falls_back_when_refresh_fails() {
        let cache = EndpointModelCache::new(Duration::from_secs(1));
        let route = route(Uuid::new_v4(), "https://alpha.example.com");
        let snapshot = EndpointModelSnapshot::from_model_ids(["gpt-4.1-mini"]);
        let now = Instant::now();
        cache.inner.write().await.insert(
            route.route_id,
            CacheEntry {
                fetched_at: now - Duration::from_secs(2),
                signature: EndpointSignature::from(&route),
                snapshot: snapshot.clone(),
            },
        );

        let loaded = cache
            .load_or_fetch(&route, &|_| async { Err(anyhow!("boom")) })
            .await
            .unwrap();

        assert_eq!(loaded, Some(snapshot));
    }

    #[tokio::test]
    async fn put_evicts_oldest_entry_over_capacity() {
        let cache = EndpointModelCache::with_max_entries(Duration::from_secs(60), 2);
        let first = route(Uuid::new_v4(), "https://first.example.com");
        let second = route(Uuid::new_v4(), "https://second.example.com");
        let third = route(Uuid::new_v4(), "https://third.example.com");
        let now = Instant::now();

        cache.inner.write().await.insert(
            first.route_id,
            CacheEntry {
                fetched_at: now - Duration::from_secs(2),
                signature: EndpointSignature::from(&first),
                snapshot: EndpointModelSnapshot::from_model_ids(["first"]),
            },
        );
        cache.inner.write().await.insert(
            second.route_id,
            CacheEntry {
                fetched_at: now - Duration::from_secs(1),
                signature: EndpointSignature::from(&second),
                snapshot: EndpointModelSnapshot::from_model_ids(["second"]),
            },
        );
        cache
            .put(&third, EndpointModelSnapshot::from_model_ids(["third"]))
            .await;

        assert_eq!(cache.lookup(&first).await, CacheLookup::Missing);
        assert_ne!(cache.lookup(&second).await, CacheLookup::Missing);
        assert_ne!(cache.lookup(&third).await, CacheLookup::Missing);
    }
}
