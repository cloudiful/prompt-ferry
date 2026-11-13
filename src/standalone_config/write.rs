use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use sqlx::Sqlite;
use uuid::Uuid;

use crate::db::entities::sqlite::standalone_model_route_targets::{
    ActiveModel as TargetActiveModel, Column as TargetColumn, Entity as TargetEntity,
};
use crate::db::entities::sqlite::standalone_model_routes::{
    ActiveModel as RouteActiveModel, Column as RouteColumn, Entity as RouteEntity,
};
use crate::db::seaorm::SqliteNativeConnection;

use super::{
    ClientKeyConfig, EndpointApiKeyConfig, ManagedRelayConfig, ModelRouteConfig,
    ModelRouteTargetConfig, ProviderEndpointConfig, Result, SettingConfig, StandaloneConfig,
    StandaloneConfigError,
};
use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

pub(crate) struct EncryptedRelay {
    relay: ManagedRelayConfig,
    relay_ca: Option<EncryptedSecretEnvelope>,
    client_cert: Option<EncryptedSecretEnvelope>,
    client_key: Option<EncryptedSecretEnvelope>,
    bridge_encryption_key: Option<EncryptedSecretEnvelope>,
}

pub(crate) struct EncryptedEndpoint {
    endpoint: ProviderEndpointConfig,
    api_key: EncryptedSecretEnvelope,
    proxy_url: Option<EncryptedSecretEnvelope>,
    api_keys: Vec<(EndpointApiKeyConfig, EncryptedSecretEnvelope)>,
}

pub(crate) struct EncryptedRouteTarget {
    target: super::ModelRouteTargetConfig,
    proxy_url_override: Option<EncryptedSecretEnvelope>,
}

pub(crate) struct EncryptedRoute {
    route: ModelRouteConfig,
    targets: Vec<EncryptedRouteTarget>,
}

pub(crate) struct EncryptedClientKey {
    client_key: ClientKeyConfig,
    secret: EncryptedSecretEnvelope,
}

pub(crate) struct EncryptedConfig {
    pub(crate) relays: Vec<EncryptedRelay>,
    pub(crate) endpoints: Vec<EncryptedEndpoint>,
    routes: Vec<EncryptedRoute>,
    pub(crate) client_keys: Vec<EncryptedClientKey>,
    settings: Vec<SettingConfig>,
}

