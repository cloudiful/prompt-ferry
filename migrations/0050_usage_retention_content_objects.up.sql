ALTER TABLE request_records
ADD COLUMN IF NOT EXISTS content_expired_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_request_records_content_expired_at
ON request_records(content_expired_at, created_at ASC, event_id ASC)
WHERE content_expired_at IS NULL;

ALTER TABLE request_record_raw_payloads
ADD COLUMN IF NOT EXISTS raw_object_key TEXT,
ADD COLUMN IF NOT EXISTS raw_object_size_bytes BIGINT,
ADD COLUMN IF NOT EXISTS raw_object_sha256 TEXT,
ADD COLUMN IF NOT EXISTS raw_object_expires_at TIMESTAMPTZ;

ALTER TABLE request_record_raw_payloads
DROP CONSTRAINT IF EXISTS ck_request_record_raw_payloads_has_payload;

ALTER TABLE request_record_raw_payloads
ADD CONSTRAINT ck_request_record_raw_payloads_has_payload CHECK (
    request_raw_json IS NOT NULL
    OR response_raw_body IS NOT NULL
    OR raw_object_key IS NOT NULL
);

ALTER TABLE request_record_raw_payloads_overflow
ADD COLUMN IF NOT EXISTS raw_object_key TEXT,
ADD COLUMN IF NOT EXISTS raw_object_size_bytes BIGINT,
ADD COLUMN IF NOT EXISTS raw_object_sha256 TEXT,
ADD COLUMN IF NOT EXISTS raw_object_expires_at TIMESTAMPTZ;

ALTER TABLE request_record_raw_payloads_overflow
DROP CONSTRAINT IF EXISTS ck_request_record_raw_payloads_overflow_has_payload;

ALTER TABLE request_record_raw_payloads_overflow
ADD CONSTRAINT ck_request_record_raw_payloads_overflow_has_payload CHECK (
    request_raw_json IS NOT NULL
    OR response_raw_body IS NOT NULL
    OR raw_object_key IS NOT NULL
);

ALTER TABLE request_records
DROP COLUMN IF EXISTS request_prompt,
DROP COLUMN IF EXISTS request_full_text,
DROP COLUMN IF EXISTS request_delta_text,
DROP COLUMN IF EXISTS upstream_redacted_request_json,
DROP COLUMN IF EXISTS restore_session_ciphertext,
DROP COLUMN IF EXISTS restore_session_nonce,
DROP COLUMN IF EXISTS restore_session_key_version;
