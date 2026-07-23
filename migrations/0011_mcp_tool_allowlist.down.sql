ALTER TABLE mcp_servers
DROP CONSTRAINT IF EXISTS ck_mcp_server_tool_filter_mode;

ALTER TABLE mcp_servers
DROP COLUMN IF EXISTS allowed_tools,
DROP COLUMN IF EXISTS tool_filter_mode;
