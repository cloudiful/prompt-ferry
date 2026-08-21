use std::collections::HashSet;

use uuid::Uuid;

use super::models::{
    BootstrapSeed, EndpointApiKeyConfig, EndpointProvider, ManagedRelayConfig,
    ProviderEndpointConfig, Result, RouteScope, StandaloneConfig, StandaloneConfigError,
};
use crate::config::{NativeApiSource, TlsMode};

impl StandaloneConfig {
    pub fn validate(&self) -> Result<()> {
        let mut relay_ids = HashSet::new();
        for relay in &self.relays {
            if !relay_ids.insert(relay.relay_id) {
                return invalid("relay_id", "must be unique");
            }
            required("relay name", &relay.name)?;
            required("relay URL", &relay.relay_url)?;
            if !relay.relay_url.starts_with("ws://") && !relay.relay_url.starts_with("wss://") {
                return invalid("relay_url", "must use ws:// or wss://");
            }
            if relay.tls_mode == TlsMode::Off && relay.relay_url.starts_with("wss://") {
                return invalid("relay_url", "wss:// requires TLS mode server or mtls");
            }
            if relay.tls_mode != TlsMode::Off && relay.relay_url.starts_with("ws://") {
                return invalid("relay_url", "TLS mode server or mtls requires wss://");
            }
            for (field, secret) in [
                ("relay_ca_pem", relay.relay_ca_pem.as_deref()),
                ("client_cert_pem", relay.client_cert_pem.as_deref()),
                ("client_key_pem", relay.client_key_pem.as_deref()),
                (
                    "bridge_encryption_key",
                    relay.bridge_encryption_key.as_deref(),
                ),
            ] {
                if secret.is_some_and(|value| value.is_empty()) {
                    return invalid(field, "must not be empty when present");
                }
            }
        }

        let mut endpoint_ids = HashSet::new();
        for endpoint in &self.endpoints {
            if !endpoint_ids.insert(endpoint.endpoint_id) {
                return invalid("endpoint_id", "must be unique");
            }
            required("endpoint name", &endpoint.name)?;
            required("endpoint base_url", &endpoint.base_url)?;
            if !endpoint.base_url.starts_with("http://")
                && !endpoint.base_url.starts_with("https://")
            {
                return invalid("base_url", "must use http:// or https://");
            }
            required("endpoint api_key", &endpoint.api_key)?;
            let mut key_ids = HashSet::new();
            for key in &endpoint.api_keys {
                if !key_ids.insert(key.key_id) {
                    return invalid("endpoint key_id", "must be unique per endpoint");
                }
                required("endpoint key_label", &key.key_label)?;
                required("endpoint api_key", &key.api_key)?;
            }
        }

        let mut route_ids = HashSet::new();
        for route in &self.routes {
            if !route_ids.insert(route.rule_id) {
                return invalid("rule_id", "must be unique");
            }
            required("model_pattern", &route.model_pattern)?;
            match route.scope {
                RouteScope::Admin if route.owner_user_id.is_some() => {
                    return invalid("owner_user_id", "admin routes cannot have an owner");
                }
                RouteScope::User if route.owner_user_id.is_none() => {
                    return invalid("owner_user_id", "user routes require an owner");
                }
                _ => {}
            }
            let mut target_ids = HashSet::new();
            for target in &route.targets {
                if !endpoint_ids.contains(&target.endpoint_id) {
                    return invalid(
                        "target.endpoint_id",
                        "does not reference a configured endpoint",
                    );
                }
                if !target_ids.insert(target.target_id) {
                    return invalid("target_id", "must be unique per route");
                }
            }
            if route.targets.is_empty() {
                return invalid("targets", "must contain at least one target");
            }
        }

        let mut client_key_ids = HashSet::new();
        for key in &self.client_keys {
            if !client_key_ids.insert(key.key_id) {
                return invalid("client key_id", "must be unique");
            }
            required("client key secret", &key.secret)?;
            required("client key label", &key.label)?;
        }

        let mut setting_keys = HashSet::new();
        for setting in &self.settings {
            if !setting_keys.insert(&setting.key) {
                return invalid("setting key", "must be unique");
            }
            required("setting key", &setting.key)?;
            if setting.version < 1 {
                return invalid("setting version", "must be positive");
            }
        }
        Ok(())
    }
}

impl BootstrapSeed {
    pub fn into_config(self) -> Result<StandaloneConfig> {
        if self.relay_urls.is_empty() {
            return invalid("relay_urls", "at least one relay URL is required");
        }
        required("upstream_base_url", &self.upstream_base_url)?;
        required("upstream_api_key", &self.upstream_api_key)?;
        let upstream_api_key = self.upstream_api_key;
        let endpoint_id = Uuid::new_v4();
        let endpoint_key_id = Uuid::new_v4();
        let mut relays = Vec::with_capacity(self.relay_urls.len());
        for (index, relay_url) in self.relay_urls.into_iter().enumerate() {
            relays.push(ManagedRelayConfig {
                relay_id: Uuid::new_v4(),
                name: format!("bootstrap-relay-{}", index + 1),
                relay_url: relay_url.trim().trim_end_matches('/').to_string(),
                enabled: true,
                tls_mode: self.tls_mode,
                bridge_encryption_mode: self.bridge_encryption_mode,
                relay_ca_pem: self.relay_ca_pem.clone(),
                client_cert_pem: self.client_cert_pem.clone(),
                client_key_pem: self.client_key_pem.clone(),
                bridge_encryption_key: self.bridge_encryption_key.clone(),
            });
        }
        let config = StandaloneConfig {
            relays,
            endpoints: vec![ProviderEndpointConfig {
                endpoint_id,
                name: "bootstrap-upstream".to_string(),
                provider: EndpointProvider::Generic,
                provider_region: None,
                base_url: self.upstream_base_url.trim_end_matches('/').to_string(),
                native_api: self.upstream_native_api,
                native_api_source: NativeApiSource::Manual,
                key_lb_enabled: false,
                enabled: true,
                mcp_enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                api_key: upstream_api_key.clone(),
                api_keys: vec![EndpointApiKeyConfig {
                    key_id: endpoint_key_id,
                    endpoint_id,
                    key_label: "bootstrap".to_string(),
                    api_key: upstream_api_key,
                    position: 0,
                    enabled: true,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                }],
            }],
            ..StandaloneConfig::default()
        };
        config.validate()?;
        Ok(config)
    }
}

fn required(field: &'static str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        invalid(field, "must not be empty")
    } else {
        Ok(())
    }
}

fn invalid<T>(field: &'static str, message: impl Into<String>) -> Result<T> {
    Err(StandaloneConfigError::InvalidInput {
        field,
        message: message.into(),
    })
}
