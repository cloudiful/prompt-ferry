-- Personal MCP OAuth persistence for the SQLite runtime (issue #1208 P2).
--
-- Mirrors PostgreSQL 20261105170618_mcp_oauth: encrypted envelopes only,
-- a random connection epoch, generation plus config fingerprint fences, and
-- an AFTER UPDATE revocation trigger on standalone_mcp_servers. Existing
-- secret columns and rows are not altered.

CREATE TABLE IF NOT EXISTS standalone_mcp_oauth_credentials (
    server_id TEXT NOT NULL REFERENCES standalone_mcp_servers(server_id) ON DELETE CASCADE,
    owner_user_id INTEGER NOT NULL REFERENCES standalone_users(user_id) ON DELETE CASCADE,
    connection_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation >= 1),
    config_fingerprint TEXT NOT NULL,
    registration_ciphertext BLOB NOT NULL,
    registration_nonce BLOB NOT NULL,
    registration_key_version INTEGER NOT NULL,
    credentials_ciphertext BLOB NOT NULL,
    credentials_nonce BLOB NOT NULL,
    credentials_key_version INTEGER NOT NULL,
    scopes TEXT NOT NULL DEFAULT '',
    expires_at TEXT,
    issuer TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (server_id, owner_user_id)
);

CREATE TABLE IF NOT EXISTS standalone_mcp_oauth_states (
    state_key_hash TEXT PRIMARY KEY,
    server_id TEXT NOT NULL REFERENCES standalone_mcp_servers(server_id) ON DELETE CASCADE,
    owner_user_id INTEGER NOT NULL REFERENCES standalone_users(user_id) ON DELETE CASCADE,
    session_binding_hash TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    config_fingerprint TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    authorization_ciphertext BLOB NOT NULL,
    authorization_nonce BLOB NOT NULL,
    authorization_key_version INTEGER NOT NULL,
    pending_client_ciphertext BLOB,
    pending_client_nonce BLOB,
    pending_client_key_version INTEGER,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CHECK (
        (pending_client_ciphertext IS NULL
            AND pending_client_nonce IS NULL
            AND pending_client_key_version IS NULL)
        OR (pending_client_ciphertext IS NOT NULL
            AND pending_client_nonce IS NOT NULL
            AND pending_client_key_version IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_standalone_mcp_oauth_states_owner
    ON standalone_mcp_oauth_states(server_id, owner_user_id);

CREATE TABLE IF NOT EXISTS standalone_mcp_oauth_leases (
    server_id TEXT NOT NULL REFERENCES standalone_mcp_servers(server_id) ON DELETE CASCADE,
    owner_user_id INTEGER NOT NULL REFERENCES standalone_users(user_id) ON DELETE CASCADE,
    connection_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation >= 1),
    lease_owner TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (server_id, owner_user_id)
);

CREATE TRIGGER IF NOT EXISTS trg_standalone_mcp_oauth_revoke
AFTER UPDATE ON standalone_mcp_servers
FOR EACH ROW
WHEN NEW.url IS NOT OLD.url
    OR NEW.auth_mode IS NOT OLD.auth_mode
    OR NEW.scope IS NOT OLD.scope
    OR NEW.owner_user_id IS NOT OLD.owner_user_id
    OR NEW.transport IS NOT OLD.transport
    OR NEW.proxy_url_ciphertext IS NOT OLD.proxy_url_ciphertext
    OR NEW.proxy_url_nonce IS NOT OLD.proxy_url_nonce
    OR NEW.proxy_url_key_version IS NOT OLD.proxy_url_key_version
BEGIN
    DELETE FROM standalone_mcp_oauth_states WHERE server_id = OLD.server_id;
    DELETE FROM standalone_mcp_oauth_credentials WHERE server_id = OLD.server_id;
    DELETE FROM standalone_mcp_oauth_leases WHERE server_id = OLD.server_id;
END;

UPDATE standalone_schema_meta
SET schema_version = 38
WHERE schema_key = 'standalone';
