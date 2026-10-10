CREATE TABLE chatgpt_quota_snapshots (
    snapshot_id BIGSERIAL PRIMARY KEY,
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    observed_at TIMESTAMPTZ NOT NULL,
    plan_type TEXT,
    limit_reached BOOLEAN,
    windows JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(windows) = 'array'),
    source TEXT NOT NULL
        CHECK (source IN ('manual', 'request', 'periodic')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_chatgpt_quota_snapshots_endpoint_snapshot
    ON chatgpt_quota_snapshots (endpoint_id, snapshot_id DESC);
CREATE INDEX idx_chatgpt_quota_snapshots_observed_snapshot
    ON chatgpt_quota_snapshots (observed_at, snapshot_id);

CREATE TABLE chatgpt_quota_refresh_state (
    endpoint_id UUID PRIMARY KEY REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    lease_owner UUID,
    lease_expires_at TIMESTAMPTZ,
    last_attempt_at TIMESTAMPTZ,
    last_success_at TIMESTAMPTZ,
    consecutive_failures INTEGER NOT NULL DEFAULT 0
        CHECK (consecutive_failures >= 0),
    next_retry_at TIMESTAMPTZ,
    last_error_code TEXT
        CHECK (
            last_error_code IS NULL
            OR last_error_code ~ '^[a-z0-9:_-]{1,64}$'
        ),
    CHECK ((lease_owner IS NULL) = (lease_expires_at IS NULL))
);
