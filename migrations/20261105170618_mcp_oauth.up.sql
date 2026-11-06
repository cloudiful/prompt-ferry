-- Personal MCP OAuth persistence (issue #1208 P2).
--
-- Credentials belong to one user-owned HTTP MCP server and one owner. Every
-- secret lives inside an encrypted envelope; the plain columns are non-secret
-- status fields only. A random connection UUID is the connection epoch, and
-- the generation plus config fingerprint fence stale refreshes.

CREATE TABLE IF NOT EXISTS mcp_oauth_credentials (
    server_id UUID NOT NULL REFERENCES mcp_servers(server_id) ON DELETE CASCADE,
    owner_user_id BIGINT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    connection_id UUID NOT NULL,
    generation BIGINT NOT NULL CHECK (generation >= 1),
    config_fingerprint TEXT NOT NULL,
    registration_ciphertext BYTEA NOT NULL,
    registration_nonce BYTEA NOT NULL,
    registration_key_version SMALLINT NOT NULL,
    credentials_ciphertext BYTEA NOT NULL,
    credentials_nonce BYTEA NOT NULL,
    credentials_key_version SMALLINT NOT NULL,
    scopes TEXT NOT NULL DEFAULT '',
    expires_at TIMESTAMPTZ,
    issuer TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (server_id, owner_user_id)
);

CREATE TABLE IF NOT EXISTS mcp_oauth_states (
    state_key_hash TEXT PRIMARY KEY,
    server_id UUID NOT NULL REFERENCES mcp_servers(server_id) ON DELETE CASCADE,
    owner_user_id BIGINT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    session_binding_hash TEXT NOT NULL,
    connection_id UUID NOT NULL,
    config_fingerprint TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    authorization_ciphertext BYTEA NOT NULL,
    authorization_nonce BYTEA NOT NULL,
    authorization_key_version SMALLINT NOT NULL,
    pending_client_ciphertext BYTEA,
    pending_client_nonce BYTEA,
    pending_client_key_version SMALLINT,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT ck_mcp_oauth_state_pending_client CHECK (
        (pending_client_ciphertext IS NULL
            AND pending_client_nonce IS NULL
            AND pending_client_key_version IS NULL)
        OR (pending_client_ciphertext IS NOT NULL
            AND pending_client_nonce IS NOT NULL
            AND pending_client_key_version IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_mcp_oauth_states_owner
    ON mcp_oauth_states(server_id, owner_user_id);

CREATE TABLE IF NOT EXISTS mcp_oauth_leases (
    server_id UUID NOT NULL REFERENCES mcp_servers(server_id) ON DELETE CASCADE,
    owner_user_id BIGINT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    connection_id UUID NOT NULL,
    generation BIGINT NOT NULL CHECK (generation >= 1),
    lease_owner UUID NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (server_id, owner_user_id)
);

-- Critical configuration edits revoke the whole personal OAuth state for that
-- server at the storage boundary, so CRUD, import, or direct SQL all behave
-- the same. Name and timeout edits deliberately do not revoke anything.
CREATE OR REPLACE FUNCTION mcp_oauth_revoke_on_config_change()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.url IS DISTINCT FROM OLD.url
        OR NEW.auth_mode IS DISTINCT FROM OLD.auth_mode
        OR NEW.scope IS DISTINCT FROM OLD.scope
        OR NEW.owner_user_id IS DISTINCT FROM OLD.owner_user_id
        OR NEW.transport IS DISTINCT FROM OLD.transport
        OR NEW.proxy_url IS DISTINCT FROM OLD.proxy_url
    THEN
        DELETE FROM mcp_oauth_states WHERE server_id = OLD.server_id;
        DELETE FROM mcp_oauth_credentials WHERE server_id = OLD.server_id;
        DELETE FROM mcp_oauth_leases WHERE server_id = OLD.server_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_mcp_oauth_revoke ON mcp_servers;
CREATE TRIGGER trg_mcp_oauth_revoke
    AFTER UPDATE ON mcp_servers
    FOR EACH ROW
    EXECUTE FUNCTION mcp_oauth_revoke_on_config_change();

ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_auth_mode;
ALTER TABLE mcp_servers
    ADD CONSTRAINT ck_mcp_server_auth_mode
    CHECK (auth_mode IN ('none', 'bearer', 'basic', 'oauth'));
