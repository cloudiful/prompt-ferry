use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::StreamDeltaBatchingSettings,
    db::entities::postgres::{user_redaction_configs, worker_settings},
    db::seaorm::{native_error, postgres_connection},
    raw_payload_store::{RawObjectStoreConfig, RawObjectStorePersisted},
    redact::RedactionConfig,
    relay_secrets::RelaySecretManager,
    worker_admin_types::UsageRetentionSettings,
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
};

#[derive(Debug, Clone)]
pub struct RedactionCustomStringRuleListItem {
    pub array_index: i64,
    pub pattern: String,
    pub match_type: String,
    pub scope: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Timestamps are display metadata written by us; tolerate hand-edited or
/// legacy junk by treating an unparsable value as missing.
fn parse_rfc3339_utc(value: Option<String>) -> Option<DateTime<Utc>> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

pub const REQUEST_CONTENT_LOGGING_SETTINGS_KEY: &str = "request_content_logging";
pub const USAGE_RETENTION_SETTINGS_KEY: &str = "usage_retention";
pub const STREAM_DELTA_BATCHING_SETTINGS_KEY: &str = "stream_delta_batching";
pub const RAW_OBJECT_STORE_SETTINGS_KEY: &str = "raw_object_store";
/// Legacy location of the relay snapshot fence. The fence now comes from the
/// `relay_snapshot_version_seq` sequence; the key stays excluded from the
/// configuration archive so an older archive can never reset coordination
/// state.
pub const RELAY_SNAPSHOT_VERSION_SETTINGS_KEY: &str = "relay_snapshot_version";

/// Settings rows that hold internal coordination state.
///
/// They belong to the running deployment and not to the archived
/// configuration: a configuration export never contains them and a
/// configuration import never resets them.
pub const INTERNAL_COORDINATION_SETTING_KEYS: [&str; 1] = [RELAY_SNAPSHOT_VERSION_SETTINGS_KEY];
const REQUEST_CONTENT_LOGGING_ENABLED_KEY: &str = "request_content_logging_enabled";
const LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY: &str = "usage_content_logging";
const LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY: &str = "usage_content_logging_enabled";
const DEFAULT_RAW_RETENTION_DAYS: i32 = 3;
const STREAM_OUTPUT_FLUSH_WINDOW_MIN_MS: u64 = 1;
const STREAM_OUTPUT_FLUSH_WINDOW_MAX_MS: u64 = 1_000;
const STREAM_OUTPUT_MAX_BUFFER_CHARS_MIN: usize = 1;
const STREAM_OUTPUT_MAX_BUFFER_CHARS_MAX: usize = 8_192;
const STREAM_OUTPUT_MAX_BUFFER_BYTES_MIN: usize = 1;
const STREAM_OUTPUT_MAX_BUFFER_BYTES_MAX: usize = 65_536;

pub async fn get_redaction_enabled(pool: &PgPool) -> Result<bool> {
    let db = postgres_connection(pool);
    let row = worker_settings::Entity::find_by_id("redaction_enabled".to_string())
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row.is_some_and(|row| row.setting_value == Value::Bool(true)))
}

pub async fn set_redaction_enabled(pool: &PgPool, enabled: bool) -> Result<()> {
    set_setting_value(pool, "redaction_enabled", Value::Bool(enabled)).await
}

pub async fn get_json_setting<T>(pool: &PgPool, key: &str) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    let db = postgres_connection(pool);
    let value = worker_settings::Entity::find_by_id(key.to_string())
        .one(&db)
        .await
        .map_err(native_error)?
        .map(|row| row.setting_value);
    value
        .map(|value| serde_json::from_value(value).context("invalid json setting value"))
        .transpose()
}

pub async fn set_json_setting<T>(pool: &PgPool, key: &str, value: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    let json_value = serde_json::to_value(value)?;
    set_setting_value(pool, key, json_value).await
}

