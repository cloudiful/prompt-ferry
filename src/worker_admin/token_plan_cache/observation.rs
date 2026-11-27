use super::*;
use crate::{
    worker_admin::quota_reset::{ResetState, reset_state, reset_state_from_remaining},
    worker_admin_types::TokenPlanKeyUsage,
};
use chrono::{DateTime, Utc};

pub(crate) struct CachedQuotaObservation {
    pub(crate) key: Option<TokenPlanKeyUsage>,
    pub(crate) observed_at: Option<DateTime<Utc>>,
    pub(crate) refresh_failed: bool,
}

/// Issue #99 P1: one cached quota snapshot of one endpoint.
///
/// The snapshot is a clone of the in-memory cache entry: it never refreshes,
/// never reaches the upstream provider, and never writes. Every successful
/// cached key keeps its own observation, so no two keys are merged.
pub(crate) struct CachedSnapshot {
    pub(crate) usage: TokenPlanUsageResponse,
    pub(crate) observations: Vec<CachedSnapshotObservation>,
}

/// The freshness of one cached key inside a [`CachedSnapshot`].
pub(crate) struct CachedSnapshotObservation {
    /// The cached key identity. It only pairs an observation with its cached
    /// key and never reaches a response.
    pub(crate) key_id: Uuid,
    pub(crate) observed_at: Option<DateTime<Utc>>,
    pub(crate) stale: bool,
}

impl CachedSnapshot {
    /// The safe observations of the cached entry.
    ///
    /// Every successful cached key that carries at least one canonical window
    /// becomes one observation; a key without a usable window carries no
    /// quota and never becomes an observation. The cached freshness metadata
    /// is preserved, and no amount is ever summed, averaged, or merged.
    pub(crate) fn safe_observations(
        &self,
    ) -> Vec<crate::worker_admin_types::CachedQuotaObservation> {
        let mut usage = self.usage.clone();
        crate::worker_admin::quota_canonical::populate_response_observations(
            usage.provider,
            &mut usage,
        );
        self.observations
            .iter()
            .filter_map(|observation| {
                let key = usage
                    .keys
                    .iter()
                    .find(|key| key.key_id == observation.key_id)?;
                crate::worker_admin_types::cached_observation(
                    key,
                    observation.observed_at,
                    observation.stale,
                )
            })
            .collect()
    }
}

pub(crate) fn quota_observation_now(
    cache: &TokenPlanQuotaCache,
    endpoint_id: Uuid,
    key_id: Uuid,
    now: Instant,
) -> Option<CachedQuotaObservation> {
    let entries = cache.entries.try_read().ok()?;
    let entry = entries.get(&endpoint_id)?;
    let raw = entry.usage.keys.iter().find(|key| key.key_id == key_id)?;
    let refresh_failed = entry.last_failure.contains_key(&key_id);
    let observed_at = entry.observed_at.get(&key_id).copied();
    let fresh = entry
        .successful
        .get(&key_id)
        .is_some_and(|at| now.saturating_duration_since(*at) < REFRESH_AFTER);
    let key = if fresh && !refresh_failed && raw.ok {
        let mut key = raw.clone();
        advance_relative_resets(
            &mut key,
            now.saturating_duration_since(entry.successful[&key_id]),
        );
        Some(key)
    } else {
        None
    };
    Some(CachedQuotaObservation {
        key,
        observed_at,
        refresh_failed,
    })
}

impl TokenPlanQuotaCache {
    /// Issue #99 P1: the cached quota snapshot of one endpoint.
    ///
    /// The read is cache-only: it clones the in-memory entry and its
    /// freshness metadata, and it never refreshes, fetches, or writes. A
    /// missing entry, a failed key, or an entry without a successful key
    /// yields `None`, so an unknown quota stays unavailable instead of
    /// reading as zero.
    pub(crate) async fn cached_snapshot(&self, endpoint_id: Uuid) -> Option<CachedSnapshot> {
        let entries = self.entries.read().await;
        let entry = entries.get(&endpoint_id)?;
        let usage = entry.usage.clone();
        let now = Instant::now();
        let mut observations = Vec::new();
        for key in &usage.keys {
            if !key.ok {
                continue;
            }
            let observed_at = entry.observed_at.get(&key.key_id).copied();
            let failed = entry.last_failure.contains_key(&key.key_id);
            let fresh = entry
                .successful
                .get(&key.key_id)
                .is_some_and(|at| now.saturating_duration_since(*at) < REFRESH_AFTER);
            observations.push(CachedSnapshotObservation {
                key_id: key.key_id,
                observed_at,
                stale: failed || !fresh,
            });
        }
        if observations.is_empty() {
            return None;
        }
        Some(CachedSnapshot {
            usage,
            observations,
        })
    }

