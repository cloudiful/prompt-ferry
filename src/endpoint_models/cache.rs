use std::{
    collections::{HashMap, HashSet},
    future::Future,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{config::NativeApi, db::EndpointProvider, db::RouteConfig};

const DEFAULT_MAX_ENTRIES: usize = 256;

/// The cap of the live waiting readers of one pending catalog read.
const MAX_FLIGHT_FOLLOWERS: usize = 64;

/// The bounded length of one upstream detail.
const MAX_DETAIL_CHARS: usize = 240;

/// The catalog of one endpoint.
///
/// The identifiers keep the order the upstream listed them, so a reader that
/// selects the first matching model sees the upstream order, while the
/// membership index keeps the lookup O(1) and the sorted listing keeps the
/// admin contract.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointModelSnapshot {
    /// The identifiers in the order they were encountered.
    model_ids: Vec<String>,
    /// The membership index of the same identifiers.
    membership: HashSet<String>,
}

impl EndpointModelSnapshot {
    /// A snapshot of the given identifiers.
    ///
    /// The first occurrence of an identifier wins, so the order of the
    /// upstream listing is preserved without duplicates.
    pub fn from_model_ids<I, S>(model_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut snapshot = Self::default();
        for model_id in model_ids {
            let model_id = model_id.into();
            if snapshot.membership.insert(model_id.clone()) {
                snapshot.model_ids.push(model_id);
            }
        }
        snapshot
    }

    pub fn contains(&self, model: &str) -> bool {
        self.membership.contains(model)
    }

    /// The identifiers in the order the upstream listed them.
    pub fn model_ids(&self) -> impl Iterator<Item = &str> + '_ {
        self.model_ids.iter().map(String::as_str)
    }

    /// The identifiers in a stable order. The snapshot already holds every
    /// identifier once, so this is the sorted, de-duplicated listing.
    pub fn sorted_model_ids(&self) -> Vec<String> {
        let mut ids = self.model_ids.clone();
        ids.sort();
        ids
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheLookup {
    Fresh(EndpointModelSnapshot),
    Stale(EndpointModelSnapshot),
    Missing,
}

/// How one catalog read sends its credential.
///
/// The mode belongs to the cache identity: the same secret sent as a bearer
/// token and as an API key are two different reads, so they never share a
/// catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogAuth {
    /// `Authorization: Bearer <credential>`.
    Bearer,
    /// `x-api-key: <credential>`.
    ApiKey,
}

impl CatalogAuth {
    fn digest_tag(self) -> u8 {
        match self {
            Self::Bearer => b'b',
            Self::ApiKey => b'a',
        }
    }
}

/// The identity of one cached catalog.
///
/// It carries the endpoint, the provider, the configured base, the requested
/// protocol, and a digest of the credential the read actually used. The
/// credential itself is never stored: only its digest, and that digest never
/// reaches a log or a `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct CatalogKey {
    endpoint_id: Uuid,
    provider: EndpointProvider,
    base_url: String,
    native_api: NativeApi,
    credential_digest: [u8; 32],
}

impl std::fmt::Debug for CatalogKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CatalogKey")
            .field("endpoint_id", &self.endpoint_id)
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("native_api", &self.native_api)
            .field("credential_digest", &"<redacted>")
            .finish()
    }
}

impl Hash for CatalogKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.endpoint_id.hash(state);
        self.provider.as_str().hash(state);
        self.base_url.hash(state);
        self.native_api.as_str().hash(state);
        self.credential_digest.hash(state);
    }
}

impl CatalogKey {
    /// The identity of one catalog read.
    pub fn new(
        endpoint_id: Uuid,
        provider: EndpointProvider,
        base_url: &str,
        native_api: NativeApi,
        auth: CatalogAuth,
        credential: &str,
    ) -> Self {
        Self {
            endpoint_id,
            provider,
            base_url: normalize_base_url(base_url),
            native_api,
            credential_digest: credential_digest(auth, credential),
        }
    }

    /// The identity of the runtime catalog read of one route.
    pub fn for_route(route: &RouteConfig) -> Self {
        Self::new(
            route.route_id,
            route.provider,
            &route.base_url,
            route.native_api,
            CatalogAuth::Bearer,
            &route.api_key,
        )
    }
}

/// The configured base in its comparable shape: trimmed and without the
/// trailing separator, so the same address always yields the same identity.
fn normalize_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

/// The digest of the credential as the read actually sends it.
///
/// The digest covers the credential mode and the exact credential bytes, so a
/// trimmed and an untrimmed secret are two identities and a bearer read never
/// shares a catalog with an API-key read.
fn credential_digest(auth: CatalogAuth, credential: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update([auth.digest_tag()]);
    hasher.update(credential.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    bytes
}

/// Why one catalog read failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogFailureKind {
    /// The upstream was never reached.
    Transport,
    /// The upstream answered with an error status.
    Status,
    /// The upstream answered with an unreadable catalog.
    Parse,
}

