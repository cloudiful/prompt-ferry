ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_applied_thinking_effort;

ALTER TABLE request_records
DROP COLUMN IF EXISTS applied_thinking_effort_override;
