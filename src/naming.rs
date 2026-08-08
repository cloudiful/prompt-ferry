pub const PRODUCT_SLUG: &str = "prompt-ferry";

pub const CONFIG_APP_NAME: &str = PRODUCT_SLUG;
pub const CONFIG_ENV_PREFIX: &str = "PROMPT_FERRY_";

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
