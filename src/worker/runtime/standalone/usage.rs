use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use tracing::warn;

use crate::standalone_config::{StandaloneConfigStore, StandaloneUsageSummaryRecord};
use crate::worker_usage::{StandaloneUsageSummary, UsageLog};

pub(crate) const DEFAULT_USAGE_CAPACITY: usize = 256;

pub(crate) struct StandaloneUsageBuffer {
    capacity: usize,
    entries: Mutex<VecDeque<StandaloneUsageSummary>>,
    persistence: Option<UsagePersistence>,
}

/// Bounded write-through reference for the standalone usage ledger. The
/// store handle is shared so persistence does not re-open the underlying
/// pool, and the prune limit matches the buffer capacity so the in-memory
/// cache and the durable ledger stay aligned in steady state.
struct UsagePersistence {
    store: Arc<StandaloneConfigStore>,
    prune_max_rows: i64,
}

impl StandaloneUsageBuffer {
    /// Construct an in-memory-only buffer used by tests that do not need
    /// durable persistence.
    #[cfg(test)]
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            persistence: None,
        }
    }

    /// Construct a buffer that mirrors every record into the standalone
    /// SQLite usage ledger through the provided store. Persistence is
    /// awaited explicitly via `persist`; nothing is ever dispatched via
    /// untracked `tokio::spawn`.
    pub(crate) fn with_persistence(capacity: usize, store: Arc<StandaloneConfigStore>) -> Self {
        let capacity = capacity.max(1);
        let prune_max_rows = i64::try_from(capacity).unwrap_or(i64::MAX);
        Self {
            capacity,
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            persistence: Some(UsagePersistence {
                store,
                prune_max_rows,
            }),
        }
    }

    /// Push the summary into the bounded in-memory cache. Persistence is
    /// intentionally decoupled: callers that want durability must invoke
    /// `persist` and await it, so the runtime's request-flow drain covers
    /// the SQLite write before shutdown.
    pub(crate) fn record(&self, log: UsageLog) -> StandaloneUsageSummary {
        let summary = log.into_standalone_summary();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(summary.clone());
        summary
    }

    /// Await the SQLite insert and bounded prune. Failures warn but do not
    /// surface; the in-memory copy pushed by `record` remains as a cache
    /// fallback so the upstream request is not failed by a persistence
    /// error.
    pub(crate) async fn persist(&self, summary: &StandaloneUsageSummary) {
        let Some(persistence) = self.persistence.as_ref() else {
            return;
        };
        let record = StandaloneUsageSummaryRecord::from(summary);
        if let Err(error) = persistence.store.insert_usage_summary(&record).await {
            warn!(
                error = %error,
                request_id = %summary.request_id,
                "failed to persist standalone usage summary; in-memory copy retained as fallback"
            );
            return;
        }
        if let Err(error) = persistence
            .store
            .prune_usage_summaries(persistence.prune_max_rows)
            .await
        {
            warn!(
                error = %error,
                request_id = %summary.request_id,
                "failed to prune standalone usage ledger"
            );
        }
    }

    #[allow(dead_code)]
    pub(crate) fn snapshot(&self) -> Vec<StandaloneUsageSummary> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Replace the in-memory cache with the most recent persisted summaries
    /// so reopening the same SQLite file restores prior state. Newer rows
    /// are appended; older rows already present in memory are dropped to
    /// keep the cache bounded. A single malformed row skips with a warning
    /// instead of failing the entire read.
    pub(crate) async fn hydrate_from_store(&self) {
        let Some(persistence) = self.persistence.as_ref() else {
            return;
        };
        let records = match persistence
            .store
            .list_usage_summaries(self.capacity as i64)
            .await
        {
            Ok(records) => records,
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to hydrate standalone usage ledger from SQLite; keeping empty in-memory cache"
                );
                return;
            }
        };
        let summaries = records
            .iter()
            .map(StandaloneUsageSummary::from)
            .collect::<Vec<_>>();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.clear();
        for summary in summaries {
            if entries.len() == self.capacity {
                entries.pop_front();
            }
            entries.push_back(summary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StandaloneUsageBuffer;
    use crate::worker_usage::{UsageLog, UsageRequestMetadata};

    fn log(request_id: uuid::Uuid, secret: &str) -> UsageLog {
        UsageLog::ai_request(
            request_id,
            UsageRequestMetadata {
                path: "/v1/responses".to_string(),
                ..UsageRequestMetadata::default()
            },
            Some("gpt-5".to_string()),
        )
        .with_request_raw_json(Some(serde_json::json!({"secret": secret})))
        .with_response(
            Some("provider-response".to_string()),
            None,
            Some(secret.to_string()),
            Some(secret.to_string()),
        )
        .with_upstream_redaction(true, Some(serde_json::json!({"secret": secret})), None)
        .with_error(
            Some("provider_error".to_string()),
            Some(secret.to_string()),
            Some(secret.to_string()),
        )
    }

    #[test]
    fn summary_drops_raw_bodies_and_session_material() {
        let buffer = StandaloneUsageBuffer::new(4);
        let request_id = uuid::Uuid::new_v4();
        let _ = buffer.record(log(request_id, "standalone-secret-body"));

        let summary = buffer.snapshot().pop().expect("summary");
        let debug = format!("{summary:?}");
        assert_eq!(summary.request_id, request_id);
        assert!(!debug.contains("standalone-secret-body"));
        assert!(!debug.contains("provider-response"));
    }

    #[test]
    fn capacity_evicts_oldest_summary_deterministically() {
        let buffer = StandaloneUsageBuffer::new(2);
        let first = uuid::Uuid::new_v4();
        let second = uuid::Uuid::new_v4();
        let third = uuid::Uuid::new_v4();
        let _ = buffer.record(log(first, "first"));
        let _ = buffer.record(log(second, "second"));
        let _ = buffer.record(log(third, "third"));

        let ids = buffer
            .snapshot()
            .into_iter()
            .map(|summary| summary.request_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![second, third]);
        assert_eq!(buffer.len(), 2);
    }
}