async fn set_setting_value(pool: &PgPool, key: &str, value: Value) -> Result<()> {
    let db = postgres_connection(pool);
    worker_settings::Entity::insert(worker_settings::ActiveModel {
        setting_key: Set(key.to_string()),
        setting_value: Set(value),
        updated_at: sea_orm::ActiveValue::NotSet,
    })
    .on_conflict(
        OnConflict::column(worker_settings::Column::SettingKey)
            .update_column(worker_settings::Column::SettingValue)
            .value(
                worker_settings::Column::UpdatedAt,
                Expr::current_timestamp(),
            )
            .to_owned(),
    )
    .exec(&db)
    .await
    .map_err(native_error)?;
    Ok(())
}

pub async fn get_redaction_config(pool: &PgPool) -> Result<RedactionConfig> {
    let enabled = get_redaction_enabled(pool).await?;
    Ok(
        get_json_setting::<RedactionConfig>(pool, "redaction_config")
            .await?
            .unwrap_or(RedactionConfig {
                enabled,
                ..RedactionConfig::default()
            }),
    )
}

pub async fn set_redaction_config(pool: &PgPool, config: &RedactionConfig) -> Result<()> {
    set_json_setting(pool, "redaction_config", config).await?;
    set_redaction_enabled(pool, config.enabled).await?;
    Ok(())
}

pub async fn get_redaction_config_updated_at(
    pool: &PgPool,
    global: bool,
    user_id: Option<i64>,
) -> Result<Option<DateTime<Utc>>> {
    let db = postgres_connection(pool);
    if global {
        Ok(
            worker_settings::Entity::find_by_id("redaction_config".to_string())
                .one(&db)
                .await
                .map_err(native_error)?
                .map(|row| row.updated_at),
        )
    } else if let Some(user_id) = user_id {
        Ok(user_redaction_configs::Entity::find_by_id(user_id)
            .one(&db)
            .await
            .map_err(native_error)?
            .map(|row| row.updated_at))
    } else {
        Ok(None)
    }
}

pub async fn get_user_redaction_config(pool: &PgPool, user_id: i64) -> Result<RedactionConfig> {
    let db = postgres_connection(pool);
    let value = user_redaction_configs::Entity::find_by_id(user_id)
        .one(&db)
        .await
        .map_err(native_error)?
        .map(|row| row.config);
    value
        .map(serde_json::from_value)
        .transpose()
        .context("invalid user redaction config")?
        .map(Ok)
        .unwrap_or_else(|| Ok(RedactionConfig::default()))
}

pub async fn list_user_redaction_configs(pool: &PgPool) -> Result<HashMap<i64, RedactionConfig>> {
    let db = postgres_connection(pool);
    let rows = user_redaction_configs::Entity::find()
        .order_by_asc(user_redaction_configs::Column::UserId)
        .all(&db)
        .await
        .map_err(native_error)?;
    let mut configs = HashMap::with_capacity(rows.len());
    for row in rows {
        configs.insert(
            row.user_id,
            serde_json::from_value(row.config).context("invalid user redaction config")?,
        );
    }
    Ok(configs)
}

pub async fn set_user_redaction_config(
    pool: &PgPool,
    user_id: i64,
    config: &RedactionConfig,
) -> Result<()> {
    let json_value = serde_json::to_value(config)?;
    let db = postgres_connection(pool);
    user_redaction_configs::Entity::insert(user_redaction_configs::ActiveModel {
        user_id: Set(user_id),
        config: Set(json_value),
        updated_at: sea_orm::ActiveValue::NotSet,
    })
    .on_conflict(
        OnConflict::column(user_redaction_configs::Column::UserId)
            .update_column(user_redaction_configs::Column::Config)
            .value(
                user_redaction_configs::Column::UpdatedAt,
                Expr::current_timestamp(),
            )
            .to_owned(),
    )
    .exec(&db)
    .await
    .map_err(native_error)?;
    Ok(())
}

