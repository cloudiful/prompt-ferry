DROP INDEX IF EXISTS uq_request_records_request_id_request;
DROP INDEX IF EXISTS idx_request_records_request_kind_created_at;
DROP INDEX IF EXISTS idx_request_records_provider_response_id;
DROP INDEX IF EXISTS idx_request_records_conversation_seq;
DROP INDEX IF EXISTS idx_request_records_request_id;
DROP INDEX IF EXISTS idx_request_records_endpoint_created_at;
DROP INDEX IF EXISTS idx_request_records_user_created_at_record_id;
DROP INDEX IF EXISTS idx_request_records_user_created_at;
DROP INDEX IF EXISTS idx_request_records_created_at_record_id;
DROP INDEX IF EXISTS idx_request_records_created_at;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_request_state;

ALTER TABLE request_records
DROP COLUMN IF EXISTS request_state;

ALTER TABLE request_records
DROP COLUMN IF EXISTS updated_at;

UPDATE request_records
SET ok = COALESCE(ok, FALSE),
    duration_ms = COALESCE(duration_ms, 0);

ALTER TABLE request_records
ALTER COLUMN ok SET NOT NULL;

ALTER TABLE request_records
ALTER COLUMN duration_ms SET NOT NULL;

ALTER TABLE request_record_assistant_artifacts
RENAME TO usage_assistant_artifacts;

ALTER TABLE request_records
RENAME TO usage_events;

CREATE INDEX IF NOT EXISTS idx_usage_events_created_at ON usage_events(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_created_at_event_id
ON usage_events(created_at DESC, event_id DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_user_created_at ON usage_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_user_created_at_event_id
ON usage_events(user_id, created_at DESC, event_id DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_endpoint_created_at ON usage_events(endpoint_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_request_id ON usage_events(request_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_conversation_seq ON usage_events(conversation_id, conversation_seq DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_provider_response_id ON usage_events(provider_response_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_request_kind_created_at
ON usage_events(request_id, event_kind, created_at ASC, event_id ASC);
