DROP INDEX IF EXISTS idx_usage_events_request_has_previous_response_id;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS request_previous_response_parent_found;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS request_previous_response_id;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS request_has_previous_response_id;

ALTER TABLE usage_events
DROP COLUMN IF EXISTS request_raw_json;
