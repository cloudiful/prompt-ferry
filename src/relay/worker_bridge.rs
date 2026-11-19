use crate::{
    bridge::link::{LinkEvent, LinkReceiver},
    ip_acl,
    protocol::{BridgeMessage, ConfigSnapshot, ResponseError},
};

use super::{
    response_forward::{
        fail_pending_for_worker, handle_mcp_response_chunk, handle_mcp_response_end,
        handle_mcp_response_error, handle_mcp_response_start, handle_realtime_server_event,
        handle_realtime_session_close, handle_realtime_session_error, handle_response_chunk,
        handle_response_end, handle_response_error, handle_response_start,
    },
    state::{AppState, MAX_WORKER_CONNECTIONS, WorkerSender},
};
use axum::http::StatusCode;
use std::collections::HashMap;
#[cfg(test)]
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Attach a local in-memory executor as a worker of this relay state.
///
/// The relay keeps the same sender registration, dispatch and disconnect
/// cleanup as the websocket path; only the transport differs. The returned
/// task drains the executor responses until the executor drops its outbound
/// half, then performs the ordinary worker cleanup.
pub(crate) async fn attach_in_memory_worker(
    state: AppState,
    worker_id: usize,
    request_tx: WorkerSender,
    mut response_rx: LinkReceiver,
) -> Option<tokio::task::JoinHandle<()>> {
    if !register_worker(&state, worker_id, request_tx).await {
        return None;
    }
    info!(worker_id, "worker connected over an in-memory link");
    Some(tokio::spawn(async move {
        while let Some(event) = response_rx.next().await {
            match event {
                LinkEvent::Data(item) => {
                    let (message, permit) = item.into_parts();
                    dispatch_worker_bridge_message(&state, message).await;
                    // Keep the outbound byte permit until the relay handled
                    // the item, matching the socket write lifetime.
                    drop(permit);
                }
                LinkEvent::Control(message) => {
                    dispatch_worker_bridge_message(&state, message).await
                }
                LinkEvent::Flush(ack) => {
                    let mut acknowledgements = vec![ack];
                    while let Some(event) = response_rx.try_next() {
                        match event {
                            LinkEvent::Data(item) => {
                                let (message, permit) = item.into_parts();
                                dispatch_worker_bridge_message(&state, message).await;
                                drop(permit);
                            }
                            LinkEvent::Control(message) => {
                                dispatch_worker_bridge_message(&state, message).await
                            }
                            LinkEvent::Flush(ack) => acknowledgements.push(ack),
                        }
                    }
                    for ack in acknowledgements {
                        let _ = ack.send(());
                    }
                }
            }
        }
        cleanup_worker(&state, worker_id).await;
    }))
}

/// Remove a worker and fail everything it still owned.
///
/// This is shared by the socket adapter and the local in-memory link so a
/// disconnect has exactly the same route reset and pending failure behaviour.
pub(crate) async fn cleanup_worker(state: &AppState, worker_id: usize) {
    // The observation of the last worker and the reset of the snapshot stay
    // in one critical section of the worker registration: a new worker that
    // registers between the observation and the reset would publish its own
    // snapshot, and the reset of the older cleanup would erase it.
    //
    // The lock order is workers, then the configuration version, then the
    // routes. A snapshot is applied under the configuration version alone, so
    // this order cannot deadlock.
    let last_worker_gone = {
        let mut workers = state.inner.workers.lock().await;
        workers.remove(&worker_id);
        let last_worker_gone = workers.is_empty();
        if last_worker_gone {
            // The relay is no longer serviceable. Drop the stale snapshot so
            // `/ready` flips red immediately and a managed key is never
            // matched against routes from a worker that is gone. The version
            // lock is held across the reset, so the reset cannot interleave
            // with a snapshot that is applied at the same time.
            let mut applied_version = state.inner.config_version.lock().await;
            state.inner.routes.lock().await.clear();
            *state.inner.relay_ip_policy.lock().await = ip_acl::CompiledRelayIpPolicy::default();
            *applied_version = None;
        }
        last_worker_gone
    };
    state.inner.worker_loads.lock().await.remove(&worker_id);
    if last_worker_gone {
        debug!(
            worker_id,
            "the last worker left; the relay configuration snapshot was reset"
        );
    }
    warn!(
        category = "mcp_bridge_diag",
        worker_id, "worker bridge connection ended; pending requests will be failed"
    );
    fail_pending_for_worker(
        state,
        worker_id,
        ResponseError {
            request_id: String::new(),
            status: StatusCode::BAD_GATEWAY.as_u16(),
            code: "worker_disconnected".to_string(),
            message: "worker disconnected".to_string(),
        },
    )
    .await;
    info!(worker_id, "worker disconnected");
}

