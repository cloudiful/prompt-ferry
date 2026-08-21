use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use super::models::{
    ClientKeyConfig, ContinuationPolicy, EndpointApiKeyConfig, EndpointProvider, EndpointRegion,
    ManagedRelayConfig, ModelRouteConfig, ModelRouteTargetConfig, ProviderEndpointConfig, Result,
    RouteScope, RoutingStrategy, SettingConfig, StandaloneConfigError,
};
use crate::{
    config::{BridgeEncryptionMode, NativeApi, NativeApiSource, TlsMode},
    relay_secrets::EncryptedSecretEnvelope,
};

pub(crate) fn uuid(row: &SqliteRow, column: &str) -> Result<Uuid> {
    let value: String = row.try_get(column)?;
    Uuid::parse_str(&value).map_err(|error| {
        StandaloneConfigError::CorruptDatabase(format!("column {column} is not a UUID: {error}"))
    })
}

pub(crate) fn required_string(row: &SqliteRow, column: &str) -> Result<String> {
    let value: String = row.try_get(column)?;
    if value.trim().is_empty() {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column {column} is empty"
        )));
    }
    Ok(value)
}

pub(crate) fn optional_string(row: &SqliteRow, column: &str) -> Result<Option<String>> {
    Ok(row.try_get(column)?)
}

pub(crate) fn bool_value(row: &SqliteRow, column: &str) -> Result<bool> {
    match row.try_get::<i64, _>(column)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(StandaloneConfigError::CorruptDatabase(format!(
            "column {column} is not a boolean: {value}"
        ))),
    }
}

pub(crate) fn optional_i32(row: &SqliteRow, column: &str) -> Result<Option<i32>> {
    let value = row.try_get::<Option<i64>, _>(column)?;
    value
        .map(|value| {
            i32::try_from(value).map_err(|_| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "column {column} is outside the i32 range"
                ))
            })
        })
        .transpose()
}

pub(crate) fn envelope(row: &SqliteRow, prefix: &str) -> Result<Option<EncryptedSecretEnvelope>> {
    let ciphertext_column = format!("{prefix}_ciphertext");
    let nonce_column = format!("{prefix}_nonce");
    let version_column = format!("{prefix}_key_version");
    let ciphertext = row.try_get::<Option<Vec<u8>>, _>(ciphertext_column.as_str())?;
    let nonce = row.try_get::<Option<Vec<u8>>, _>(nonce_column.as_str())?;
    let key_version = row.try_get::<Option<i64>, _>(version_column.as_str())?;
    match (ciphertext, nonce, key_version) {
        (None, None, None) => Ok(None),
        (Some(ciphertext), Some(nonce), Some(key_version)) => Ok(Some(EncryptedSecretEnvelope {
            ciphertext,
            nonce,
            key_version: i16::try_from(key_version).map_err(|_| {
                StandaloneConfigError::CorruptDatabase(format!(
                    "{prefix}_key_version is outside the i16 range"
                ))
            })?,
        })),
        _ => Err(StandaloneConfigError::CorruptDatabase(format!(
            "secret envelope {prefix} has incomplete columns"
        ))),
    }
}

pub(crate) fn relay(
    row: &SqliteRow,
) -> Result<(ManagedRelayConfig, [Option<EncryptedSecretEnvelope>; 4])> {
    let envelopes = [
        envelope(row, "relay_ca")?,
        envelope(row, "client_cert")?,
        envelope(row, "client_key")?,
        envelope(row, "bridge_encryption_key")?,
    ];
    let tls_mode = parse_tls_mode(&required_string(row, "tls_mode")?)?;
    let bridge_encryption_mode =
        parse_bridge_mode(&required_string(row, "bridge_encryption_mode")?)?;
    Ok((
        ManagedRelayConfig {
            relay_id: uuid(row, "relay_id")?,
            name: required_string(row, "name")?,
            relay_url: required_string(row, "relay_url")?,
            enabled: bool_value(row, "enabled")?,
            tls_mode,
            bridge_encryption_mode,
            relay_ca_pem: None,
            client_cert_pem: None,
            client_key_pem: None,
            bridge_encryption_key: None,
        },
        envelopes,
    ))
}

pub(crate) fn relay_envelopes(row: &SqliteRow) -> Result<[Option<EncryptedSecretEnvelope>; 4]> {
    Ok([
        envelope(row, "relay_ca")?,
        envelope(row, "client_cert")?,
        envelope(row, "client_key")?,
        envelope(row, "bridge_encryption_key")?,
    ])
}

pub(crate) fn endpoint(
    row: &SqliteRow,
) -> Result<(ProviderEndpointConfig, Option<EncryptedSecretEnvelope>)> {
    let created_at = sqlite_timestamp(row, "created_at")?;
    let updated_at = sqlite_timestamp(row, "updated_at")?;
    Ok((
        ProviderEndpointConfig {
            endpoint_id: uuid(row, "endpoint_id")?,
            name: required_string(row, "name")?,
            provider: EndpointProvider::parse(&required_string(row, "provider")?)?,
            provider_region: EndpointRegion::parse(
                optional_string(row, "provider_region")?.as_deref(),
            )?,
            base_url: required_string(row, "base_url")?,
            native_api: parse_native_api(&required_string(row, "native_api")?)?,
            native_api_source: parse_native_api_source(&required_string(
                row,
                "native_api_source",
            )?)?,
            key_lb_enabled: bool_value(row, "key_lb_enabled")?,
            enabled: bool_value(row, "enabled")?,
            mcp_enabled: bool_value(row, "mcp_enabled")?,
            created_at,
            updated_at,
            api_key: String::new(),
            api_keys: Vec::new(),
        },
        envelope(row, "api_key")?,
    ))
}

