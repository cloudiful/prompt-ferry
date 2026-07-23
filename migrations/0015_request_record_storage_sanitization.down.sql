ALTER TABLE request_records
DROP COLUMN IF EXISTS storage_sanitized_nul_count;

ALTER TABLE request_records
DROP COLUMN IF EXISTS storage_sanitized;
