ALTER TABLE mcp_servers
DROP COLUMN IF EXISTS http_headers_json,
DROP COLUMN IF EXISTS bearer_token;
