use std::{collections::VecDeque, sync::Mutex};

use crate::worker_usage::{StandaloneUsageSummary, UsageLog};

pub(crate) const DEFAULT_USAGE_CAPACITY: usize = 256;

pub(crate) struct StandaloneUsageBuffer {
    capacity: usize,
    entries: Mutex<VecDeque<StandaloneUsageSummary>>,
}

impl StandaloneUsageBuffer {
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    pub(crate) fn record(&self, log: UsageLog) {
        let summary = log.into_standalone_summary();
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.len() == self.capacity {
            entries.pop_front();
        }
        entries.push_back(summary);
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
        buffer.record(log(request_id, "standalone-secret-body"));

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
        buffer.record(log(first, "first"));
        buffer.record(log(second, "second"));
        buffer.record(log(third, "third"));

        let ids = buffer
            .snapshot()
            .into_iter()
            .map(|summary| summary.request_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![second, third]);
        assert_eq!(buffer.len(), 2);
    }
}
