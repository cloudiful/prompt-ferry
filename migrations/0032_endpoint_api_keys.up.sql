CREATE TABLE IF NOT EXISTS endpoint_api_keys (
    key_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    key_label TEXT NOT NULL,
    api_key TEXT NOT NULL,
    position INTEGER NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_endpoint_api_keys_endpoint_position
ON endpoint_api_keys(endpoint_id, position);

ALTER TABLE provider_endpoints
ADD COLUMN IF NOT EXISTS key_lb_enabled BOOLEAN NOT NULL DEFAULT FALSE;

INSERT INTO endpoint_api_keys(endpoint_id, key_label, api_key, position, enabled, created_at, updated_at)
SELECT endpoint_id, COALESCE(NULLIF(name, ''), 'primary'), api_key, 0, TRUE, created_at, updated_at
FROM provider_endpoints
ON CONFLICT DO NOTHING;