pub(crate) fn endpoint_key(
    row: &SqliteRow,
) -> Result<(EndpointApiKeyConfig, EncryptedSecretEnvelope)> {
    let created_at = sqlite_timestamp(row, "created_at")?;
    let updated_at = sqlite_timestamp(row, "updated_at")?;
    Ok((
        EndpointApiKeyConfig {
            key_id: uuid(row, "key_id")?,
            endpoint_id: uuid(row, "endpoint_id")?,
            key_label: required_string(row, "key_label")?,
            api_key: String::new(),
            position: i32::try_from(row.try_get::<i64, _>("position")?).map_err(|_| {
                StandaloneConfigError::CorruptDatabase(
                    "endpoint key position is outside i32 range".to_string(),
                )
            })?,
            enabled: bool_value(row, "enabled")?,
            created_at,
            updated_at,
        },
        envelope(row, "api_key")?.ok_or_else(|| {
            StandaloneConfigError::CorruptDatabase(
                "endpoint key is missing its secret envelope".to_string(),
            )
        })?,
    ))
}

fn sqlite_timestamp(row: &SqliteRow, column: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let value: String = row.try_get(column)?;
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(&value) {
        return Ok(timestamp.with_timezone(&chrono::Utc));
    }
    let timestamp =
        chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%d %H:%M:%S").map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a timestamp: {error}"
            ))
        })?;
    Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        timestamp,
        chrono::Utc,
    ))
}

pub(crate) fn route(row: &SqliteRow) -> Result<ModelRouteConfig> {
    Ok(ModelRouteConfig {
        rule_id: uuid(row, "rule_id")?,
        scope: RouteScope::parse(&required_string(row, "scope")?)?,
        owner_user_id: row.try_get("owner_user_id")?,
        model_pattern: required_string(row, "model_pattern")?,
        routing_strategy: RoutingStrategy::parse(&required_string(row, "routing_strategy")?)?,
        daily_max_requests: optional_i32(row, "daily_max_requests")?,
        monthly_max_requests: optional_i32(row, "monthly_max_requests")?,
        enabled: bool_value(row, "enabled")?,
        targets: Vec::new(),
    })
}

pub(crate) fn route_target(row: &SqliteRow) -> Result<(Uuid, ModelRouteTargetConfig)> {
    Ok((
        uuid(row, "rule_id")?,
        ModelRouteTargetConfig {
            target_id: uuid(row, "target_id")?,
            endpoint_id: uuid(row, "endpoint_id")?,
            position: i32::try_from(row.try_get::<i64, _>("position")?).map_err(|_| {
                StandaloneConfigError::CorruptDatabase(
                    "route target position is outside i32 range".to_string(),
                )
            })?,
            enabled: bool_value(row, "enabled")?,
            upstream_model: optional_string(row, "upstream_model")?,
            responses_continuation_policy: ContinuationPolicy::parse(&required_string(
                row,
                "responses_continuation_policy",
            )?)?,
        },
    ))
}

pub(crate) fn client_key(row: &SqliteRow) -> Result<(ClientKeyConfig, EncryptedSecretEnvelope)> {
    Ok((
        ClientKeyConfig {
            key_id: uuid(row, "key_id")?,
            user_id: row.try_get("user_id")?,
            key_prefix: required_string(row, "key_prefix")?,
            label: required_string(row, "label")?,
            secret: String::new(),
            enabled: bool_value(row, "enabled")?,
        },
        envelope(row, "secret")?.ok_or_else(|| {
            StandaloneConfigError::CorruptDatabase(
                "client key is missing its secret envelope".to_string(),
            )
        })?,
    ))
}

pub(crate) fn setting(row: &SqliteRow) -> Result<SettingConfig> {
    let json = required_string(row, "value_json")?;
    Ok(SettingConfig {
        key: required_string(row, "setting_key")?,
        version: row.try_get("value_version")?,
        value: serde_json::from_str(&json)?,
    })
}

fn parse_tls_mode(value: &str) -> Result<TlsMode> {
    match value {
        "off" => Ok(TlsMode::Off),
        "server" => Ok(TlsMode::Server),
        "mtls" => Ok(TlsMode::Mtls),
        _ => Err(StandaloneConfigError::CorruptDatabase(format!(
            "unknown TLS mode {value:?}"
        ))),
    }
}

fn parse_bridge_mode(value: &str) -> Result<BridgeEncryptionMode> {
    match value {
        "off" => Ok(BridgeEncryptionMode::Off),
        "required" => Ok(BridgeEncryptionMode::Required),
        _ => Err(StandaloneConfigError::CorruptDatabase(format!(
            "unknown bridge encryption mode {value:?}"
        ))),
    }
}

fn parse_native_api(value: &str) -> Result<NativeApi> {
    match value {
        "auto" => Ok(NativeApi::Auto),
        "anthropic_messages" => Ok(NativeApi::AnthropicMessages),
        "chat" => Ok(NativeApi::Chat),
        "responses" => Ok(NativeApi::Responses),
        "realtime" => Ok(NativeApi::Realtime),
        _ => Err(StandaloneConfigError::CorruptDatabase(format!(
            "unknown native API {value:?}"
        ))),
    }
}

fn parse_native_api_source(value: &str) -> Result<NativeApiSource> {
    match value {
        "auto" => Ok(NativeApiSource::Auto),
        "detected" => Ok(NativeApiSource::Detected),
        "manual" => Ok(NativeApiSource::Manual),
        _ => Err(StandaloneConfigError::CorruptDatabase(format!(
            "unknown native API source {value:?}"
        ))),
    }
}
