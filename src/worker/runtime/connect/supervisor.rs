use reqwest::Client;
use tokio::task::JoinSet;
use tracing::warn;

use super::{
    WorkerConfig, WorkerRuntimeState,
    config::RelayConnectionConfig,
    session::run_relay_loop,
    support::{MissingRelayTarget, SESSION_TEARDOWN_ALLOWANCE},
};

/// Whether the end of a relay session is an unexpected termination.
///
/// A session that ends while the worker is still running lost its relay
/// connection without being asked to, so the worker stops instead of
/// serving without a bridge.
pub(super) fn unexpected_relay_termination(session_ended: bool, shutting_down: bool) -> bool {
    session_ended && !shutting_down
}

/// Spawn one reconnecting bridge session per locally configured relay target.
///
/// Targets come only from `worker.relay_urls`; there is no database or admin
/// surface that adds, removes or edits them while the process runs, so the set
/// is resolved and validated before the worker starts serving and is passed in
/// here. That keeps every fallible relay construction before the services
/// start: this function cannot fail while the worker already owns registry
/// tasks.
///
/// The supervisor waits for the runtime shutdown before it starts its bounded
/// join: the teardown allowance bounds the end of the sessions, it never cuts
/// a healthy session short. A session that ends while the worker still runs is
/// an unexpected termination and stops the worker, because a relay connection
/// that silently disappeared would leave the worker unable to serve.
pub(super) fn spawn_relay_supervisor(
    relays: Vec<RelayConnectionConfig>,
    config: WorkerConfig,
    client: Client,
    admin_state: Option<crate::worker_admin::AdminState>,
    standalone_state: Option<crate::worker::runtime::standalone::StandaloneRuntimeState>,
    runtime_state: WorkerRuntimeState,
    relay_tasks: &mut JoinSet<()>,
) {
    let mut missing = MissingRelayTarget::default();
    missing.observe(!relays.is_empty(), "local relay targets");
    relay_tasks.spawn(async move {
        let (ended_tx, mut ended_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let mut handles = Vec::new();
        for relay in relays {
            let ended_tx = ended_tx.clone();
            let config = config.clone();
            let client = client.clone();
            let admin_state = admin_state.clone();
            let standalone_state = standalone_state.clone();
            let runtime_state = runtime_state.clone();
            handles.push(tokio::spawn(async move {
                run_relay_loop(
                    relay,
                    config,
                    client,
                    admin_state,
                    standalone_state,
                    runtime_state,
                )
                .await;
                let _ = ended_tx.send(());
            }));
        }
        drop(ended_tx);
        // The supervisor waits for the explicit close-bridges signal, which
        // the runtime sends after the accepted work drained. The teardown
        // deadline starts here, never at the beginning of the shutdown: a
        // healthy bridge must survive the drain of an accepted response.
        //
        // A session that ends while the worker still runs is an unexpected
        // termination: it stops the worker, but it never cuts the bridges of
        // the other sessions short. The runtime closes the bridges after the
        // drain of the accepted work.
        loop {
            tokio::select! {
                _ = runtime_state.control.wait_for_session_stop() => break,
                ended = ended_rx.recv() => {
                    if ended.is_none() {
                        // Every session ended.
                        break;
                    }
                    if unexpected_relay_termination(true, runtime_state.control.is_shutting_down()) {
                        warn!(
                            "a relay session ended while the worker was running; stopping the worker service"
                        );
                        runtime_state.control.begin_shutdown();
                    }
                }
            }
        }
        // The sessions get a bounded window to flush their own teardown.
        join_relay_sessions(handles).await;
    });
}

/// Give each session supervisor bounded time to finish its own flush, then
/// abort whatever is still running. The allowance is global, so N draining
/// sessions never add up to an unbounded serial wait.
async fn join_relay_sessions(handles: Vec<tokio::task::JoinHandle<()>>) {
    let deadline = tokio::time::sleep(SESSION_TEARDOWN_ALLOWANCE);
    tokio::pin!(deadline);
    for mut handle in handles {
        tokio::select! {
            _ = &mut handle => {}
            _ = &mut deadline => {
                handle.abort();
                let _ = handle.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unexpected_relay_termination;

    #[test]
    fn a_session_that_ends_during_the_shutdown_is_not_an_unexpected_termination() {
        assert!(unexpected_relay_termination(true, false));
        assert!(!unexpected_relay_termination(true, true));
        assert!(!unexpected_relay_termination(false, false));
        assert!(!unexpected_relay_termination(false, true));
    }
}