pub(super) async fn handle_config_snapshot(state: &AppState, snapshot: ConfigSnapshot) {
    let version = snapshot.version;
    // Several workers publish snapshots for the same relay. The versions come
    // from a shared database counter, so a snapshot that is not newer than the
    // applied one is stale and must not undo it.
    //
    // The version check and the whole apply are one serialized section: the
    // configuration version lock is held while the routes, the IP policy and
    // the version itself are replaced, so two concurrent snapshots can never
    // interleave and let an older one win.
    let mut applied_version = state.inner.config_version.lock().await;
    if let Some(current) = *applied_version
        && version <= current
    {
        warn!(version, current, "ignoring a stale relay config snapshot");
        return;
    }
    let routes = snapshot
        .keys
        .into_iter()
        .map(|route| (route.key_hash.clone(), route))
        .collect::<HashMap<_, _>>();
    let route_count = routes.len();
    let relay_ip_policy = match ip_acl::compile_policy(&snapshot.relay_ip_policy) {
        Ok(policy) => policy,
        Err(err) => {
            warn!(error = %err, "ignoring invalid relay ip policy from worker snapshot");
            state.inner.relay_ip_policy.lock().await.clone()
        }
    };
    *state.inner.routes.lock().await = routes;
    *state.inner.relay_ip_policy.lock().await = relay_ip_policy;
    *applied_version = Some(version);
    info!(version, route_count, "relay config snapshot applied");
}

pub(crate) async fn register_worker(state: &AppState, worker_id: usize, tx: WorkerSender) -> bool {
    let mut workers = state.inner.workers.lock().await;
    if workers.len() >= MAX_WORKER_CONNECTIONS {
        warn!(
            worker_id,
            max_workers = MAX_WORKER_CONNECTIONS,
            "worker connection rejected: worker connection limit reached"
        );
        return false;
    }
    workers.insert(worker_id, tx);
    state.inner.worker_loads.lock().await.insert(worker_id, 0);
    true
}