pub async fn list_redaction_custom_string_rules(
    pool: &PgPool,
    global: bool,
    user_id: Option<i64>,
    first: i64,
    rows: i64,
    search: Option<&str>,
) -> Result<(Vec<RedactionCustomStringRuleListItem>, i64)> {
    let total = sqlx::query_file!(
        "src/sql/settings/count_redaction_custom_string_rules.sql",
        global,
        user_id,
        search,
    )
    .fetch_one(pool)
    .await?;
    let rows = sqlx::query_file!(
        "src/sql/settings/list_redaction_custom_string_rules.sql",
        global,
        user_id,
        first.max(0),
        rows.max(1),
        search,
    )
    .fetch_all(pool)
    .await?;
    Ok((
        rows.into_iter()
            .map(|row| RedactionCustomStringRuleListItem {
                array_index: row.array_index,
                pattern: row.pattern,
                match_type: row.match_type,
                scope: row.scope,
                created_at: parse_rfc3339_utc(row.created_at),
                updated_at: parse_rfc3339_utc(row.updated_at),
            })
            .collect(),
        total.total,
    ))
}

pub async fn get_bool_setting(pool: &PgPool, key: &str, default: bool) -> Result<bool> {
    let db = postgres_connection(pool);
    let row = worker_settings::Entity::find_by_id(key.to_string())
        .one(&db)
        .await
        .map_err(native_error)?;
    Ok(row
        .map(|row| row.setting_value == Value::Bool(true))
        .unwrap_or(default))
}

pub async fn set_bool_setting(pool: &PgPool, key: &str, enabled: bool) -> Result<()> {
    set_setting_value(pool, key, Value::Bool(enabled)).await
}

pub async fn get_request_content_logging(pool: &PgPool) -> Result<RequestContentLoggingResponse> {
    if let Some(setting) = get_json_setting::<RequestContentLoggingResponse>(
        pool,
        REQUEST_CONTENT_LOGGING_SETTINGS_KEY,
    )
    .await?
    {
        let normalized = normalize_request_content_logging(setting);
        delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY).await?;
        delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY).await?;
        return Ok(normalized);
    }
    if let Some(setting) = get_json_setting::<RequestContentLoggingResponse>(
        pool,
        LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY,
    )
    .await?
    {
        let normalized = normalize_request_content_logging(setting);
        set_json_setting(pool, REQUEST_CONTENT_LOGGING_SETTINGS_KEY, &normalized).await?;
        delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY).await?;
        delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY).await?;
        return Ok(normalized);
    }

    let legacy_enabled =
        get_bool_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY, false).await?;
    let enabled =
        get_bool_setting(pool, REQUEST_CONTENT_LOGGING_ENABLED_KEY, false).await? || legacy_enabled;
    let response = RequestContentLoggingResponse {
        mode: if enabled {
            RequestContentLoggingMode::NormalizedOnly
        } else {
            RequestContentLoggingMode::Off
        },
        raw_retention_days: DEFAULT_RAW_RETENTION_DAYS,
    };
    if enabled {
        set_json_setting(pool, REQUEST_CONTENT_LOGGING_SETTINGS_KEY, &response).await?;
    }
    delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY).await?;
    delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY).await?;
    Ok(response)
}

pub async fn set_request_content_logging(
    pool: &PgPool,
    value: &RequestContentLoggingResponse,
) -> Result<RequestContentLoggingResponse> {
    let normalized = normalize_request_content_logging(value.clone());
    set_json_setting(pool, REQUEST_CONTENT_LOGGING_SETTINGS_KEY, &normalized).await?;
    set_bool_setting(
        pool,
        REQUEST_CONTENT_LOGGING_ENABLED_KEY,
        normalized.mode.captures_normalized(),
    )
    .await?;
    delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY).await?;
    delete_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY).await?;
    let mut retention = get_usage_retention(pool).await?;
    retention.raw_retention_days = normalized.raw_retention_days;
    set_usage_retention(pool, &retention).await?;
    Ok(normalized)
}

async fn delete_setting(pool: &PgPool, key: &str) -> Result<()> {
    let db = postgres_connection(pool);
    worker_settings::Entity::delete_many()
        .filter(worker_settings::Column::SettingKey.eq(key))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(())
}

pub async fn get_usage_retention(pool: &PgPool) -> Result<UsageRetentionSettings> {
    Ok(
        get_json_setting::<UsageRetentionSettings>(pool, USAGE_RETENTION_SETTINGS_KEY)
            .await?
            .unwrap_or_default()
            .normalized(),
    )
}

