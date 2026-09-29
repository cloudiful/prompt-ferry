-- no-transaction
-- Issue #637: mirror the PostgreSQL 20260929024454 generic service-tier
-- override for the standalone SQLite store. SQLite cannot DROP a column
-- DEFAULT or NOT NULL constraint, so `standalone_provider_endpoints` is
-- rebuilt with `service_tier TEXT` (nullable, no default): NULL/blank means
-- inherit (leave the caller/provider default untouched). Explicit non-default
-- tiers survive; the legacy `standard` default migrates to NULL because it is
-- semantically the provider default. `standalone_model_route_targets` gains an
-- optional override that wins over the endpoint value at resolution time.
--
-- Foreign keys reference the rebuilt table from endpoint keys, route targets
-- and managed MCP servers, so this migration opts out of the implicit sqlx
-- transaction before dropping the table (same pattern as 0032).
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
    active_windows, created_at, updated_at,
    CASE
        WHEN service_tier IS NULL OR trim(service_tier) = '' THEN NULL
        WHEN service_tier = 'standard' THEN NULL
        ELSE trim(service_tier)
    END
FROM standalone_provider_endpoints;

DROP TABLE standalone_provider_endpoints;

ALTER TABLE standalone_provider_endpoints_new RENAME TO standalone_provider_endpoints;

PRAGMA foreign_keys = ON;

ALTER TABLE standalone_model_route_targets ADD COLUMN service_tier TEXT;

UPDATE standalone_schema_meta
SET schema_version = 34
WHERE schema_key = 'standalone';
