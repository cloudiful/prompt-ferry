ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS event_kind TEXT NOT NULL DEFAULT 'request';

ALTER TABLE usage_events
DROP CONSTRAINT IF EXISTS ck_usage_events_event_kind;

ALTER TABLE usage_events
ADD CONSTRAINT ck_usage_events_event_kind
CHECK (event_kind IN ('request', 'upstream_attempt'));

CREATE INDEX IF NOT EXISTS idx_usage_events_request_kind_created_at
ON usage_events(request_id, event_kind, created_at ASC, event_id ASC);

CREATE TABLE IF NOT EXISTS model_avoidances (
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    consecutive_capacity_failures INTEGER NOT NULL DEFAULT 0,
    backoff_level INTEGER NOT NULL DEFAULT 0,
    avoid_until TIMESTAMPTZ,
    last_status INTEGER,
    last_error_message TEXT,
    last_error_body TEXT,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY(endpoint_id, model)
);

CREATE INDEX IF NOT EXISTS idx_model_avoidances_avoid_until
ON model_avoidances(avoid_until);

CREATE TABLE IF NOT EXISTS conversation_endpoint_overrides (
    conversation_id UUID PRIMARY KEY,
    endpoint_id UUID NOT NULL REFERENCES provider_endpoints(endpoint_id) ON DELETE CASCADE,
    created_by_user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_conversation_endpoint_overrides_endpoint_id
ON conversation_endpoint_overrides(endpoint_id);