pub async fn set_usage_retention(
    pool: &PgPool,
    value: &UsageRetentionSettings,
) -> Result<UsageRetentionSettings> {
    let normalized = value.clone().normalized();
    set_json_setting(pool, USAGE_RETENTION_SETTINGS_KEY, &normalized).await?;
    Ok(normalized)
}

pub async fn get_stream_delta_batching(pool: &PgPool) -> Result<StreamDeltaBatchingSettings> {
    Ok(
        get_json_setting::<StreamDeltaBatchingSettings>(pool, STREAM_DELTA_BATCHING_SETTINGS_KEY)
            .await?
            .map(normalize_stream_delta_batching)
            .unwrap_or_default(),
    )
}

pub async fn set_stream_delta_batching(
    pool: &PgPool,
    value: &StreamDeltaBatchingSettings,
) -> Result<StreamDeltaBatchingSettings> {
    let normalized = normalize_stream_delta_batching(value.clone());
    set_json_setting(pool, STREAM_DELTA_BATCHING_SETTINGS_KEY, &normalized).await?;
    Ok(normalized)
}

fn normalize_request_content_logging(
    mut value: RequestContentLoggingResponse,
) -> RequestContentLoggingResponse {
    if value.raw_retention_days <= 0 {
        value.raw_retention_days = DEFAULT_RAW_RETENTION_DAYS;
    }
    value
}

fn normalize_stream_delta_batching(
    mut value: StreamDeltaBatchingSettings,
) -> StreamDeltaBatchingSettings {
    value.flush_window_ms = value.flush_window_ms.clamp(
        STREAM_OUTPUT_FLUSH_WINDOW_MIN_MS,
        STREAM_OUTPUT_FLUSH_WINDOW_MAX_MS,
    );
    value.max_buffer_chars = value.max_buffer_chars.clamp(
        STREAM_OUTPUT_MAX_BUFFER_CHARS_MIN,
        STREAM_OUTPUT_MAX_BUFFER_CHARS_MAX,
    );
    value.max_buffer_bytes = value.max_buffer_bytes.clamp(
        STREAM_OUTPUT_MAX_BUFFER_BYTES_MIN,
        STREAM_OUTPUT_MAX_BUFFER_BYTES_MAX,
    );
    value
}

/// Advisory lock that serializes the relay snapshot publication.
///
/// The lock is held from before the fence token allocation and the snapshot
/// content read until the snapshot is sent, so a version always labels content
/// that is at least as fresh as the content of every earlier version.
const RELAY_SNAPSHOT_PUBLICATION_LOCK: i64 = 8_675_309_004;

/// Upper bound for the whole acquisition of the publication boundary.
const RELAY_SNAPSHOT_PUBLICATION_TIMEOUT: Duration = Duration::from_secs(15);

/// Backoff between two attempts to enter the publication boundary.
const RELAY_SNAPSHOT_PUBLICATION_BACKOFF: Duration = Duration::from_millis(50);

/// Longest backoff between two attempts to enter the publication boundary.
const RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF: Duration = Duration::from_millis(500);

/// The backoff before the next attempt to enter the publication boundary.
fn publication_retry_backoff(current: Duration) -> Duration {
    current
        .saturating_mul(2)
        .min(RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF)
}

/// Open the relay snapshot publication boundary.
///
/// The boundary is entered with a non-blocking lock: a caller that does not
/// get it rolls its transaction back, releases the connection and retries
/// after a bounded backoff. A waiting publisher therefore never holds a
/// database connection, so the pool cannot be exhausted by concurrent
/// publications.
pub async fn begin_snapshot_publication(
    pool: &PgPool,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>> {
    let deadline = tokio::time::Instant::now() + RELAY_SNAPSHOT_PUBLICATION_TIMEOUT;
    let mut backoff = RELAY_SNAPSHOT_PUBLICATION_BACKOFF;
    loop {
        let mut transaction = pool.begin().await?;
        let acquired: bool = sqlx::query_file_unchecked!(
            "src/sql/settings/try_lock_relay_snapshot_publication.sql",
            RELAY_SNAPSHOT_PUBLICATION_LOCK
        )
        .fetch_one(&mut *transaction)
        .await?
        .acquired
        .unwrap_or(false);
        if acquired {
            return Ok(transaction);
        }
        // The boundary belongs to another publisher: the transaction and its
        // connection are released before the next attempt.
        transaction.rollback().await?;
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "the relay snapshot publication boundary stayed busy"
            ));
        }
        tokio::time::sleep(backoff).await;
        backoff = publication_retry_backoff(backoff);
    }
}

