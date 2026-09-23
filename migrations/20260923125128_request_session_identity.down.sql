ALTER TABLE request_records
DROP COLUMN IF EXISTS session_parent_id;

ALTER TABLE request_records
DROP COLUMN IF EXISTS session_header_id;
