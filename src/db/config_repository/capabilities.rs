//! Per-path capability gating for the unified admin API.
//!
//! The router middleware uses `Capability::for_path` to map a request path to
//! a backend capability and `sqlite_supported` to decide whether SQLite is
//! ready to serve it. Phase 3 turns on the core configuration domains; later
//! phases expand the set.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Endpoints,
    ModelRoutes,
    ModelRouteTest,
    Relays,
    ClientKeys,
    Settings,
    EndpointSetting,
    McpServers,
    McpCredentials,
    McpCatalog,
    McpQuota,
    ConversationEndpointOverride,
    AvailableModels,
    SnapshotPublication,
}

impl Capability {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Endpoints => "sqlite_endpoints_unavailable",
            Self::ModelRoutes => "sqlite_model_routes_unavailable",
            Self::ModelRouteTest => "sqlite_model_route_test_unavailable",
            Self::Relays => "sqlite_relays_unavailable",
            Self::ClientKeys => "sqlite_client_keys_unavailable",
            Self::Settings => "sqlite_settings_unavailable",
            Self::EndpointSetting => "sqlite_endpoint_setting_unavailable",
            Self::McpServers => "sqlite_mcp_servers_unavailable",
            Self::McpCredentials => "sqlite_mcp_credentials_unavailable",
            Self::McpCatalog => "sqlite_mcp_catalog_unavailable",
            Self::McpQuota => "sqlite_mcp_quota_unavailable",
            Self::ConversationEndpointOverride => {
                "sqlite_conversation_endpoint_override_unavailable"
            }
            Self::AvailableModels => "sqlite_available_models_unavailable",
            Self::SnapshotPublication => "sqlite_snapshot_publication_unavailable",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Endpoints => "endpoint CRUD is not yet available on SQLite",
            Self::ModelRoutes => "model route CRUD is not yet available on SQLite",
            Self::ModelRouteTest => "model route resolution test is not yet available on SQLite",
            Self::Relays => "managed relay CRUD is not yet available on SQLite",
            Self::ClientKeys => "client key CRUD is not yet available on SQLite",
            Self::Settings => "settings CRUD is not yet available on SQLite",
            Self::EndpointSetting => "user endpoint setting is not yet available on SQLite",
            Self::McpServers => "MCP server configuration is not yet available on SQLite",
            Self::McpCredentials => "MCP credential configuration is not yet available on SQLite",
            Self::McpCatalog => "MCP catalog probing is not available on SQLite",
            Self::McpQuota => "MCP quota and usage ledgers are not available on SQLite",
            Self::ConversationEndpointOverride => {
                "conversation endpoint overrides are not yet available on SQLite"
            }
            Self::AvailableModels => "available model discovery is not yet available on SQLite",
            Self::SnapshotPublication => "snapshot publication is not yet available on SQLite",
        }
    }

    pub fn sqlite_supported(self) -> bool {
        // Phase 3: the core configuration domains are implemented against the
        // unified repository. The remaining capabilities are deferred to a
        // later phase or to PostgreSQL-only runtime features.
        matches!(
            self,
            Self::Endpoints
                | Self::ModelRoutes
                | Self::Relays
                | Self::ClientKeys
                | Self::Settings
                | Self::EndpointSetting
                | Self::McpServers
                | Self::McpCredentials
                | Self::McpCatalog
                | Self::SnapshotPublication
        )
    }

    pub fn for_path(path: &str) -> Option<Self> {
        if path == "/admin/endpoints" || path == "/admin/endpoints/test" {
            return Some(Self::Endpoints);
        }
        if path.starts_with("/admin/endpoints/") {
            return Some(Self::Endpoints);
        }
        // The dedicated test endpoint depends on PostgreSQL-only
        // `model_route_candidates` and is gated separately from the regular
        // CRUD endpoints so the rejection carries its own capability code.
        if path == "/admin/model-routes/test" {
            return Some(Self::ModelRouteTest);
        }
        if path == "/admin/model-routes" {
            return Some(Self::ModelRoutes);
        }
        if path.starts_with("/admin/model-routes/") {
            return Some(Self::ModelRoutes);
        }
        if path == "/admin/relays" {
            return Some(Self::Relays);
        }
        if path.starts_with("/admin/relays/") {
            return Some(Self::Relays);
        }
        if path == "/me/client-keys" || path.starts_with("/me/client-keys/") {
            return Some(Self::ClientKeys);
        }
        if path.starts_with("/admin/users/") && path.contains("/client-keys") {
            return Some(Self::ClientKeys);
        }
        match path {
            "/settings/endpoint" => return Some(Self::EndpointSetting),
            "/settings/redaction"
            | "/settings/redaction/custom-strings"
            | "/settings/redaction/preview" => return Some(Self::Settings),
            "/settings/request-content-logging"
            | "/settings/usage-retention"
            | "/settings/stream-delta-batching"
            | "/settings/model-route-whitelist"
            | "/settings/relay-ip-whitelist"
            | "/settings/llm-review" => return Some(Self::Settings),
            _ => {}
        }
        if path.starts_with("/admin/conversations/") && path.ends_with("/endpoint-override") {
            return Some(Self::ConversationEndpointOverride);
        }
        if path == "/admin/mcp-servers" {
            return Some(Self::McpServers);
        }
        if path.starts_with("/admin/mcp-servers/") {
            if path.ends_with("/catalog") || path.ends_with("/test") {
                return Some(Self::McpCatalog);
            }
            if path.ends_with("/credentials") {
                return Some(Self::McpCredentials);
            }
            if path.contains("/credentials/") {
                return Some(Self::McpQuota);
            }
            return Some(Self::McpServers);
        }
        if path.starts_with("/admin/mcp-quota-groups") {
            return Some(Self::McpQuota);
        }
        if path == "/me/models" {
            return Some(Self::AvailableModels);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_paths_map_to_their_capability() {
        assert_eq!(
            Capability::for_path("/admin/endpoints"),
            Some(Capability::Endpoints)
        );
        assert_eq!(
            Capability::for_path("/admin/endpoints/abc/test"),
            Some(Capability::Endpoints)
        );
        assert_eq!(
            Capability::for_path("/admin/model-routes"),
            Some(Capability::ModelRoutes)
        );
        assert_eq!(
            Capability::for_path("/admin/model-routes/abc/test"),
            Some(Capability::ModelRoutes)
        );
        assert_eq!(
            Capability::for_path("/admin/model-routes/test"),
            Some(Capability::ModelRouteTest)
        );
        assert_eq!(
            Capability::for_path("/admin/relays"),
            Some(Capability::Relays)
        );
        assert_eq!(
            Capability::for_path("/admin/relays/abc/reconnect"),
            Some(Capability::Relays)
        );
        assert_eq!(
            Capability::for_path("/me/client-keys"),
            Some(Capability::ClientKeys)
        );
        assert_eq!(
            Capability::for_path("/admin/users/7/client-keys"),
            Some(Capability::ClientKeys)
        );
        assert_eq!(
            Capability::for_path("/settings/endpoint"),
            Some(Capability::EndpointSetting)
        );
        assert_eq!(
            Capability::for_path("/settings/redaction"),
            Some(Capability::Settings)
        );
        assert_eq!(
            Capability::for_path("/settings/usage-retention"),
            Some(Capability::Settings)
        );
        assert_eq!(
            Capability::for_path("/admin/conversations/abc/endpoint-override"),
            Some(Capability::ConversationEndpointOverride)
        );
        assert_eq!(
            Capability::for_path("/me/models"),
            Some(Capability::AvailableModels)
        );
        assert_eq!(
            Capability::for_path("/admin/mcp-servers"),
            Some(Capability::McpServers)
        );
        assert_eq!(
            Capability::for_path("/admin/mcp-servers/abc/catalog"),
            Some(Capability::McpCatalog)
        );
        assert_eq!(
            Capability::for_path("/admin/mcp-servers/abc/credentials"),
            Some(Capability::McpCredentials)
        );
        assert_eq!(
            Capability::for_path("/admin/mcp-servers/abc/credentials/def/quota-group"),
            Some(Capability::McpQuota)
        );
        assert_eq!(
            Capability::for_path("/admin/mcp-quota-groups"),
            Some(Capability::McpQuota)
        );
        assert_eq!(Capability::for_path("/auth/me"), None);
    }

    #[test]
    fn sqlite_supported_capabilities_are_explicit() {
        assert!(Capability::SnapshotPublication.sqlite_supported());
        assert!(Capability::Endpoints.sqlite_supported());
        assert!(Capability::ModelRoutes.sqlite_supported());
        assert!(Capability::Relays.sqlite_supported());
        assert!(Capability::ClientKeys.sqlite_supported());
        assert!(Capability::Settings.sqlite_supported());
        assert!(Capability::EndpointSetting.sqlite_supported());
        assert!(!Capability::ConversationEndpointOverride.sqlite_supported());
        assert!(!Capability::AvailableModels.sqlite_supported());
        assert!(!Capability::ModelRouteTest.sqlite_supported());
    }
}