/// The bounded failure of one catalog read.
///
/// It carries the upstream status when the upstream answered, so a caller
/// keeps its own status contract, and a bounded detail that never holds a
/// credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogFailure {
    pub kind: CatalogFailureKind,
    pub status: Option<u16>,
    pub detail: String,
}

impl CatalogFailure {
    pub fn transport(detail: impl Into<String>) -> Self {
        Self {
            kind: CatalogFailureKind::Transport,
            status: None,
            detail: bounded_detail(detail.into()),
        }
    }

    pub fn status(status: u16, detail: impl Into<String>) -> Self {
        Self {
            kind: CatalogFailureKind::Status,
            status: Some(status),
            detail: bounded_detail(detail.into()),
        }
    }

    pub fn parse(detail: impl Into<String>) -> Self {
        Self {
            kind: CatalogFailureKind::Parse,
            status: None,
            detail: bounded_detail(detail.into()),
        }
    }
}

impl std::fmt::Display for CatalogFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for CatalogFailure {}

/// The outcome of one shared catalog read.
#[derive(Clone, Debug)]
pub enum CatalogHit {
    /// A fresh catalog: fetched now, or served from an entry inside its
    /// lifetime.
    Fresh(EndpointModelSnapshot),
    /// The refresh failed. The retained snapshot is stale and must never be
    /// reported as a fresh read.
    Stale {
        snapshot: EndpointModelSnapshot,
        failure: CatalogFailure,
    },
}

#[derive(Clone, Debug)]
struct CacheEntry {
    fetched_at: Instant,
    snapshot: EndpointModelSnapshot,
}

#[derive(Default)]
struct CacheState {
    entries: HashMap<CatalogKey, CacheEntry>,
    /// The pending catalog reads. The map never holds more identities than
    /// the entry budget allows.
    flights: HashMap<CatalogKey, Flight>,
    /// The generation of the next flight, so a released guard never removes a
    /// newer flight of the same identity.
    next_generation: u64,
}

/// One pending catalog read.
///
/// The flight owns a single completion slot. The leader publishes the result
/// into it and every waiting reader reads it, so a reader that goes away
/// leaves no retained sender behind.
struct Flight {
    /// The completion slot of the flight.
    result: watch::Sender<Option<Result<CatalogHit, CatalogFailure>>>,
    /// The live waiting readers of this flight.
    live_followers: usize,
    /// The generation of this flight.
    generation: u64,
}

/// One waiting reader of a pending catalog read.
struct Follower {
    /// The completion slot of the flight it joined.
    receiver: watch::Receiver<Option<Result<CatalogHit, CatalogFailure>>>,
    /// The generation of the flight it joined.
    generation: u64,
}

/// How one read joins the cache.
enum Admission {
    /// The read waits for the flight of its identity.
    Follower(Follower),
    /// The read fetches the catalog for its identity.
    Leader(u64),
    /// The read was refused by the local capacity.
    Rejected(CatalogFailure),
}

/// The shared model-catalog cache.
///
/// One entry holds one catalog of one endpoint under its complete request
/// identity, so distinct protocols, providers, addresses, or credentials of
/// the same endpoint coexist inside the entry budget instead of overwriting
/// each other. Concurrent reads of one identity share a single upstream
/// fetch.
pub struct EndpointModelCache {
    ttl: Duration,
    max_entries: usize,
    state: Arc<Mutex<CacheState>>,
}

impl Clone for EndpointModelCache {
    fn clone(&self) -> Self {
        Self {
            ttl: self.ttl,
            max_entries: self.max_entries,
            state: self.state.clone(),
        }
    }
}

impl std::fmt::Debug for EndpointModelCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        formatter
            .debug_struct("EndpointModelCache")
            .field("ttl", &self.ttl)
            .field("max_entries", &self.max_entries)
            .field("entries", &state.entries.len())
            .field("pending", &state.flights.len())
            .finish()
    }
}

impl EndpointModelCache {
    pub fn new(ttl: Duration) -> Self {
        Self::with_max_entries(ttl, DEFAULT_MAX_ENTRIES)
    }

