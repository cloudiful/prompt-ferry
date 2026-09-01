ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS auth_mode TEXT NOT NULL DEFAULT 'none';
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS basic_username TEXT;
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS basic_password TEXT;

UPDATE mcp_servers
SET auth_mode = 'bearer'
WHERE auth_mode = 'none'
  AND bearer_tokens_json IS NOT NULL
  AND bearer_tokens_json <> '[]'::jsonb;

ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_auth_mode;
ALTER TABLE mcp_servers ADD CONSTRAINT ck_mcp_server_auth_mode CHECK (auth_mode IN ('none', 'bearer', 'basic'));
