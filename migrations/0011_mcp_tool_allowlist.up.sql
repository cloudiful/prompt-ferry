ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS tool_filter_mode TEXT NOT NULL DEFAULT 'blacklist',
ADD COLUMN IF NOT EXISTS allowed_tools JSONB NOT NULL DEFAULT '[]'::JSONB;

ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_server_tool_filter_mode;

ALTER TABLE mcp_servers
ADD CONSTRAINT ck_mcp_server_tool_filter_mode
CHECK (tool_filter_mode IN ('blacklist', 'whitelist'));
