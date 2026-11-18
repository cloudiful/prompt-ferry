mod session;
#[cfg(test)]
mod support;

pub(crate) use session::ExecutorPump;
#[cfg(test)]
pub(super) use support::is_expected_relay_disconnect;

#[cfg(test)]
use super::{WorkerRuntimeState, lifecycle::stop_accepted_tasks};
#[cfg(test)]
use std::{sync::Arc, time::Duration};

#[cfg(test)]
const FINAL_DRAIN_WAIT: Duration = Duration::from_secs(10);

#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct AcceptedStopGate {
    lock: Arc<tokio::sync::Mutex<()>>,
}

#[cfg(test)]
impl AcceptedStopGate {
    pub(super) async fn stop(&self, runtime_state: &WorkerRuntimeState, allowance: Duration) {
        let _gate = self.lock.lock().await;
        stop_accepted_tasks(runtime_state, allowance).await;
    }
}

#[cfg(test)]
pub(super) struct FailClosedMonitor {
    stop_tx: tokio::sync::watch::Sender<bool>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

#[cfg(test)]
impl FailClosedMonitor {
    pub(super) fn spawn(
        runtime_state: WorkerRuntimeState,
        admin: Option<crate::worker_admin::state::AdminExecutionTracker>,
        stop_gate: AcceptedStopGate,
    ) -> Self {
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let control = runtime_state.control.clone();
        let handle = tokio::spawn(async move {
            let escalation = async { control.wait_for_fail_closed().await };
            let stop = async {
                while !*stop_rx.borrow() {
                    if stop_rx.changed().await.is_err() {
                        return;
                    }
                }
            };
            tokio::select! { _ = escalation => {}, _ = stop => {} }
            if runtime_state.control.is_fail_closed() {
                if let Some(tracker) = &admin {
                    tracker.cancel();
                }
                cancel_in_flight_requests(&runtime_state).await;
                stop_gate.stop(&runtime_state, FINAL_DRAIN_WAIT).await;
            }
        });
        Self {
            stop_tx,
            handle: Some(handle),
        }
    }

    pub(super) async fn finish(mut self) {
        let _ = self.stop_tx.send(true);
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    #[cfg(test)]
    pub(super) fn handle_mut(&mut self) -> &mut tokio::task::JoinHandle<()> {
        self.handle.as_mut().expect("the monitor task is owned")
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct AdminExecutionLifecycle {
    tracker: Option<crate::worker_admin::state::AdminExecutionTracker>,
}

#[cfg(test)]
impl AdminExecutionLifecycle {
    pub(super) fn monitor_tracker(
        &self,
    ) -> Option<crate::worker_admin::state::AdminExecutionTracker> {
        self.tracker.clone()
    }
    pub(super) async fn wait_for_idle(&self, budget: Duration) -> usize {
        match &self.tracker {
            Some(tracker) => tracker.wait_for_idle(budget).await,
            None => 0,
        }
    }
    #[cfg(test)]
    pub(super) fn without_postgres_runtime_state(
        tracker: crate::worker_admin::state::AdminExecutionTracker,
    ) -> Self {
        Self {
            tracker: Some(tracker),
        }
    }
}

#[cfg(test)]
async fn cancel_in_flight_requests(runtime_state: &WorkerRuntimeState) {
    let requests = {
        let cancellations = runtime_state.request_cancellations.lock().await;
        cancellations.values().cloned().collect::<Vec<_>>()
    };
    let mcp_requests = {
        let cancellations = runtime_state.mcp_request_cancellations.lock().await;
        cancellations.values().cloned().collect::<Vec<_>>()
    };
    for cancellation in requests.into_iter().chain(mcp_requests) {
        cancellation.cancel();
    }
}
