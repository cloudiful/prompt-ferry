use super::*;
use crate::{
    db::EndpointProvider,
    worker_admin::{
        chatgpt_quota_service::{
            ChatGptQuotaRoutingSnapshot, ChatGptQuotaService, RoutingSnapshot,
        },
        quota_reset::{subscription_window_is_exhausted, subscription_windows_all_reset},
    },
};

const SUBSCRIPTION_FRESHNESS: chrono::Duration = chrono::Duration::minutes(5);
const SUBSCRIPTION_CACHE_CAPACITY: usize = 4096;

impl TokenPlanQuotaCache {
    pub(crate) async fn hydrate_chatgpt_subscription(
        &self,
        service: Option<&ChatGptQuotaService>,
        endpoint_id: Uuid,
        provider: EndpointProvider,
    ) {
        let snapshot = if provider == EndpointProvider::OpenAi {
            match service.map(|service| service.routing_snapshot(endpoint_id)) {
                Some(read) => match read.await {
                    Ok(RoutingSnapshot::Subscription(snapshot)) => snapshot,
                    Ok(RoutingSnapshot::NoSubscription) | Err(_) => None,
                },
                None => None,
            }
        } else {
            None
        };
        self.store_chatgpt_subscription_snapshot(endpoint_id, snapshot);
    }

    pub(crate) fn store_chatgpt_subscription_snapshot(
        &self,
        endpoint_id: Uuid,
        snapshot: Option<ChatGptQuotaRoutingSnapshot>,
    ) {
        let now = chrono::Utc::now();
        let mut entries = self
            .subscription
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|_, entry| {
            !entry.stale
                && now.signed_duration_since(entry.observed_at) >= chrono::Duration::zero()
                && now.signed_duration_since(entry.observed_at) <= SUBSCRIPTION_FRESHNESS
        });
        match snapshot {
            Some(snapshot)
                if !snapshot.stale
                    && now.signed_duration_since(snapshot.observed_at)
                        >= chrono::Duration::zero()
                    && now.signed_duration_since(snapshot.observed_at)
                        <= SUBSCRIPTION_FRESHNESS
                    && (entries.contains_key(&endpoint_id)
                        || entries.len() < SUBSCRIPTION_CACHE_CAPACITY) =>
            {
                entries.insert(endpoint_id, snapshot);
            }
            _ => {
                entries.remove(&endpoint_id);
            }
        }
    }

    pub(crate) fn chatgpt_subscription_exhausted_now(&self, endpoint_id: Uuid) -> bool {
        let entries = self
            .subscription
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries
            .get(&endpoint_id)
            .is_some_and(|snapshot| exhausted(snapshot, chrono::Utc::now()))
    }
}

fn exhausted(snapshot: &ChatGptQuotaRoutingSnapshot, now: chrono::DateTime<chrono::Utc>) -> bool {
    if snapshot.stale
        || now < snapshot.observed_at
        || now.signed_duration_since(snapshot.observed_at) > SUBSCRIPTION_FRESHNESS
    {
        return false;
    }
    if snapshot.limit_reached == Some(true)
        && !subscription_windows_all_reset(&snapshot.windows, snapshot.observed_at, now)
    {
        return true;
    }
    snapshot
        .windows
        .iter()
        .any(|window| subscription_window_is_exhausted(window, snapshot.observed_at, now))
}
