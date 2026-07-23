ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_servers_aggregate_naming_mode;

ALTER TABLE mcp_servers
DROP COLUMN IF EXISTS aggregate_naming_mode;
