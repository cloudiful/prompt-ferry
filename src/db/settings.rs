use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::PgPool;

use crate::{
    db::StreamDeltaBatchingSettings,
    db::entities::postgres::worker_settings,
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
    let row = sqlx::query_file!(
        "src/sql/settings/get_redaction_config_updated_at.sql",
        global,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(row.updated_at)
}

pub async fn get_user_redaction_config(pool: &PgPool, user_id: i64) -> Result<RedactionConfig> {
    let value = sqlx::query_file!("src/sql/settings/get_user_redaction_config.sql", user_id,)
        .fetch_optional(pool)
        .await?
        .map(|row| row.config);
    value
        .map(serde_json::from_value)
        .transpose()
        .context("invalid user redaction config")?
        .map(Ok)
        .unwrap_or_else(|| Ok(RedactionConfig::default()))
}

pub async fn list_user_redaction_configs(pool: &PgPool) -> Result<HashMap<i64, RedactionConfig>> {
    let rows = sqlx::query_file!("src/sql/settings/list_user_redaction_configs.sql")
        .fetch_all(pool)
        .await?;
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
    sqlx::query_file!(
        "src/sql/settings/set_user_redaction_config.sql",
        user_id,
        json_value,
    )
    .execute(pool)
    .await?;
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
    use super::parse_rfc3339_utc;

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
