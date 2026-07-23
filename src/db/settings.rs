use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};
use sqlx::PgPool;

use crate::{
    db::StreamDeltaBatchingSettings,
    redact::RedactionConfig,
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
};

#[derive(Debug, Clone)]
pub struct RedactionCustomStringRuleListItem {
    pub array_index: i64,
    pub pattern: String,
    pub match_type: String,
    pub scope: String,
}

pub const REQUEST_CONTENT_LOGGING_SETTINGS_KEY: &str = "request_content_logging";
pub const STREAM_DELTA_BATCHING_SETTINGS_KEY: &str = "stream_delta_batching";
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
    Ok(
        sqlx::query_file!("src/sql/settings/get_redaction_enabled.sql")
            .fetch_optional(pool)
            .await?
            .map(|row| row.enabled)
            .unwrap_or(false),
    )
}

pub async fn set_redaction_enabled(pool: &PgPool, enabled: bool) -> Result<()> {
    sqlx::query_file!("src/sql/settings/set_redaction_enabled.sql", enabled,)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_json_setting<T>(pool: &PgPool, key: &str) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    let value = sqlx::query_file!("src/sql/settings/get_json_setting.sql", key,)
        .fetch_optional(pool)
        .await?
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
    sqlx::query_file!("src/sql/settings/set_json_setting.sql", key, json_value,)
        .execute(pool)
        .await?;
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
) -> Result<(
    Vec<RedactionCustomStringRuleListItem>,
    i64,
    Option<DateTime<Utc>>,
)> {
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
            })
            .collect(),
        total.total,
        total.updated_at,
    ))
}

pub async fn get_bool_setting(pool: &PgPool, key: &str, default: bool) -> Result<bool> {
    Ok(
        sqlx::query_file!("src/sql/settings/get_bool_setting.sql", key,)
            .fetch_optional(pool)
            .await?
            .map(|row| row.enabled)
            .unwrap_or(default),
    )
}

pub async fn set_bool_setting(pool: &PgPool, key: &str, enabled: bool) -> Result<()> {
    sqlx::query_file!("src/sql/settings/set_bool_setting.sql", key, enabled,)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_request_content_logging(pool: &PgPool) -> Result<RequestContentLoggingResponse> {
    if let Some(setting) = get_json_setting::<RequestContentLoggingResponse>(
        pool,
        REQUEST_CONTENT_LOGGING_SETTINGS_KEY,
    )
    .await?
    {
        return Ok(normalize_request_content_logging(setting));
    }
    if let Some(setting) = get_json_setting::<RequestContentLoggingResponse>(
        pool,
        LEGACY_USAGE_CONTENT_LOGGING_SETTINGS_KEY,
    )
    .await?
    {
        return Ok(normalize_request_content_logging(setting));
    }

    let enabled = get_bool_setting(pool, REQUEST_CONTENT_LOGGING_ENABLED_KEY, false).await?
        || get_bool_setting(pool, LEGACY_USAGE_CONTENT_LOGGING_ENABLED_KEY, false).await?;
    Ok(RequestContentLoggingResponse {
        mode: if enabled {
            RequestContentLoggingMode::NormalizedOnly
        } else {
            RequestContentLoggingMode::Off
        },
        raw_retention_days: DEFAULT_RAW_RETENTION_DAYS,
    })
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
