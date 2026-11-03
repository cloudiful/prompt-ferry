//! Bounded, cancellable ownership of MCP catalog refresh work.
//!
//! The coordinator is owned by the cache, so every service wrapper that shares
//! one cache shares the same permit pool, the same per-server scheduling and
//! the same cancellation scope. A transient service inner therefore never
//! carries refresh state of its own.
//!
//! One counter tracks every admitted operation: queued reservations, direct
//! reads, and manual refreshes. Cancellation and admission share one lock, so a
//! drain can never report zero while an operation is still untracked.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use chrono::{DateTime, Utc};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tracing::debug;
use uuid::Uuid;

/// The production bound on concurrent catalog refresh work.
pub(crate) const DEFAULT_REFRESH_CONCURRENCY: usize = 8;

/// The bound on outstanding refresh work that has not finished yet.
///
/// This counts reservations, not distinct servers: a superseded generation
/// still consumes a slot until its owner retires, so configuration churn can
/// never grow an unbounded waiting queue.
const PENDING_REFRESH_LIMIT: usize = 128;

/// One queued refresh of one server configuration generation.
struct PendingRefresh {
    token: Uuid,
    updated_at: DateTime<Utc>,
}

/// The admit/drain state guarded by one lock.
struct ActivityState {
    cancelled: bool,
    outstanding: usize,
    pending: HashMap<Uuid, PendingRefresh>,
}

/// The shared refresh coordinator of one catalog cache.
pub(crate) struct CatalogRefreshCoordinator {
    capacity: AtomicUsize,
    started: AtomicBool,
    permits: OnceLock<Arc<Semaphore>>,
    state: Mutex<ActivityState>,
    cancel_notify: Notify,
    drained_notify: Notify,
}

impl Default for CatalogRefreshCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl CatalogRefreshCoordinator {
    pub(crate) fn new() -> Self {
        Self {
            capacity: AtomicUsize::new(DEFAULT_REFRESH_CONCURRENCY),
            started: AtomicBool::new(false),
            permits: OnceLock::new(),
            state: Mutex::new(ActivityState {
                cancelled: false,
                outstanding: 0,
                pending: HashMap::new(),
            }),
            cancel_notify: Notify::new(),
            drained_notify: Notify::new(),
        }
    }

    /// Adopt the refresh bound of the service policy that will use this
    /// coordinator.
    ///
    /// The bound is shared, so the first policy to drive real work wins: a
    /// later wrapper never resizes a pool that already has permits in flight.
    pub(crate) fn configure_capacity(&self, concurrency: usize) {
        if self.started.load(Ordering::SeqCst) {
            return;
        }
        self.capacity.store(concurrency.max(1), Ordering::SeqCst);
    }

    fn semaphore(&self) -> &Arc<Semaphore> {
        self.permits
            .get_or_init(|| Arc::new(Semaphore::new(self.capacity.load(Ordering::SeqCst).max(1))))
    }

    fn mark_started(&self) {
        self.started.store(true, Ordering::SeqCst);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ActivityState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    /// Resolve when refresh work is cancelled.
    pub(crate) async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.cancel_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }

    /// Stop scheduling new refresh work and wake every in-flight owner.
    pub(crate) fn cancel(&self) {
        let cancelled_now = {
            let mut state = self.lock();
            let cancelled_now = !state.cancelled;
            state.cancelled = true;
            cancelled_now
        };
        if cancelled_now {
            self.cancel_notify.notify_waiters();
        }
    }

    /// Admit one tracked operation.
    ///
    /// `None` means the coordinator is cancelled; the caller must not start any
    /// refresh work. The returned activity keeps the operation visible to the
    /// drain until the last clone is dropped.
    pub(crate) fn admit(self: &Arc<Self>) -> Option<CatalogActivity> {
        let mut state = self.lock();
        if state.cancelled {
            return None;
        }
        state.outstanding += 1;
        drop(state);
        self.mark_started();
        Some(self.activity())
    }

    fn activity(self: &Arc<Self>) -> CatalogActivity {
        CatalogActivity {
            _handle: Arc::new(ActivityHandle {
                coordinator: self.clone(),
            }),
        }
    }

    /// Take one owner permit.
    ///
    /// `None` means the coordinator is cancelled and the refresh must not
    /// start.
    pub(crate) async fn acquire_permit(self: &Arc<Self>) -> Option<OwnedSemaphorePermit> {
        if self.is_cancelled() {
            return None;
        }
        self.mark_started();
        let semaphore = self.semaphore().clone();
        tokio::select! {
            _ = self.cancelled() => None,
            permit = semaphore.acquire_owned() => permit.ok(),
        }
    }

