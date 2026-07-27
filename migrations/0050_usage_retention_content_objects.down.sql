ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS request_prompt TEXT,
ADD COLUMN IF NOT EXISTS request_full_text TEXT,
ADD COLUMN IF NOT EXISTS request_delta_text TEXT,
ADD COLUMN IF NOT EXISTS upstream_redacted_request_json JSONB,
ADD COLUMN IF NOT EXISTS restore_session_ciphertext BYTEA,
ADD COLUMN IF NOT EXISTS restore_session_nonce BYTEA,
ADD COLUMN IF NOT EXISTS restore_session_key_version SMALLINT;

DROP INDEX IF EXISTS idx_request_records_content_expired_at;

ALTER TABLE request_records
DROP COLUMN IF EXISTS content_expired_at;

-- Object-only rows cannot be represented by the pre-0050 schema. Remove their
-- metadata before restoring the old payload constraint; the object store copy
-- is managed independently and is not recoverable through this rollback.
DELETE FROM request_record_raw_payloads
WHERE request_raw_json IS NULL
  AND response_raw_body IS NULL;

DELETE FROM request_record_raw_payloads_overflow
WHERE request_raw_json IS NULL
  AND response_raw_body IS NULL;

ALTER TABLE request_record_raw_payloads
DROP CONSTRAINT IF EXISTS ck_request_record_raw_payloads_has_payload;

ALTER TABLE request_record_raw_payloads
ADD CONSTRAINT ck_request_record_raw_payloads_has_payload CHECK (
    request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
);

ALTER TABLE request_record_raw_payloads_overflow
DROP CONSTRAINT IF EXISTS ck_request_record_raw_payloads_overflow_has_payload;

ALTER TABLE request_record_raw_payloads_overflow
ADD CONSTRAINT ck_request_record_raw_payloads_overflow_has_payload CHECK (
    request_raw_json IS NOT NULL OR response_raw_body IS NOT NULL
);

ALTER TABLE request_record_raw_payloads
DROP COLUMN IF EXISTS raw_object_key,
DROP COLUMN IF EXISTS raw_object_size_bytes,
DROP COLUMN IF EXISTS raw_object_sha256,
DROP COLUMN IF EXISTS raw_object_expires_at;

ALTER TABLE request_record_raw_payloads_overflow
DROP COLUMN IF EXISTS raw_object_key,
DROP COLUMN IF EXISTS raw_object_size_bytes,
DROP COLUMN IF EXISTS raw_object_sha256,
DROP COLUMN IF EXISTS raw_object_expires_at;
