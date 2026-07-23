DROP INDEX IF EXISTS idx_request_record_tool_calls_conversation_status;
DROP INDEX IF EXISTS idx_request_record_tool_calls_parent;
DROP TABLE IF EXISTS request_record_tool_calls;

DROP TABLE IF EXISTS conversation_counters;

DROP INDEX IF EXISTS idx_request_records_active_lease;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_request_state;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_request_state
CHECK (request_state IN (
    'received',
    'awaiting_approval',
    'upstream_processing',
    'completed',
    'failed'
));

ALTER TABLE request_records
DROP COLUMN IF EXISTS last_heartbeat_at;

ALTER TABLE request_records
DROP COLUMN IF EXISTS lease_expires_at;

ALTER TABLE request_records
DROP COLUMN IF EXISTS owner_worker_id;
