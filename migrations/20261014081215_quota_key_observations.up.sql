CREATE TABLE quota_key_observations (
    observation_id BIGSERIAL PRIMARY KEY,
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    key_id UUID NOT NULL,
    provider TEXT NOT NULL
        CHECK (provider IN ('generic', 'minimax', 'command_code', 'opencode_go', 'openrouter', 'glm', 'deepseek', 'openai')),
    key_label VARCHAR(200) NOT NULL,
    source TEXT NOT NULL
        CHECK (source IN ('manual', 'request', 'periodic')),
    observed_at TIMESTAMPTZ NOT NULL,
    ok BOOLEAN NOT NULL,
    status TEXT,
    error_code VARCHAR(64)
        CHECK (
            error_code IS NULL
            OR error_code ~ '^[a-z0-9:_-]{1,64}$'
        ),
    plan_type VARCHAR(128),
    limit_reached BOOLEAN,
    windows JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(windows) = 'array'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_quota_key_observations_endpoint_key_observed
        UNIQUE (endpoint_id, key_id, observed_at)
);

CREATE INDEX idx_quota_key_observations_endpoint_key_observed
    ON quota_key_observations (endpoint_id, key_id, observed_at DESC);
CREATE INDEX idx_quota_key_observations_endpoint_key_id
    ON quota_key_observations (endpoint_id, key_id, observation_id DESC);
CREATE INDEX idx_quota_key_observations_observed
    ON quota_key_observations (observed_at);
