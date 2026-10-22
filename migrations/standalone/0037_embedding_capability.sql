-- no-transaction
-- Issue #1032 P0: mirror the PostgreSQL embeddings capability for the
-- standalone SQLite store.
--
-- `native_api` gains the `embeddings` protocol at the endpoint level.
-- SQLite cannot widen or drop a CHECK constraint, so
-- `standalone_provider_endpoints` is rebuilt with the exact columns, keys
-- and encrypted envelopes it had before; only the allowed `native_api`
-- values change. The rebuild runs outside the implicit transaction so
-- `PRAGMA foreign_keys = OFF` is effective, which keeps the endpoint keys,
-- route targets and managed MCP servers that reference the endpoint table
-- intact (same pattern as 0034).
--
-- `standalone_model_route_targets.embedding_dimensions_override` is a
-- nullable positive integer: NULL inherits the caller's dimensions, a value
-- forces them. It is an override, never a cap, so the only rule is
-- "positive integer". SQLite INTEGER affinity does not require integer
-- storage, so the CHECK also requires the `integer` storage class: a
-- fractional or non-numeric value is rejected instead of being stored as a
-- real or text value. SQLite integers are 64-bit, so the column also caps
-- the value at the i32 maximum.
PRAGMA foreign_keys = OFF;

CREATE TABLE standalone_provider_endpoints_new (
    endpoint_id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    provider TEXT NOT NULL CHECK (provider IN ('generic', 'minimax', 'command_code', 'opencode_go', 'openrouter', 'glm', 'deepseek', 'openai')),
    provider_region TEXT CHECK (provider_region IS NULL OR provider_region IN ('cn', 'global')),
    base_url TEXT NOT NULL,
    native_api TEXT NOT NULL CHECK (native_api IN ('auto', 'anthropic_messages', 'chat', 'responses', 'realtime', 'embeddings')),
    native_api_source TEXT NOT NULL CHECK (native_api_source IN ('auto', 'detected', 'manual')),
    key_lb_enabled INTEGER NOT NULL CHECK (key_lb_enabled IN (0, 1)),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    mcp_enabled INTEGER NOT NULL CHECK (mcp_enabled IN (0, 1)),
    api_key_ciphertext BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    api_key_key_version INTEGER NOT NULL,
    proxy_url_ciphertext BLOB,
    proxy_url_nonce BLOB,
    proxy_url_key_version INTEGER,
    active_windows TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    service_tier TEXT
);

INSERT INTO standalone_provider_endpoints_new (
    endpoint_id, name, provider, provider_region, base_url, native_api,
    native_api_source, key_lb_enabled, enabled, mcp_enabled,
    api_key_ciphertext, api_key_nonce, api_key_key_version,
    proxy_url_ciphertext, proxy_url_nonce, proxy_url_key_version,
    active_windows, created_at, updated_at, service_tier
)
SELECT
    endpoint_id, name, provider, provider_region, base_url, native_api,
    native_api_source, key_lb_enabled, enabled, mcp_enabled,
    api_key_ciphertext, api_key_nonce, api_key_key_version,
    proxy_url_ciphertext, proxy_url_nonce, proxy_url_key_version,
    active_windows, created_at, updated_at, service_tier
FROM standalone_provider_endpoints;

DROP TABLE standalone_provider_endpoints;

ALTER TABLE standalone_provider_endpoints_new RENAME TO standalone_provider_endpoints;

PRAGMA foreign_keys = ON;

ALTER TABLE standalone_model_route_targets
ADD COLUMN embedding_dimensions_override INTEGER
CHECK (embedding_dimensions_override IS NULL OR (typeof(embedding_dimensions_override) = 'integer' AND embedding_dimensions_override > 0 AND embedding_dimensions_override <= 2147483647));

UPDATE standalone_schema_meta
SET schema_version = 37
WHERE schema_key = 'standalone';
