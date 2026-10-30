//! Continuous-session cache alert monitor (issue #548).
//!
//! Every `min(window_minutes, 60)` minutes the monitor aggregates completed
//! AI turns per conversation, keeps the ones whose fold-aware cache read rate
//! is below the configured threshold, skips channels still inside their own
//! cooldown, and delivers the configured DingTalk and SMTP alerts. Each channel
//! keeps an independent cooldown clock, so a delivery that succeeded on one
//! channel does not repeat while a failed channel retries on the next tick. One
//! failing conversation or channel never stops the others; the job is
//! coordinated so only one worker evaluates each tick.

use super::background_job::{
    connect_coordinated_store, coordinated_scheduler, run_coordinated_scheduler, run_scheduler,
    try_postgres_advisory_lease,
};
use super::lifecycle::RuntimeControl;
use crate::{config::WorkerConfig, db, notify, worker_admin_types::CacheAlertSettings};
use scheduler::{InMemoryStateStore, Job, Schedule, Scheduler, SchedulerConfig, Task, TaskContext};
use sqlx::PgPool;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
};
use tracing::{info, warn};

/// Stable scheduler job id so coordinated runs resume after a restart.
pub const CACHE_ALERT_JOB_ID: &str = "prompt-ferry:cache-alert-monitor";

/// Scheduler granularity: the job wakes up every minute and the in-process
/// gate enforces the effective `min(window_minutes, 60)` cadence, so a
/// settings change applies without a worker restart.
const CACHE_ALERT_TICK_INTERVAL: Duration = Duration::from_secs(60);
const CACHE_ALERT_TICK_MINUTES: i32 = 60;
/// Advisory lock key ("pfcache1") used when no Valkey coordination exists.
const POSTGRES_CACHE_ALERT_LOCK_KEY: i64 = 0x7066_6361_6368_6531;

#[derive(Clone)]
pub struct CacheAlertDependencies {
    pub pool: PgPool,
    pub settings: Arc<RwLock<CacheAlertSettings>>,
    pub client: reqwest::Client,
}

/// One enabled delivery channel for the current policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CacheAlertChannel {
    Dingtalk,
    Smtp,
}

impl CacheAlertChannel {
    fn name(self) -> &'static str {
        match self {
            Self::Dingtalk => notify::CACHE_ALERT_CHANNEL_DINGTALK,
            Self::Smtp => notify::CACHE_ALERT_CHANNEL_SMTP,
        }
    }
}

/// Channels the current policy can actually deliver through. The global
/// `enabled` switch gates both; DingTalk additionally needs its webhook and
/// SMTP needs its own channel switch.
fn enabled_channels(settings: &CacheAlertSettings) -> Vec<CacheAlertChannel> {
    if !settings.enabled {
        return Vec::new();
    }
    let mut channels = Vec::new();
    if !settings.dingtalk_webhook_url.trim().is_empty() {
        channels.push(CacheAlertChannel::Dingtalk);
    }
    if settings.smtp.enabled {
        channels.push(CacheAlertChannel::Smtp);
    }
    channels
}

