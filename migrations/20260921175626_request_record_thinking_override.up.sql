-- Issue #546: snapshot the route target's thinking effort override at
-- request time so the request detail keeps showing the value that actually
-- reached the upstream. NULL means the target had no override for this
-- request (inherit) and stays NULL when the snapshot is unknown.
ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS applied_thinking_effort_override TEXT NULL;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_applied_thinking_effort;

ALTER TABLE request_records
ADD CONSTRAINT ck_request_records_applied_thinking_effort
CHECK (applied_thinking_effort_override IS NULL OR applied_thinking_effort_override IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'));
