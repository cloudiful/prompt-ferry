ALTER TABLE standalone_mcp_servers ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'none';
ALTER TABLE standalone_mcp_servers ADD COLUMN basic_username TEXT;
ALTER TABLE standalone_mcp_servers ADD COLUMN basic_password_ciphertext BLOB;
ALTER TABLE standalone_mcp_servers ADD COLUMN basic_password_nonce BLOB;
ALTER TABLE standalone_mcp_servers ADD COLUMN basic_password_key_version INTEGER;

-- Basic password envelope columns must be all NULL or all NOT NULL
-- (handled by application validation; no additional CHECK needed for migration).

UPDATE standalone_schema_meta
SET schema_version = 10
WHERE schema_key = 'standalone';