/// One alerting pass. Returns the number of alerts actually delivered
/// (cooldown skips, disabled policies, and disabled channels count zero).
pub async fn run_cache_alert_check(deps: &CacheAlertDependencies) -> anyhow::Result<usize> {
    let settings = deps.settings.read().await.clone().normalized();
    if !settings.enabled {
        return Ok(0);
    }
    let channels = enabled_channels(&settings);
    if channels.is_empty() {
        warn!("cache alert monitor is enabled but no delivery channel is configured; skipping");
        return Ok(0);
    }
    let conversations = db::find_low_cache_conversations(
        &deps.pool,
        settings.window_minutes,
        settings.min_turns,
        settings.threshold,
    )
    .await?;
    let cooldown = chrono::Duration::minutes(i64::from(settings.cooldown_minutes));
    let mut delivered = 0usize;
    for conversation in conversations {
        let conversation_id = conversation.conversation_id;
        let cache_rate = conversation.cache_rate.unwrap_or_default();
        for channel in &channels {
            // Both sides of the comparison come from the database clock:
            // `window_end` is the candidate query's `NOW()` and
            // `last_alerted_at` is written by `record_cache_alert`, so a worker
            // clock that drifts from the database cannot move the cooldown
            // window. Each channel has its own row and therefore its own clock.
            match db::last_cache_alert_at(&deps.pool, conversation_id, channel.name()).await {
                Ok(Some(last_alerted_at))
                    if cooldown_active(conversation.window_end, last_alerted_at, cooldown) =>
                {
                    continue;
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(
                        conversation_id = %conversation_id,
                        channel = channel.name(),
                        error = %error,
                        "cache alert cooldown lookup failed"
                    );
                    continue;
                }
            }
            let delivery = match channel {
                CacheAlertChannel::Dingtalk => {
                    notify::send_cache_alert(&deps.client, &settings, &conversation).await
                }
                CacheAlertChannel::Smtp => {
                    notify::send_cache_alert_email(&deps.client, &settings, &conversation).await
                }
            };
            match delivery {
                Ok(()) => {
                    if let Err(error) = db::record_cache_alert(
                        &deps.pool,
                        conversation_id,
                        channel.name(),
                        cache_rate,
                        conversation.turns,
                    )
                    .await
                    {
                        warn!(
                            conversation_id = %conversation_id,
                            channel = channel.name(),
                            error = %error,
                            "cache alert state write failed"
                        );
                    }
                    delivered += 1;
                    info!(
                        conversation_id = %conversation_id,
                        channel = channel.name(),
                        model = conversation.model.as_deref().unwrap_or("-"),
                        turns = conversation.turns,
                        cache_rate = cache_rate,
                        threshold = settings.threshold,
                        window_minutes = settings.window_minutes,
                        "cache alert delivered"
                    );
                }
                Err(error) => {
                    warn!(
                        conversation_id = %conversation_id,
                        channel = channel.name(),
                        error = %error,
                        "cache alert delivery failed"
                    );
                }
            }
        }
    }
    Ok(delivered)
}

/// True while a conversation's last alert is still inside its cooldown.
///
/// Both timestamps are database-owned: `now` is the candidate query's
/// `window_end` and `last_alerted_at` is the value `record_cache_alert` wrote,
/// so a worker clock drifted from the database clock cannot shorten or extend
/// the cooldown.
fn cooldown_active(
    now: chrono::DateTime<chrono::Utc>,
    last_alerted_at: chrono::DateTime<chrono::Utc>,
    cooldown: chrono::Duration,
) -> bool {
    now.signed_duration_since(last_alerted_at) < cooldown
}

pub(super) fn spawn_cache_alert_monitor(
    config: &WorkerConfig,
    pool: PgPool,
    settings: Arc<RwLock<CacheAlertSettings>>,
    client: reqwest::Client,
    control: RuntimeControl,
) -> JoinHandle<()> {
    let valkey_url = config.valkey_url.trim().to_string();
    tokio::spawn(async move {
        let runtime = Arc::new(CacheAlertRuntime {
            deps: Arc::new(CacheAlertDependencies {
                pool,
                settings,
                client,
            }),
            postgres_coordination: valkey_url.is_empty(),
            last_run: Mutex::new(None),
        });

        if valkey_url.is_empty() {
            // No Valkey coordination: the PostgreSQL advisory lease plus this
            // eager pass keeps the first check immediate and single-flight.
            if let Err(error) = runtime.run_due().await {
                warn!(error = %error, "initial cache alert check failed");
            }
            let scheduler = Scheduler::new(SchedulerConfig::default(), InMemoryStateStore::new());
            if let Err(error) = run_scheduler(scheduler, cache_alert_job(runtime), control).await {
                warn!(error = %error, "cache alert scheduler stopped");
            }
            return;
        }

        // Coordinated mode: the first pass comes from the leased scheduler
        // tick so a multi-worker cluster never evaluates it uncoordinated.
        let Some(store) = connect_coordinated_store(&valkey_url).await else {
            return;
        };
        let scheduler = coordinated_scheduler(store);
        if let Err(error) =
            run_coordinated_scheduler(scheduler, cache_alert_job(runtime), valkey_url, control)
                .await
        {
            warn!(
                error = %error,
                capability = "alert_coordination",
                "coordinated cache alert scheduler stopped; local scheduling is not safe"
            );
        }
    })
}

fn cache_alert_job(runtime: Arc<CacheAlertRuntime>) -> Job<Arc<CacheAlertRuntime>> {
    Job::new(
        CACHE_ALERT_JOB_ID,
        Schedule::Interval(CACHE_ALERT_TICK_INTERVAL),
        runtime,
        Task::from_async(|context: TaskContext<Arc<CacheAlertRuntime>>| async move {
            context
                .deps
                .run_due()
                .await
                .map_err(|error| error.to_string())
        }),
    )
}

struct CacheAlertRuntime {
    deps: Arc<CacheAlertDependencies>,
    postgres_coordination: bool,
    last_run: Mutex<Option<Instant>>,
}