    /// Queue one refresh for a server configuration generation.
    ///
    /// `None` means nothing new was queued: the same generation is already
    /// queued or running, the outstanding work is at its bound, or the
    /// coordinator is cancelled.
    pub(crate) fn reserve(
        self: &Arc<Self>,
        server_id: Uuid,
        updated_at: DateTime<Utc>,
    ) -> Option<RefreshReservation> {
        let mut state = self.lock();
        if state.cancelled {
            return None;
        }
        if let Some(existing) = state.pending.get(&server_id)
            && existing.updated_at == updated_at
        {
            return None;
        }
        if state.outstanding >= PENDING_REFRESH_LIMIT {
            drop(state);
            debug!(
                server_id = %server_id,
                limit = PENDING_REFRESH_LIMIT,
                "MCP catalog refresh queue is at its bound; the next pass retries"
            );
            return None;
        }
        let token = Uuid::new_v4();
        state
            .pending
            .insert(server_id, PendingRefresh { token, updated_at });
        state.outstanding += 1;
        drop(state);
        self.mark_started();
        Some(RefreshReservation {
            coordinator: self.clone(),
            server_id,
            token,
            activity: self.activity(),
            finished: false,
        })
    }

    /// Retire the queue entry of one reservation without touching accounting.
    ///
    /// The reservation's activity guard owns the decrement, so an aborted
    /// caller that still has lease cleanup running stays tracked.
    fn retire(&self, server_id: Uuid, token: Uuid) {
        {
            let mut state = self.lock();
            if state
                .pending
                .get(&server_id)
                .is_some_and(|entry| entry.token == token)
            {
                state.pending.remove(&server_id);
            }
        }
        self.drained_notify.notify_waiters();
    }

    fn release(&self) {
        {
            let mut state = self.lock();
            state.outstanding = state.outstanding.saturating_sub(1);
        }
        self.drained_notify.notify_waiters();
    }

    /// Stop scheduling and wait until every admitted operation finished.
    ///
    /// A cancelled owner releases its lease explicitly, so the shared ownership
    /// ends with this call rather than with the lease expiry.
    pub(crate) async fn cancel_and_drain(&self, deadline: Duration) {
        self.cancel();
        let _ = tokio::time::timeout(deadline, async {
            loop {
                let notified = self.drained_notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.lock().outstanding == 0 {
                    return;
                }
                notified.await;
            }
        })
        .await;
    }

    /// How many operations the coordinator currently tracks.
    #[cfg(test)]
    pub(crate) fn tracked_refreshes(&self) -> usize {
        self.lock().outstanding
    }

    /// How many distinct servers are queued and not yet completed.
    #[cfg(test)]
    pub(crate) fn queued_refreshes(&self) -> usize {
        self.lock().pending.len()
    }
}

/// One admitted operation that must finish before a drain may return.
///
/// Clones share one ref-counted guard, so the operation is decremented exactly
/// once, after the last clone — including a clone moved into lease cleanup —
/// drops. A caller future that is aborted therefore keeps its accounting until
/// the bounded cleanup finishes.
#[derive(Clone)]
pub(crate) struct CatalogActivity {
    // Intentional RAII-only field: the ref-count and its Drop, not a read, are
    // what keep the operation tracked.
    _handle: Arc<ActivityHandle>,
}

struct ActivityHandle {
    coordinator: Arc<CatalogRefreshCoordinator>,
}

impl Drop for ActivityHandle {
    fn drop(&mut self) {
        self.coordinator.release();
    }
}

/// One queued refresh owned by the coordinator.
pub(crate) struct RefreshReservation {
    coordinator: Arc<CatalogRefreshCoordinator>,
    server_id: Uuid,
    token: Uuid,
    activity: CatalogActivity,
    finished: bool,
}

impl RefreshReservation {
    /// A clone of the reservation's accounting, for lease cleanup ownership.
    pub(crate) fn activity(&self) -> CatalogActivity {
        self.activity.clone()
    }

    pub(crate) fn finish(mut self) {
        self.coordinator.retire(self.server_id, self.token);
        self.finished = true;
    }
}

