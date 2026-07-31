ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_abort_reason;

ALTER TABLE request_records
DROP CONSTRAINT IF EXISTS ck_request_records_abort_from_state;

ALTER TABLE request_records
DROP COLUMN IF EXISTS abort_reason,
DROP COLUMN IF EXISTS abort_from_state,
DROP COLUMN IF EXISTS abort_response_started;
