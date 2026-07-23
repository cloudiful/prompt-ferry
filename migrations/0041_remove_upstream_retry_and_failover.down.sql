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

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_event_kind;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_event_kind
CHECK (event_kind IN ('request', 'upstream_attempt'));

CREATE INDEX IF NOT EXISTS idx_request_records_request_kind_created_at
ON request_records(request_id, event_kind, created_at ASC, event_id ASC);

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_route_selection_reason
CHECK (
    route_selection_reason IN (
        'default',
        'session_affinity',
        'conversation_override',
        'avoidance_fallback'
    )
);
