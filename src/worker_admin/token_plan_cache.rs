use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    db,
    worker_admin_types::{TokenPlanKeyUsage, TokenPlanModelUsage, TokenPlanUsageResponse},
};

const REFRESH_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub(crate) struct TokenPlanQuotaCache {
    entries: Arc<RwLock<HashMap<Uuid, CachedUsage>>>,
    refresh_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
    reservations: Arc<std::sync::Mutex<HashMap<(Uuid, Uuid), u64>>>,
}

#[derive(Clone)]
struct CachedUsage {
    usage: TokenPlanUsageResponse,
    fetched_at: Instant,
}

impl Default for TokenPlanQuotaCache {
    fn default() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
            reservations: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

impl TokenPlanQuotaCache {
    pub(crate) async fn refresh_if_due(
        &self,
        pool: &PgPool,
        endpoint_id: Uuid,
    ) -> Result<Option<TokenPlanUsageResponse>> {
        if self.is_fresh(endpoint_id).await {
            return Ok(self.snapshot(endpoint_id).await);
        }

        let lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks
                .entry(endpoint_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        if self.is_fresh(endpoint_id).await {
            return Ok(self.snapshot(endpoint_id).await);
        }

        let Some(endpoint) = db::get_endpoint(pool, endpoint_id).await? else {
            return Ok(None);
        };
        if endpoint.provider != db::EndpointProvider::Minimax {
            return Ok(None);
        }

        let usage = super::token_plan::fetch_endpoint_usage(&endpoint).await?;
        self.entries.write().await.insert(
            endpoint_id,
            CachedUsage {
                usage: usage.clone(),
                fetched_at: Instant::now(),
            },
        );
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .retain(|(id, _), _| *id != endpoint_id);
        Ok(Some(usage))
    }

    pub(crate) async fn snapshot(&self, endpoint_id: Uuid) -> Option<TokenPlanUsageResponse> {
        self.entries
            .read()
            .await
            .get(&endpoint_id)
            .map(|entry| entry.usage.clone())
    }

    pub(crate) async fn invalidate(&self, endpoint_id: Uuid) {
        self.entries.write().await.remove(&endpoint_id);
        self.reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .retain(|(id, _), _| *id != endpoint_id);
    }

    pub(crate) async fn is_fresh(&self, endpoint_id: Uuid) -> bool {
        let entries = self.entries.read().await;
        let Some(entry) = entries.get(&endpoint_id) else {
            return false;
        };
        entry.fetched_at.elapsed() < REFRESH_AFTER
    }

    pub(crate) fn key_remaining_percent_now(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        model: Option<&str>,
    ) -> Option<f64> {
        let usage = self.entries.try_read().ok()?;
        let key = usage
            .get(&endpoint_id)?
            .usage
            .keys
            .iter()
            .find(|key| key.key_id == key_id && key.ok)?;
        let usage = model_usage(key, model)?;
        let remaining = effective_remaining_percent(usage)?;
        let reserved_tokens = self
            .reservations
            .lock()
            .expect("quota reservation lock is not poisoned")
            .get(&(endpoint_id, key_id))
            .copied()
            .unwrap_or_default();
        Some((remaining - reserved_percent(usage, reserved_tokens)).max(0.0))
    }

    pub(crate) fn reserve_estimated_tokens(
        &self,
        endpoint_id: Uuid,
        key_id: Uuid,
        estimated_tokens: u64,
    ) {
        let mut reservations = self
            .reservations
            .lock()
            .expect("quota reservation lock is not poisoned");
        let entry = reservations.entry((endpoint_id, key_id)).or_default();
        *entry = entry.saturating_add(estimated_tokens);
    }

    #[cfg(test)]
    pub(crate) async fn store_for_test(&self, endpoint_id: Uuid, usage: TokenPlanUsageResponse) {
        self.entries.write().await.insert(
            endpoint_id,
            CachedUsage {
                usage,
                fetched_at: Instant::now(),
            },
        );
    }
}

fn model_usage<'a>(
    key: &'a TokenPlanKeyUsage,
    model: Option<&str>,
) -> Option<&'a TokenPlanModelUsage> {
    let model = model?.trim();
    key.model_remains
        .iter()
        .find(|usage| usage.model_name.eq_ignore_ascii_case(model))
        .or_else(|| {
            key.model_remains
                .iter()
                .find(|usage| usage.model_name.eq_ignore_ascii_case("general"))
        })
        .or_else(|| (key.model_remains.len() == 1).then(|| &key.model_remains[0]))
}

