-- Issue #430 Task1: remove standalone daily/monthly quota columns (destructive, forward-only).
-- standalone_model_routes has no CHECK on the two columns, so DROP COLUMN avoids
-- rebuilding the parent of standalone_model_route_targets (FK ON DELETE CASCADE).
-- standalone_mcp_servers carries quota CHECKs (0005:36-37), so it is rebuilt from
-- the post-0019 definition (0005 + 0010 auth/basic + 0017 provider_kind + 0019 proxy).
-- standalone_provider_endpoints never had quota columns; no mcp_credentials table exists.
PRAGMA foreign_keys=OFF;
ALTER TABLE standalone_model_routes DROP COLUMN daily_max_requests;
ALTER TABLE standalone_model_routes DROP COLUMN monthly_max_requests;
CREATE TABLE standalone_mcp_servers_new (
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
    auth_mode TEXT NOT NULL DEFAULT 'none',
    basic_username TEXT,
    basic_password_ciphertext BLOB,
    basic_password_nonce BLOB,
    basic_password_key_version INTEGER,
    provider_kind TEXT,
    proxy_url_ciphertext BLOB,
    proxy_url_nonce BLOB,
    proxy_url_key_version INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK ((scope = 'admin' AND owner_user_id IS NULL) OR (scope = 'user' AND owner_user_id IS NOT NULL)),
    CHECK (timeout_ms >= 100 AND timeout_ms <= 300000),
    CHECK (source_endpoint_id IS NULL OR transport = 'builtin_minimax')
);
INSERT INTO standalone_mcp_servers_new (
    server_id, source_endpoint_id, scope, owner_user_id, name,
    aggregate_naming_mode, transport, url, command, args_json, http_headers_json,
    tool_filter_mode, allowed_tools_json, disabled_tools_json, disabled_resources_json,
    enabled, timeout_ms, lifecycle_policy, lifecycle_manual_protocol_version,
    lifecycle_learned_mode, lifecycle_learned_protocol_version,
    lifecycle_learned_for_updated_at, lifecycle_learned_at,
    env_ciphertext, env_nonce, env_key_version,
    bearer_tokens_ciphertext, bearer_tokens_nonce, bearer_tokens_key_version,
    auth_mode, basic_username, basic_password_ciphertext, basic_password_nonce,
    basic_password_key_version, provider_kind,
    proxy_url_ciphertext, proxy_url_nonce, proxy_url_key_version,
    created_at, updated_at
) SELECT
    server_id, source_endpoint_id, scope, owner_user_id, name,
    aggregate_naming_mode, transport, url, command, args_json, http_headers_json,
    tool_filter_mode, allowed_tools_json, disabled_tools_json, disabled_resources_json,
    enabled, timeout_ms, lifecycle_policy, lifecycle_manual_protocol_version,
    lifecycle_learned_mode, lifecycle_learned_protocol_version,
    lifecycle_learned_for_updated_at, lifecycle_learned_at,
    env_ciphertext, env_nonce, env_key_version,
    bearer_tokens_ciphertext, bearer_tokens_nonce, bearer_tokens_key_version,
    auth_mode, basic_username, basic_password_ciphertext, basic_password_nonce,
    basic_password_key_version, provider_kind,
    proxy_url_ciphertext, proxy_url_nonce, proxy_url_key_version,
    created_at, updated_at
FROM standalone_mcp_servers;
DROP TABLE standalone_mcp_servers;
ALTER TABLE standalone_mcp_servers_new RENAME TO standalone_mcp_servers;
CREATE INDEX IF NOT EXISTS idx_standalone_mcp_servers_visible
    ON standalone_mcp_servers(enabled, scope, owner_user_id, name);
CREATE UNIQUE INDEX IF NOT EXISTS idx_standalone_mcp_servers_source_endpoint
    ON standalone_mcp_servers(source_endpoint_id)
    WHERE source_endpoint_id IS NOT NULL;
PRAGMA foreign_keys=ON;
UPDATE standalone_schema_meta
SET schema_version = 25
WHERE schema_key = 'standalone';
