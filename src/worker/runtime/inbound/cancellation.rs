use std::{collections::HashMap, future::Future, sync::Arc};

use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    db::{self, RequestAbortReason, RequestRecordCategory, RequestRecordState, UsageEventKind},
    worker_usage::{UsageLog, UsageRequestMetadata},
};

use super::super::WorkerRuntimeState;
use super::super::context::RuntimeServices;
use super::super::request_assembly::{PendingIncomingRequest, RequestCancellation};

pub(super) struct CancellationTarget<'a> {
    pub(super) pending_requests: &'a Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    pub(super) cancellations: &'a Arc<Mutex<HashMap<String, RequestCancellation>>>,
    pub(super) request_id: String,
}

/// Remove the owned entries and trip both cancellation tokens.
///
/// Only short in-memory mutexes are held here, so the bridge frame loop keeps
/// running while a slow recorder unwinds. Returns whether any owned entry
/// existed, which is what decides whether the best-effort bookkeeping task is
/// needed at all.
pub(super) async fn trigger_cancellation(target: CancellationTarget<'_>) -> bool {
    let CancellationTarget {
        pending_requests,
        cancellations,
        request_id,
    } = target;
    let mut owned = false;
    if let Some(pending) = pending_requests.lock().await.remove(&request_id) {
        pending.cancellation.cancel();
        owned = true;
    }
    if let Some(cancellation) = cancellations.lock().await.remove(&request_id) {
        cancellation.cancel();
        owned = true;
    }
    owned
}

/// Dispatch a cancellation without awaiting its bookkeeping.
///
/// The trigger runs first; only an actually owned request may start a
/// bookkeeping task, and that task takes the ordinary admission guard before
/// it is spawned so the shutdown drain still owns and joins it. When
/// admission is closed the tokens are still tripped, but no task or guard is
/// created, matching the existing in-flight cancellation path.
pub(super) async fn dispatch_cancellation<T, B, F>(
    runtime_state: &WorkerRuntimeState,
    trigger: T,
    bookkeeping: F,
) where
    T: Future<Output = bool>,
    F: FnOnce() -> B + Send + 'static,
    B: Future<Output = ()> + Send + 'static,
{
    if !trigger.await {
        return;
    }
    let Some(active_guard) = runtime_state.try_track_request() else {
        return;
    };
    runtime_state
        .spawn_accepted(async move {
            let _active_guard = active_guard;
            bookkeeping().await;
        })
        .await;
}

pub(super) async fn record_cancellation(
    services: RuntimeServices,
    request_id: String,
    reason: String,
    category: RequestRecordCategory,
    response_started: bool,
) {
    let abort_reason = RequestAbortReason::from_relay_reason(&reason);
    let abort_message = abort_message(&reason, abort_reason);
    if let Some(state) = services.admin_state()
        && let Ok(request_id) = Uuid::parse_str(&request_id)
    {
        match db::abort_request_record(
            &state.lease_pool,
            request_id,
            abort_reason,
            response_started,
            &abort_message,
        )
        .await
        {
            Ok(0) => {
                debug!(
                    request_id = %request_id,
                    relay_reason = %reason,
                    "request cancellation did not find an active usage record"
                );
            }
            Ok(count) => {
                debug!(
                    request_id = %request_id,
                    aborted_records = count,
                    relay_reason = %reason,
                    "recorded request cancellation"
                );
            }
            Err(err) => {
                warn!(
                    error = %err,
                    request_id = %request_id,
                    relay_reason = %reason,
                    "failed to record request cancellation"
                );
            }
        }
    }

    if let Some(state) = services.standalone_state()
        && let Ok(request_id) = Uuid::parse_str(&request_id)
    {
        let log = match category {
            RequestRecordCategory::Ai => UsageLog::ai_request(
                request_id,
                UsageRequestMetadata {
                    path: "/v1/unknown".to_string(),
                    ..UsageRequestMetadata::default()
                },
                None,
            ),
            RequestRecordCategory::Mcp => UsageLog::mcp_request(
                request_id,
                UsageRequestMetadata {
                    path: "/mcp".to_string(),
                    ..UsageRequestMetadata::default()
                },
                None,
                None,
                None,
            ),
        }
        .with_state(UsageEventKind::Request, RequestRecordState::Aborted)
        .with_status(None, Some(false), None, None)
        .with_error(Some(abort_reason.as_str().to_string()), None, None);
        // Mirror the standalone request-flow contract: push into the
        // bounded in-memory cache synchronously, then await the durable
        // SQLite write-through inline so the cancellation summary is
        // durable before the tracked recorder completes. Persistence
        // failures still warn and keep the in-memory copy as a fallback so
        // the upstream cancellation is not failed by a persistence error.
        let summary = state.record_usage(log);
        state.persist_usage(&summary).await;
    }
}

