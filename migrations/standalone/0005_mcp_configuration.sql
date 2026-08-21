CREATE TABLE IF NOT EXISTS standalone_mcp_servers (
    server_id TEXT PRIMARY KEY,
    source_endpoint_id TEXT REFERENCES standalone_provider_endpoints(endpoint_id) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
    owner_user_id INTEGER REFERENCES standalone_users(user_id) ON DELETE CASCADE,
    name TEXT NOT NULL UNIQUE,
    aggregate_naming_mode TEXT NOT NULL CHECK (aggregate_naming_mode IN ('qualified_only', 'passthrough_preferred')),
    transport TEXT NOT NULL CHECK (transport IN ('http', 'stdio', 'builtin_minimax')),
    url TEXT,
    command TEXT,
    args_json TEXT NOT NULL,
    http_headers_json TEXT NOT NULL,
    tool_filter_mode TEXT NOT NULL CHECK (tool_filter_mode IN ('blacklist', 'whitelist')),
    allowed_tools_json TEXT NOT NULL,
    disabled_tools_json TEXT NOT NULL,
    disabled_resources_json TEXT NOT NULL,
    daily_max_requests INTEGER,
    monthly_max_requests INTEGER,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    timeout_ms INTEGER NOT NULL,
    lifecycle_policy TEXT NOT NULL CHECK (lifecycle_policy IN ('auto', 'legacy_initialize')),
    lifecycle_manual_protocol_version TEXT,
    lifecycle_learned_mode TEXT,
    lifecycle_learned_protocol_version TEXT,
    lifecycle_learned_for_updated_at TEXT,
    lifecycle_learned_at TEXT,
    env_ciphertext BLOB NOT NULL,
    env_nonce BLOB NOT NULL,
    env_key_version INTEGER NOT NULL,
    bearer_tokens_ciphertext BLOB NOT NULL,
    bearer_tokens_nonce BLOB NOT NULL,
    bearer_tokens_key_version INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((scope = 'admin' AND owner_user_id IS NULL) OR (scope = 'user' AND owner_user_id IS NOT NULL)),
    CHECK (daily_max_requests IS NULL OR daily_max_requests > 0),
    CHECK (monthly_max_requests IS NULL OR monthly_max_requests > 0),
    CHECK (timeout_ms >= 100 AND timeout_ms <= 300000),
    CHECK (source_endpoint_id IS NULL OR transport = 'builtin_minimax')
);

CREATE INDEX IF NOT EXISTS idx_standalone_mcp_servers_visible
    ON standalone_mcp_servers(enabled, scope, owner_user_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_standalone_mcp_servers_source_endpoint
    ON standalone_mcp_servers(source_endpoint_id)
    WHERE source_endpoint_id IS NOT NULL;

UPDATE standalone_schema_meta
SET schema_version = 5
WHERE schema_key = 'standalone';
