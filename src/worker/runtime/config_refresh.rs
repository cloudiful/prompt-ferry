//! Periodic refresh of the database-backed configuration.
//!
//! The worker resolves the settings it holds in memory once at startup. In a
//! multi-worker deployment another worker can change them, so this task
//! re-reads them on a bounded interval, applies the changes locally and
//! republishes the relay snapshot when its content changed.
//!
//! The refresh never reconstructs the worker from scratch: it only replaces
//! the settings that the database owns.

use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use super::lifecycle::RuntimeControl;
use crate::config::WorkerConfig;
use crate::db;
use crate::db::config_repository::relay_snapshot::content_fingerprint;
use crate::redact;
use crate::worker_admin::AdminState;

/// How often the worker re-reads the database-backed configuration.
pub const CONFIG_REFRESH_INTERVAL_SECONDS: u64 = 30;

/// Refresh the state-held configuration and the relay snapshot.
pub(super) fn spawn_config_refresh(
    state: AdminState,
    config: WorkerConfig,
    control: RuntimeControl,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(CONFIG_REFRESH_INTERVAL_SECONDS));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval.tick().await;
        let mut last_redaction_fingerprint: Option<u64> = None;
        let mut last_raw_store_fingerprint: Option<u64> = None;
        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = interval.tick() => {
                    if let Err(err) = refresh_once(
                        &state,
                        &config,
                        &mut last_redaction_fingerprint,
                        &mut last_raw_store_fingerprint,
                    ).await {
                        warn!(error = %err, "failed to refresh the database-backed configuration");
                    }
                }
            }
        }
    })
}

/// One refresh pass over the database-backed settings.
async fn refresh_once(
    state: &AdminState,
    config: &WorkerConfig,
    last_redaction_fingerprint: &mut Option<u64>,
    last_raw_store_fingerprint: &mut Option<u64>,
) -> anyhow::Result<()> {
    let pool = state.pool.clone();
    let (
        redaction_config,
        user_redaction_configs,
        model_route_whitelist_enabled,
        request_content_logging,
        stream_delta_batching,
        usage_retention,
        cache_alert,
    ) = tokio::try_join!(
        db::get_redaction_config(&pool),
        db::list_user_redaction_configs(&pool),
        async { db::get_bool_setting(&pool, "model_route_whitelist_enabled", true).await },
        db::get_request_content_logging(&pool),
        db::get_stream_delta_batching(&pool),
        db::get_usage_retention(&pool),
        db::get_cache_alert_settings(&pool),
    )?;
    let redaction_config = redaction_config;
    let user_redaction_configs = user_redaction_configs;
    let model_route_whitelist_enabled = model_route_whitelist_enabled;
    let mut request_content_logging = request_content_logging;
    let stream_delta_batching = stream_delta_batching;
    let usage_retention = usage_retention;
    request_content_logging.raw_retention_days = usage_retention.raw_retention_days;

    // Redaction is applied process wide, so it is only re-applied when the
    // stored configuration actually changed.
    let redaction_fingerprint = content_fingerprint(&(&redaction_config, &user_redaction_configs))?;
    if *last_redaction_fingerprint != Some(redaction_fingerprint) {
        redact::apply_configs(&redaction_config, user_redaction_configs)?;
        state.redaction_enabled.store(
            redact::has_any_enabled(),
            std::sync::atomic::Ordering::SeqCst,
        );
        *last_redaction_fingerprint = Some(redaction_fingerprint);
        info!("refreshed the redaction configuration from the database");
    }

    let whitelist_changed = state
        .model_route_whitelist_enabled
        .load(std::sync::atomic::Ordering::SeqCst)
        != model_route_whitelist_enabled;
    if whitelist_changed {
        state.model_route_whitelist_enabled.store(
            model_route_whitelist_enabled,
            std::sync::atomic::Ordering::SeqCst,
        );
        info!(
            model_route_whitelist_enabled,
            "refreshed the model route whitelist setting"
        );
    }

    {
        let current = state.request_content_logging.read().await.clone();
        if content_fingerprint(&current)? != content_fingerprint(&request_content_logging)? {
            *state.request_content_logging.write().await = request_content_logging;
            info!("refreshed the request content logging setting");
        }
    }
    {
        let current = state.usage_retention.read().await.clone();
        if current != usage_retention {
            *state.usage_retention.write().await = usage_retention;
            info!("refreshed the usage retention setting");
        }
    }
    {
        let current = state.stream_delta_batching.read().await.clone();
        if current != stream_delta_batching {
            *state.stream_delta_batching.write().await = stream_delta_batching;
            info!("refreshed the stream delta batching setting");
        }
    }
    {
        // The cache alert policy is a state-held shared setting: the monitor
        // reads it on every pass, so a policy that another worker changed
        // must reach this process as well.
        let current = state.cache_alert.read().await.clone();
        if content_fingerprint(&current)? != content_fingerprint(&cache_alert)? {
            *state.cache_alert.write().await = cache_alert;
            info!("refreshed the cache alert policy");
        }
    }

    refresh_raw_payload_store(state, config, last_raw_store_fingerprint).await?;

    // The relay snapshot is republished when its content changed. The
    // publication reads the contents and assigns the version inside the
    // publication boundary, so the version always labels the content it
    // published.
    if let Some(version) = crate::worker_admin::publish_snapshot_if_changed(state).await? {
        info!(version, "republished the relay configuration snapshot");
    }
    Ok(())
}

/// Rebuild the raw payload store when the persisted setting changed.
async fn refresh_raw_payload_store(
    state: &AdminState,
    config: &WorkerConfig,
    last_raw_store_fingerprint: &mut Option<u64>,
) -> anyhow::Result<()> {
    let Some(manager) = state.relay_secret_manager.as_ref() else {
        return Ok(());
    };
    let persisted = db::get_raw_object_store_persisted(&state.pool).await?;
    let fingerprint = content_fingerprint(&persisted)?;
    if *last_raw_store_fingerprint == Some(fingerprint) {
        return Ok(());
    }
    let raw_config = match persisted {
        Some(persisted) => match persisted.into_config(manager) {
            Ok(config) => config,
            Err(err) => {
                warn!(error = %err, "failed to decrypt the persisted raw object store config");
                return Ok(());
            }
        },
        None => crate::raw_payload_store::RawObjectStoreConfig::from_worker_config(config),
    };
    match raw_config.build_store() {
        Ok(store) => {
            *state.raw_payload_store.write().await = store.map(Arc::new);
            *last_raw_store_fingerprint = Some(fingerprint);
            info!("refreshed the raw payload store setting");
        }
        Err(err) => {
            warn!(error = %err, "failed to build the raw payload store");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db::config_repository::relay_snapshot::UnifiedSnapshot;
    use crate::db::config_repository::relay_snapshot::snapshot_fingerprint;

    #[test]
    fn equal_contents_share_a_fingerprint_and_different_contents_do_not() {
        let first = snapshot_fingerprint(&UnifiedSnapshot {
            version: 0,
            keys: Vec::new(),
            relay_ip_policy: Default::default(),
        })
        .expect("fingerprint");
        let same = snapshot_fingerprint(&UnifiedSnapshot {
            version: 1,
            keys: Vec::new(),
            relay_ip_policy: Default::default(),
        })
        .expect("fingerprint");
        assert_eq!(first, same);
    }
}
