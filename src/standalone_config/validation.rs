use std::collections::HashSet;

use uuid::Uuid;

use super::models::{
    BootstrapSeed, EndpointApiKeyConfig, EndpointProvider, ProviderEndpointConfig, Result,
    RouteScope, StandaloneConfig, StandaloneConfigError,
};
use crate::config::NativeApiSource;

impl StandaloneConfig {
    pub fn validate(&self) -> Result<()> {
        let mut endpoint_ids = HashSet::new();
        for endpoint in &self.endpoints {
            if !endpoint_ids.insert(endpoint.endpoint_id) {
                return invalid("endpoint_id", "must be unique");
            }
            required("endpoint name", &endpoint.name)?;
            // Issue #248: preset providers derive their official base
            // server-side and ignore any stored value; Generic keeps the
            // normalized stored base.
            let provider =
                crate::db::EndpointProvider::from_str_or_default(endpoint.provider.as_str());
            let region = endpoint.provider_region.map(|region| match region {
                super::models::EndpointRegion::Cn => crate::db::EndpointRegion::Cn,
                super::models::EndpointRegion::Global => crate::db::EndpointRegion::Global,
            });
            let normalized_base = match crate::upstream_presets::preset_base_url(
                provider.into(),
                region.map(|region| region.into()),
                endpoint.native_api.into(),
            ) {
                Some(derived) => derived.to_string(),
                None => {
                    required("endpoint base_url", &endpoint.base_url)?;
                    normalize_endpoint_base_url(&endpoint.base_url)
                }
            };
            if !normalized_base.starts_with("http://") && !normalized_base.starts_with("https://") {
                return invalid("base_url", "must use http:// or https://");
            }
            // Issue #1004 P1: an OpenAI endpoint may be stored without a
            // platform API key so the ChatGPT device login can be completed
            // first. Every other provider keeps the mandatory key.
            if provider != crate::db::EndpointProvider::OpenAi {
                required("endpoint api_key", &endpoint.api_key)?;
            }
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
            let mut preferred_targets = 0usize;
            for target in &route.targets {
                if target.prefer_new_sessions {
                    preferred_targets += 1;
                }
                if !endpoint_ids.contains(&target.endpoint_id) {
                    return invalid(
                        "target.endpoint_id",
                        "does not reference a configured endpoint",
                    );
                }
                if !target_ids.insert(target.target_id) {
                    return invalid("target_id", "must be unique per route");
                }
                // Issue #1032: the dimension override is a positive i32 or
                // inherit; never clamped.
                if target
                    .embedding_dimensions_override
                    .is_some_and(|dimensions| dimensions < 1)
                {
                    return invalid(
                        "embedding_dimensions_override",
                        "must be a positive integer",
                    );
                }
            }
            if route.targets.is_empty() {
                return invalid("targets", "must contain at least one target");
            }
            if preferred_targets > 1 {
                return invalid(
                    "prefer_new_sessions",
                    "at most one target may be preferred for new sessions",
                );
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
        required("upstream_base_url", &self.upstream_base_url)?;
        required("upstream_api_key", &self.upstream_api_key)?;
        let upstream_api_key = self.upstream_api_key;
        let endpoint_id = Uuid::new_v4();
        let endpoint_key_id = Uuid::new_v4();
        let config = StandaloneConfig {
            endpoints: vec![ProviderEndpointConfig {
                endpoint_id,
                name: "bootstrap-upstream".to_string(),
                provider: EndpointProvider::Generic,
                provider_region: None,
                service_tier: None,
                base_url: normalize_endpoint_base_url(&self.upstream_base_url),
                native_api: self.upstream_native_api,
                native_api_source: NativeApiSource::Manual,
                key_lb_enabled: false,
                enabled: true,
                mcp_enabled: false,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                api_key: upstream_api_key.clone(),
                proxy_url: None,
                active_windows: None,
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

/// Normalize a Generic endpoint `base_url` for the standalone config mirror.
///
/// Strips trailing `/v1` segments (and a chained `…/v1/v1`) so the saved
/// value matches the canonical API root. Preset providers derive their base
/// instead (issue #248), so the GLM exemption from issue #241 is gone.
pub(crate) fn normalize_endpoint_base_url(base_url: &str) -> String {
    let mut normalized = base_url.trim().to_string();
    loop {
        normalized = normalized.trim_end_matches('/').to_string();
        if let Some(stripped) = normalized.strip_suffix("/v1") {
            normalized = stripped.to_string();
        } else {
            break;
        }
    }
    normalized
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

#[cfg(test)]
mod tests {
    use super::{EndpointProvider, normalize_endpoint_base_url};

    #[test]
    fn normalize_strips_trailing_v1_chain_but_keeps_case_variants() {
        for (input, expected) in [
            ("https://api.openai.com", "https://api.openai.com"),
            ("https://api.openai.com/", "https://api.openai.com"),
            ("https://api.openai.com/v1", "https://api.openai.com"),
            ("https://api.openai.com/v1/", "https://api.openai.com"),
            ("https://api.openai.com/v1/v1", "https://api.openai.com"),
            ("  https://api.openai.com/v1  ", "https://api.openai.com"),
            (
                "https://api.commandcode.ai/provider/v1",
                "https://api.commandcode.ai/provider",
            ),
            ("https://openrouter.ai/api/v1", "https://openrouter.ai/api"),
            (
                "https://api.commandcode.ai/provider",
                "https://api.commandcode.ai/provider",
            ),
        ] {
            assert_eq!(
                normalize_endpoint_base_url(input),
                expected,
                "input {input:?}"
            );
        }
        assert_eq!(
            normalize_endpoint_base_url("https://api.openai.com/V1"),
            "https://api.openai.com/V1"
        );
        assert_eq!(
            normalize_endpoint_base_url("https://api.openai.com/v10"),
            "https://api.openai.com/v10"
        );
    }

    #[test]
    fn preset_providers_derive_a_valid_base_without_a_stored_one() {
        // Issue #248: a preset row with an empty stored base still validates
        // because the base is derived from the provider/protocol.
        let provider =
            crate::db::EndpointProvider::from_str_or_default(EndpointProvider::Glm.as_str());
        let derived = crate::upstream_presets::preset_base_url(
            provider.into(),
            None,
            crate::config::NativeApi::Responses.into(),
        );
        assert_eq!(
            derived,
            Some(crate::upstream_presets::GLM_RESPONSES_BASE_URL)
        );
        // Issue #589: OpenAI derives its official root the same way.
        let provider =
            crate::db::EndpointProvider::from_str_or_default(EndpointProvider::OpenAi.as_str());
        let derived = crate::upstream_presets::preset_base_url(
            provider.into(),
            None,
            crate::config::NativeApi::Responses.into(),
        );
        assert_eq!(derived, Some(crate::upstream_presets::OPENAI_BASE_URL));
    }
}
