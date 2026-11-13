use super::*;
use crate::worker_admin_types::{SubscriptionWindowAvailability, SubscriptionWindowUsage};
use crate::{
    db::EndpointProvider,
    worker_admin::{
        chatgpt_quota_service::{
            ChatGptQuotaRoutingSnapshot, ChatGptQuotaService, RoutingSnapshot,
        },
        quota_reset::{
            ResetState, subscription_reset_state, subscription_window_is_exhausted,
            subscription_windows_all_reset,
        },
        quota_urgency::{clamp_percent, urgent_remaining},
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

    /// The subscription's percent-scale draw weight, on the same
    /// `0..=100` scale as platform-key quota weights.
    ///
    /// Only fresh, known, non-exhausted windows contribute. Each window is
    /// lifted by its reset urgency and the tightest window wins; a window
    /// without reset data keeps its raw remaining. A missing, stale, unknown
    /// or fully expired snapshot has no signal (`None`) so the caller keeps
    /// its neutral behavior.
    pub(crate) fn chatgpt_subscription_weight_now(&self, endpoint_id: Uuid) -> Option<f64> {
        let entries = self
            .subscription
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = entries.get(&endpoint_id)?;
        let now = chrono::Utc::now();
        if !snapshot_usable(snapshot, now) {
            return None;
        }
        snapshot
            .windows
            .iter()
            .filter_map(|window| window_weight_percent(window, snapshot.observed_at, now))
            .min_by(|a, b| a.total_cmp(b))
            .map(clamp_percent)
    }
}

fn snapshot_usable(
    snapshot: &ChatGptQuotaRoutingSnapshot,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    !snapshot.stale
        && now >= snapshot.observed_at
        && now.signed_duration_since(snapshot.observed_at) <= SUBSCRIPTION_FRESHNESS
}

fn window_weight_percent(
    window: &SubscriptionWindowUsage,
    observed_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<f64> {
    if window.availability != SubscriptionWindowAvailability::Known {
        return None;
    }
    let remaining = window.remaining_percent.filter(|value| value.is_finite())?;
    // A window whose reset deadline already passed carries stale data: it is
    // not a usable signal and never fabricates remaining quota.
    if subscription_reset_state(window, observed_at, now) == ResetState::Passed {
        return None;
    }
    let period_seconds = window
        .window_seconds
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds as f64)
        .unwrap_or_default();
    let resets_in_seconds = window
        .reset_at
        .map(|reset| (reset - now).num_milliseconds() as f64 / 1000.0)
        .or_else(|| {
            window.reset_after_seconds.map(|seconds| {
                let reset = observed_at + chrono::Duration::seconds(seconds);
                (reset - now).num_milliseconds() as f64 / 1000.0
            })
        });
    Some(urgent_remaining(
        remaining,
        resets_in_seconds,
        period_seconds,
    ))
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

#[cfg(test)]
mod weight_tests {
    use super::*;
    use crate::worker_admin_types::SubscriptionWindowUsage;

    fn window(
        remaining: Option<f64>,
        availability: SubscriptionWindowAvailability,
        window_seconds: Option<i64>,
        reset_after_seconds: Option<i64>,
    ) -> SubscriptionWindowUsage {
        SubscriptionWindowUsage {
            source_window: "primary".to_string(),
            window_seconds,
            used_percent: remaining.map(|value| 100.0 - value),
            remaining_percent: remaining,
            reset_at: None,
            reset_after_seconds,
            availability,
        }
    }

    #[test]
    fn tightest_urgency_adjusted_window_wins() {
        let now = chrono::Utc::now();
        let observed = now - chrono::Duration::seconds(30);
        // A tighter 5h window that resets soon and a busier weekly window:
        // urgency lifts the bottleneck but it stays the tightest.
        let weekly = window(
            Some(50.0),
            SubscriptionWindowAvailability::Known,
            Some(7 * 24 * 3600),
            None,
        );
        let five_hour = window(
            Some(5.0),
            SubscriptionWindowAvailability::Known,
            Some(5 * 3600),
            Some((4 * 60 + 30) * 60),
        );
        let weight = window_weight_percent(&five_hour, observed, now).unwrap();
        assert!(
            weight > 5.0 && weight < 50.0,
            "urgency lifts the bottleneck without beating the busier window: {weight}"
        );
        assert_eq!(
            window_weight_percent(&weekly, observed, now),
            Some(50.0),
            "a window without reset data keeps its raw remaining"
        );
    }

    #[test]
    fn missing_unknown_expired_and_exhausted_windows_carry_no_positive_signal() {
        let now = chrono::Utc::now();
        let observed = now - chrono::Duration::seconds(1);
        assert_eq!(
            window_weight_percent(
                &window(None, SubscriptionWindowAvailability::Known, None, None),
                observed,
                now,
            ),
            None
        );
        assert_eq!(
            window_weight_percent(
                &window(
                    Some(50.0),
                    SubscriptionWindowAvailability::Unknown,
                    None,
                    None
                ),
                observed,
                now,
            ),
            None
        );
        assert_eq!(
            window_weight_percent(
                &window(
                    Some(50.0),
                    SubscriptionWindowAvailability::Known,
                    None,
                    Some(0)
                ),
                observed,
                now,
            ),
            None,
            "an already reset window is stale data, not remaining quota"
        );
        assert_eq!(
            window_weight_percent(
                &window(
                    Some(0.0),
                    SubscriptionWindowAvailability::Known,
                    Some(5 * 3600),
                    Some(60)
                ),
                observed,
                now,
            ),
            Some(0.0)
        );
    }

    #[test]
    fn stale_or_missing_snapshot_keeps_the_neutral_no_signal_result() {
        let cache = TokenPlanQuotaCache::default();
        let endpoint = Uuid::new_v4();
        assert_eq!(cache.chatgpt_subscription_weight_now(endpoint), None);
        cache.store_chatgpt_subscription_snapshot(
            endpoint,
            Some(ChatGptQuotaRoutingSnapshot {
                observed_at: chrono::Utc::now(),
                limit_reached: None,
                windows: vec![window(
                    Some(42.0),
                    SubscriptionWindowAvailability::Known,
                    Some(5 * 3600),
                    Some(3600),
                )],
                stale: true,
            }),
        );
        assert_eq!(cache.chatgpt_subscription_weight_now(endpoint), None);
        cache.store_chatgpt_subscription_snapshot(
            endpoint,
            Some(ChatGptQuotaRoutingSnapshot {
                observed_at: chrono::Utc::now(),
                limit_reached: None,
                windows: vec![window(
                    Some(42.0),
                    SubscriptionWindowAvailability::Known,
                    Some(5 * 3600),
                    Some(3600),
                )],
                stale: false,
            }),
        );
        assert!(
            cache
                .chatgpt_subscription_weight_now(endpoint)
                .is_some_and(|weight| weight > 42.0)
        );
    }
}
