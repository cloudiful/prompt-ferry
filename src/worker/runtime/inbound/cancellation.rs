use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{db, db::RequestAbortReason, worker_admin::AdminState};

use super::super::request_assembly::{PendingIncomingRequest, RequestCancellation};

pub(super) async fn cancel_request(
    pending_requests: &Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    cancellations: &Arc<Mutex<HashMap<String, RequestCancellation>>>,
    admin_state: Option<&AdminState>,
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
    use super::abort_message;
    use crate::db::RequestAbortReason;

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
}
