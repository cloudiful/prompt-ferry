use std::{future::Future, sync::Arc, time::Duration};

use crate::{config::WorkerConfig, worker_admin::chatgpt_quota_service::ChatGptQuotaService};
use scheduler::{Job, Schedule, Task, TaskContext};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::warn;
use uuid::Uuid;

use super::{
    background_job::{connect_coordinated_store, coordinated_scheduler, run_coordinated_scheduler},
    lifecycle::RuntimeControl,
};

const COLLECTOR_TICK: Duration = Duration::from_secs(60);
const REQUEST_WAKE_TIMEOUT: Duration = Duration::from_secs(44);

#[cfg(test)]
mod tests;

pub(super) fn spawn(
    config: &WorkerConfig,
    service: Arc<ChatGptQuotaService>,
    control: RuntimeControl,
) -> JoinHandle<()> {
    let valkey_url = config.valkey_url.trim().to_string();
    tokio::spawn(async move {
        let Some(receiver) = service.take_request_wake_receiver() else {
            warn!("ChatGPT quota request wake worker was already started");
            return;
        };
        let request_worker = tokio::spawn(run_request_wakes(
            service.clone(),
            receiver,
            control.clone(),
        ));

        if !periodic_coordination_configured(&valkey_url) {
            warn!(
                capability = "chatgpt_quota_collection",
                "periodic ChatGPT quota collection disabled because Valkey coordination is not configured"
            );
        } else if let Some(store) = connect_coordinated_store(&valkey_url).await {
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
                control.wait_for_shutdown().await;
            }
        } else {
            control.wait_for_shutdown().await;
        }
        if !control.is_shutting_down() {
            control.wait_for_shutdown().await;
        }

        match tokio::time::timeout(REQUEST_WAKE_TIMEOUT, request_worker).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!(error = %error, "ChatGPT quota request worker join failed"),
            Err(_) => warn!(
                timeout_seconds = REQUEST_WAKE_TIMEOUT.as_secs(),
                "ChatGPT quota request worker did not stop within its shutdown budget"
            ),
        }
    })
}

fn periodic_coordination_configured(valkey_url: &str) -> bool {
    !valkey_url.trim().is_empty()
}

async fn run_request_wakes(
    service: Arc<ChatGptQuotaService>,
    mut receiver: mpsc::Receiver<Uuid>,
    control: RuntimeControl,
) {
    loop {
        let endpoint_id = tokio::select! {
            _ = control.wait_for_shutdown() => break,
            endpoint_id = receiver.recv() => match endpoint_id {
                Some(endpoint_id) => endpoint_id,
                None => break,
            }
        };
        tokio::select! {
            _ = control.wait_for_shutdown() => break,
            result = tokio::time::timeout(
                REQUEST_WAKE_TIMEOUT,
                service.refresh_selected_request(endpoint_id),
            ) => match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => warn!(
                    endpoint_id = %endpoint_id,
                    error = error.code(),
                    "selected-request ChatGPT quota refresh failed"
                ),
                Err(_) => warn!(
                    endpoint_id = %endpoint_id,
                    "selected-request ChatGPT quota refresh exceeded its time budget"
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