    pub fn with_max_entries(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries: max_entries.max(1),
            state: Arc::new(Mutex::new(CacheState::default())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, CacheState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn is_fresh(&self, entry: &CacheEntry, now: Instant) -> bool {
        now.duration_since(entry.fetched_at) <= self.ttl
    }

    /// The shared catalog read.
    ///
    /// A fresh entry answers without an upstream request. Otherwise one
    /// caller fetches and every overlapping caller waits for that fetch, so
    /// eight simultaneous reads of one identity cause exactly one upstream
    /// request. A failed fetch is never cached: the next read retries, and a
    /// retained stale snapshot keeps its own age, so it never passes for a
    /// fresh read.
    ///
    /// The pending state is bounded: at most one flight per identity inside
    /// the entry budget, at most [`MAX_FLIGHT_FOLLOWERS`] waiting readers per
    /// flight, and a reader that goes away frees its slot immediately. A read
    /// that exceeds the local capacity is refused with a bounded, retryable
    /// failure and never reaches the upstream.
    pub async fn get_or_fetch<F, Fut>(
        &self,
        key: &CatalogKey,
        fetcher: F,
    ) -> Result<CatalogHit, CatalogFailure>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<EndpointModelSnapshot, CatalogFailure>>,
    {
        let now = Instant::now();
        let admission = {
            let mut state = self.lock();
            if let Some(entry) = state.entries.get(key)
                && self.is_fresh(entry, now)
            {
                return Ok(CatalogHit::Fresh(entry.snapshot.clone()));
            }
            match state.flights.get_mut(key) {
                Some(flight) => {
                    if flight.live_followers >= MAX_FLIGHT_FOLLOWERS {
                        Admission::Rejected(CatalogFailure::transport(
                            "the catalog read has too many waiting readers; retry shortly",
                        ))
                    } else {
                        flight.live_followers += 1;
                        Admission::Follower(Follower {
                            receiver: flight.result.subscribe(),
                            generation: flight.generation,
                        })
                    }
                }
                None => {
                    if state.flights.len() >= self.max_entries {
                        Admission::Rejected(CatalogFailure::transport(
                            "the catalog cache is at its pending capacity; retry shortly",
                        ))
                    } else {
                        let generation = state.next_generation;
                        state.next_generation += 1;
                        let (result, _reader) = watch::channel(None);
                        state.flights.insert(
                            key.clone(),
                            Flight {
                                result,
                                live_followers: 0,
                                generation,
                            },
                        );
                        Admission::Leader(generation)
                    }
                }
            }
        };
        match admission {
            Admission::Follower(follower) => self.await_flight(key, follower).await,
            Admission::Leader(generation) => self.lead_flight(key, generation, fetcher).await,
            Admission::Rejected(failure) => Err(failure),
        }
    }

    /// Wait for the flight of one identity.
    async fn await_flight(
        &self,
        key: &CatalogKey,
        follower: Follower,
    ) -> Result<CatalogHit, CatalogFailure> {
        let Follower {
            mut receiver,
            generation,
        } = follower;
        // The registration is released as soon as this reader ends, so a
        // cancelled reader frees its slot without waiting for the leader.
        let _registration = FollowerRegistration {
            cache: self,
            key: key.clone(),
            generation,
        };
        loop {
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            if receiver.changed().await.is_err() {
                return Err(CatalogFailure::transport("the catalog refresh was dropped"));
            }
        }
    }

    /// Fetch the catalog of one identity and publish the result.
    async fn lead_flight<F, Fut>(
        &self,
        key: &CatalogKey,
        generation: u64,
        fetcher: F,
    ) -> Result<CatalogHit, CatalogFailure>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<EndpointModelSnapshot, CatalogFailure>>,
    {
        let mut flight = FlightGuard {
            cache: self,
            key: key.clone(),
            generation,
            settled: false,
        };
        let fetched = fetcher().await;
        let result = match fetched {
            Ok(snapshot) => {
                self.store(key, snapshot.clone());
                Ok(CatalogHit::Fresh(snapshot))
            }
            Err(failure) => match self.stale_snapshot(key) {
                Some(snapshot) => Ok(CatalogHit::Stale { snapshot, failure }),
                None => Err(failure),
            },
        };
        self.publish(key, generation, &result);
        flight.settled = true;
        result
    }

    /// The runtime catalog read of one route.
    ///
    /// A failed refresh keeps the retained stale snapshot, as the runtime
    /// discovery has always done, and a failed read without a snapshot stays
    /// an error.
    pub async fn load_or_fetch<F, Fut>(
        &self,
        route: &RouteConfig,
        fetcher: &F,
    ) -> anyhow::Result<Option<EndpointModelSnapshot>>
    where
        F: Fn(&RouteConfig) -> Fut,
        Fut: Future<Output = anyhow::Result<EndpointModelSnapshot>>,
    {
        let key = CatalogKey::for_route(route);
        match self
            .get_or_fetch(&key, || async {
                fetcher(route)
                    .await
                    .map_err(|error| CatalogFailure::transport(error.to_string()))
            })
            .await
        {
            Ok(CatalogHit::Fresh(snapshot)) => Ok(Some(snapshot)),
            Ok(CatalogHit::Stale { snapshot, .. }) => Ok(Some(snapshot)),
            Err(failure) => Err(anyhow::anyhow!(failure.to_string())),
        }
    }

    pub async fn lookup(&self, route: &RouteConfig) -> CacheLookup {
        self.lookup_key(&CatalogKey::for_route(route), Instant::now())
    }

    pub async fn put(&self, route: &RouteConfig, snapshot: EndpointModelSnapshot) {
        self.store(&CatalogKey::for_route(route), snapshot);
    }

    fn lookup_key(&self, key: &CatalogKey, now: Instant) -> CacheLookup {
        let state = self.lock();
        let Some(entry) = state.entries.get(key) else {
            return CacheLookup::Missing;
        };
        if self.is_fresh(entry, now) {
            return CacheLookup::Fresh(entry.snapshot.clone());
        }
        CacheLookup::Stale(entry.snapshot.clone())
    }

    fn store(&self, key: &CatalogKey, snapshot: EndpointModelSnapshot) {
        let mut state = self.lock();
        state.entries.insert(
            key.clone(),
            CacheEntry {
                fetched_at: Instant::now(),
                snapshot,
            },
        );
        evict_oldest_entries(&mut state.entries, self.max_entries);
    }

    /// The retained snapshot of one identity, whatever its age.
    fn stale_snapshot(&self, key: &CatalogKey) -> Option<EndpointModelSnapshot> {
        self.lock()
            .entries
            .get(key)
            .map(|entry| entry.snapshot.clone())
    }

    /// Publish the result of one flight and release its slot.
    ///
    /// Only the flight of the same generation is released, so a leader that
    /// was cancelled late never removes a newer flight of the same identity.
    fn publish(
        &self,
        key: &CatalogKey,
        generation: u64,
        result: &Result<CatalogHit, CatalogFailure>,
    ) {
        let mut state = self.lock();
        let Some(flight) = state.flights.get(key) else {
            return;
        };
        if flight.generation != generation {
            return;
        }
        flight.result.send_replace(Some(result.clone()));
        state.flights.remove(key);
    }
}

/// The live registration of one waiting reader.
///
/// Dropping it frees the reader's slot of its flight immediately, so a
/// cancelled reader never keeps its registration alive.
struct FollowerRegistration<'a> {
    cache: &'a EndpointModelCache,
    key: CatalogKey,
    generation: u64,
}

impl Drop for FollowerRegistration<'_> {
    fn drop(&mut self) {
        let mut state = self.cache.lock();
        if let Some(flight) = state.flights.get_mut(&self.key)
            && flight.generation == self.generation
        {
            flight.live_followers = flight.live_followers.saturating_sub(1);
        }
    }
}