/// Allocate the next global relay snapshot fence.
///
/// The token comes from a PostgreSQL sequence on the SAME connection that
/// holds the publication boundary. A sequence is non-transactional, so the
/// token survives a rollback and is never reused, and a publisher whose
/// boundary connection was lost cannot allocate a newer fence from a healthy
/// pooled connection after it has already read older contents: the fence is
/// fixed before any content read.
pub async fn next_relay_snapshot_version(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<i64> {
    let version: Option<i64> =
        sqlx::query_file_scalar!("src/sql/settings/next_relay_snapshot_version.sql")
            .fetch_one(&mut **transaction)
            .await?;
    version.ok_or_else(|| anyhow::anyhow!("the relay snapshot sequence returned no version"))
}

pub async fn get_raw_object_store_persisted(
    pool: &PgPool,
) -> Result<Option<RawObjectStorePersisted>> {
    get_json_setting::<RawObjectStorePersisted>(pool, RAW_OBJECT_STORE_SETTINGS_KEY).await
}

pub async fn set_raw_object_store_persisted(
    pool: &PgPool,
    value: &RawObjectStorePersisted,
) -> Result<()> {
    set_json_setting(pool, RAW_OBJECT_STORE_SETTINGS_KEY, value).await
}

pub async fn get_raw_object_store_config(
    pool: &PgPool,
    manager: &RelaySecretManager,
) -> Result<Option<RawObjectStoreConfig>> {
    if let Some(persisted) = get_raw_object_store_persisted(pool).await? {
        Ok(Some(persisted.into_config(manager)?))
    } else {
        Ok(None)
    }
}

pub async fn set_raw_object_store_config(
    pool: &PgPool,
    manager: &RelaySecretManager,
    config: &RawObjectStoreConfig,
) -> Result<RawObjectStorePersisted> {
    let persisted = RawObjectStorePersisted::from_config(config, manager)?;
    set_raw_object_store_persisted(pool, &persisted).await?;
    Ok(persisted)
}

#[cfg(test)]
mod tests {
    use super::{
        RELAY_SNAPSHOT_PUBLICATION_BACKOFF, RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF,
        parse_rfc3339_utc, publication_retry_backoff,
    };
    use std::time::Duration;

    #[test]
    fn the_publication_backoff_grows_and_stays_bounded() {
        let mut backoff = RELAY_SNAPSHOT_PUBLICATION_BACKOFF;
        assert!(backoff < RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF);
        for _ in 0..16 {
            let next = publication_retry_backoff(backoff);
            assert!(next >= backoff, "the backoff never shrinks");
            assert!(next <= RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF);
            backoff = next;
        }
        assert_eq!(backoff, RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF);
        assert_eq!(
            publication_retry_backoff(RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF),
            RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF
        );
        assert_eq!(
            publication_retry_backoff(Duration::from_secs(30)),
            RELAY_SNAPSHOT_PUBLICATION_MAX_BACKOFF
        );
    }

    #[test]
    fn parse_rfc3339_utc_handles_valid_blank_junk_and_missing() {
        assert_eq!(
            parse_rfc3339_utc(Some("2026-01-02T03:04:05Z".to_string()))
                .map(|value| value.to_rfc3339()),
            Some("2026-01-02T03:04:05+00:00".to_string())
        );
        assert_eq!(parse_rfc3339_utc(Some("   ".to_string())), None);
        assert_eq!(parse_rfc3339_utc(Some("garbage".to_string())), None);
        assert_eq!(parse_rfc3339_utc(None), None);
    }
}
