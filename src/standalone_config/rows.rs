use std::sync::Arc;

use sea_orm::{DbErr, RuntimeErr};
use sqlx::{Row, sqlite::SqliteRow};
use uuid::Uuid;

use super::models::{
    ClientKeyConfig, EndpointApiKeyConfig, EndpointProvider, EndpointRegion, ManagedRelayConfig,
    ModelRouteConfig, ModelRouteTargetConfig, ProviderEndpointConfig, Result, RouteScope,
    RoutingStrategy, SettingConfig, StandaloneConfigError,
};
use crate::{
    config::{BridgeEncryptionMode, NativeApi, NativeApiSource, TlsMode},
    db::McpServer,
    db::entities::sqlite::{
        standalone_client_keys, standalone_endpoint_keys, standalone_mcp_servers,
        standalone_model_route_targets, standalone_model_routes, standalone_provider_endpoints,
        standalone_relays, standalone_settings,
    },
    relay_secrets::EncryptedSecretEnvelope,
};

/// Converts a native SeaORM error into the store error without losing the
/// driver error the existing SQLx path exposed.
pub(crate) fn native_error(error: DbErr) -> StandaloneConfigError {
    let message = error.to_string();
    match error {
        DbErr::Exec(RuntimeErr::SqlxError(error))
        | DbErr::Query(RuntimeErr::SqlxError(error))
        | DbErr::Conn(RuntimeErr::SqlxError(error)) => match Arc::try_unwrap(error) {
            Ok(error) => StandaloneConfigError::Database(error),
            Err(error) => {
                StandaloneConfigError::CorruptDatabase(format!("native SQLite error: {error}"))
            }
        },
        _ => StandaloneConfigError::CorruptDatabase(format!("native SQLite error: {message}")),
    }
}

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

fn model_bool(value: i32, column: &str) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(StandaloneConfigError::CorruptDatabase(format!(
            "column {column} is not a boolean: {value}"
        ))),
    }
}