/// The pending state of one identity.
///
/// A leader that is cancelled while it fetches releases its own flight and
/// wakes the waiting readers with a bounded error, so the next read retries
/// instead of waiting forever.
struct FlightGuard<'a> {
    cache: &'a EndpointModelCache,
    key: CatalogKey,
    generation: u64,
    settled: bool,
}

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let mut state = self.cache.lock();
        let Some(flight) = state.flights.get(&self.key) else {
            return;
        };
        if flight.generation != self.generation {
            return;
        }
        flight
            .result
            .send_replace(Some(Err(CatalogFailure::transport(
                "the catalog refresh was cancelled; retry shortly",
            ))));
        state.flights.remove(&self.key);
    }
}

fn evict_oldest_entries(entries: &mut HashMap<CatalogKey, CacheEntry>, max_entries: usize) {
    while entries.len() > max_entries {
        let Some(oldest_key) = entries
            .iter()
            .min_by_key(|(_, entry)| entry.fetched_at)
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        entries.remove(&oldest_key);
    }
}

/// The bounded detail of one failure.
fn bounded_detail(detail: String) -> String {
    if detail.chars().count() <= MAX_DETAIL_CHARS {
        return detail;
    }
    let mut bounded = String::new();
    for character in detail.chars().take(MAX_DETAIL_CHARS - 3) {
        bounded.push(character);
    }
    bounded.push_str("...");
    bounded
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    use super::*;
    use crate::db::{EndpointProvider, RouteSelectionReason};

    fn route(id: Uuid, base_url: &str) -> RouteConfig {
        RouteConfig {
            embedding_dimensions_override: None,
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
            model_route_rule_id: None,
            route_selection_reason: RouteSelectionReason::Default,
            provider: EndpointProvider::Generic,
            service_tier: None,
            proxy_url: None,
            dev_system_normalize: false,
            thinking_effort_override: None,
            compact_mode: crate::db::CompactMode::Passthrough,
            thinking_downgrade_enabled: false,
            has_oauth_token: false,
        }
    }

    fn key(
        endpoint_id: Uuid,
        provider: EndpointProvider,
        base: &str,
        api: NativeApi,
    ) -> CatalogKey {
        CatalogKey::new(
            endpoint_id,
            provider,
            base,
            api,
            CatalogAuth::Bearer,
            "secret",
        )
    }

    fn snapshot(ids: &[&str]) -> EndpointModelSnapshot {
        EndpointModelSnapshot::from_model_ids(ids.iter().copied())
    }

    #[test]
    fn the_snapshot_keeps_the_upstream_order_and_the_sorted_listing() {
        // Issue #1052 P2 attempt 2: the first occurrence of an identifier
        // wins, the iteration follows the upstream order, and the sorted
        // listing stays the admin contract.
        let snapshot = EndpointModelSnapshot::from_model_ids(["zzz", "aaa", "zzz"]);

        assert_eq!(snapshot.model_ids().collect::<Vec<_>>(), vec!["zzz", "aaa"]);
        assert!(snapshot.contains("zzz"));
        assert!(snapshot.contains("aaa"));
        assert!(!snapshot.contains("mmm"));
        assert_eq!(
            snapshot.sorted_model_ids(),
            vec!["aaa".to_string(), "zzz".to_string()]
        );
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
        cache.lock().entries.insert(
            CatalogKey::for_route(&route),
            CacheEntry {
                fetched_at: now - Duration::from_secs(2),
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

        cache.lock().entries.insert(
            CatalogKey::for_route(&first),
            CacheEntry {
                fetched_at: now - Duration::from_secs(2),
                snapshot: EndpointModelSnapshot::from_model_ids(["first"]),
            },
        );
        cache.lock().entries.insert(
            CatalogKey::for_route(&second),
            CacheEntry {
                fetched_at: now - Duration::from_secs(1),
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

    #[tokio::test]
    async fn a_fresh_entry_answers_without_a_fetch() {
        // Issue #1052 P2: a repeated read inside the entry lifetime never
        // reaches the upstream again.
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let fetches = Arc::new(AtomicUsize::new(0));

        for _ in 0..2 {
            let fetches = fetches.clone();
            let hit = cache
                .get_or_fetch(&identity, move || async move {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    Ok(snapshot(&["gpt-4o"]))
                })
                .await
                .expect("catalog read");
            assert!(matches!(hit, CatalogHit::Fresh(_)));
        }

        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "a repeated read inside the lifetime causes no fetch"
        );
    }

    #[tokio::test]
    async fn distinct_identities_coexist_inside_the_entry_budget() {
        // Issue #1052 P2: the protocol, the provider, the address, and the
        // credential are part of the identity, so a rotation never evicts the
        // previous catalog.
        let cache = EndpointModelCache::with_max_entries(Duration::from_secs(60), 8);
        let endpoint_id = Uuid::new_v4();
        let chat = key(
            endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let embeddings = key(
            endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Embeddings,
        );
        let auto = key(
            endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Auto,
        );
        let other_provider = key(
            endpoint_id,
            EndpointProvider::OpenRouter,
            "https://a.test",
            NativeApi::Chat,
        );
        let other_base = key(
            endpoint_id,
            EndpointProvider::Generic,
            "https://b.test",
            NativeApi::Chat,
        );
        let other_credential = CatalogKey::new(
            endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
            CatalogAuth::Bearer,
            "another-secret",
        );

        for identity in [
            &chat,
            &embeddings,
            &auto,
            &other_provider,
            &other_base,
            &other_credential,
        ] {
            cache
                .get_or_fetch(identity, || async { Ok(snapshot(&["gpt-4o"])) })
                .await
                .expect("catalog read");
        }

        assert_eq!(cache.lock().entries.len(), 6);
        for identity in [
            &chat,
            &embeddings,
            &auto,
            &other_provider,
            &other_base,
            &other_credential,
        ] {
            assert!(matches!(
                cache.lookup_key(identity, Instant::now()),
                CacheLookup::Fresh(_)
            ));
        }
    }

    #[tokio::test]
    async fn one_credential_never_shares_a_catalog_with_another() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let endpoint_id = Uuid::new_v4();
        let first = CatalogKey::new(
            endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
            CatalogAuth::Bearer,
            "secret-one",
        );
        let second = CatalogKey::new(
            endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
            CatalogAuth::Bearer,
            "secret-two",
        );
        let fetches = Arc::new(AtomicUsize::new(0));

        for identity in [&first, &second] {
            let fetches = fetches.clone();
            cache
                .get_or_fetch(identity, move || async move {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    Ok(snapshot(&["gpt-4o"]))
                })
                .await
                .expect("catalog read");
        }

        assert_eq!(
            fetches.load(Ordering::SeqCst),
            2,
            "two credentials never share one catalog"
        );
    }

    #[tokio::test]
    async fn the_credential_mode_is_part_of_the_identity() {
        let bearer = CatalogKey::new(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
            CatalogAuth::Bearer,
            "secret",
        );
        let api_key = CatalogKey::new(
            bearer.endpoint_id,
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
            CatalogAuth::ApiKey,
            "secret",
        );
        assert_ne!(bearer, api_key);
    }

    #[test]
    fn the_identity_never_exposes_the_credential() {
        let identity = CatalogKey::new(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
            CatalogAuth::Bearer,
            "super-secret-key",
        );
        let rendered = format!("{identity:?}");
        assert!(!rendered.contains("super-secret-key"));
        assert!(rendered.contains("redacted"));
    }

    #[tokio::test]
    async fn eight_simultaneous_reads_cause_one_fetch() {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            eight_simultaneous_reads_cause_one_fetch_inner(),
        )
        .await
        .expect("the eight-reader scenario finishes");
    }

    async fn eight_simultaneous_reads_cause_one_fetch_inner() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let fetches = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let started = Arc::new(Notify::new());

        let mut readers = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let identity = identity.clone();
            let fetches = fetches.clone();
            let release = release.clone();
            let started = started.clone();
            readers.push(tokio::spawn(async move {
                cache
                    .get_or_fetch(&identity, move || async move {
                        fetches.fetch_add(1, Ordering::SeqCst);
                        started.notify_one();
                        release.notified().await;
                        Ok(snapshot(&["gpt-4o"]))
                    })
                    .await
            }));
        }

        // The first reader fetches; the others wait for that fetch.
        started.notified().await;
        let mut waiting = 0;
        for _ in 0..10_000 {
            waiting = cache
                .lock()
                .flights
                .values()
                .map(|flight| flight.live_followers)
                .sum::<usize>();
            if waiting == 7 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(waiting, 7, "the other seven readers wait for one fetch");
        release.notify_waiters();

        for reader in readers {
            let hit = reader.await.expect("reader").expect("catalog read");
            assert!(matches!(hit, CatalogHit::Fresh(_)));
        }
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "eight simultaneous reads cause exactly one fetch"
        );
        assert!(
            cache.lock().flights.is_empty(),
            "the finished flight leaves no pending state"
        );
    }

    #[tokio::test]
    async fn a_failed_read_is_not_cached_and_the_next_read_retries() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let fetches = Arc::new(AtomicUsize::new(0));

        for _ in 0..2 {
            let fetches = fetches.clone();
            let failure = cache
                .get_or_fetch(&identity, move || async move {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    Err(CatalogFailure::status(502, "upstream failure"))
                })
                .await;
            assert!(failure.is_err());
        }

        assert_eq!(fetches.load(Ordering::SeqCst), 2);
        assert!(
            cache.lock().entries.is_empty(),
            "a failed read never becomes a cached catalog"
        );
    }

    #[tokio::test]
    async fn a_failed_refresh_keeps_the_stale_snapshot_and_its_age() {
        let cache = EndpointModelCache::new(Duration::from_secs(1));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let stale = snapshot(&["gpt-4o"]);
        let fetched_at = Instant::now() - Duration::from_secs(30);
        cache.lock().entries.insert(
            identity.clone(),
            CacheEntry {
                fetched_at,
                snapshot: stale.clone(),
            },
        );

        let hit = cache
            .get_or_fetch(&identity, || async {
                Err(CatalogFailure::status(502, "upstream failure"))
            })
            .await
            .expect("the retained snapshot answers");
        match hit {
            CatalogHit::Stale { snapshot, failure } => {
                assert_eq!(snapshot, stale);
                assert_eq!(failure.status, Some(502));
            }
            CatalogHit::Fresh(_) => panic!("a failed refresh is never a fresh read"),
        }

        let entry = cache
            .lock()
            .entries
            .get(&identity)
            .expect("the stale entry is retained")
            .fetched_at;
        assert_eq!(
            entry, fetched_at,
            "a retained stale snapshot keeps its own age"
        );
    }

    /// Wait until a condition holds, without sleeping.
    async fn wait_for(mut condition: impl FnMut() -> bool) {
        for _ in 0..10_000 {
            if condition() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the awaited condition was not reached");
    }

    /// A leader that stays blocked until it is released.
    fn blocked_leader(
        cache: EndpointModelCache,
        identity: CatalogKey,
        started: Arc<Notify>,
        release: Arc<Notify>,
        fetches: Arc<AtomicUsize>,
    ) -> tokio::task::JoinHandle<Result<CatalogHit, CatalogFailure>> {
        tokio::spawn(async move {
            cache
                .get_or_fetch(&identity, move || async move {
                    fetches.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    release.notified().await;
                    Ok(snapshot(&["gpt-4o"]))
                })
                .await
        })
    }

    #[tokio::test]
    async fn repeated_cancelled_readers_do_not_grow_the_pending_state() {
        // Issue #1052 P2 attempt 3: a reader that goes away frees its slot
        // immediately, so repeated joins and cancellations never grow the
        // retained pending state.
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            repeated_cancelled_readers_do_not_grow_the_pending_state_inner(),
        )
        .await
        .expect("the repeated cancellation scenario finishes");
    }

    async fn repeated_cancelled_readers_do_not_grow_the_pending_state_inner() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let fetches = Arc::new(AtomicUsize::new(0));
        let leader = blocked_leader(
            cache.clone(),
            identity.clone(),
            started.clone(),
            release.clone(),
            fetches.clone(),
        );
        started.notified().await;

        for _ in 0..20 {
            let follower = {
                let cache = cache.clone();
                let identity = identity.clone();
                tokio::spawn(async move {
                    cache
                        .get_or_fetch(&identity, || async { Ok(snapshot(&["x"])) })
                        .await
                })
            };
            wait_for(|| {
                cache
                    .lock()
                    .flights
                    .values()
                    .any(|flight| flight.live_followers == 1)
            })
            .await;
            follower.abort();
            wait_for(|| {
                cache
                    .lock()
                    .flights
                    .values()
                    .all(|flight| flight.live_followers == 0)
            })
            .await;
        }

        let state = cache.lock();
        assert_eq!(state.flights.len(), 1, "the pending state keeps one flight");
        assert_eq!(
            state
                .flights
                .values()
                .next()
                .expect("the flight")
                .live_followers,
            0,
            "a cancelled reader frees its slot immediately"
        );
        drop(state);
        release.notify_waiters();
        let _ = leader.await;
    }

    #[tokio::test]
    async fn the_pending_identities_are_bounded() {
        // Issue #1052 P2 attempt 3: the pending identities never exceed the
        // entry budget, and a refused read never reaches the upstream.
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            the_pending_identities_are_bounded_inner(),
        )
        .await
        .expect("the pending capacity scenario finishes");
    }

    async fn the_pending_identities_are_bounded_inner() {
        let cache = EndpointModelCache::with_max_entries(Duration::from_secs(60), 2);
        let first = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let second = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://b.test",
            NativeApi::Chat,
        );
        let third = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://c.test",
            NativeApi::Chat,
        );
        let fetches = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let first_started = Arc::new(Notify::new());
        let second_started = Arc::new(Notify::new());
        let first_leader = blocked_leader(
            cache.clone(),
            first.clone(),
            first_started.clone(),
            release.clone(),
            fetches.clone(),
        );
        first_started.notified().await;
        let second_leader = blocked_leader(
            cache.clone(),
            second.clone(),
            second_started.clone(),
            release.clone(),
            fetches.clone(),
        );
        second_started.notified().await;

        // The pending identities are full, so a new identity is refused.
        let refused = cache
            .get_or_fetch(&third, || async {
                panic!("a refused read never reaches the upstream")
            })
            .await;
        assert!(
            refused.is_err(),
            "a new identity is refused at the pending capacity"
        );
        assert_eq!(cache.lock().flights.len(), 2);

        // A cached fresh entry is still served at the pending capacity.
        cache.store(&third, snapshot(&["gpt-4o"]));
        let hit = cache
            .get_or_fetch(&third, || async {
                panic!("a fresh entry never reaches the upstream")
            })
            .await
            .expect("the fresh entry answers");
        assert!(matches!(hit, CatalogHit::Fresh(_)));

        // An existing flight is still joined at the pending capacity.
        let follower = {
            let cache = cache.clone();
            let first = first.clone();
            tokio::spawn(async move {
                cache
                    .get_or_fetch(&first, || async { Ok(snapshot(&["x"])) })
                    .await
            })
        };
        wait_for(|| {
            cache
                .lock()
                .flights
                .values()
                .any(|flight| flight.live_followers == 1)
        })
        .await;
        release.notify_waiters();
        let follower_hit = tokio::time::timeout(std::time::Duration::from_secs(5), follower)
            .await
            .expect("the joined reader finishes")
            .expect("the joined reader task")
            .expect("catalog read");
        assert!(matches!(follower_hit, CatalogHit::Fresh(_)));
        let _ = first_leader.await;
        let _ = second_leader.await;
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            2,
            "the refused read never reached the upstream"
        );
    }

    #[tokio::test]
    async fn the_waiting_readers_of_one_flight_are_bounded() {
        // Issue #1052 P2 attempt 3: one flight never holds more than the
        // reader cap, and the reader that exceeds it is refused locally.
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            the_waiting_readers_of_one_flight_are_bounded_inner(),
        )
        .await
        .expect("the reader capacity scenario finishes");
    }

    async fn the_waiting_readers_of_one_flight_are_bounded_inner() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let fetches = Arc::new(AtomicUsize::new(0));
        let leader = blocked_leader(
            cache.clone(),
            identity.clone(),
            started.clone(),
            release.clone(),
            fetches.clone(),
        );
        started.notified().await;

        let mut followers = Vec::new();
        for _ in 0..MAX_FLIGHT_FOLLOWERS {
            let cache = cache.clone();
            let identity = identity.clone();
            followers.push(tokio::spawn(async move {
                cache
                    .get_or_fetch(&identity, || async { Ok(snapshot(&["x"])) })
                    .await
            }));
        }
        wait_for(|| {
            cache
                .lock()
                .flights
                .values()
                .any(|flight| flight.live_followers == MAX_FLIGHT_FOLLOWERS)
        })
        .await;

        let refused = cache
            .get_or_fetch(&identity, || async {
                panic!("a refused reader never reaches the upstream")
            })
            .await;
        assert!(
            refused.is_err(),
            "the reader that exceeds the cap is refused"
        );
        assert_eq!(
            cache
                .lock()
                .flights
                .values()
                .next()
                .expect("the flight")
                .live_followers,
            MAX_FLIGHT_FOLLOWERS,
            "the flight never holds more readers than the cap"
        );

        release.notify_waiters();
        for follower in followers {
            let hit = tokio::time::timeout(std::time::Duration::from_secs(5), follower)
                .await
                .expect("the reader finishes")
                .expect("the reader task")
                .expect("catalog read");
            assert!(matches!(hit, CatalogHit::Fresh(_)));
        }
        let _ = leader.await;
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "one flight causes exactly one fetch"
        );
    }

    #[tokio::test]
    async fn independent_identities_fetch_concurrently() {
        // Issue #1052 P2 attempt 3: distinct identities keep their own
        // progress; the pending bookkeeping never serializes them.
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            independent_identities_fetch_concurrently_inner(),
        )
        .await
        .expect("the concurrency scenario finishes");
    }

    async fn independent_identities_fetch_concurrently_inner() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let first = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let second = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://b.test",
            NativeApi::Chat,
        );
        let fetches = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let first_started = Arc::new(Notify::new());
        let second_started = Arc::new(Notify::new());
        let first_leader = blocked_leader(
            cache.clone(),
            first.clone(),
            first_started.clone(),
            release.clone(),
            fetches.clone(),
        );
        first_started.notified().await;
        let second_leader = blocked_leader(
            cache.clone(),
            second.clone(),
            second_started.clone(),
            release.clone(),
            fetches.clone(),
        );
        second_started.notified().await;

        assert_eq!(
            fetches.load(Ordering::SeqCst),
            2,
            "two identities fetch at the same time"
        );
        assert_eq!(cache.lock().flights.len(), 2);
        release.notify_waiters();
        let _ = first_leader.await;
        let _ = second_leader.await;
    }

    #[tokio::test]
    async fn a_cancelled_reader_releases_the_waiting_readers() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let blocked = Arc::new(Notify::new());

        let leader = {
            let cache = cache.clone();
            let identity = identity.clone();
            let blocked = blocked.clone();
            tokio::spawn(async move {
                cache
                    .get_or_fetch(&identity, move || async move {
                        blocked.notify_one();
                        std::future::pending::<()>().await;
                        Ok(snapshot(&["gpt-4o"]))
                    })
                    .await
            })
        };
        blocked.notified().await;

        let follower = {
            let cache = cache.clone();
            let identity = identity.clone();
            tokio::spawn(async move {
                cache
                    .get_or_fetch(&identity, || async { Ok(snapshot(&["x"])) })
                    .await
            })
        };
        for _ in 0..10_000 {
            if cache
                .lock()
                .flights
                .values()
                .any(|flight| flight.live_followers > 0)
            {
                break;
            }
            tokio::task::yield_now().await;
        }

        leader.abort();
        let follower_outcome = tokio::time::timeout(std::time::Duration::from_secs(5), follower)
            .await
            .expect("the follower is released")
            .expect("the follower task");
        assert!(
            follower_outcome.is_err(),
            "a cancelled reader leaves the waiting reader with an error"
        );
        assert!(
            cache.lock().flights.is_empty(),
            "a cancelled reader leaves no pending entry"
        );

        // The identity is free again, so the next read retries.
        let hit = cache
            .get_or_fetch(&identity, || async { Ok(snapshot(&["gpt-4o"])) })
            .await
            .expect("the retry succeeds");
        assert!(matches!(hit, CatalogHit::Fresh(_)));
    }

    #[tokio::test]
    async fn a_cancelled_waiter_does_not_block_the_reader() {
        let cache = EndpointModelCache::new(Duration::from_secs(60));
        let identity = key(
            Uuid::new_v4(),
            EndpointProvider::Generic,
            "https://a.test",
            NativeApi::Chat,
        );
        let release = Arc::new(Notify::new());
        let started = Arc::new(Notify::new());

        let leader = {
            let cache = cache.clone();
            let identity = identity.clone();
            let release = release.clone();
            let started = started.clone();
            tokio::spawn(async move {
                cache
                    .get_or_fetch(&identity, move || async move {
                        started.notify_one();
                        release.notified().await;
                        Ok(snapshot(&["gpt-4o"]))
                    })
                    .await
            })
        };
        started.notified().await;

        let waiter = {
            let cache = cache.clone();
            let identity = identity.clone();
            tokio::spawn(async move {
                cache
                    .get_or_fetch(&identity, || async { Ok(snapshot(&["x"])) })
                    .await
            })
        };
        for _ in 0..10_000 {
            if cache
                .lock()
                .flights
                .values()
                .any(|flight| flight.live_followers > 0)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        waiter.abort();

        release.notify_waiters();
        let hit = tokio::time::timeout(std::time::Duration::from_secs(5), leader)
            .await
            .expect("the reader is not blocked")
            .expect("the reader task")
            .expect("catalog read");
        assert!(matches!(hit, CatalogHit::Fresh(_)));
        assert!(cache.lock().flights.is_empty());
    }
}
