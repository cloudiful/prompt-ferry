use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use uuid::Uuid;

#[derive(Clone, Default)]
pub(super) struct EndpointLoadTracker {
    active: Arc<Mutex<HashMap<Uuid, usize>>>,
}

impl EndpointLoadTracker {
    pub(super) fn reserve(&self, endpoint_id: Uuid) -> Option<EndpointLoadGuard> {
        if endpoint_id.is_nil() {
            return None;
        }
        let mut active = self.active.lock().unwrap_or_else(|err| err.into_inner());
        *active.entry(endpoint_id).or_default() += 1;
        Some(EndpointLoadGuard {
            tracker: self.clone(),
            endpoint_id,
        })
    }

    pub(super) fn reserve_least_loaded(
        &self,
        endpoint_ids_in_tie_order: &[Uuid],
    ) -> Option<(Uuid, EndpointLoadGuard)> {
        let mut active = self.active.lock().unwrap_or_else(|err| err.into_inner());
        let endpoint_id = endpoint_ids_in_tie_order
            .iter()
            .copied()
            .filter(|endpoint_id| !endpoint_id.is_nil())
            .min_by_key(|endpoint_id| active.get(endpoint_id).copied().unwrap_or_default())?;
        *active.entry(endpoint_id).or_default() += 1;
        Some((
            endpoint_id,
            EndpointLoadGuard {
                tracker: self.clone(),
                endpoint_id,
            },
        ))
    }

    #[cfg(test)]
    pub(super) fn active_count(&self, endpoint_id: Uuid) -> usize {
        self.active
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&endpoint_id)
            .copied()
            .unwrap_or_default()
    }
}

pub(super) struct EndpointLoadGuard {
    tracker: EndpointLoadTracker,
    endpoint_id: Uuid,
}

impl Drop for EndpointLoadGuard {
    fn drop(&mut self) {
        let mut active = self
            .tracker
            .active
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let Some(count) = active.get_mut(&self.endpoint_id) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            active.remove(&self.endpoint_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_least_loaded_atomically_and_releases_with_guard() {
        let tracker = EndpointLoadTracker::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();

        let (_, first_guard) = tracker.reserve_least_loaded(&[first, second]).unwrap();
        let (selected, second_guard) = tracker.reserve_least_loaded(&[first, second]).unwrap();

        assert_eq!(selected, second);
        assert_eq!(tracker.active_count(first), 1);
        assert_eq!(tracker.active_count(second), 1);
        drop(first_guard);
        drop(second_guard);
        assert_eq!(tracker.active_count(first), 0);
        assert_eq!(tracker.active_count(second), 0);
    }

    #[test]
    fn tie_order_is_stable() {
        let tracker = EndpointLoadTracker::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let (selected, _guard) = tracker.reserve_least_loaded(&[second, first]).unwrap();
        assert_eq!(selected, second);
    }
}
