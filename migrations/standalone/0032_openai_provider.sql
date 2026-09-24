-- no-transaction
-- P1 (issue #589): mirror the PostgreSQL 20260924061635 provider widening for
-- the standalone SQLite store. SQLite cannot DROP a table CHECK, so the table
-- is rebuilt with the same columns (including the 0018 proxy envelope and the
-- 0021 active_windows column) and the provider list extended to
-- ('generic', 'minimax', 'command_code', 'opencode_go', 'openrouter', 'glm',
-- 'deepseek', 'openai'). Provider/region combination rules stay enforced by
-- the shared admin handler validation, matching the pre-existing standalone
-- convention (the column-level region CHECK is unchanged).
--
-- Foreign keys reference standalone_provider_endpoints from endpoint keys,
-- route targets, and managed MCP servers. sqlx wraps SQLite migrations in a
-- transaction by default, and `PRAGMA foreign_keys` is a no-op inside one, so
-- the first line opts this migration out of the implicit transaction: without
-- it the DROP below would run with foreign keys ON and its implicit delete
-- would cascade the referencing rows away.
PRAGMA foreign_keys = OFF;

CREATE TABLE standalone_provider_endpoints_new (
    endpoint_id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    provider TEXT NOT NULL CHECK (provider IN ('generic', 'minimax', 'command_code', 'opencode_go', 'openrouter', 'glm', 'deepseek', 'openai')),
    provider_region TEXT CHECK (provider_region IS NULL OR provider_region IN ('cn', 'global')),
    base_url TEXT NOT NULL,
    native_api TEXT NOT NULL CHECK (native_api IN ('auto', 'anthropic_messages', 'chat', 'responses', 'realtime')),
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
    service_tier TEXT NOT NULL DEFAULT 'standard'
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

UPDATE standalone_schema_meta
SET schema_version = 32
WHERE schema_key = 'standalone';