pub(crate) async fn dispatch_worker_bridge_message(state: &AppState, message: BridgeMessage) {
    match message {
        BridgeMessage::ResponseStart(start) => handle_response_start(state, start).await,
        BridgeMessage::ResponseChunk(chunk) => handle_response_chunk(state, chunk).await,
        BridgeMessage::ResponseEnd(end) => handle_response_end(state, end).await,
        BridgeMessage::ResponseError(err) => {
            handle_response_error(state, err.clone()).await;
            handle_mcp_response_error(state, err.clone()).await;
            handle_realtime_session_error(state, err).await;
        }
        BridgeMessage::RealtimeServerEvent(event) => {
            handle_realtime_server_event(state, event).await
        }
        BridgeMessage::RealtimeSessionClose(close) => {
            handle_realtime_session_close(state, close).await
        }
        BridgeMessage::McpResponseStart(start) => handle_mcp_response_start(state, start).await,
        BridgeMessage::McpResponseChunk(chunk) => handle_mcp_response_chunk(state, chunk).await,
        BridgeMessage::McpResponseEnd(end) => handle_mcp_response_end(state, end).await,
        BridgeMessage::ConfigSnapshot(snapshot) => handle_config_snapshot(state, snapshot).await,
        BridgeMessage::Pong | BridgeMessage::Ping => debug!("worker heartbeat"),
        BridgeMessage::RealtimeSessionStart(_) | BridgeMessage::RealtimeClientEvent(_) => {
            warn!("worker sent unexpected realtime request")
        }
        BridgeMessage::RequestStart(_)
        | BridgeMessage::RequestChunk(_)
        | BridgeMessage::RequestEnd(_)
        | BridgeMessage::RequestCancel(_) => warn!("worker sent unexpected request"),
        BridgeMessage::McpRequestStart(_)
        | BridgeMessage::McpRequestChunk(_)
        | BridgeMessage::McpRequestEnd(_)
        | BridgeMessage::McpRequestCancel(_) => warn!("worker sent unexpected mcp request"),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{response_forward::fail_pending_for_worker, state::test_state};
    use super::*;
    use crate::bridge::link::{InMemoryLink, in_memory_link};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn fail_pending_for_worker_reports_disconnect_to_waiting_requests() {
        let state = test_state();
        let (start_tx, start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(1);
        let (forward_tx, _forward_rx) = mpsc::unbounded_channel();
        let (worker, _worker_rx) = mpsc::channel(1);
        state.inner.pending.lock().await.insert(
            "req-1".to_string(),
            super::super::state::PendingRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                forward_tx,
                worker_id: 1,
                worker,
                queued_bytes: 0,
                response_started: false,
            },
        );

        fail_pending_for_worker(
            &state,
            1,
            ResponseError {
                request_id: String::new(),
                status: StatusCode::BAD_GATEWAY.as_u16(),
                code: "worker_disconnected".to_string(),
                message: "worker disconnected".to_string(),
            },
        )
        .await;

        let err = start_rx.await.unwrap().unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_GATEWAY.as_u16());
        assert_eq!(err.code, "worker_disconnected");
        assert_eq!(err.request_id, "req-1");
    }

    #[tokio::test]
    async fn fail_pending_for_worker_reports_mcp_disconnect_to_waiting_requests() {
        let state = test_state();
        let (start_tx, start_rx) = oneshot::channel();
        let (chunk_tx, _chunk_rx) = mpsc::channel(1);
        let (forward_tx, _forward_rx) = mpsc::unbounded_channel();
        let (worker, _worker_rx) = mpsc::channel(1);
        state.inner.pending_mcp.lock().await.insert(
            "mcp-req-1".to_string(),
            super::super::state::PendingMcpRequest {
                start_tx: Some(start_tx),
                chunk_tx,
                forward_tx,
                worker_id: 3,
                worker,
                queued_bytes: 0,
                response_started: false,
            },
        );

        fail_pending_for_worker(
            &state,
            3,
            ResponseError {
                request_id: String::new(),
                status: StatusCode::BAD_GATEWAY.as_u16(),
                code: "worker_disconnected".to_string(),
                message: "worker disconnected".to_string(),
            },
        )
        .await;

        let err = start_rx.await.unwrap().unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_GATEWAY.as_u16());
        assert_eq!(err.code, "worker_disconnected");
        assert_eq!(err.request_id, "mcp-req-1");
    }

    #[tokio::test]
    async fn in_memory_worker_link_registers_dispatches_and_cleans_up() {
        use std::time::Duration;

        let state = test_state();
        let InMemoryLink {
            relay_request_tx,
            mut executor_request_rx,
            executor_response_tx,
            relay_response_rx,
        } = in_memory_link();
        let relay_tx = relay_request_tx.clone();
        let task = attach_in_memory_worker(state.clone(), 7, relay_request_tx, relay_response_rx)
            .await
            .expect("the in-memory worker is registered");
        assert!(state.inner.workers.lock().await.contains_key(&7));
        assert_eq!(state.inner.worker_loads.lock().await.get(&7), Some(&0));

        let cancel = BridgeMessage::RequestCancel(crate::protocol::BridgeRequestCancel {
            request_id: "request-1".to_string(),
            reason: "client_cancel".to_string(),
            response_started: false,
        });
        relay_tx
            .send(cancel.clone())
            .await
            .expect("requests reach the executor half");
        assert_eq!(executor_request_rx.recv().await, Some(cancel));

        executor_response_tx
            .send(BridgeMessage::Pong)
            .await
            .expect("responses reach the relay half");
        assert!(
            executor_response_tx.flush(Duration::from_secs(1)).await,
            "the relay acknowledges a flush only after dispatching the preceding response"
        );

        drop(executor_response_tx);
        task.await
            .expect("the relay link task ends with the executor half");
        assert!(!state.inner.workers.lock().await.contains_key(&7));
        assert!(!state.inner.worker_loads.lock().await.contains_key(&7));
    }

    #[tokio::test]
    async fn the_last_worker_cleanup_keeps_the_registration_and_the_reset_together() {
        use crate::protocol::{ClientRoute, ConfigSnapshot};

        fn snapshot(version: i64, key_hash: &str) -> ConfigSnapshot {
            ConfigSnapshot {
                version,
                keys: vec![ClientRoute {
                    key_hash: key_hash.to_string(),
                    key_prefix: "pfy_".to_string(),
                    user_id: 1,
                    route_id: "route".to_string(),
                }],
                relay_ip_policy: Default::default(),
            }
        }

        let state = test_state();
        let (tx, _rx) = mpsc::channel(1);
        assert!(register_worker(&state, 1, tx).await);
        handle_config_snapshot(&state, snapshot(7, "hash-7")).await;
        assert_eq!(*state.inner.config_version.lock().await, Some(7));

        // The reset of the snapshot is blocked on the routes, so the cleanup
        // stops between the observation of the last worker and the reset.
        let routes = state.inner.routes.lock().await;
        let cleanup = tokio::spawn({
            let state = state.clone();
            async move { cleanup_worker(&state, 1).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // A new worker must not register while the cleanup of the old one is
        // still resetting the snapshot: the new registration and its snapshot
        // would be erased by the older cleanup.
        let (new_tx, _new_rx) = mpsc::channel(1);
        let register = tokio::spawn({
            let state = state.clone();
            async move { register_worker(&state, 2, new_tx).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !register.is_finished(),
            "a new worker registered while the last worker cleanup was still resetting the snapshot"
        );

        drop(routes);
        cleanup.await.expect("cleanup joins");
        assert!(register.await.expect("registration joins"));
        assert_eq!(state.inner.workers.lock().await.len(), 1);

        // The snapshot of the new worker survives the cleanup of the old one.
        handle_config_snapshot(&state, snapshot(8, "hash-8")).await;
        assert_eq!(*state.inner.config_version.lock().await, Some(8));
        assert_eq!(state.inner.routes.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn choose_worker_returns_none_without_workers() {
        let state = test_state();
        assert!(
            super::super::response_forward::choose_worker(&state)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn register_worker_accepts_workers_until_capacity() {
        let state = test_state();
        for worker_id in 1..=MAX_WORKER_CONNECTIONS {
            let (tx, _rx) = mpsc::channel(1);
            assert!(register_worker(&state, worker_id, tx).await);
        }
        let (tx, _rx) = mpsc::channel(1);
        assert!(!register_worker(&state, MAX_WORKER_CONNECTIONS + 1, tx).await);
        assert_eq!(
            state.inner.workers.lock().await.len(),
            MAX_WORKER_CONNECTIONS
        );
    }
}
