-- Issue #599 R2a: mirror PostgreSQL 0094 per-endpoint ChatGPT subscription
-- OAuth token for the standalone SQLite store. Secrets use the envelope
-- pattern (`*_ciphertext BLOB / *_nonce BLOB / *_key_version INTEGER`, NULL
-- means cleared), matching `api_key` and the 0018 proxy envelope. Presence
-- means `refresh_token_ciphertext IS NOT NULL`. `ON DELETE CASCADE` drops the
-- token with its endpoint; the runtime enables SQLite foreign keys (see
-- `db::connect_sqlite`), so no orphaned secret survives endpoint deletion.
CREATE TABLE IF NOT EXISTS standalone_endpoint_oauth_tokens (
    endpoint_id TEXT PRIMARY KEY REFERENCES standalone_provider_endpoints(endpoint_id) ON DELETE CASCADE,
    access_token_ciphertext BLOB,
    access_token_nonce BLOB,
    access_token_key_version INTEGER,
    refresh_token_ciphertext BLOB,
    refresh_token_nonce BLOB,
    refresh_token_key_version INTEGER,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

UPDATE standalone_schema_meta
SET schema_version = 33
WHERE schema_key = 'standalone';