impl EncryptedConfig {
    pub(crate) fn from_snapshot(
        manager: &RelaySecretManager,
        snapshot: &StandaloneConfig,
    ) -> Result<Self> {
        Ok(Self {
            relays: snapshot
                .relays
                .iter()
                .map(|relay| {
                    Ok(EncryptedRelay {
                        relay: relay.clone(),
                        relay_ca: encrypt_optional(manager, relay.relay_ca_pem.as_deref())?,
                        client_cert: encrypt_optional(manager, relay.client_cert_pem.as_deref())?,
                        client_key: encrypt_optional(manager, relay.client_key_pem.as_deref())?,
                        bridge_encryption_key: encrypt_optional(
                            manager,
                            relay.bridge_encryption_key.as_deref(),
                        )?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            endpoints: snapshot
                .endpoints
                .iter()
                .map(|endpoint| {
                    Ok(EncryptedEndpoint {
                        endpoint: endpoint.clone(),
                        api_key: manager.encrypt(&endpoint.api_key)?,
                        proxy_url: encrypt_optional(
                            manager,
                            endpoint
                                .proxy_url
                                .as_deref()
                                .map(str::trim)
                                .filter(|v| !v.is_empty()),
                        )?,
                        api_keys: endpoint
                            .api_keys
                            .iter()
                            .map(|key| Ok((key.clone(), manager.encrypt(&key.api_key)?)))
                            .collect::<anyhow::Result<Vec<_>>>()
                            .map_err(StandaloneConfigError::Encryption)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            routes: snapshot
                .routes
                .iter()
                .map(|route| {
                    Ok(EncryptedRoute {
                        route: ModelRouteConfig {
                            targets: Vec::new(),
                            ..route.clone()
                        },
                        targets: route
                            .targets
                            .iter()
                            .map(|target| {
                                Ok(EncryptedRouteTarget {
                                    target: target.clone(),
                                    proxy_url_override: encrypt_optional(
                                        manager,
                                        target
                                            .proxy_url_override
                                            .as_deref()
                                            .map(str::trim)
                                            .filter(|v| !v.is_empty()),
                                    )?,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            client_keys: snapshot
                .client_keys
                .iter()
                .map(|key| {
                    Ok(EncryptedClientKey {
                        client_key: key.clone(),
                        secret: manager.encrypt(&key.secret)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            settings: snapshot.settings.clone(),
        })
    }
}

pub(crate) async fn delete_all(transaction: &mut sqlx::Transaction<'_, Sqlite>) -> Result<()> {
    {
        let connection = SqliteNativeConnection::new(&mut **transaction);
        delete_route_targets(&connection, None).await?;
        RouteEntity::delete_many()
            .exec(&connection)
            .await
            .map_err(crate::standalone_config::rows::native_error)?;
    }
    standalone_query!("src/sql/standalone/delete_endpoint_keys_all.sql")
        .execute(&mut **transaction)
        .await?;
    standalone_query!("src/sql/standalone/delete_client_keys.sql")
        .execute(&mut **transaction)
        .await?;
    standalone_query!("src/sql/standalone/delete_endpoints.sql")
        .execute(&mut **transaction)
        .await?;
    standalone_query!("src/sql/standalone/delete_relays.sql")
        .execute(&mut **transaction)
        .await?;
    standalone_query!("src/sql/standalone/delete_settings.sql")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(crate) async fn insert_all(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    config: &EncryptedConfig,
) -> Result<()> {
    for relay in &config.relays {
        insert_relay(transaction, relay).await?;
    }
    for endpoint in &config.endpoints {
        insert_endpoint(transaction, endpoint).await?;
    }
    for route in &config.routes {
        insert_encrypted_route(transaction, route).await?;
    }
    for key in &config.client_keys {
        insert_client_key(transaction, key).await?;
    }
    for setting in &config.settings {
        standalone_query!("src/sql/standalone/save_setting.sql")
            .bind(&setting.key)
            .bind(setting.version)
            .bind(serde_json::to_string(&setting.value)?)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

pub(crate) async fn insert_relay(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    relay: &EncryptedRelay,
) -> Result<()> {
    let query = standalone_query!("src/sql/standalone/save_relay.sql");
    query
        .bind(relay.relay.relay_id.to_string())
        .bind(&relay.relay.name)
        .bind(&relay.relay.relay_url)
        .bind(bool_i64(relay.relay.enabled))
        .bind(relay.relay.tls_mode.as_str())
        .bind(relay.relay.bridge_encryption_mode.as_str())
        .bind(envelope_part(&relay.relay_ca, EnvelopePart::Ciphertext))
        .bind(envelope_part(&relay.relay_ca, EnvelopePart::Nonce))
        .bind(envelope_version(&relay.relay_ca))
        .bind(envelope_part(&relay.client_cert, EnvelopePart::Ciphertext))
        .bind(envelope_part(&relay.client_cert, EnvelopePart::Nonce))
        .bind(envelope_version(&relay.client_cert))
        .bind(envelope_part(&relay.client_key, EnvelopePart::Ciphertext))
        .bind(envelope_part(&relay.client_key, EnvelopePart::Nonce))
        .bind(envelope_version(&relay.client_key))
        .bind(envelope_part(
            &relay.bridge_encryption_key,
            EnvelopePart::Ciphertext,
        ))
        .bind(envelope_part(
            &relay.bridge_encryption_key,
            EnvelopePart::Nonce,
        ))
        .bind(envelope_version(&relay.bridge_encryption_key))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

pub(crate) async fn insert_endpoint(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    endpoint: &EncryptedEndpoint,
) -> Result<()> {
    standalone_query!("src/sql/standalone/save_endpoint.sql")
        .bind(endpoint.endpoint.endpoint_id.to_string())
        .bind(&endpoint.endpoint.name)
        .bind(endpoint.endpoint.provider.as_str())
        .bind(
            endpoint
                .endpoint
                .provider_region
                .map(|value| value.as_str()),
        )
        .bind(endpoint.endpoint.service_tier.as_deref())
        .bind(&endpoint.endpoint.base_url)
        .bind(endpoint.endpoint.native_api.as_str())
        .bind(endpoint.endpoint.native_api_source.as_str())
        .bind(bool_i64(endpoint.endpoint.key_lb_enabled))
        .bind(bool_i64(endpoint.endpoint.enabled))
        .bind(bool_i64(endpoint.endpoint.mcp_enabled))
        .bind(envelope_part(
            &Some(endpoint.api_key.clone()),
            EnvelopePart::Ciphertext,
        ))
        .bind(envelope_part(
            &Some(endpoint.api_key.clone()),
            EnvelopePart::Nonce,
        ))
        .bind(envelope_version(&Some(endpoint.api_key.clone())))
        .bind(envelope_part(&endpoint.proxy_url, EnvelopePart::Ciphertext))
        .bind(envelope_part(&endpoint.proxy_url, EnvelopePart::Nonce))
        .bind(envelope_version(&endpoint.proxy_url))
        // Issue #392 Phase K: plaintext schedule; empty normalizes to NULL.
        .bind(
            endpoint
                .endpoint
                .active_windows
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
        )
        .bind(timestamp(endpoint.endpoint.created_at))
        .bind(timestamp(endpoint.endpoint.updated_at))
        .execute(&mut **transaction)
        .await?;
    for (key, envelope) in &endpoint.api_keys {
        standalone_query!("src/sql/standalone/save_endpoint_key.sql")
            .bind(key.key_id.to_string())
            .bind(endpoint.endpoint.endpoint_id.to_string())
            .bind(&key.key_label)
            .bind(bool_i64(key.enabled))
            .bind(key.position)
            .bind(envelope_part(
                &Some(envelope.clone()),
                EnvelopePart::Ciphertext,
            ))
            .bind(envelope_part(&Some(envelope.clone()), EnvelopePart::Nonce))
            .bind(envelope_version(&Some(envelope.clone())))
            .bind(timestamp(key.created_at))
            .bind(timestamp(key.updated_at))
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

/// Inserts or replaces one route and its targets on a native connection.
pub(crate) async fn insert_route<C: ConnectionTrait>(
    connection: &C,
    manager: &RelaySecretManager,
    route: &ModelRouteConfig,
) -> Result<()> {
    let mut targets = Vec::with_capacity(route.targets.len());
    for target in &route.targets {
        targets.push((
            target.clone(),
            encrypt_optional(
                manager,
                target
                    .proxy_url_override
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty()),
            )?,
        ));
    }
    insert_route_rows(connection, &route, &targets).await
}

/// Inserts or replaces one route and its already encrypted targets on the
/// shared configuration transaction.
pub(crate) async fn insert_encrypted_route(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    encrypted: &EncryptedRoute,
) -> Result<()> {
    let connection = SqliteNativeConnection::new(&mut **transaction);
    let targets = encrypted
        .targets
        .iter()
        .map(|target| (target.target.clone(), target.proxy_url_override.clone()))
        .collect::<Vec<_>>();
    insert_route_rows(&connection, &encrypted.route, &targets).await
}

async fn insert_route_rows<C: ConnectionTrait>(
    connection: &C,
    route: &ModelRouteConfig,
    targets: &[(ModelRouteTargetConfig, Option<EncryptedSecretEnvelope>)],
) -> Result<()> {
    upsert_route(connection, route).await?;
    for (target, proxy_envelope) in targets {
        upsert_route_target(connection, route.rule_id, target, proxy_envelope.as_ref()).await?;
    }
    Ok(())
}

/// Inserts or replaces the route row itself.
pub(crate) async fn upsert_route<C: ConnectionTrait>(
    connection: &C,
    route: &ModelRouteConfig,
) -> Result<()> {
    RouteEntity::insert(RouteActiveModel {
        rule_id: Set(route.rule_id.to_string()),
        scope: Set(route.scope.as_str().to_string()),
        owner_user_id: Set(route.owner_user_id),
        model_pattern: Set(route.model_pattern.clone()),
        routing_strategy: Set(route.routing_strategy.as_str().to_string()),
        enabled: Set(i32::from(route.enabled)),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(RouteColumn::RuleId)
            .update_columns([
                RouteColumn::Scope,
                RouteColumn::OwnerUserId,
                RouteColumn::ModelPattern,
                RouteColumn::RoutingStrategy,
                RouteColumn::Enabled,
            ])
            .value(RouteColumn::UpdatedAt, Expr::current_timestamp())
            .to_owned(),
    )
    .exec(connection)
    .await
    .map_err(crate::standalone_config::rows::native_error)?;
    Ok(())
}

/// Inserts or replaces one route target.
pub(crate) async fn upsert_route_target<C: ConnectionTrait>(
    connection: &C,
    rule_id: Uuid,
    target: &ModelRouteTargetConfig,
    proxy_envelope: Option<&EncryptedSecretEnvelope>,
) -> Result<()> {
    // Issue #378 Phase I: plaintext schedule; empty normalizes to NULL.
    let active_windows = target
        .active_windows
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    TargetEntity::insert(TargetActiveModel {
        target_id: Set(target.target_id.to_string()),
        rule_id: Set(rule_id.to_string()),
        endpoint_id: Set(target.endpoint_id.to_string()),
        position: Set(target.position),
        enabled: Set(i32::from(target.enabled)),
        upstream_model: Set(target.upstream_model.clone()),
        native_api: Set(target.native_api.as_str().to_string()),
        proxy_url_override_ciphertext: Set(
            proxy_envelope.map(|envelope| envelope.ciphertext.clone())
        ),
        proxy_url_override_nonce: Set(proxy_envelope.map(|envelope| envelope.nonce.clone())),
        proxy_url_override_key_version: Set(
            proxy_envelope.map(|envelope| i32::from(envelope.key_version))
        ),
        active_windows: Set(active_windows),
        dev_system_normalize: Set(i32::from(target.dev_system_normalize)),
        thinking_effort_override: Set(target
            .thinking_effort_override
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)),
        // Issue #502 Task 5: empty normalizes to `passthrough`.
        compact_mode: Set({
            let trimmed = target.compact_mode.trim();
            if trimmed.is_empty() {
                "passthrough".to_string()
            } else {
                trimmed.to_string()
            }
        }),
        thinking_downgrade_enabled: Set(i32::from(target.thinking_downgrade_enabled)),
        // Issue #637: free-form override; blank means inherit (NULL).
        service_tier: Set(target.service_tier.clone()),
        // Issue #1032: nullable positive dimension override; `None`
        // inherits the caller's dimensions.
        embedding_dimensions_override: Set(target.embedding_dimensions_override),
        // Manual new-session preference, default off.
        prefer_new_sessions: Set(i32::from(target.prefer_new_sessions)),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(TargetColumn::TargetId)
            .update_columns([
                TargetColumn::RuleId,
                TargetColumn::EndpointId,
                TargetColumn::Position,
                TargetColumn::Enabled,
                TargetColumn::UpstreamModel,
                TargetColumn::NativeApi,
                TargetColumn::ProxyUrlOverrideCiphertext,
                TargetColumn::ProxyUrlOverrideNonce,
                TargetColumn::ProxyUrlOverrideKeyVersion,
                TargetColumn::ActiveWindows,
                TargetColumn::DevSystemNormalize,
                TargetColumn::ThinkingEffortOverride,
                TargetColumn::CompactMode,
                TargetColumn::ThinkingDowngradeEnabled,
                TargetColumn::ServiceTier,
                TargetColumn::EmbeddingDimensionsOverride,
                TargetColumn::PreferNewSessions,
            ])
            .value(TargetColumn::UpdatedAt, Expr::current_timestamp())
            .to_owned(),
    )
    .exec(connection)
    .await
    .map_err(crate::standalone_config::rows::native_error)?;
    Ok(())
}

/// Removes the targets of one route, or every target when no rule is given.
pub(crate) async fn delete_route_targets<C: ConnectionTrait>(
    connection: &C,
    rule_id: Option<Uuid>,
) -> Result<()> {
    let mut delete = TargetEntity::delete_many();
    if let Some(rule_id) = rule_id {
        delete = delete.filter(TargetColumn::RuleId.eq(rule_id.to_string()));
    }
    delete
        .exec(connection)
        .await
        .map_err(crate::standalone_config::rows::native_error)?;
    Ok(())
}

pub(crate) async fn insert_client_key(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    key: &EncryptedClientKey,
) -> Result<()> {
    standalone_query!("src/standalone_config/sql/users/ensure_client_key_user.sql")
        .bind(key.client_key.user_id)
        .execute(&mut **transaction)
        .await?;
    standalone_query!("src/sql/standalone/save_client_key.sql")
        .bind(key.client_key.key_id.to_string())
        .bind(key.client_key.user_id)
        .bind(&key.client_key.key_prefix)
        .bind(&key.client_key.label)
        .bind(bool_i64(key.client_key.enabled))
        .bind(envelope_part(
            &Some(key.secret.clone()),
            EnvelopePart::Ciphertext,
        ))
        .bind(envelope_part(
            &Some(key.secret.clone()),
            EnvelopePart::Nonce,
        ))
        .bind(envelope_version(&Some(key.secret.clone())))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn encrypt_optional(
    manager: &RelaySecretManager,
    value: Option<&str>,
) -> Result<Option<EncryptedSecretEnvelope>> {
    value
        .map(|value| {
            manager
                .encrypt(value)
                .map_err(StandaloneConfigError::Encryption)
        })
        .transpose()
}

pub(crate) fn decrypt_optional(
    manager: &RelaySecretManager,
    envelope: Option<&EncryptedSecretEnvelope>,
) -> Result<Option<String>> {
    envelope
        .map(|value| {
            manager
                .decrypt(value)
                .map_err(StandaloneConfigError::Encryption)
        })
        .transpose()
}

fn bool_i64(value: bool) -> i64 {
    i64::from(value)
}

fn timestamp(value: chrono::DateTime<chrono::Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
}

#[derive(Clone, Copy)]
pub(crate) enum EnvelopePart {
    Ciphertext,
    Nonce,
}

pub(crate) fn envelope_part(
    envelope: &Option<EncryptedSecretEnvelope>,
    part: EnvelopePart,
) -> Option<Vec<u8>> {
    envelope.as_ref().map(|value| match part {
        EnvelopePart::Ciphertext => value.ciphertext.clone(),
        EnvelopePart::Nonce => value.nonce.clone(),
    })
}

pub(crate) fn envelope_version(envelope: &Option<EncryptedSecretEnvelope>) -> Option<i64> {
    envelope.as_ref().map(|value| i64::from(value.key_version))
}
