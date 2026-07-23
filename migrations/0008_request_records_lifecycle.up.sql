ALTER TABLE usage_events RENAME TO request_records;

ALTER TABLE usage_assistant_artifacts
RENAME TO request_record_assistant_artifacts;

ALTER TABLE request_records
ALTER COLUMN ok DROP NOT NULL;

ALTER TABLE request_records
ALTER COLUMN duration_ms DROP NOT NULL;

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS request_state TEXT NOT NULL DEFAULT 'completed';

ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

UPDATE request_records
SET request_state = CASE
    WHEN ok IS TRUE THEN 'completed'
    ELSE 'failed'
END
WHERE event_kind = 'request';

UPDATE request_records
SET request_state = 'failed'
WHERE event_kind = 'upstream_attempt';

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_request_state;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_request_state
CHECK (request_state IN ('received', 'awaiting_approval', 'upstream_processing', 'completed', 'failed'));

DROP INDEX IF EXISTS idx_usage_events_created_at;
DROP INDEX IF EXISTS idx_usage_events_created_at_event_id;
DROP INDEX IF EXISTS idx_usage_events_user_created_at;
DROP INDEX IF EXISTS idx_usage_events_user_created_at_event_id;
DROP INDEX IF EXISTS idx_usage_events_endpoint_created_at;
DROP INDEX IF EXISTS idx_usage_events_request_id;
DROP INDEX IF EXISTS idx_usage_events_conversation_seq;
DROP INDEX IF EXISTS idx_usage_events_provider_response_id;
DROP INDEX IF EXISTS idx_usage_events_request_kind_created_at;

CREATE INDEX IF NOT EXISTS idx_request_records_created_at
ON request_records(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_created_at_record_id
ON request_records(created_at DESC, event_id DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_user_created_at
ON request_records(user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_user_created_at_record_id
ON request_records(user_id, created_at DESC, event_id DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_endpoint_created_at
ON request_records(endpoint_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_request_id
ON request_records(request_id);

CREATE INDEX IF NOT EXISTS idx_request_records_conversation_seq
ON request_records(conversation_id, conversation_seq DESC);

CREATE INDEX IF NOT EXISTS idx_request_records_provider_response_id
ON request_records(provider_response_id);

CREATE INDEX IF NOT EXISTS idx_request_records_request_kind_created_at
ON request_records(request_id, event_kind, created_at ASC, event_id ASC);

CREATE UNIQUE INDEX IF NOT EXISTS uq_request_records_request_id_request
ON request_records(request_id)
WHERE event_kind = 'request';
