ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_auth_mode;
ALTER TABLE mcp_servers DROP COLUMN IF EXISTS basic_password;
ALTER TABLE mcp_servers DROP COLUMN IF EXISTS basic_username;
ALTER TABLE mcp_servers DROP COLUMN IF EXISTS auth_mode;
