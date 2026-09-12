ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_provider_kind;
ALTER TABLE mcp_servers DROP COLUMN IF EXISTS provider_kind;
