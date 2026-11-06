-- Revert personal MCP OAuth persistence (issue #1208 P2).
--
-- Restoring the narrower auth-mode constraint intentionally fails while any
-- server still carries `auth_mode = 'oauth'`; a down migration must never
-- silently delete live OAuth configuration. Run it only on an isolated
-- fixture with no OAuth rows.

DROP TRIGGER IF EXISTS trg_mcp_oauth_revoke ON mcp_servers;
DROP FUNCTION IF EXISTS mcp_oauth_revoke_on_config_change();

DROP TABLE IF EXISTS mcp_oauth_leases;
DROP TABLE IF EXISTS mcp_oauth_states;
DROP TABLE IF EXISTS mcp_oauth_credentials;

ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_auth_mode;
ALTER TABLE mcp_servers
    ADD CONSTRAINT ck_mcp_server_auth_mode
    CHECK (auth_mode IN ('none', 'bearer', 'basic'));
