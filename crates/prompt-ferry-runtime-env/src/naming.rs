pub const PRODUCT_SLUG: &str = "prompt-ferry";

pub const CONFIG_APP_NAME: &str = PRODUCT_SLUG;
pub const CONFIG_ENV_PREFIX: &str = "PROMPT_FERRY_";

/// Environment prefix of the node configuration: `PROMPT_FERRY__<FIELD>`.
///
/// The trailing double underscore keeps the node fields flat (`BIND`,
/// `MASTER_SECRET`, ...) while the single-underscore prefix above continues to
/// address the nested application configuration.
pub const NODE_CONFIG_ENV_PREFIX: &str = "PROMPT_FERRY__";

pub const SESSION_COOKIE_NAME: &str = "prompt_ferry_session";

pub const CLIENT_KEY_PREFIX: &str = "pfy_";
pub const REALTIME_CLIENT_SECRET_PREFIX: &str = "pfy_rt_";
pub const HKDF_INFO: &[u8] = b"prompt-ferry relay-worker v1";
pub const CONVERSATION_HASH_NAMESPACE: &[u8] = b"prompt-ferry-conversation";
pub const BRIDGE_FRAME_PREFIX: &str = "prompt-ferry";

pub const MCP_IMPLEMENTATION_NAME: &str = "prompt-ferry";
pub const MCP_SERVER_NAME: &str = "prompt-ferry-mcp";

pub const MODEL_ROUTE_TEST_ROUTING_KEY: &str = "prompt-ferry-model-route-test";
pub const MODEL_ROUTE_TEST_SESSION_KEY: &str = "prompt-ferry-model-route-test-session";

#[cfg(test)]
mod tests {
    use super::{CONFIG_ENV_PREFIX, NODE_CONFIG_ENV_PREFIX};

    #[test]
    fn node_env_prefix_addresses_flat_root_fields() {
        assert!(NODE_CONFIG_ENV_PREFIX.starts_with(CONFIG_ENV_PREFIX));
        assert_eq!(NODE_CONFIG_ENV_PREFIX, "PROMPT_FERRY__");
        assert_eq!(
            format!("{NODE_CONFIG_ENV_PREFIX}BIND"),
            "PROMPT_FERRY__BIND"
        );
        // A nested application key keeps its single underscore and never
        // collides with the flat node namespace.
        let nested = format!("{CONFIG_ENV_PREFIX}WORKER__DATABASE_URL");
        assert!(!nested.starts_with(NODE_CONFIG_ENV_PREFIX));
    }
}
