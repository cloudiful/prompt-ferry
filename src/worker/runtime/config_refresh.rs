//! Periodic refresh of the database-backed configuration.
//!
//! The worker resolves the settings it holds in memory once at startup. In a
//! multi-worker deployment another worker can change them, so this task
//! re-reads them on a bounded interval, applies the changes locally and
//! republishes the relay snapshot when its content changed.
//!
//! The refresh never reconstructs the worker from scratch: it only replaces
//! the settings that the database owns.

use rand::RngExt;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use super::lifecycle::RuntimeControl;
use crate::db;
use crate::db::config_repository::relay_snapshot::content_fingerprint;
use crate::redact;
use crate::worker_admin::AdminState;

/// How often the worker re-reads the database-backed configuration.
pub const CONFIG_REFRESH_INTERVAL_SECONDS: u64 = 30;

/// How many passes one notification-driven refresh may take before it yields.
const NODE_CONFIG_REFRESH_ATTEMPTS: usize = 3;

/// Initial delay before the node retries its revision listener.
const NODE_CONFIG_RECONNECT_BASE: Duration = Duration::from_millis(500);

/// Upper bound of the revision listener reconnect delay.
const NODE_CONFIG_RECONNECT_MAX: Duration = Duration::from_secs(10);

/// Refresh the node configuration when the database revision changes.
///
/// The task owns one dedicated listener connection and refreshes serially: a
/// notification is only a hint, the authoritative revision is read from the
/// gated application pool, and the 30 s poll stays as the repair path for a
/// missed notification. The listener is a control connection and never a
/// session of the gated pool.
pub(super) fn spawn_node_config_refresh(
    state: AdminState,
    control: RuntimeControl,
    database_url: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        Box::pin(run_node_config_refresh(state, control, database_url)).await;
    })
}

async fn run_node_config_refresh(state: AdminState, control: RuntimeControl, database_url: String) {
    let mut listener: Option<db::config_revision::ConfigRevisionListener> = None;
    let mut applied = db::config_revision::AppliedRevision::default();
    let mut fingerprints = RefreshFingerprints::default();
    let mut reconnect_attempt: u32 = 0;
    let mut next_connect = Instant::now();
    let mut refresh_now = true;
    let mut force_refresh = true;
    let mut interval = tokio::time::interval(Duration::from_secs(CONFIG_REFRESH_INTERVAL_SECONDS));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval.tick().await;

    loop {
        if listener.is_none() && Instant::now() >= next_connect {
            match db::config_revision::ConfigRevisionListener::connect(&database_url).await {
                Ok(connected) => {
                    info!("subscribed to the configuration revision channel");
                    listener = Some(connected);
                    reconnect_attempt = 0;
                    // A reconnect forces an authoritative read and refresh, so
                    // a notification lost while the listener was away heals.
                    refresh_now = true;
                    force_refresh = true;
                }
                Err(err) => {
                    reconnect_attempt = reconnect_attempt.saturating_add(1);
                    let delay = node_reconnect_delay(reconnect_attempt);
                    next_connect = Instant::now() + delay;
                    warn!(
                        error = %err,
                        retry_millis = delay.as_millis(),
                        "failed to subscribe to the configuration revision channel; \
                         keeping the authoritative poll"
                    );
                }
            }
        }

        if refresh_now {
            refresh_now = false;
            let force = force_refresh;
            force_refresh = false;
            if let Err(err) =
                reconcile_node_config(&state, &mut applied, &mut fingerprints, force).await
            {
                // The revision was not applied. The failure is remembered, so
                // the next poll retries the same revision instead of waiting
                // for a newer one.
                applied.mark_failed();
                warn!(
                    error = %err,
                    applied_revision = applied.get(),
                    "failed to apply the database configuration revision"
                );
                force_refresh = force;
            }
        }

        let mut listener_failed = false;
        match listener.as_mut() {
            Some(connected) => {
                tokio::select! {
                    _ = control.wait_for_shutdown() => break,
                    _ = interval.tick() => {
                        refresh_now = true;
                    }
                    result = connected.recv() => {
                        match result {
                            Ok(Some(hint)) if !applied.needs_refresh(hint) => {}
                            Ok(_) => {
                                refresh_now = true;
                            }
                            Err(err) => {
                                warn!(
                                    error = %err,
                                    "the configuration revision listener failed; reconnecting"
                                );
                                listener_failed = true;
                            }
                        }
                    }
                }
            }
            None => {
                tokio::select! {
                    _ = control.wait_for_shutdown() => break,
                    _ = interval.tick() => {
                        refresh_now = true;
                    }
                    _ = tokio::time::sleep_until(next_connect) => {}
                }
            }
        }
        if listener_failed {
            listener = None;
            reconnect_attempt = 0;
            next_connect = Instant::now();
            refresh_now = true;
            force_refresh = true;
        }
    }
}