fn envelope_from_parts(
    prefix: &str,
    ciphertext: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
    key_version: Option<i32>,
) -> Result<Option<EncryptedSecretEnvelope>> {
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

fn required_envelope(
    prefix: &str,
    ciphertext: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
    key_version: Option<i32>,
    label: &str,
) -> Result<EncryptedSecretEnvelope> {
    envelope_from_parts(prefix, ciphertext, nonce, key_version)?.ok_or_else(|| {
        StandaloneConfigError::CorruptDatabase(format!(
            "MCP server is missing its {label} secret envelope"
        ))
    })
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

// Issue #368 Phase A: proxy envelopes were added by standalone 0018.
// Pre-migration rows lack the columns entirely; treat a missing column
// as `None` (direct) so legacy snapshots keep loading.
pub(crate) fn envelope_opt(
    row: &SqliteRow,
    prefix: &str,
) -> Result<Option<EncryptedSecretEnvelope>> {
    match envelope(row, prefix) {
        Ok(value) => Ok(value),
        Err(StandaloneConfigError::Database(sqlx::Error::ColumnNotFound(_))) => Ok(None),
        Err(error) => Err(error),
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

fn model_required_string(value: &str, column: &str) -> Result<String> {
    if value.trim().is_empty() {
        return Err(StandaloneConfigError::CorruptDatabase(format!(
            "column {column} is empty"
        )));
    }
    Ok(value.to_string())
}

fn model_uuid(value: &str, column: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|error| {
        StandaloneConfigError::CorruptDatabase(format!("column {column} is not a UUID: {error}"))
    })
}

fn model_optional_uuid(value: Option<&str>, column: &str) -> Result<Option<Uuid>> {
    value.map(|value| model_uuid(value, column)).transpose()
}

fn model_optional_timestamp(
    value: Option<&str>,
    column: &str,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    value
        .map(|value| parse_timestamp(value, column))
        .transpose()
}

pub(crate) fn relay_envelopes_from_model(
    model: &standalone_relays::Model,
) -> Result<[Option<EncryptedSecretEnvelope>; 4]> {
    Ok([
        envelope_from_parts(
            "relay_ca",
            model.relay_ca_ciphertext.clone(),
            model.relay_ca_nonce.clone(),
            model.relay_ca_key_version,
        )?,
        envelope_from_parts(
            "client_cert",
            model.client_cert_ciphertext.clone(),
            model.client_cert_nonce.clone(),
            model.client_cert_key_version,
        )?,
        envelope_from_parts(
            "client_key",
            model.client_key_ciphertext.clone(),
            model.client_key_nonce.clone(),
            model.client_key_key_version,
        )?,
        envelope_from_parts(
            "bridge_encryption_key",
            model.bridge_encryption_key_ciphertext.clone(),
            model.bridge_encryption_key_nonce.clone(),
            model.bridge_encryption_key_key_version,
        )?,
    ])
}

pub(crate) fn relay_from_model(
    model: &standalone_relays::Model,
) -> Result<(ManagedRelayConfig, [Option<EncryptedSecretEnvelope>; 4])> {
    let relay = ManagedRelayConfig {
        relay_id: model_uuid(&model.relay_id, "relay_id")?,
        name: model_required_string(&model.name, "name")?,
        relay_url: model_required_string(&model.relay_url, "relay_url")?,
        enabled: model_bool(model.enabled, "enabled")?,
        tls_mode: parse_tls_mode(&model_required_string(&model.tls_mode, "tls_mode")?)?,
        bridge_encryption_mode: parse_bridge_mode(&model_required_string(
            &model.bridge_encryption_mode,
            "bridge_encryption_mode",
        )?)?,
        relay_ca_pem: None,
        client_cert_pem: None,
        client_key_pem: None,
        bridge_encryption_key: None,
    };
    Ok((relay, relay_envelopes_from_model(model)?))
}

pub(crate) fn endpoint(
    row: &SqliteRow,
) -> Result<(
    ProviderEndpointConfig,
    Option<EncryptedSecretEnvelope>,
    Option<EncryptedSecretEnvelope>,
)> {
    let created_at = sqlite_timestamp(row, "created_at")?;
    let updated_at = sqlite_timestamp(row, "updated_at")?;
    // Issue #637: free-form service-tier override (standalone 0034). NULL or
    // blank means inherit; a missing column (pre-migration snapshot) also
    // reads as inherit.
    let service_tier = match row.try_get::<Option<String>, _>("service_tier") {
        Ok(value) => value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        Err(sqlx::Error::ColumnNotFound(_)) => None,
        Err(error) => return Err(error.into()),
    };
    // Issue #392 Phase K: 0021 `active_windows` is plaintext; pre-migration
    // rows lack the column and read as all-day (`None`).
    let active_windows = match row.try_get::<Option<String>, _>("active_windows") {
        Ok(value) => value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        Err(sqlx::Error::ColumnNotFound(_)) => None,
        Err(error) => return Err(error.into()),
    };
    Ok((
        ProviderEndpointConfig {
            endpoint_id: uuid(row, "endpoint_id")?,
            name: required_string(row, "name")?,
            provider: EndpointProvider::parse(&required_string(row, "provider")?)?,
            provider_region: EndpointRegion::parse(
                optional_string(row, "provider_region")?.as_deref(),
            )?,
            service_tier,
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
            proxy_url: None,
            active_windows,
        },
        envelope(row, "api_key")?,
        envelope_opt(row, "proxy_url")?,
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

pub(crate) fn endpoint_from_model(
    model: &standalone_provider_endpoints::Model,
) -> Result<(
    ProviderEndpointConfig,
    Option<EncryptedSecretEnvelope>,
    Option<EncryptedSecretEnvelope>,
)> {
    Ok((
        ProviderEndpointConfig {
            endpoint_id: model_uuid(&model.endpoint_id, "endpoint_id")?,
            name: model_required_string(&model.name, "name")?,
            provider: EndpointProvider::parse(&model_required_string(
                &model.provider,
                "provider",
            )?)?,
            provider_region: EndpointRegion::parse(model.provider_region.as_deref())?,
            service_tier: model
                .service_tier
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            base_url: model_required_string(&model.base_url, "base_url")?,
            native_api: parse_native_api(&model_required_string(&model.native_api, "native_api")?)?,
            native_api_source: parse_native_api_source(&model_required_string(
                &model.native_api_source,
                "native_api_source",
            )?)?,
            key_lb_enabled: model_bool(model.key_lb_enabled, "key_lb_enabled")?,
            enabled: model_bool(model.enabled, "enabled")?,
            mcp_enabled: model_bool(model.mcp_enabled, "mcp_enabled")?,
            created_at: parse_timestamp(&model.created_at, "created_at")?,
            updated_at: parse_timestamp(&model.updated_at, "updated_at")?,
            api_key: String::new(),
            api_keys: Vec::new(),
            proxy_url: None,
            active_windows: model
                .active_windows
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        },
        envelope_from_parts(
            "api_key",
            Some(model.api_key_ciphertext.clone()),
            Some(model.api_key_nonce.clone()),
            Some(model.api_key_key_version),
        )?,
        envelope_from_parts(
            "proxy_url",
            model.proxy_url_ciphertext.clone(),
            model.proxy_url_nonce.clone(),
            model.proxy_url_key_version,
        )?,
    ))
}

pub(crate) fn endpoint_key_from_model(
    model: &standalone_endpoint_keys::Model,
) -> Result<(EndpointApiKeyConfig, EncryptedSecretEnvelope)> {
    Ok((
        EndpointApiKeyConfig {
            key_id: model_uuid(&model.key_id, "key_id")?,
            endpoint_id: model_uuid(&model.endpoint_id, "endpoint_id")?,
            key_label: model_required_string(&model.key_label, "key_label")?,
            api_key: String::new(),
            position: model.position,
            enabled: model_bool(model.enabled, "enabled")?,
            created_at: parse_timestamp(&model.created_at, "created_at")?,
            updated_at: parse_timestamp(&model.updated_at, "updated_at")?,
        },
        envelope_from_parts(
            "api_key",
            Some(model.api_key_ciphertext.clone()),
            Some(model.api_key_nonce.clone()),
            Some(model.api_key_key_version),
        )?
        .ok_or_else(|| {
            StandaloneConfigError::CorruptDatabase(
                "endpoint key is missing its secret envelope".to_string(),
            )
        })?,
    ))
}

pub(crate) type McpServerRow = (
    McpServer,
    EncryptedSecretEnvelope,
    EncryptedSecretEnvelope,
    Option<EncryptedSecretEnvelope>,
    Option<EncryptedSecretEnvelope>,
);

fn model_json(value: &str, column: &str) -> Result<serde_json::Value> {
    let value = model_required_string(value, column)?;
    serde_json::from_str(&value).map_err(StandaloneConfigError::Serialization)
}

pub(crate) fn mcp_server_from_model(model: &standalone_mcp_servers::Model) -> Result<McpServerRow> {
    let auth_mode = model.auth_mode.clone();
    Ok((
        McpServer {
            server_id: model_uuid(&model.server_id, "server_id")?,
            source_endpoint_id: model_optional_uuid(
                model.source_endpoint_id.as_deref(),
                "source_endpoint_id",
            )?,
            scope: model_required_string(&model.scope, "scope")?,
            owner_user_id: model.owner_user_id,
            name: model_required_string(&model.name, "name")?,
            aggregate_naming_mode: model_required_string(
                &model.aggregate_naming_mode,
                "aggregate_naming_mode",
            )?,
            transport: model_required_string(&model.transport, "transport")?,
            provider_kind: model.provider_kind.clone(),
            url: model.url.clone(),
            command: model.command.clone(),
            args: model_json(&model.args_json, "args_json")?,
            env_json: serde_json::Value::Null,
            bearer_tokens_json: serde_json::Value::Null,
            http_headers_json: model_json(&model.http_headers_json, "http_headers_json")?,
            auth_mode,
            basic_username: model.basic_username.clone(),
            basic_password: None,
            proxy_url: None,
            has_proxy_url: false,
            tool_filter_mode: model_required_string(&model.tool_filter_mode, "tool_filter_mode")?,
            allowed_tools: model_json(&model.allowed_tools_json, "allowed_tools_json")?,
            disabled_tools: model_json(&model.disabled_tools_json, "disabled_tools_json")?,
            disabled_resources: model_json(
                &model.disabled_resources_json,
                "disabled_resources_json",
            )?,
            enabled: model_bool(model.enabled, "enabled")?,
            timeout_ms: model.timeout_ms,
            lifecycle_policy: model_required_string(&model.lifecycle_policy, "lifecycle_policy")?,
            lifecycle_manual_protocol_version: model.lifecycle_manual_protocol_version.clone(),
            lifecycle_learned_mode: model.lifecycle_learned_mode.clone(),
            lifecycle_learned_protocol_version: model.lifecycle_learned_protocol_version.clone(),
            lifecycle_learned_for_updated_at: model_optional_timestamp(
                model.lifecycle_learned_for_updated_at.as_deref(),
                "lifecycle_learned_for_updated_at",
            )?,
            lifecycle_learned_at: model_optional_timestamp(
                model.lifecycle_learned_at.as_deref(),
                "lifecycle_learned_at",
            )?,
            created_at: parse_timestamp(&model.created_at, "created_at")?,
            updated_at: parse_timestamp(&model.updated_at, "updated_at")?,
        },
        required_envelope(
            "env",
            Some(model.env_ciphertext.clone()),
            Some(model.env_nonce.clone()),
            Some(model.env_key_version),
            "environment",
        )?,
        required_envelope(
            "bearer_tokens",
            Some(model.bearer_tokens_ciphertext.clone()),
            Some(model.bearer_tokens_nonce.clone()),
            Some(model.bearer_tokens_key_version),
            "bearer token",
        )?,
        envelope_from_parts(
            "basic_password",
            model.basic_password_ciphertext.clone(),
            model.basic_password_nonce.clone(),
            model.basic_password_key_version,
        )?,
        envelope_from_parts(
            "proxy_url",
            model.proxy_url_ciphertext.clone(),
            model.proxy_url_nonce.clone(),
            model.proxy_url_key_version,
        )?,
    ))
}

pub(crate) fn setting_from_model(model: &standalone_settings::Model) -> Result<SettingConfig> {
    let value = model_required_string(&model.value_json, "value_json")?;
    Ok(SettingConfig {
        key: model_required_string(&model.setting_key, "setting_key")?,
        version: model.value_version,
        value: serde_json::from_str(&value)?,
    })
}

fn sqlite_timestamp(row: &SqliteRow, column: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let value: String = row.try_get(column)?;
    parse_timestamp(&value, column)
}

fn parse_timestamp(value: &str, column: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(timestamp.with_timezone(&chrono::Utc));
    }
    let timestamp =
        chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").map_err(|error| {
            StandaloneConfigError::CorruptDatabase(format!(
                "column {column} is not a timestamp: {error}"
            ))
        })?;
    Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        timestamp,
        chrono::Utc,
    ))
}

pub(crate) fn route_from_model(model: &standalone_model_routes::Model) -> Result<ModelRouteConfig> {
    Ok(ModelRouteConfig {
        rule_id: model_uuid(&model.rule_id, "rule_id")?,
        scope: RouteScope::parse(&model_required_string(&model.scope, "scope")?)?,
        owner_user_id: model.owner_user_id,
        model_pattern: model_required_string(&model.model_pattern, "model_pattern")?,
        routing_strategy: RoutingStrategy::parse(&model_required_string(
            &model.routing_strategy,
            "routing_strategy",
        )?)?,
        enabled: model_bool(model.enabled, "enabled")?,
        targets: Vec::new(),
    })
}

pub(crate) fn route_target_from_model(
    model: &standalone_model_route_targets::Model,
) -> Result<(
    Uuid,
    ModelRouteTargetConfig,
    Option<EncryptedSecretEnvelope>,
)> {
    let proxy_envelope = envelope_from_parts(
        "proxy_url_override",
        model.proxy_url_override_ciphertext.clone(),
        model.proxy_url_override_nonce.clone(),
        model.proxy_url_override_key_version,
    )?;
    Ok((
        model_uuid(&model.rule_id, "rule_id")?,
        ModelRouteTargetConfig {
            target_id: model_uuid(&model.target_id, "target_id")?,
            endpoint_id: model_uuid(&model.endpoint_id, "endpoint_id")?,
            position: model.position,
            enabled: model_bool(model.enabled, "enabled")?,
            upstream_model: model.upstream_model.clone(),
            native_api: parse_native_api_lossy(&model.native_api),
            proxy_url_override: None,
            active_windows: model
                .active_windows
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            dev_system_normalize: model_bool(model.dev_system_normalize, "dev_system_normalize")?,
            thinking_downgrade_enabled: model_bool(
                model.thinking_downgrade_enabled,
                "thinking_downgrade_enabled",
            )?,
            thinking_effort_override: model
                .thinking_effort_override
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            compact_mode: {
                let trimmed = model.compact_mode.trim();
                if trimmed.is_empty() {
                    "passthrough".to_string()
                } else {
                    trimmed.to_string()
                }
            },
            service_tier: model
                .service_tier
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            embedding_dimensions_override: model.embedding_dimensions_override,
            prefer_new_sessions: model_bool(model.prefer_new_sessions, "prefer_new_sessions")?,
        },
        proxy_envelope,
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

pub(crate) fn client_key_from_model(
    model: &standalone_client_keys::Model,
) -> Result<(ClientKeyConfig, EncryptedSecretEnvelope)> {
    Ok((
        ClientKeyConfig {
            key_id: model_uuid(&model.key_id, "key_id")?,
            user_id: model.user_id,
            key_prefix: model_required_string(&model.key_prefix, "key_prefix")?,
            label: model_required_string(&model.label, "label")?,
            secret: String::new(),
            enabled: model_bool(model.enabled, "enabled")?,
        },
        envelope_from_parts(
            "secret",
            Some(model.secret_ciphertext.clone()),
            Some(model.secret_nonce.clone()),
            Some(model.secret_key_version),
        )?
        .ok_or_else(|| {
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
        "embeddings" => Ok(NativeApi::Embeddings),
        "responses" => Ok(NativeApi::Responses),
        "realtime" => Ok(NativeApi::Realtime),
        _ => Err(StandaloneConfigError::CorruptDatabase(format!(
            "unknown native API {value:?}"
        ))),
    }
}

// Issue #409 Phase 1: lossy parse for the 0023 `native_api` column.
// Unknown/empty values read as `Auto` so a corrupt row degrades to
// caller-following instead of failing the whole snapshot load.
fn parse_native_api_lossy(value: &str) -> NativeApi {
    match value.trim() {
        "auto" => NativeApi::Auto,
        "anthropic_messages" => NativeApi::AnthropicMessages,
        "chat" => NativeApi::Chat,
        "embeddings" => NativeApi::Embeddings,
        "responses" => NativeApi::Responses,
        "realtime" => NativeApi::Realtime,
        _ => NativeApi::Auto,
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
