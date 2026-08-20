ALTER TABLE mcp_servers
    DROP CONSTRAINT IF EXISTS ck_mcp_server_transport;

ALTER TABLE mcp_servers
    DROP CONSTRAINT IF EXISTS mcp_servers_transport_check;

ALTER TABLE mcp_servers
    ADD CONSTRAINT ck_mcp_server_transport
    CHECK (transport IN ('http', 'stdio', 'builtin_minimax'));

ALTER TABLE mcp_servers
    ADD COLUMN IF NOT EXISTS source_endpoint_id UUID
    REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_source_endpoint
    ON mcp_servers(source_endpoint_id)
    WHERE source_endpoint_id IS NOT NULL;

ALTER TABLE provider_endpoints
    ADD COLUMN IF NOT EXISTS mcp_enabled BOOLEAN NOT NULL DEFAULT FALSE;