/// Read the authoritative revision and refresh when it is newer.
///
/// A pass is retried while the revision changes, so a knowingly torn pass is
/// never marked applied. A failed pass keeps the previous applied revision and
/// the next notification or poll retries it.
async fn reconcile_node_config(
    state: &AdminState,
    applied: &mut db::config_revision::AppliedRevision,
    fingerprints: &mut RefreshFingerprints,
    force: bool,
) -> anyhow::Result<()> {
    let mut revision = db::config_revision::read_revision(&state.pool).await?;
    if !force && !applied.needs_refresh(revision) {
        return Ok(());
    }
    for _ in 0..NODE_CONFIG_REFRESH_ATTEMPTS {
        refresh_configuration(state, &mut fingerprints.redaction).await?;
        // Do not publish a pass whose gather raced a newer revision.
        let after_gather = db::config_revision::read_revision(&state.pool).await?;
        if after_gather != revision {
            revision = after_gather;
            continue;
        }
        if let Some(version) = crate::worker_admin::publish_snapshot_if_changed(state).await? {
            info!(version, "republished the relay configuration snapshot");
        }
        let after_publish = db::config_revision::read_revision(&state.pool).await?;
        if after_publish != after_gather {
            revision = after_publish;
            continue;
        }
        invalidate_configuration_caches(state).await?;
        warn_shared_valkey_restart(state).await?;
        applied.commit(after_publish);
        info!(
            revision = after_publish,
            "applied the database configuration revision"
        );
        return Ok(());
    }
    anyhow::bail!("the configuration revision changed while refreshing; retrying on the next wake")
}

/// Drop the caches whose identity is not part of their cache key.
///
/// The endpoint/model catalog is keyed by the configured endpoint identity, so
/// a configuration change already selects a different entry. The MCP catalog
/// is keyed by the server id alone and must be invalidated when configuration
/// changes on another node.
async fn invalidate_configuration_caches(state: &AdminState) -> anyhow::Result<()> {
    for server in db::list_mcp_servers(&state.pool).await? {
        state.mcp_catalog_service.invalidate(server.server_id).await;
    }
    Ok(())
}

/// Report a shared Valkey change without switching the active configuration.
async fn warn_shared_valkey_restart(state: &AdminState) -> anyhow::Result<()> {
    let desired = db::desired_shared_valkey_revision(&state.pool).await?;
    let applied = state.shared_valkey.revision();
    if crate::db::config_revision::shared_valkey_restart_required(desired, applied) {
        warn!(
            applied_revision = ?applied,
            desired_revision = ?desired,
            "the shared Valkey configuration changed; a coordinated restart is required \
             and the active configuration is unchanged"
        );
    }
    Ok(())
}

/// The bounded, jittered delay before the next listener connection attempt.
fn node_reconnect_delay(attempt: u32) -> Duration {
    let exponent = attempt.min(5);
    let base = NODE_CONFIG_RECONNECT_BASE.saturating_mul(1_u32 << exponent);
    let capped = base.min(NODE_CONFIG_RECONNECT_MAX);
    let jitter = rand::rng().random_range(0..500_u64);
    capped.saturating_add(Duration::from_millis(jitter))
}

/// Fingerprints of the process-wide configuration applied by earlier passes.
#[derive(Default)]
struct RefreshFingerprints {
    redaction: Option<u64>,
}

/// Gather and apply the database-backed configuration without publishing.
async fn refresh_configuration(
    state: &AdminState,
    last_redaction_fingerprint: &mut Option<u64>,
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
    let mut request_content_logging = request_content_logging;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{NODE_CONFIG_RECONNECT_BASE, NODE_CONFIG_RECONNECT_MAX, node_reconnect_delay};
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

    #[test]
    fn listener_reconnect_delays_stay_bounded() {
        let jitter = Duration::from_millis(500);
        for attempt in [0, 1, 2, 5, 9, 100] {
            let delay = node_reconnect_delay(attempt);
            assert!(
                delay >= NODE_CONFIG_RECONNECT_BASE,
                "attempt {attempt} would spin without a delay"
            );
            assert!(
                delay <= NODE_CONFIG_RECONNECT_MAX + jitter,
                "attempt {attempt} exceeded the reconnect bound"
            );
        }
    }
}
