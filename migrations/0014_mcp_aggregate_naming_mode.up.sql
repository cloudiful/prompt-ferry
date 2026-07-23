ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS aggregate_naming_mode TEXT NOT NULL DEFAULT 'passthrough_preferred';

ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_servers_aggregate_naming_mode;

ALTER TABLE mcp_servers
ADD CONSTRAINT ck_mcp_servers_aggregate_naming_mode
CHECK (aggregate_naming_mode IN ('qualified_only', 'passthrough_preferred'));