fn effective_remaining_percent(usage: &TokenPlanModelUsage) -> Option<f64> {
    let interval = usage
        .interval
        .as_ref()
        .and_then(|window| window.remaining_percent);
    let weekly = usage
        .weekly
        .as_ref()
        .and_then(|window| window.remaining_percent);
    match (interval, weekly) {
        (Some(interval), Some(weekly)) => Some(interval.min(weekly).clamp(0.0, 100.0)),
        (Some(remaining), None) | (None, Some(remaining)) => Some(remaining.clamp(0.0, 100.0)),
        (None, None) => None,
    }
}

fn reserved_percent(usage: &TokenPlanModelUsage, reserved_tokens: u64) -> f64 {
    let total_count = [
        usage
            .interval
            .as_ref()
            .and_then(|window| window.total_count),
        usage.weekly.as_ref().and_then(|window| window.total_count),
    ]
    .into_iter()
    .flatten()
    .filter(|total| *total > 0)
    .min()
    .unwrap_or_default();
    if total_count <= 0 {
        return 0.0;
    }
    (reserved_tokens as f64 / total_count as f64 * 100.0).min(100.0)
}

pub(crate) fn estimate_input_tokens(body: &[u8]) -> u64 {
    let chars = String::from_utf8_lossy(body).chars().count() as u64;
    chars.saturating_add(3).saturating_div(4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker_admin_types::{TokenPlanModelUsage, TokenPlanWindowUsage};

    fn usage(interval: Option<f64>, weekly: Option<f64>) -> TokenPlanModelUsage {
        TokenPlanModelUsage {
            model_name: "general".to_string(),
            interval: interval.map(|remaining_percent| TokenPlanWindowUsage {
                status: Some(1),
                remaining_percent: Some(remaining_percent),
                total_count: None,
                usage_count: None,
                boost_permille: None,
                start_at: None,
                end_at: None,
                remains_time_ms: None,
            }),
            weekly: weekly.map(|remaining_percent| TokenPlanWindowUsage {
                status: Some(1),
                remaining_percent: Some(remaining_percent),
                total_count: None,
                usage_count: None,
                boost_permille: None,
                start_at: None,
                end_at: None,
                remains_time_ms: None,
            }),
        }
    }

    #[test]
    fn effective_remaining_uses_the_most_constrained_window() {
        assert_eq!(
            effective_remaining_percent(&usage(Some(0.0), Some(69.0))),
            Some(0.0)
        );
        assert_eq!(
            effective_remaining_percent(&usage(Some(42.0), Some(69.0))),
            Some(42.0)
        );
        assert_eq!(
            effective_remaining_percent(&usage(Some(42.0), None)),
            Some(42.0)
        );
        assert_eq!(effective_remaining_percent(&usage(None, None)), None);
    }

    #[test]
    fn local_estimator_is_conservative_for_small_payloads() {
        assert_eq!(estimate_input_tokens(br#"{"model":"MiniMax-M3"}"#), 6);
        assert!(estimate_input_tokens("中文请求".as_bytes()) >= 1);
    }

    #[test]
    fn reservation_is_converted_to_a_quota_percentage_when_total_is_known() {
        let model = TokenPlanModelUsage {
            model_name: "general".to_string(),
            interval: Some(TokenPlanWindowUsage {
                status: Some(1),
                remaining_percent: Some(100.0),
                total_count: Some(1_000),
                usage_count: None,
                boost_permille: None,
                start_at: None,
                end_at: None,
                remains_time_ms: None,
            }),
            weekly: None,
        };
        assert_eq!(reserved_percent(&model, 100), 10.0);
        assert_eq!(reserved_percent(&model, 2_000), 100.0);
    }
}
