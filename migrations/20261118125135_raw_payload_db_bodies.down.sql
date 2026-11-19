-- Destructive rollback: every PostgreSQL-stored raw body is lost. The four
-- raw_object_* physical columns stay in place for the later storage cleanup;
-- this down migration only removes the body columns and their checks.
ALTER TABLE request_record_raw_payloads
    DROP CONSTRAINT IF EXISTS ck_request_record_raw_payloads_has_payload;
ALTER TABLE request_record_raw_payloads_overflow
    DROP CONSTRAINT IF EXISTS ck_request_record_raw_payloads_overflow_has_payload;
ALTER TABLE request_record_raw_payloads
    DROP COLUMN IF EXISTS response_raw_body,
    DROP COLUMN IF EXISTS request_raw_json;
ALTER TABLE request_record_raw_payloads_overflow
    DROP COLUMN IF EXISTS response_raw_body,
    DROP COLUMN IF EXISTS request_raw_json;
