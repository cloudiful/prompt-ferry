-- Issue #589: optional per-endpoint OpenAI Admin API key for the official
-- organization usage/cost read. The secret uses the envelope pattern
-- (`admin_api_key_ciphertext BLOB / admin_api_key_nonce BLOB /
-- admin_api_key_key_version INTEGER`, NULL means cleared), matching the 0018
-- proxy envelope and the 0033 OAuth token table. Presence means
-- `admin_api_key_ciphertext IS NOT NULL`. `ON DELETE CASCADE` drops the key
-- with its endpoint; the runtime enables SQLite foreign keys (see
-- `db::connect_sqlite`), so no orphaned secret survives endpoint deletion.
CREATE TABLE IF NOT EXISTS standalone_endpoint_admin_keys (
    endpoint_id TEXT PRIMARY KEY REFERENCES standalone_provider_endpoints(endpoint_id) ON DELETE CASCADE,
    admin_api_key_ciphertext BLOB,
    admin_api_key_nonce BLOB,
    admin_api_key_key_version INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

UPDATE standalone_schema_meta
SET schema_version = 35
WHERE schema_key = 'standalone';
