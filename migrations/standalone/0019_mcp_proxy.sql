-- Issue #375 Phase F: mirror PostgreSQL 0082 per-row MCP proxy for the
-- standalone SQLite store. Secrets use the envelope pattern
-- (`*_ciphertext BLOB / *_nonce BLOB / *_key_version INTEGER`, NULL means
-- inherit: empty row falls back to process env, then direct), matching
-- `basic_password` and the endpoint 0018 proxy envelope. No CHECK,
-- matching the pre-existing standalone convention (allowed schemes stay
-- enforced by shared admin validation).
ALTER TABLE standalone_mcp_servers ADD COLUMN proxy_url_ciphertext BLOB;
ALTER TABLE standalone_mcp_servers ADD COLUMN proxy_url_nonce BLOB;
ALTER TABLE standalone_mcp_servers ADD COLUMN proxy_url_key_version INTEGER;

UPDATE standalone_schema_meta
SET schema_version = 19
WHERE schema_key = 'standalone';