fn abort_message(reason: &str, abort_reason: RequestAbortReason) -> String {
    format!(
        "request aborted before completion (relay reason: {reason}; abort reason: {})",
        abort_reason.as_str()
    )
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use tokio::sync::{mpsc, oneshot};

    use super::{
        CancellationTarget, RequestCancellation, abort_message, dispatch_cancellation,
        trigger_cancellation,
    };
    use crate::db::RequestAbortReason;
    use crate::worker::runtime::request_assembly::PendingIncomingRequest;
    use crate::worker::runtime::{WorkerRuntimeState, lifecycle};

    #[test]
    fn classifies_downstream_disconnects() {
        assert_eq!(
            abort_message("downstream_closed", RequestAbortReason::DownstreamClosed),
            "request aborted before completion (relay reason: downstream_closed; abort reason: downstream_closed)"
        );
    }

    #[test]
    fn classifies_bridge_backpressure() {
        assert_eq!(
            abort_message(
                "bridge_backpressure_full",
                RequestAbortReason::BridgeBackpressureFull
            ),
            "request aborted before completion (relay reason: bridge_backpressure_full; abort reason: bridge_backpressure_full)"
        );
    }

    #[test]
    fn preserves_unknown_relay_reason_for_diagnostics() {
        assert_eq!(
            abort_message("worker_timeout", RequestAbortReason::RelayUnknown),
            "request aborted before completion (relay reason: worker_timeout; abort reason: relay_unknown)"
        );
    }

    #[test]
    fn maps_legacy_relay_reasons_to_structured_abort_reasons() {
        assert_eq!(
            RequestAbortReason::from_relay_reason("request_cancelled"),
            RequestAbortReason::DownstreamClosed
        );
        assert_eq!(
            RequestAbortReason::from_relay_reason("bridge_backpressure"),
            RequestAbortReason::BridgeBackpressureFull
        );
    }

    fn pending_entry() -> (PendingIncomingRequest, mpsc::Receiver<Vec<u8>>) {
        let (chunk_tx, chunk_rx) = mpsc::channel(1);
        (
            PendingIncomingRequest {
                chunk_tx,
                end_tx: None,
                cancellation: RequestCancellation::default(),
            },
            chunk_rx,
        )
    }

    async fn owned_state(
        request_id: &str,
    ) -> (
        WorkerRuntimeState,
        RequestCancellation,
        RequestCancellation,
        mpsc::Receiver<Vec<u8>>,
    ) {
        let runtime_state = WorkerRuntimeState::default();
        let (pending, chunk_rx) = pending_entry();
        let pending_token = pending.cancellation.clone();
        let cancellation_token = RequestCancellation::default();
        runtime_state
            .pending_requests
            .lock()
            .await
            .insert(request_id.to_string(), pending);
        runtime_state
            .request_cancellations
            .lock()
            .await
            .insert(request_id.to_string(), cancellation_token.clone());
        (runtime_state, pending_token, cancellation_token, chunk_rx)
    }

    fn target<'a>(
        runtime_state: &'a WorkerRuntimeState,
        request_id: &'a str,
    ) -> CancellationTarget<'a> {
        CancellationTarget {
            pending_requests: &runtime_state.pending_requests,
            cancellations: &runtime_state.request_cancellations,
            request_id: request_id.to_string(),
        }
    }

    #[tokio::test]
    async fn trigger_cancels_both_tokens_and_removes_both_entries() {
        let (runtime_state, pending_token, cancellation_token, _chunk_rx) =
            owned_state("owned-trigger").await;

        let owned = trigger_cancellation(target(&runtime_state, "owned-trigger")).await;

        assert!(owned);
        assert!(pending_token.is_cancelled());
        assert!(cancellation_token.is_cancelled());
        assert!(runtime_state.pending_requests.lock().await.is_empty());
        assert!(runtime_state.request_cancellations.lock().await.is_empty());
    }

    #[tokio::test]
    async fn trigger_reports_missing_entries_without_cancelling_anything() {
        let runtime_state = WorkerRuntimeState::default();

        let owned = trigger_cancellation(target(&runtime_state, "unknown")).await;

        assert!(!owned);
        assert!(runtime_state.pending_requests.lock().await.is_empty());
        assert!(runtime_state.request_cancellations.lock().await.is_empty());
    }

    #[tokio::test]
    async fn dispatch_returns_while_the_recorder_is_blocked_after_the_token_cancelled() {
        let (runtime_state, pending_token, _, _chunk_rx) = owned_state("blocked").await;
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let recorder_token = pending_token.clone();

        dispatch_cancellation(
            &runtime_state,
            trigger_cancellation(target(&runtime_state, "blocked")),
            move || async move {
                assert!(
                    recorder_token.is_cancelled(),
                    "the token must be cancelled before the recorder starts"
                );
                let _ = started_tx.send(());
                let _ = release_rx.await;
            },
        )
        .await;

        assert!(pending_token.is_cancelled());
        assert!(runtime_state.pending_requests.lock().await.is_empty());
        assert!(runtime_state.request_cancellations.lock().await.is_empty());
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("the recorder starts while the dispatcher already returned")
            .expect("recorder start signal");

        let _ = release_tx.send(());
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime_state
                    .control
                    .wait_for_drain(Duration::from_millis(1))
                    .await
                    == 0
                {
                    break;
                }
            }
        })
        .await
        .expect("the released recorder releases its guard");
    }

    #[tokio::test]
    async fn duplicate_cancel_dispatch_spawns_no_second_recorder() {
        let (runtime_state, _, _, _chunk_rx) = owned_state("duplicate").await;
        let calls = Arc::new(AtomicUsize::new(0));
        let first_calls = Arc::clone(&calls);
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel::<()>();

        dispatch_cancellation(
            &runtime_state,
            trigger_cancellation(target(&runtime_state, "duplicate")),
            move || async move {
                first_calls.fetch_add(1, Ordering::SeqCst);
                let _ = started_tx.send(());
                let _ = release_rx.await;
            },
        )
        .await;
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("first recorder starts")
            .expect("recorder start signal");

        let second_calls = Arc::clone(&calls);
        dispatch_cancellation(
            &runtime_state,
            trigger_cancellation(target(&runtime_state, "duplicate")),
            move || async move {
                second_calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let _ = release_tx.send(());
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if runtime_state
                    .control
                    .wait_for_drain(Duration::from_millis(1))
                    .await
                    == 0
                {
                    break;
                }
            }
        })
        .await
        .expect("the released recorder releases its guard");
    }

    #[tokio::test]
    async fn admission_closed_cancels_without_scheduling_a_recorder_or_leaking_a_guard() {
        let (runtime_state, pending_token, cancellation_token, _chunk_rx) =
            owned_state("closing").await;
        runtime_state.control.begin_shutdown();
        let calls = Arc::new(AtomicUsize::new(0));
        let recorder_calls = Arc::clone(&calls);

        dispatch_cancellation(
            &runtime_state,
            trigger_cancellation(target(&runtime_state, "closing")),
            move || async move {
                recorder_calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;

        assert!(pending_token.is_cancelled());
        assert!(cancellation_token.is_cancelled());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            runtime_state.control.wait_for_drain(Duration::ZERO).await,
            0,
            "a closed admission must not leak an active guard"
        );
    }

    #[tokio::test]
    async fn tracked_recorder_joins_the_drain_and_a_zero_budget_stop_leaves_no_task_or_guard() {
        let (runtime_state, _, _, _chunk_rx) = owned_state("drainable").await;
        let (started_tx, started_rx) = oneshot::channel();

        dispatch_cancellation(
            &runtime_state,
            trigger_cancellation(target(&runtime_state, "drainable")),
            move || async move {
                let _ = started_tx.send(());
                std::future::pending::<()>().await;
            },
        )
        .await;
        tokio::time::timeout(Duration::from_secs(1), started_rx)
            .await
            .expect("recorder starts")
            .expect("recorder start signal");
        assert_eq!(
            runtime_state.control.wait_for_drain(Duration::ZERO).await,
            1,
            "the blocked recorder is owned by the active drain"
        );

        lifecycle::stop_accepted_tasks(&runtime_state, Duration::ZERO).await;

        assert_eq!(
            runtime_state.control.wait_for_drain(Duration::ZERO).await,
            0
        );
        assert!(runtime_state.take_accepted_tasks().await.is_empty());
    }
}
