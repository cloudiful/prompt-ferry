use std::{future::Future, sync::Arc, time::Duration};

use crate::worker_admin::chatgpt_quota_service::{
    ChatGptQuotaService, RequestWakeReason, RequestWakeReceiver,
};
use scheduler::{Job, Schedule, Task, TaskContext, ValkeyCoordinatedStateStore};
use tokio::task::JoinHandle;
use tracing::warn;

use super::{
    background_job::{coordinated_scheduler, run_coordinated_scheduler},
    lifecycle::RuntimeControl,
};

const COLLECTOR_TICK: Duration = Duration::from_secs(60);
const REQUEST_WAKE_TIMEOUT: Duration = Duration::from_secs(44);

#[cfg(test)]
mod tests;

pub(super) fn spawn(
    shared_valkey: &crate::db::SharedValkeyConfig,
    service: Arc<ChatGptQuotaService>,
    control: RuntimeControl,
    coordinated_store: Option<ValkeyCoordinatedStateStore>,
) -> JoinHandle<()> {
    let valkey_url = shared_valkey.url_or_empty().trim().to_string();
    tokio::spawn(async move {
        let Some(receiver) = service.take_request_wake_receiver() else {
            warn!("ChatGPT quota request wake worker was already started");
            return;
        };
        let request_wakes = run_request_wakes(service.clone(), receiver, control.clone());

        let periodic = async {
            if !periodic_coordination_configured(&valkey_url) {
                warn!(
                    capability = "chatgpt_quota_collection",
                    "periodic ChatGPT quota collection disabled because Valkey coordination is not configured"
                );
            } else if let Some(store) = coordinated_store {
                let scheduler = coordinated_scheduler(store);
                if let Err(error) = run_coordinated_scheduler(
                    scheduler,
                    collector_job(service, control.clone()),
                    valkey_url,
                    control.clone(),
                )
                .await
                {
                    warn!(
                        error = %error,
                        capability = "chatgpt_quota_collection",
                        "coordinated ChatGPT quota collector stopped"
                    );
                    control.begin_fail_closed_shutdown();
                }
            } else {
                warn!(
                    capability = "chatgpt_quota_collection",
                    "the coordinated ChatGPT quota store is unavailable; stopping the worker"
                );
                control.begin_fail_closed_shutdown();
            }
            if !control.is_shutting_down() {
                control.wait_for_shutdown().await;
            }
        };
        run_owned_worker(request_wakes, periodic).await;
    })
}

/// Run the request-wake loop and the periodic collector as one owned future.
///
/// The request-wake loop is not spawned: it is part of this task, so aborting
/// or joining the outer task also cancels the loop. A nested `JoinHandle`
/// would detach on that abort and outlive the worker teardown.
async fn run_owned_worker<W, P>(request_wakes: W, periodic: P)
where
    W: Future<Output = ()>,
    P: Future<Output = ()>,
{
    tokio::join!(request_wakes, periodic);
}

fn periodic_coordination_configured(valkey_url: &str) -> bool {
    !valkey_url.trim().is_empty()
}

async fn run_request_wakes(
    service: Arc<ChatGptQuotaService>,
    mut receiver: RequestWakeReceiver,
    control: RuntimeControl,
) {
    loop {
        let wake = tokio::select! {
            _ = control.wait_for_shutdown() => break,
            wake = receiver.recv_event() => match wake {
                Some(wake) => wake,
                None => break,
            }
        };
        let refresh = async {
            match wake.reason {
                RequestWakeReason::SelectedRequest => {
                    service.refresh_selected_request(wake.endpoint_id).await
                }
                RequestWakeReason::QuotaError => {
                    service.refresh_after_quota_error(wake.endpoint_id).await
                }
            }
        };
        tokio::select! {
            _ = control.wait_for_shutdown() => break,
            result = tokio::time::timeout(REQUEST_WAKE_TIMEOUT, refresh) => match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => warn!(
                    endpoint_id = %wake.endpoint_id,
                    error = error.code(),
                    wake_reason = ?wake.reason,
                    "ChatGPT quota request wake refresh failed"
                ),
                Err(_) => warn!(
                    endpoint_id = %wake.endpoint_id,
                    wake_reason = ?wake.reason,
                    "ChatGPT quota request wake refresh exceeded its time budget"
                ),
            }
        }
    }
}

struct CollectorRuntime {
    service: Arc<ChatGptQuotaService>,
    control: RuntimeControl,
}

async fn run_periodic_pass_until_shutdown<F>(
    control: &RuntimeControl,
    pass: F,
) -> Result<(), String>
where
    F: Future<Output = Result<(), crate::worker_admin::chatgpt_quota_service::ChatGptQuotaError>>,
{
    tokio::select! {
        _ = control.wait_for_shutdown() => Ok(()),
        result = pass => result.map_err(|error| error.code().to_string()),
    }
}

fn collector_job(
    service: Arc<ChatGptQuotaService>,
    control: RuntimeControl,
) -> Job<CollectorRuntime> {
    Job::new(
        "chatgpt-quota-collector",
        Schedule::Interval(COLLECTOR_TICK),
        CollectorRuntime { service, control },
        Task::from_async(|context: TaskContext<CollectorRuntime>| async move {
            run_periodic_pass_until_shutdown(
                &context.deps.control,
                context.deps.service.run_periodic_pass(),
            )
            .await
        }),
    )
}