impl Drop for RefreshReservation {
    fn drop(&mut self) {
        if !self.finished {
            self.coordinator.retire(self.server_id, self.token);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator() -> Arc<CatalogRefreshCoordinator> {
        Arc::new(CatalogRefreshCoordinator::new())
    }

    #[tokio::test]
    async fn a_second_reservation_of_one_generation_is_coalesced() {
        let coordinator = coordinator();
        let server_id = Uuid::new_v4();
        let generation = Utc::now();

        let first = coordinator
            .reserve(server_id, generation)
            .expect("first reservation");
        assert!(
            coordinator.reserve(server_id, generation).is_none(),
            "the same generation must not be queued twice"
        );

        first.finish();
        assert!(
            coordinator.reserve(server_id, generation).is_some(),
            "a finished refresh may be queued again"
        );
    }

    #[tokio::test]
    async fn a_newer_generation_replaces_a_queued_one() {
        let coordinator = coordinator();
        let server_id = Uuid::new_v4();
        let newer = Utc::now() + chrono::Duration::seconds(1);
        let first = coordinator
            .reserve(server_id, Utc::now())
            .expect("first reservation");
        let second = coordinator
            .reserve(server_id, newer)
            .expect("newer generation is queued");

        // The superseded refresh finishing must not drop the newer queue entry.
        first.finish();
        assert!(
            coordinator.reserve(server_id, newer).is_none(),
            "the newer generation is still queued"
        );
        second.finish();
    }

    #[tokio::test]
    async fn the_cancellation_stops_scheduling_and_drains_owners() {
        let coordinator = coordinator();
        let lease = coordinator
            .reserve(Uuid::new_v4(), Utc::now())
            .expect("reservation");
        coordinator.cancel();

        assert!(coordinator.reserve(Uuid::new_v4(), Utc::now()).is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(10), coordinator.acquire_permit())
                .await
                .expect("permit acquisition resolves")
                .is_none()
        );

        let drain = tokio::spawn({
            let coordinator = coordinator.clone();
            async move { coordinator.cancel_and_drain(Duration::from_secs(1)).await }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!drain.is_finished(), "the owner still holds the drain open");
        lease.finish();
        drain.await.expect("drain task");
    }

    #[tokio::test]
    async fn reviewer_r2_replaced_generations_stay_inside_pending_bound() {
        let coordinator = coordinator();
        let server_id = Uuid::new_v4();
        let generation = Utc::now();
        let mut reservations = Vec::new();
        for index in 0..=PENDING_REFRESH_LIMIT {
            if let Some(reservation) = coordinator.reserve(
                server_id,
                generation + chrono::Duration::seconds(index as i64),
            ) {
                reservations.push(reservation);
            }
        }
        let tracked = coordinator.tracked_refreshes();
        drop(reservations);
        coordinator.cancel_and_drain(Duration::from_secs(1)).await;
        assert!(
            tracked <= PENDING_REFRESH_LIMIT,
            "pending bound is {PENDING_REFRESH_LIMIT}, but {tracked} generations remain tracked"
        );
    }

    #[tokio::test]
    async fn a_superseded_reservation_releases_its_own_accounting() {
        let coordinator = coordinator();
        let server_id = Uuid::new_v4();
        let generation = Utc::now();
        let superseded = coordinator
            .reserve(server_id, generation)
            .expect("first reservation");
        let current = coordinator
            .reserve(server_id, generation + chrono::Duration::seconds(1))
            .expect("newer reservation");
        assert_eq!(coordinator.tracked_refreshes(), 2);

        superseded.finish();
        assert_eq!(
            coordinator.tracked_refreshes(),
            1,
            "the superseded generation must release exactly its own slot"
        );
        assert_eq!(
            coordinator.queued_refreshes(),
            1,
            "the newer generation is still queued"
        );
        current.finish();
        assert_eq!(coordinator.tracked_refreshes(), 0);
    }

    #[tokio::test]
    async fn a_direct_activity_is_drained() {
        let coordinator = coordinator();
        let activity = coordinator.admit().expect("activity");
        coordinator.cancel();
        assert!(coordinator.admit().is_none(), "cancel closes admission");

        let drain = tokio::spawn({
            let coordinator = coordinator.clone();
            async move { coordinator.cancel_and_drain(Duration::from_secs(1)).await }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !drain.is_finished(),
            "a direct operation still holds the drain open"
        );
        drop(activity);
        drain.await.expect("drain task");
    }

    #[tokio::test]
    async fn the_permit_pool_is_bounded() {
        let coordinator = coordinator();
        coordinator.configure_capacity(2);

        let first = coordinator.acquire_permit().await.expect("first permit");
        let second = coordinator.acquire_permit().await.expect("second permit");
        assert!(
            tokio::time::timeout(Duration::from_millis(20), coordinator.acquire_permit())
                .await
                .is_err(),
            "a third owner must wait for the configured bound"
        );

        drop(first);
        assert!(
            tokio::time::timeout(Duration::from_millis(200), coordinator.acquire_permit())
                .await
                .expect("a released permit is handed out")
                .is_some()
        );
        drop(second);
    }
}