    pub(crate) fn fresh_observed_key_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
    ) -> Option<TokenPlanKeyUsage> {
        self.observed_key_at(endpoint_id, key_id, Instant::now())
    }

    pub(super) fn observed_key_at(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        now: Instant,
    ) -> Option<TokenPlanKeyUsage> {
        let entries = self.entries.try_read().ok()?;
        let entry = entries.get(&endpoint_id)?;
        if entry.last_failure.contains_key(&key_id) {
            return None;
        }
        let observed_at = entry.successful.get(&key_id)?;
        if now.saturating_duration_since(*observed_at) >= REFRESH_AFTER {
            return None;
        }
        let mut key = entry
            .usage
            .keys
            .iter()
            .find(|key| key.key_id == key_id && key.ok)?
            .clone();
        advance_relative_resets(&mut key, now.saturating_duration_since(*observed_at));
        Some(key)
    }

    pub(super) async fn raw_snapshot(&self, endpoint_id: Uuid) -> Option<TokenPlanUsageResponse> {
        self.entries
            .read()
            .await
            .get(&endpoint_id)
            .map(|entry| entry.usage.clone())
    }

    pub(super) async fn store_observation(
        &self,
        endpoint_id: Uuid,
        mut usage: TokenPlanUsageResponse,
        now: Instant,
    ) {
        let observed_at_now = Utc::now();
        let mut entries = self.entries.write().await;
        let mut successful = HashMap::new();
        let mut observed_at = HashMap::new();
        let mut last_failure = HashMap::new();
        for key in &mut usage.keys {
            let key_id = key.key_id;
            if key.ok {
                successful.insert(key_id, now);
                observed_at.insert(key_id, observed_at_now);
                continue;
            }
            last_failure.insert(key_id, now);
            if let Some(previous) = entries.get(&endpoint_id) {
                if let Some(at) = previous.successful.get(&key_id) {
                    successful.insert(key_id, *at);
                }
                if let Some(at) = previous.observed_at.get(&key_id) {
                    observed_at.insert(key_id, *at);
                }
                if let Some(good) = previous
                    .usage
                    .keys
                    .iter()
                    .find(|good| good.key_id == key_id && good.ok)
                {
                    *key = good.clone();
                }
            }
        }
        entries.retain(|id, entry| {
            *id == endpoint_id
                || now.saturating_duration_since(entry.fetched_at) < Duration::from_secs(7200)
        });
        entries.insert(
            endpoint_id,
            CachedUsage {
                usage,
                fetched_at: now,
                successful,
                observed_at,
                last_failure,
            },
        );
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .retain(|(id, _), _| entries.contains_key(id));
    }
}

pub(super) fn advance_relative_resets(key: &mut TokenPlanKeyUsage, age: Duration) {
    let elapsed_ms = age.as_millis().min(i64::MAX as u128) as i64;
    for model in &mut key.model_remains {
        for window in [&mut model.interval, &mut model.weekly]
            .into_iter()
            .flatten()
        {
            window.remains_time_ms = window
                .remains_time_ms
                .map(|ms| ms.saturating_sub(elapsed_ms));
        }
    }
}

pub(super) fn expired_zero(
    key: &TokenPlanKeyUsage,
    model: Option<&str>,
    now: DateTime<Utc>,
) -> bool {
    expired_zero_scoped(key, model, now, true)
}

pub(super) fn expired_zero_scoped(
    key: &TokenPlanKeyUsage,
    model: Option<&str>,
    now: DateTime<Utc>,
    allow_general: bool,
) -> bool {
    if super::super::command_code_parsing::command_code_balance_exhausted(key) {
        return false;
    }
    let command = [key.five_hour.as_ref(), key.weekly.as_ref()]
        .into_iter()
        .flatten()
        .map(|w| {
            (
                w.remaining_percent.is_some_and(|p| p <= 0.0),
                reset_state(w.reset_at, None, now) == ResetState::Passed,
            )
        });
    let go = [
        key.opencodego_rolling.as_ref(),
        key.opencodego_weekly.as_ref(),
        key.opencodego_monthly.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|w| {
        (
            w.percent.is_some_and(|p| p >= 100.0),
            reset_state(w.resets_at, None, now) == ResetState::Passed,
        )
    });
    let glm = [key.glm_five_hour.as_ref(), key.glm_weekly.as_ref()]
        .into_iter()
        .flatten()
        .map(|w| {
            (
                w.percentage.is_some_and(|p| p >= 100.0),
                reset_state(w.next_reset_at, None, now) == ResetState::Passed,
            )
        });
    let minimax = token_plan_weight::model_usage_scoped(key, model, allow_general)
        .into_iter()
        .flat_map(|model| [model.interval.as_ref(), model.weekly.as_ref()])
        .flatten()
        .map(|w| {
            (
                w.remaining_percent.is_some_and(|p| p <= 0.0),
                reset_state_from_remaining(w.end_at, w.remains_time_ms, now) == ResetState::Passed,
            )
        });
    let mut expired = false;
    for (zero, reset_passed) in command.chain(go).chain(glm).chain(minimax) {
        if zero && !reset_passed {
            return false;
        }
        expired |= zero && reset_passed;
    }
    expired
}
