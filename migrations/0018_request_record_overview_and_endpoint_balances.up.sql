ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS failure_family TEXT,
ADD COLUMN IF NOT EXISTS mcp_bearer_token_slot SMALLINT,
ADD COLUMN IF NOT EXISTS route_selection_reason TEXT NOT NULL DEFAULT 'default',
ADD COLUMN IF NOT EXISTS route_balance_value DOUBLE PRECISION,
ADD COLUMN IF NOT EXISTS route_balance_unit TEXT,
ADD COLUMN IF NOT EXISTS route_balance_share DOUBLE PRECISION;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_failure_family;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_failure_family
CHECK (
    failure_family IS NULL
    OR failure_family IN (
        'auth',
        'rate_limit',
        'quota',
        'timeout',
        'upstream_4xx',
        'upstream_5xx',
        'network',
        'empty_success',
        'policy',
        'unknown'
    )
);

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_route_selection_reason
CHECK (
    route_selection_reason IN (
        'default',
        'session_affinity',
        'conversation_override',
        'avoidance_fallback',
        'balance_weighted'
    )
);

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_mcp_bearer_token_slot;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_mcp_bearer_token_slot
CHECK (mcp_bearer_token_slot IS NULL OR mcp_bearer_token_slot > 0);

CREATE INDEX IF NOT EXISTS idx_request_records_model_created_at
ON request_records(model, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_token_slot_created_at
ON request_records(mcp_bearer_token_slot, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_failure_family_created_at
ON request_records(failure_family, created_at DESC);

CREATE TABLE IF NOT EXISTS endpoint_balance_snapshots (
    endpoint_id UUID PRIMARY KEY REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    provider_family TEXT NOT NULL,
    probe_status TEXT NOT NULL,
    balance_value DOUBLE PRECISION,
    used_value DOUBLE PRECISION,
    limit_value DOUBLE PRECISION,
    unit TEXT,
    currency TEXT,
    checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    error_message TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE endpoint_balance_snapshots
DROP CONSTRAINT IF EXISTS ck_endpoint_balance_snapshots_provider_family;

ALTER TABLE endpoint_balance_snapshots
ADD CONSTRAINT ck_endpoint_balance_snapshots_provider_family
CHECK (
    provider_family IN (
        'newapi',
        'openai_compat',
        'deepseek',
        'openrouter',
        'moonshot',
        'siliconflow',
        'unknown'
    )
);

ALTER TABLE endpoint_balance_snapshots
DROP CONSTRAINT IF EXISTS ck_endpoint_balance_snapshots_probe_status;

ALTER TABLE endpoint_balance_snapshots
ADD CONSTRAINT ck_endpoint_balance_snapshots_probe_status
CHECK (probe_status IN ('available', 'unsupported', 'error', 'stale'));

CREATE INDEX IF NOT EXISTS idx_endpoint_balance_snapshots_checked_at
ON endpoint_balance_snapshots(checked_at DESC);
