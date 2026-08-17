ALTER TABLE mcp_servers
    ADD COLUMN IF NOT EXISTS lifecycle_policy TEXT NOT NULL DEFAULT 'auto',
    ADD COLUMN IF NOT EXISTS lifecycle_manual_protocol_version TEXT,
    ADD COLUMN IF NOT EXISTS lifecycle_learned_mode TEXT,
    ADD COLUMN IF NOT EXISTS lifecycle_learned_protocol_version TEXT,
    ADD COLUMN IF NOT EXISTS lifecycle_learned_for_updated_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS lifecycle_learned_at TIMESTAMPTZ;

ALTER TABLE mcp_servers
    DROP CONSTRAINT IF EXISTS ck_mcp_server_lifecycle_policy,
    DROP CONSTRAINT IF EXISTS ck_mcp_server_lifecycle_learned_mode;

ALTER TABLE mcp_servers
    ADD CONSTRAINT ck_mcp_server_lifecycle_policy
        CHECK (lifecycle_policy IN ('auto', 'legacy_initialize')),
    ADD CONSTRAINT ck_mcp_server_lifecycle_learned_mode
        CHECK (lifecycle_learned_mode IS NULL OR lifecycle_learned_mode IN ('modern_discover', 'legacy_initialize'));
