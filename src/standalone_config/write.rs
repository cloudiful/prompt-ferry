use sqlx::Sqlite;

use super::{
    ClientKeyConfig, EndpointApiKeyConfig, ManagedRelayConfig, ModelRouteConfig,
    ProviderEndpointConfig, Result, SettingConfig, StandaloneConfig, StandaloneConfigError,
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
    api_keys: Vec<(EndpointApiKeyConfig, EncryptedSecretEnvelope)>,
}

pub(crate) struct EncryptedClientKey {
    client_key: ClientKeyConfig,
    secret: EncryptedSecretEnvelope,
}

pub(crate) struct EncryptedConfig {
    pub(crate) relays: Vec<EncryptedRelay>,
    pub(crate) endpoints: Vec<EncryptedEndpoint>,
    routes: Vec<ModelRouteConfig>,
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
                        api_keys: endpoint
                            .api_keys
                            .iter()
                            .map(|key| Ok((key.clone(), manager.encrypt(&key.api_key)?)))
                            .collect::<anyhow::Result<Vec<_>>>()
                            .map_err(StandaloneConfigError::Encryption)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            routes: snapshot.routes.clone(),
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
    standalone_query!("src/sql/standalone/delete_route_targets_all.sql")
        .execute(&mut **transaction)
        .await?;
    standalone_query!("src/sql/standalone/delete_routes.sql")
        .execute(&mut **transaction)
        .await?;
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
        insert_route(transaction, route).await?;
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
        .bind(endpoint.endpoint.service_tier.as_str())
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

pub(crate) async fn insert_route(
    transaction: &mut sqlx::Transaction<'_, Sqlite>,
    route: &ModelRouteConfig,
) -> Result<()> {
    standalone_query!("src/sql/standalone/save_route.sql")
        .bind(route.rule_id.to_string())
        .bind(route.scope.as_str())
        .bind(route.owner_user_id)
        .bind(&route.model_pattern)
        .bind(route.routing_strategy.as_str())
        .bind(route.daily_max_requests)
        .bind(route.monthly_max_requests)
        .bind(bool_i64(route.enabled))
        .execute(&mut **transaction)
        .await?;
    for target in &route.targets {
        standalone_query!("src/sql/standalone/save_route_target.sql")
            .bind(target.target_id.to_string())
            .bind(route.rule_id.to_string())
            .bind(target.endpoint_id.to_string())
            .bind(target.position)
            .bind(bool_i64(target.enabled))
            .bind(&target.upstream_model)
            .bind(target.responses_continuation_policy.as_str())
            .execute(&mut **transaction)
            .await?;
    }
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
enum EnvelopePart {
    Ciphertext,
    Nonce,
}

fn envelope_part(
    envelope: &Option<EncryptedSecretEnvelope>,
    part: EnvelopePart,
) -> Option<Vec<u8>> {
    envelope.as_ref().map(|value| match part {
        EnvelopePart::Ciphertext => value.ciphertext.clone(),
        EnvelopePart::Nonce => value.nonce.clone(),
    })
}

fn envelope_version(envelope: &Option<EncryptedSecretEnvelope>) -> Option<i64> {
    envelope.as_ref().map(|value| i64::from(value.key_version))
}
