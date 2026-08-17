ALTER TABLE mcp_servers
    DROP CONSTRAINT IF EXISTS ck_mcp_server_lifecycle_learned_mode,
    DROP CONSTRAINT IF EXISTS ck_mcp_server_lifecycle_policy;

ALTER TABLE mcp_servers
    DROP COLUMN IF EXISTS lifecycle_learned_at,
    DROP COLUMN IF EXISTS lifecycle_learned_for_updated_at,
    DROP COLUMN IF EXISTS lifecycle_learned_protocol_version,
    DROP COLUMN IF EXISTS lifecycle_learned_mode,
    DROP COLUMN IF EXISTS lifecycle_manual_protocol_version,
    DROP COLUMN IF EXISTS lifecycle_policy;