impl CacheAlertRuntime {
    async fn run_due(&self) -> anyhow::Result<()> {
        let interval = self.effective_interval().await;
        {
            let last_run = self.last_run.lock().await;
            if last_run.is_some_and(|last| last.elapsed() < interval) {
                return Ok(());
            }
        }
        let result = self.run_once().await;
        *self.last_run.lock().await = Some(Instant::now());
        result
    }

    async fn effective_interval(&self) -> Duration {
        let window_minutes = self
            .deps
            .settings
            .read()
            .await
            .window_minutes
            .clamp(1, CACHE_ALERT_TICK_MINUTES);
        Duration::from_secs(60 * window_minutes as u64)
    }

    async fn run_once(&self) -> anyhow::Result<()> {
        let lease = if self.postgres_coordination {
            match try_postgres_advisory_lease(&self.deps.pool, POSTGRES_CACHE_ALERT_LOCK_KEY)
                .await?
            {
                Some(lease) => Some(lease),
                None => return Ok(()),
            }
        } else {
            None
        };
        let result = run_cache_alert_check(&self.deps).await;
        if let Some(lease) = lease {
            lease.release().await?;
        }
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CACHE_ALERT_TICK_INTERVAL, CACHE_ALERT_TICK_MINUTES, CacheAlertChannel, cooldown_active,
        enabled_channels,
    };
    use crate::worker_admin_types::{CacheAlertSettings, CacheAlertSmtpSettings};
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn legacy_dingtalk_only_policy_keeps_its_single_channel() {
        let settings = CacheAlertSettings {
            enabled: true,
            dingtalk_webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=x".to_string(),
            ..CacheAlertSettings::default()
        };

        assert_eq!(
            enabled_channels(&settings),
            vec![CacheAlertChannel::Dingtalk]
        );
    }

    #[test]
    fn smtp_channel_adds_itself_without_removing_dingtalk() {
        let settings = CacheAlertSettings {
            enabled: true,
            dingtalk_webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=x".to_string(),
            smtp: CacheAlertSmtpSettings {
                enabled: true,
                ..CacheAlertSmtpSettings::default()
            },
            ..CacheAlertSettings::default()
        };

        assert_eq!(
            enabled_channels(&settings),
            vec![CacheAlertChannel::Dingtalk, CacheAlertChannel::Smtp]
        );
    }

    #[test]
    fn disabled_policy_or_channels_are_not_attempted() {
        let disabled = CacheAlertSettings {
            enabled: false,
            dingtalk_webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=x".to_string(),
            smtp: CacheAlertSmtpSettings {
                enabled: true,
                ..CacheAlertSmtpSettings::default()
            },
            ..CacheAlertSettings::default()
        };
        assert!(enabled_channels(&disabled).is_empty());

        let smtp_only = CacheAlertSettings {
            enabled: true,
            smtp: CacheAlertSmtpSettings {
                enabled: true,
                ..CacheAlertSmtpSettings::default()
            },
            ..CacheAlertSettings::default()
        };
        assert_eq!(enabled_channels(&smtp_only), vec![CacheAlertChannel::Smtp]);
    }

    #[test]
    fn scheduler_tick_is_short_enough_for_a_settings_driven_cadence() {
        // The job wakes every minute and the in-process gate picks the real
        // interval, so the effective cadence never exceeds 60 minutes and a
        // settings change applies on the next tick.
        assert_eq!(CACHE_ALERT_TICK_INTERVAL.as_secs(), 60);
        const { assert!(CACHE_ALERT_TICK_MINUTES <= 60) };
    }

    #[test]
    fn cooldown_boundary_lets_the_next_alert_through() {
        let now = Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap();
        let cooldown = Duration::minutes(60);

        // Exactly at the boundary the cooldown is over.
        assert!(!cooldown_active(now, now - cooldown, cooldown));
        // One second earlier it still suppresses the alert.
        assert!(cooldown_active(
            now,
            now - cooldown + Duration::seconds(1),
            cooldown
        ));
    }

    #[test]
    fn cooldown_decision_follows_the_supplied_database_clock() {
        // Both timestamps sit in the far future relative to the wall clock; the
        // decision must follow the supplied pair instead of consulting the
        // worker clock, which is what kept a clock-skewed worker from
        // re-alerting early.
        let db_now = Utc.with_ymd_and_hms(2030, 1, 1, 12, 0, 0).unwrap();
        let cooldown = Duration::minutes(60);

        assert!(!cooldown_active(db_now, db_now - cooldown, cooldown));
        assert!(cooldown_active(db_now, db_now, cooldown));
    }
}
