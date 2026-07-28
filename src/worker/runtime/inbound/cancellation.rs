use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{db, worker_admin::AdminState};

use super::super::request_assembly::{PendingIncomingRequest, RequestCancellation};

pub(super) async fn cancel_request(
    pending_requests: &Arc<Mutex<HashMap<String, PendingIncomingRequest>>>,
    cancellations: &Arc<Mutex<HashMap<String, RequestCancellation>>>,
    admin_state: Option<&AdminState>,
    request_id: &str,
    reason: &str,
) {
    let abort_message = abort_message(reason);
    if let Some(state) = admin_state
        && let Ok(request_id) = Uuid::parse_str(request_id)
    {
        match db::abort_request_record(&state.lease_pool, request_id, &abort_message).await {
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

fn abort_message(reason: &str) -> String {
    let source = match reason {
        "request_cancelled" => "downstream client",
        "bridge_backpressure" => "relay bridge backpressure",
        _ => "relay",
    };
    format!("request cancelled by {source} before completion (relay reason: {reason})")
}

#[cfg(test)]
mod tests {
    use super::abort_message;

    #[test]
    fn classifies_downstream_disconnects() {
        assert_eq!(
            abort_message("request_cancelled"),
            "request cancelled by downstream client before completion (relay reason: request_cancelled)"
        );
    }

    #[test]
    fn classifies_bridge_backpressure() {
        assert_eq!(
            abort_message("bridge_backpressure"),
            "request cancelled by relay bridge backpressure before completion (relay reason: bridge_backpressure)"
        );
    }

    #[test]
    fn preserves_unknown_relay_reason_for_diagnostics() {
        assert_eq!(
            abort_message("worker_timeout"),
            "request cancelled by relay before completion (relay reason: worker_timeout)"
        );
    }
}
