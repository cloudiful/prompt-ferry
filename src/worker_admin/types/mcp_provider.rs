//! Read-only MCP provider registry descriptor for the admin API (issue #296
//! Phase 2). The frontend consumes this endpoint instead of hardcoding preset
//! ids, display names, default URLs, auth styles, or usage units.

use serde::Serialize;
use utoipa::ToSchema;

use crate::db;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct McpProviderDescriptor {
    pub id: String,
    pub display_name: String,
    /// Official hosted endpoint for the preset; `null` for generic/minimax.
    pub default_url: Option<String>,
    /// `none` or `bearer`.
    pub auth: String,
    /// Usage unit used by quota groups and usage views: `requests`/`credits`.
    pub unit: String,
    pub provider_balance_supported: bool,
}

/// Provider registry as an ordered, serializable descriptor list.
pub fn mcp_provider_descriptors() -> Vec<McpProviderDescriptor> {
    db::MCP_PROVIDER_REGISTRY
        .iter()
        .map(|info| McpProviderDescriptor {
            id: info.id.to_string(),
            display_name: info.display_name.to_string(),
            default_url: info.default_url.map(str::to_string),
            auth: match info.auth {
                db::McpProviderAuth::None => "none".to_string(),
                db::McpProviderAuth::Bearer => "bearer".to_string(),
            },
            unit: match info.unit {
                db::McpProviderUnit::Requests => "requests".to_string(),
                db::McpProviderUnit::Credits => "credits".to_string(),
            },
            provider_balance_supported: info.provider_balance_supported,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_cover_registry_with_hosted_endpoints() {
        let descriptors = mcp_provider_descriptors();
        assert_eq!(descriptors.len(), db::MCP_PROVIDER_REGISTRY.len());

        let context7 = descriptors
            .iter()
            .find(|descriptor| descriptor.id == "context7")
            .expect("context7 descriptor");
        assert_eq!(context7.display_name, "Context7");
        assert_eq!(
            context7.default_url.as_deref(),
            Some("https://mcp.context7.com/mcp")
        );
        assert_eq!(context7.auth, "bearer");
        assert_eq!(context7.unit, "requests");
        assert!(!context7.provider_balance_supported);

        let firecrawl = descriptors
            .iter()
            .find(|descriptor| descriptor.id == "firecrawl")
            .expect("firecrawl descriptor");
        assert_eq!(
            firecrawl.default_url.as_deref(),
            Some("https://mcp.firecrawl.dev/v2/mcp")
        );
        assert_eq!(firecrawl.unit, "credits");
        assert!(firecrawl.provider_balance_supported);

        let generic = descriptors
            .iter()
            .find(|descriptor| descriptor.id == "generic")
            .expect("generic descriptor");
        assert!(generic.default_url.is_none());
        assert_eq!(generic.auth, "none");
    }
}
