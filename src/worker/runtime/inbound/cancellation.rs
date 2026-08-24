use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    db::{self, RequestAbortReason, RequestRecordCategory, RequestRecordState, UsageEventKind},
    worker::runtime::standalone::StandaloneRuntimeState,
    worker_admin::AdminState,
    worker_usage::{UsageLog, UsageRequestMetadata},
};

use super::super::request_assembly::{PendingIncomingRequest, RequestCancellation};

pub(super) async fn cancel_request(
    pending_requests: &Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    cancellations: &Arc<Mutex<HashMap<String, RequestCancellation>>>,
    admin_state: Option<&AdminState>,
    standalone_state: Option<&StandaloneRuntimeState>,
    category: RequestRecordCategory,
    request_id: &str,
    reason: &str,
    response_started: bool,
) {
    let abort_reason = RequestAbortReason::from_relay_reason(reason);
    let abort_message = abort_message(reason, abort_reason);
    if let Some(state) = admin_state
        && let Ok(request_id) = Uuid::parse_str(request_id)
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
                    relay_reason = reason,
                    "request cancellation did not find an active usage record"
                );
            }
            Ok(count) => {
                debug!(
                    request_id = %request_id,
                    aborted_records = count,
                    relay_reason = reason,
                    "recorded request cancellation"
                );
            }
            Err(err) => {
                warn!(
                    error = %err,
                    request_id = %request_id,
                    relay_reason = reason,
                    "failed to record request cancellation"
                );
            }
        }
    }

    if let Some(state) = standalone_state
        && let Ok(request_id) = Uuid::parse_str(request_id)
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
        // durable before this call returns. Persistence failures still
        // warn and keep the in-memory copy as a fallback so the
        // upstream cancellation is not failed by a persistence error.
        let summary = state.record_usage(log);
        state.persist_usage(&summary).await;
    }

    if let Some(pending) = pending_requests.lock().await.remove(request_id) {
        pending.cancellation.cancel();
    }
    if let Some(cancellation) = cancellations.lock().await.remove(request_id) {
        cancellation.cancel();
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
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use tokio::sync::Mutex;
    use uuid::Uuid;

    use super::abort_message;
    use crate::{
        db::{RequestAbortReason, RequestRecordCategory},
        relay_secrets::RelaySecretManager,
        standalone_config::StandaloneConfig,
        standalone_config::StandaloneConfigStore,
        worker::runtime::request_assembly::{PendingIncomingRequest, RequestCancellation},
        worker::runtime::standalone::StandaloneRuntimeState,
    };

    fn database_path(label: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("prompt-ferry-cancel-{label}-{suffix}.sqlite"))
    }

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

    #[tokio::test]
    async fn standalone_cancel_request_awaits_durable_persistence_and_survives_reopen() {
        let path = database_path("awaited");
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([7_u8; 32])).expect("manager");
        let state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        state.hydrate_usage().await;

        let pending_requests: Arc<
            Mutex<std::collections::HashMap<String, PendingIncomingRequest>>,
        > = Arc::new(Mutex::new(std::collections::HashMap::new()));
        let cancellations: Arc<Mutex<std::collections::HashMap<String, RequestCancellation>>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));
        let request_id = Uuid::new_v4();

        // Awaiting cancel_request must complete the durable SQLite write
        // before it returns, so the reopened ledger already contains the
        // aborted summary without any further polling.
        super::cancel_request(
            &pending_requests,
            &cancellations,
            None,
            Some(&state),
            RequestRecordCategory::Ai,
            &request_id.to_string(),
            "downstream_closed",
            false,
        )
        .await;

        // After the awaited call, the open store must already observe the
        // abort event without re-opening the database. This proves the
        // write completed before cancel_request returned.
        let persisted = store
            .list_usage_summaries(16)
            .await
            .expect("list before reopen");
        let aborted = persisted
            .iter()
            .find(|record| record.request_id == request_id)
            .expect("aborted summary must be present after awaited cancel");
        assert_eq!(aborted.state, "aborted");
        assert_eq!(aborted.category, "ai");
        assert_eq!(aborted.event_kind, "request");
        assert_eq!(
            aborted.error_code.as_deref(),
            Some(RequestAbortReason::DownstreamClosed.as_str())
        );

        // Drop the in-process state and reopen the SQLite file to prove
        // the cancelled summary survives process restart like any other
        // standalone usage event.
        drop(state);
        drop(store);

        let reopened = Arc::new(StandaloneConfigStore::open(&path).await.expect("reopen"));
        let reopened_records = reopened
            .list_usage_summaries(16)
            .await
            .expect("list after reopen");
        let reopened_aborted = reopened_records
            .iter()
            .find(|record| record.request_id == request_id)
            .expect("aborted summary must survive reopening the SQLite file");
        assert_eq!(reopened_aborted.state, "aborted");
        assert_eq!(reopened_aborted.category, "ai");
        assert_eq!(
            reopened_aborted.error_code.as_deref(),
            Some(RequestAbortReason::DownstreamClosed.as_str())
        );

        drop(reopened);
        let _ = std::fs::remove_file(path);
    }
}
