-- Issue #296 Phase 1: MCP provider preset contract. `provider_kind` is an
-- optional preset id (generic/minimax/context7/firecrawl); NULL keeps the
-- legacy untyped behavior and unknown values remain readable so existing
-- rows are never broken. Transport still expresses the wire protocol only.
ALTER TABLE mcp_servers
ADD COLUMN IF NOT EXISTS provider_kind TEXT;

ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS ck_mcp_server_provider_kind;

ALTER TABLE mcp_servers
ADD CONSTRAINT ck_mcp_server_provider_kind
CHECK (provider_kind IN ('generic', 'minimax', 'context7', 'firecrawl'));

-- Managed MiniMax rows are the only preset rows that exist today; label them
-- so the registry and admin views can distinguish them from generic rows.
UPDATE mcp_servers
SET provider_kind = 'minimax'
WHERE transport = 'builtin_minimax'
  AND provider_kind IS NULL;
