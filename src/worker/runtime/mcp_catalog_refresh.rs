//! The managed periodic MCP catalog refresh.
//!
//! One task owns the catalog refresh lifecycle of the worker: it performs the
//! startup bulk load and then refreshes every catalog that aged past the soft
//! freshness window. The task is cancellable, so a shutdown cancels an
//! in-flight upstream fetch instead of waiting for it.

use std::time::Duration;

use rand::RngExt;
use tokio::task::JoinHandle;
use tracing::info;

use super::lifecycle::RuntimeControl;
use crate::{
    config::{McpWarmupMode, WorkerConfig},
    mcp::{CATALOG_REFRESH_DRAIN_TIMEOUT, McpCatalogService},
};

/// The base interval between two periodic catalog refresh passes.
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// The share of the interval that is added as a random jitter, so several
/// workers do not fetch the same catalogs at the same moment.
const REFRESH_JITTER_PERCENT: u64 = 10;

/// Start the managed catalog refresh task.
pub(super) fn spawn(
    config: &WorkerConfig,
    service: McpCatalogService,
    control: RuntimeControl,
) -> JoinHandle<()> {
    spawn_task(config, service, control, REFRESH_INTERVAL)
}

fn spawn_task(
    config: &WorkerConfig,
    service: McpCatalogService,
    control: RuntimeControl,
    interval: Duration,
) -> JoinHandle<()> {
    // `all` loads every enabled catalog at startup. `lazy` and `off` skip the
    // bulk cold start; their catalogs are loaded by the read that needs them
    // and refreshed by the periodic pass once they exist.
    let bulk_load = matches!(config.mcp_warmup, McpWarmupMode::All);
    tokio::spawn(async move {
        if bulk_load && !run_pass(&control, &service, service.warm_enabled_servers()).await {
            stop_and_drain(&service).await;
            return;
        }
        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = tokio::time::sleep(next_interval(interval)) => {}
            }
            if !run_pass(&control, &service, service.refresh_aging_entries()).await {
                break;
            }
        }
        stop_and_drain(&service).await;
        info!(
            category = "mcp_catalog_refresh",
            "MCP catalog refresh task stopped"
        );
    })
}

/// Run one pass, stopping the work it owns as soon as shutdown starts.
///
/// Returns `false` when the shutdown interrupted the pass. The coordinator is
/// cancelled first, so every refresh it owns — including the ones the pass only
/// scheduled — releases its lease explicitly instead of waiting for the lease
/// expiry.
async fn run_pass<F>(control: &RuntimeControl, service: &McpCatalogService, pass: F) -> bool
where
    F: std::future::Future<Output = ()>,
{
    tokio::pin!(pass);
    tokio::select! {
        _ = pass.as_mut() => true,
        _ = control.wait_for_shutdown() => {
            service.cancel_refreshes();
            let _ = tokio::time::timeout(CATALOG_REFRESH_DRAIN_TIMEOUT, pass.as_mut()).await;
            false
        }
    }
}

/// Stop scheduling refresh work and wait for the owned refreshes to stop.
async fn stop_and_drain(service: &McpCatalogService) {
    service.cancel_refreshes();
    service.drain_refreshes(CATALOG_REFRESH_DRAIN_TIMEOUT).await;
}

/// The interval until the next pass, with a random jitter on top.
fn next_interval(base: Duration) -> Duration {
    let jitter_max = base.as_millis() as u64 * REFRESH_JITTER_PERCENT / 100;
    let jitter = rand::rng().random_range(0..=jitter_max);
    base + Duration::from_millis(jitter)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A worker harness over a real SQLite configuration store.

    #[test]
    fn the_periodic_interval_stays_inside_the_documented_window() {
        for _ in 0..64 {
            let interval = next_interval(REFRESH_INTERVAL);
            assert!(interval >= REFRESH_INTERVAL);
            assert!(
                interval
                    <= REFRESH_INTERVAL
                        + Duration::from_millis(
                            REFRESH_INTERVAL.as_millis() as u64 * REFRESH_JITTER_PERCENT / 100,
                        )
            );
        }
        assert_eq!(REFRESH_INTERVAL, Duration::from_secs(60));
    }
}
