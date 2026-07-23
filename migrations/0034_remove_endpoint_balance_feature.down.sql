ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS balance_api_key TEXT,
ADD COLUMN IF NOT EXISTS balance_api_user TEXT;

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

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS route_balance_value DOUBLE PRECISION,
ADD COLUMN IF NOT EXISTS route_balance_unit TEXT,
ADD COLUMN IF NOT EXISTS route_balance_share DOUBLE PRECISION;

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
