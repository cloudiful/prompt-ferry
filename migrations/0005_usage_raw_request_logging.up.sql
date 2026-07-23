ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS request_raw_json JSONB;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS request_has_previous_response_id BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS request_previous_response_id TEXT;

ALTER TABLE usage_events
ADD COLUMN IF NOT EXISTS request_previous_response_parent_found BOOLEAN;

CREATE INDEX IF NOT EXISTS idx_usage_events_request_has_previous_response_id
ON usage_events(request_has_previous_response_id, created_at DESC);
